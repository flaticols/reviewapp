//! A [`PullRequestSource`] backed by GitHub through the `gh` CLI, which supplies the
//! signed-in user's token. Every call runs `gh api` and blocks.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use diff_model::FileChange;
use pr_model::{
    CheckStatus, Inbox, MergeQueueState, PullRequest, PullRequestKey, PullRequestState, RepoSlug,
    ReviewDecision, StackPosition, Timestamp,
};
use review_model::{
    Actor, AutoMerge, CheckRun, CheckStep, Comment, Commit, Dependency, LogLine, LogLineKind,
    MergeInfo, MergeMethod, MergeState, MergeStatus, PullRequestDetail, ReviewThread,
    ReviewVerdict, Reviewer, ReviewerState, Stack, StackEntry, TimelineItem,
};
use serde_json::{Value, json};

use crate::time::parse_rfc3339;
use crate::{ApiError, PullRequestSource, ReviewSubmission};

/// Lines of a failed job's log kept for the check sheet.
const LOG_TAIL: usize = 40;
/// Pull requests fetched per inbox.
const PER_INBOX: u32 = 50;

/// Reads pull requests for the `gh` account on `host`.
pub struct GhSource {
    host: String,
    /// Set once the host said it lacks a newer pull request field, as a GitHub Enterprise
    /// Server without stacks does.
    without_newer_fields: AtomicBool,
}

impl GhSource {
    /// A source for `host`, normally `github.com`.
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            without_newer_fields: AtomicBool::new(false),
        }
    }

    fn api(&self, args: &[&str], input: Option<&Value>) -> Result<Value, ApiError> {
        let mut command = tool_path::command("gh");
        command
            .arg("api")
            .arg("--hostname")
            .arg(&self.host)
            .args(args);
        if input.is_some() {
            command.args(["--input", "-"]);
            command.stdin(std::process::Stdio::piped());
        }
        command
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|error| ApiError::new(format!("Couldn’t run the GitHub CLI: {error}")))?;
        if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
            use std::io::Write as _;
            stdin
                .write_all(input.to_string().as_bytes())
                .map_err(|error| ApiError::new(error.to_string()))?;
        }
        let output = child
            .wait_with_output()
            .map_err(|error| ApiError::new(error.to_string()))?;
        if !output.status.success() {
            let message = String::from_utf8_lossy(&output.stderr);
            return Err(ApiError::new(format!(
                "GitHub: {}",
                message
                    .lines()
                    .last()
                    .unwrap_or("request failed")
                    .trim_start_matches("gh: ")
            )));
        }
        if output.stdout.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&output.stdout)
            .map_err(|error| ApiError::new(format!("Unexpected GitHub response: {error}")))
    }

    fn graphql(&self, query: &str, variables: Value) -> Result<Value, ApiError> {
        let response = self.api(
            &["graphql"],
            Some(&json!({ "query": query, "variables": variables })),
        )?;
        if let Some(errors) = response.get("errors").and_then(Value::as_array)
            && let Some(first) = errors.first()
        {
            return Err(ApiError::new(format!(
                "GitHub: {}",
                first["message"].as_str().unwrap_or("query failed")
            )));
        }
        Ok(response["data"].clone())
    }

    /// Runs a pull request query whose `template` has a [`NEWER_FIELDS`] placeholder,
    /// with `newer_fields` in its place. One unknown field fails a whole query, so on a
    /// host that lacks one of them this asks again without them and stops asking for them.
    fn pull_request_query(
        &self,
        template: &str,
        newer_fields: &str,
        variables: Value,
    ) -> Result<Value, ApiError> {
        if !self.without_newer_fields.load(Ordering::Relaxed) {
            let query = template.replace(NEWER_FIELDS, newer_fields);
            match self.graphql(&query, variables.clone()) {
                Err(error) if lacks_pull_request_field(&error.to_string()) => {
                    self.without_newer_fields.store(true, Ordering::Relaxed);
                }
                result => return result,
            }
        }
        self.graphql(&template.replace(NEWER_FIELDS, ""), variables)
    }

    fn rest_paginated(&self, path: &str) -> Result<Vec<Value>, ApiError> {
        let response = self.api(&["--paginate", "--slurp", path], None)?;
        Ok(response
            .as_array()
            .map(|pages| {
                pages
                    .iter()
                    .flat_map(|page| page.as_array().cloned().unwrap_or_default())
                    .collect()
            })
            .unwrap_or_default())
    }

    fn rest_text(&self, path: &str) -> Result<String, ApiError> {
        let output = tool_path::command("gh")
            .args(["api", "--hostname", &self.host, path])
            .output()
            .map_err(|error| ApiError::new(error.to_string()))?;
        if !output.status.success() {
            return Err(ApiError::new("The log isn’t available."));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// Where a pull request query takes the fields an older GitHub Enterprise Server may
/// not have: the stack and the merge queue entry.
const NEWER_FIELDS: &str = "__NEWER_FIELDS__";

/// A list row's merge queue state and place in its stack. The stack's entries stay out
/// of the list query: nested under every row of every inbox they would multiply its cost.
const SUMMARY_EXTRAS: &str =
    "mergeQueueEntry { state } stack { number size } stackEntry { position }";

/// The merge queue state and the stack with every member, for the review screen.
const DETAIL_EXTRAS: &str = "mergeQueueEntry { state } stack { number size baseRefName entries(first: 50) { nodes { position pullRequest { number title state isDraft } } } } stackEntry { position }";

/// Whether GitHub's error `message` says the host's schema lacks a pull request field.
fn lacks_pull_request_field(message: &str) -> bool {
    message.contains("doesn't exist on type 'PullRequest'")
}

const SEARCH_QUERY: &str = r#"
query($query: String!, $first: Int!) {
  search(query: $query, type: ISSUE, first: $first) {
    nodes {
      ... on PullRequest {
        number title isDraft state updatedAt
        repository { nameWithOwner }
        author { login ... on User { name } }
        headRefName baseRefName reviewDecision
        comments { totalCount }
        reviewThreads { totalCount }
        labels(first: 10) { nodes { name } }
        milestone { title }
        __NEWER_FIELDS__
        commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }
        bodyText additions deletions changedFiles
        files(first: 40) { nodes { path } }
      }
    }
  }
}"#;

const DETAIL_QUERY: &str = r#"
query($owner: String!, $name: String!, $number: Int!) {
  viewer { login }
  repository(owner: $owner, name: $name) {
    mergeCommitAllowed squashMergeAllowed rebaseMergeAllowed deleteBranchOnMerge viewerDefaultMergeMethod
    pullRequest(number: $number) {
      id headRefOid mergeStateStatus viewerCanEnableAutoMerge viewerCanDisableAutoMerge viewerCanUpdateBranch
      isCrossRepository
      autoMergeRequest { mergeMethod enabledBy { login } }
      number title body isDraft state updatedAt createdAt
      author { login ... on User { name } }
      headRefName baseRefName reviewDecision
      repository { nameWithOwner }
      comments { totalCount }
      labels(first: 10) { nodes { name } }
      milestone { title }
      __NEWER_FIELDS__
      commits(last: 100) { nodes { commit { oid messageHeadline statusCheckRollup { state } } } }
      reviewRequests(first: 20) { nodes { requestedReviewer { ... on User { login name } ... on Team { name } ... on Bot { login } } } }
      latestReviews(first: 30) { nodes { author { login ... on User { name } } state } }
      reviewThreads(first: 100) {
        nodes {
          id isResolved path line originalLine
          comments(first: 50) { nodes { databaseId author { login ... on User { name } } body createdAt } }
        }
      }
      timelineItems(first: 200, itemTypes: [ISSUE_COMMENT, PULL_REQUEST_REVIEW, PULL_REQUEST_COMMIT, REVIEW_REQUESTED_EVENT]) {
        nodes {
          __typename
          ... on IssueComment { databaseId author { login ... on User { name } } body createdAt }
          ... on PullRequestReview { author { login ... on User { name } } state submittedAt body }
          ... on PullRequestCommit { commit { oid committedDate author { user { login name } name } } }
          ... on ReviewRequestedEvent { createdAt actor { login ... on User { name } } requestedReviewer { ... on User { login name } ... on Team { name } ... on Bot { login } } }
        }
      }
    }
  }
}"#;

fn timestamp(value: &Value) -> Timestamp {
    value
        .as_str()
        .and_then(parse_rfc3339)
        .unwrap_or_else(Timestamp::now)
}

/// A person from a GitHub `author`, `actor` or reviewer object, with their profile name
/// when they set one.
fn person(value: &Value) -> Actor {
    Actor::person(login(value)).with_name(value["name"].as_str())
}

fn login(value: &Value) -> String {
    value["login"]
        .as_str()
        .or_else(|| value["name"].as_str())
        .unwrap_or("ghost")
        .to_string()
}

fn ci_status(rollup: &Value) -> Option<CheckStatus> {
    match rollup["state"].as_str()? {
        "SUCCESS" => Some(CheckStatus::Passed),
        "FAILURE" | "ERROR" => Some(CheckStatus::Failed),
        "PENDING" | "EXPECTED" => Some(CheckStatus::Running),
        _ => None,
    }
}

/// What merging involves, from the repository's settings and the pull request.
fn merge_info(repository: &Value, node: &Value) -> Option<MergeInfo> {
    let node_id = node["id"].as_str()?;
    let methods: Vec<MergeMethod> = [
        ("mergeCommitAllowed", MergeMethod::Merge),
        ("squashMergeAllowed", MergeMethod::Squash),
        ("rebaseMergeAllowed", MergeMethod::Rebase),
    ]
    .into_iter()
    .filter(|(field, _)| repository[*field].as_bool().unwrap_or(false))
    .map(|(_, method)| method)
    .collect();
    let mut info = MergeInfo::new(
        node_id,
        node["headRefOid"].as_str().unwrap_or_default(),
        MergeState::from_graphql(node["mergeStateStatus"].as_str().unwrap_or_default()),
        methods,
    );
    if let Some(default) = repository["viewerDefaultMergeMethod"]
        .as_str()
        .and_then(MergeMethod::from_graphql)
        .filter(|method| info.methods.contains(method))
    {
        info.default_method = Some(default);
    }
    info.auto_merge = node["autoMergeRequest"]["mergeMethod"]
        .as_str()
        .and_then(MergeMethod::from_graphql)
        .map(|method| {
            AutoMerge::new(
                method,
                node["autoMergeRequest"]["enabledBy"]["login"]
                    .as_str()
                    .unwrap_or_default(),
            )
        });
    info.can_enable_auto_merge = node["viewerCanEnableAutoMerge"].as_bool().unwrap_or(false);
    info.can_disable_auto_merge = node["viewerCanDisableAutoMerge"].as_bool().unwrap_or(false);
    info.can_update_branch = node["viewerCanUpdateBranch"].as_bool().unwrap_or(false);
    info.deletes_branch = repository["deleteBranchOnMerge"].as_bool().unwrap_or(false);
    info.from_fork = node["isCrossRepository"].as_bool().unwrap_or(false);
    Some(info)
}

/// The REST merge request. Without a title GitHub writes its own title and message.
fn merge_body(method: MergeMethod, title: &str, message: &str, head_sha: &str) -> Value {
    let mut body = json!({ "merge_method": method.api_name(), "sha": head_sha });
    if method.has_message() && !title.is_empty() {
        body["commit_title"] = json!(title);
        body["commit_message"] = json!(message);
    }
    body
}

/// The merge status query for `keys`: one `pN` alias each, with owners, names and
/// numbers passed as variables.
fn merge_status_query(keys: &[PullRequestKey]) -> (String, Value) {
    let mut params = Vec::new();
    let mut aliases = String::new();
    let mut variables = serde_json::Map::new();
    for (ix, key) in keys.iter().enumerate() {
        params.push(format!("$o{ix}: String!, $n{ix}: String!, $p{ix}: Int!"));
        variables.insert(format!("o{ix}"), json!(key.repo().owner()));
        variables.insert(format!("n{ix}"), json!(key.repo().name()));
        variables.insert(format!("p{ix}"), json!(key.number()));
        // ponytail: the first 100 check contexts only; page them if a repository has more.
        aliases.push_str(&format!(
            r#"
  p{ix}: repository(owner: $o{ix}, name: $n{ix}) {{
    mergeCommitAllowed squashMergeAllowed rebaseMergeAllowed deleteBranchOnMerge viewerDefaultMergeMethod
    pullRequest(number: $p{ix}) {{
      id state isDraft headRefOid mergeStateStatus reviewDecision
      viewerCanEnableAutoMerge viewerCanDisableAutoMerge viewerCanUpdateBranch isCrossRepository
      autoMergeRequest {{ mergeMethod enabledBy {{ login }} }}
      commits(last: 1) {{ nodes {{ commit {{ statusCheckRollup {{ contexts(first: 100) {{ nodes {{
        __typename
        ... on CheckRun {{ status conclusion isRequired(pullRequestNumber: $p{ix}) }}
        ... on StatusContext {{ state isRequired(pullRequestNumber: $p{ix}) }}
      }} }} }} }} }} }}
    }}
  }}"#
        ));
    }
    let query = format!("query({}) {{{aliases}\n}}", params.join(", "));
    (query, Value::Object(variables))
}

/// The merge statuses in a merge status query's `data`, in `keys` order, leaving out
/// pull requests GitHub didn't return.
fn parse_merge_statuses(keys: &[PullRequestKey], data: &Value) -> Vec<MergeStatus> {
    keys.iter()
        .enumerate()
        .filter_map(|(ix, key)| {
            let repository = &data[format!("p{ix}")];
            let node = &repository["pullRequest"];
            let mut status =
                MergeStatus::new(key.clone(), state_from(node), merge_info(repository, node)?);
            status.required_checks = required_checks(
                &node["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"]["nodes"],
            );
            status.needs_review = matches!(
                node["reviewDecision"].as_str(),
                Some("REVIEW_REQUIRED" | "CHANGES_REQUESTED")
            );
            Some(status)
        })
        .collect()
}

/// Failed when a required check failed, Running when one hasn't finished, Passed
/// otherwise. Optional checks don't count.
fn required_checks(contexts: &Value) -> CheckStatus {
    let required: Vec<&Value> = contexts
        .as_array()
        .map(|nodes| {
            nodes
                .iter()
                .filter(|context| context["isRequired"].as_bool() == Some(true))
                .collect()
        })
        .unwrap_or_default();
    let failed = |context: &&Value| match context["__typename"].as_str() {
        Some("StatusContext") => matches!(context["state"].as_str(), Some("FAILURE" | "ERROR")),
        _ => matches!(
            context["conclusion"].as_str(),
            Some("FAILURE" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE")
        ),
    };
    let running = |context: &&Value| match context["__typename"].as_str() {
        Some("StatusContext") => matches!(context["state"].as_str(), Some("PENDING" | "EXPECTED")),
        _ => context["status"].as_str() != Some("COMPLETED"),
    };
    if required.iter().any(failed) {
        CheckStatus::Failed
    } else if required.iter().any(running) {
        CheckStatus::Running
    } else {
        CheckStatus::Passed
    }
}

/// How much of a description the list keeps, enough for triage to judge by.
const BODY_EXCERPT_CHARS: usize = 1500;

fn state_from(node: &Value) -> PullRequestState {
    match (node["state"].as_str(), node["isDraft"].as_bool()) {
        (Some("MERGED"), _) => PullRequestState::Merged,
        (Some("CLOSED"), _) => PullRequestState::Closed,
        (_, Some(true)) => PullRequestState::Draft,
        _ => PullRequestState::Open,
    }
}

/// The stack `node` is part of, with its members in `repo`. An entry whose pull request
/// the viewer can't read comes back null and is left out.
fn stack_from(repo: &RepoSlug, node: &Value) -> Option<Stack> {
    let stack = &node["stack"];
    let number = stack["number"].as_u64()?;
    let entries = stack["entries"]["nodes"]
        .as_array()?
        .iter()
        .filter_map(|entry| {
            let pull_request = &entry["pullRequest"];
            Some(StackEntry::new(
                entry["position"].as_u64()? as u32,
                PullRequestKey::new(repo.clone(), pull_request["number"].as_u64()?),
                pull_request["title"].as_str().unwrap_or_default(),
                state_from(pull_request),
            ))
        })
        .collect();
    Some(Stack::new(
        number,
        stack["baseRefName"].as_str().unwrap_or_default(),
        entries,
    ))
}

fn summary_from(node: &Value) -> Option<PullRequest> {
    let repo = RepoSlug::parse(node["repository"]["nameWithOwner"].as_str()?)?;
    let number = node["number"].as_u64()?;
    let mut pull_request = PullRequest::new(
        PullRequestKey::new(repo, number),
        node["title"].as_str().unwrap_or_default(),
        login(&node["author"]),
        node["headRefName"].as_str().unwrap_or_default(),
        node["baseRefName"].as_str().unwrap_or_default(),
        timestamp(&node["updatedAt"]),
    );
    pull_request.state = state_from(node);
    pull_request.review = match node["reviewDecision"].as_str() {
        Some("APPROVED") => ReviewDecision::Approved,
        Some("CHANGES_REQUESTED") => ReviewDecision::ChangesRequested,
        Some("REVIEW_REQUIRED") => ReviewDecision::ReviewRequested,
        _ if node["reviewThreads"]["totalCount"].as_u64().unwrap_or(0) > 0 => {
            ReviewDecision::Commented
        }
        _ => ReviewDecision::NoReviews,
    };
    pull_request.ci = node["commits"]["nodes"]
        .get(0)
        .and_then(|commit| ci_status(&commit["commit"]["statusCheckRollup"]));
    pull_request.comments = (node["comments"]["totalCount"].as_u64().unwrap_or(0)
        + node["reviewThreads"]["totalCount"].as_u64().unwrap_or(0))
        as u32;
    pull_request.labels = node["labels"]["nodes"]
        .as_array()
        .map(|labels| {
            labels
                .iter()
                .filter_map(|label| label["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    pull_request.milestone = node["milestone"]["title"].as_str().map(str::to_string);
    pull_request.stack = node["stack"]["number"].as_u64().and_then(|stack| {
        Some(StackPosition::new(
            stack,
            node["stackEntry"]["position"].as_u64()? as u32,
            node["stack"]["size"].as_u64()? as u32,
        ))
    });
    pull_request.merge_queue = node["mergeQueueEntry"]["state"]
        .as_str()
        .map(MergeQueueState::from_graphql);
    pull_request.body_excerpt = node["bodyText"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(BODY_EXCERPT_CHARS)
        .collect();
    let count = |field: &str| node[field].as_u64().unwrap_or(0) as u32;
    pull_request.additions = count("additions");
    pull_request.deletions = count("deletions");
    pull_request.changed_files = count("changedFiles");
    pull_request.paths = node["files"]["nodes"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter_map(|file| file["path"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Some(pull_request)
}

/// `#477` references after "depends on" or "lands after" in a description.
fn dependency_numbers(body: &str) -> Vec<u64> {
    let lower = body.to_lowercase();
    let mut numbers = Vec::new();
    for marker in ["depends on", "lands after", "blocked by"] {
        let mut rest = lower.as_str();
        while let Some(at) = rest.find(marker) {
            rest = &rest[at + marker.len()..];
            let digits: String = rest
                .trim_start()
                .trim_start_matches('#')
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if rest.trim_start().starts_with('#')
                && let Ok(number) = digits.parse()
                && !numbers.contains(&number)
            {
                numbers.push(number);
            }
        }
    }
    numbers
}

impl GhSource {
    /// The head commit's check runs, looked up by the pull request's ref so the head
    /// SHA isn't needed first.
    fn head_checks(&self, key: &PullRequestKey) -> Result<Vec<CheckRun>, ApiError> {
        let repo = key.repo();
        let response = self.api(
            &[&format!(
                "repos/{repo}/commits/refs/pull/{}/head/check-runs?per_page=100",
                key.number()
            )],
            None,
        )?;
        let runs = response["check_runs"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let workflow_runs: Vec<u64> = {
            let mut ids: Vec<u64> = runs
                .iter()
                .filter_map(|run| run["html_url"].as_str().and_then(workflow_run_id))
                .collect();
            ids.sort_unstable();
            ids.dedup();
            ids
        };
        let steps_by_job: HashMap<u64, Vec<Value>> = std::thread::scope(|scope| {
            let handles: Vec<_> = workflow_runs
                .iter()
                .map(|run| {
                    scope.spawn(move || {
                        self.api(
                            &[&format!(
                                "repos/{repo}/actions/runs/{run}/jobs?per_page=100"
                            )],
                            None,
                        )
                        .map(|jobs| jobs["jobs"].as_array().cloned().unwrap_or_default())
                        .unwrap_or_default()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|handle| handle.join().unwrap_or_default())
                .filter_map(|job| {
                    Some((
                        job["id"].as_u64()?,
                        job["steps"].as_array().cloned().unwrap_or_default(),
                    ))
                })
                .collect()
        });
        Ok(runs
            .iter()
            .map(|run| {
                let status = check_status(run["status"].as_str(), run["conclusion"].as_str());
                let elapsed = elapsed_secs(&run["started_at"], &run["completed_at"]);
                let id = run["id"].as_u64().unwrap_or_default();
                let steps = steps_by_job
                    .get(&id)
                    .map(|steps| job_steps(steps))
                    .unwrap_or_default();
                CheckRun::new(
                    id.to_string(),
                    run["name"].as_str().unwrap_or("check"),
                    status,
                    elapsed,
                    run["head_sha"].as_str().unwrap_or_default(),
                    steps,
                )
                .with_url(run["html_url"].as_str().unwrap_or_default())
            })
            .collect())
    }

    fn files(&self, key: &PullRequestKey) -> Result<Vec<diff_model::FileDiff>, ApiError> {
        let files = self.rest_paginated(&format!(
            "repos/{}/pulls/{}/files?per_page=100",
            key.repo(),
            key.number()
        ))?;
        Ok(files.iter().map(file_diff).collect())
    }
}

/// A file of a pull request or commit as GitHub's REST API lists it, with its patch.
/// GitHub leaves the patch out of binary files and very large diffs.
fn file_diff(file: &Value) -> diff_model::FileDiff {
    let path = file["filename"].as_str().unwrap_or_default();
    let change = match file["status"].as_str() {
        Some("added") => FileChange::Added,
        Some("removed") => FileChange::Deleted,
        Some("renamed") => FileChange::Renamed,
        _ => FileChange::Modified,
    };
    let diff = match file["patch"].as_str() {
        Some(patch) => git_diff::parse_patch(path, change, patch),
        None => diff_model::FileDiff::new(path, change, Vec::new()).with_binary(true),
    };
    match file["previous_filename"].as_str() {
        Some(old) => diff.with_old_path(old),
        None => diff,
    }
}

/// The workflow run id in a check run's Actions URL, `…/actions/runs/{run}/job/{job}`.
fn workflow_run_id(url: &str) -> Option<u64> {
    let rest = url.split_once("/actions/runs/")?.1;
    rest.split('/').next()?.parse().ok()
}

fn check_status(status: Option<&str>, conclusion: Option<&str>) -> CheckStatus {
    match (status, conclusion) {
        (Some("completed"), Some("success" | "neutral")) => CheckStatus::Passed,
        (Some("completed"), Some("skipped")) => CheckStatus::Skipped,
        (Some("completed"), _) => CheckStatus::Failed,
        (Some("queued" | "waiting" | "pending"), _) => CheckStatus::Queued,
        _ => CheckStatus::Running,
    }
}

fn elapsed_secs(started: &Value, completed: &Value) -> u32 {
    match (
        started.as_str().and_then(parse_rfc3339),
        completed.as_str().and_then(parse_rfc3339),
    ) {
        (Some(start), Some(end)) => (end.unix() - start.unix()).max(0) as u32,
        (Some(start), None) => (Timestamp::now().unix() - start.unix()).max(0) as u32,
        _ => 0,
    }
}

/// A job's steps, without logs; a failed job's log is fetched when it is opened.
fn job_steps(steps: &[Value]) -> Vec<CheckStep> {
    steps
        .iter()
        .map(|step| {
            let status = check_status(step["status"].as_str(), step["conclusion"].as_str());
            let duration = step["completed_at"]
                .as_str()
                .map(|_| elapsed_secs(&step["started_at"], &step["completed_at"]));
            CheckStep::new(
                step["name"].as_str().unwrap_or("step"),
                status,
                duration,
                Vec::new(),
            )
        })
        .collect()
}

/// The last `count` lines of a job log, numbered, with timestamps stripped and errors
/// marked.
fn tail_log(text: &str, count: usize) -> Vec<LogLine> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(count);
    lines[start..]
        .iter()
        .enumerate()
        .map(|(ix, line)| {
            let line = line
                .split_once(' ')
                .filter(|(stamp, _)| stamp.len() >= 20 && stamp.as_bytes()[4] == b'-')
                .map(|(_, rest)| rest)
                .unwrap_or(line);
            let kind = if line.contains("##[error]")
                || line.contains("FAILED")
                || line.starts_with("error")
            {
                LogLineKind::Error
            } else {
                LogLineKind::Normal
            };
            let text = line
                .replace("##[error]", "")
                .replace("##[group]", "")
                .replace("##[endgroup]", "");
            LogLine::new((start + ix + 1) as u32, text, kind)
        })
        .collect()
}

impl PullRequestSource for GhSource {
    fn pull_requests(&self) -> Result<Vec<PullRequest>, ApiError> {
        let searches = [
            (
                "is:pr is:open review-requested:@me",
                Some(Inbox::ReviewRequested),
            ),
            ("is:pr is:open assignee:@me", Some(Inbox::AssignedToMe)),
            ("is:pr is:open author:@me", Some(Inbox::CreatedByMe)),
            ("is:pr is:open mentions:@me", Some(Inbox::Mentions)),
            ("is:pr is:open involves:@me", None),
            ("is:pr is:merged involves:@me sort:updated-desc", None),
            (
                "is:pr is:closed is:unmerged involves:@me sort:updated-desc",
                None,
            ),
        ];
        let results: Vec<Result<Value, ApiError>> = std::thread::scope(|scope| {
            let handles: Vec<_> = searches
                .iter()
                .map(|(query, inbox)| {
                    scope.spawn(move || {
                        let first = if inbox.is_none() && !query.contains("is:open") {
                            20
                        } else {
                            PER_INBOX
                        };
                        self.pull_request_query(
                            SEARCH_QUERY,
                            SUMMARY_EXTRAS,
                            json!({ "query": query, "first": first }),
                        )
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .unwrap_or_else(|_| Err(ApiError::new("A search failed.")))
                })
                .collect()
        });
        let mut by_key: HashMap<PullRequestKey, PullRequest> = HashMap::new();
        let mut order: Vec<PullRequestKey> = Vec::new();
        for ((_, inbox), data) in searches.iter().zip(results) {
            let inbox = *inbox;
            let data = data?;
            for node in data["search"]["nodes"]
                .as_array()
                .cloned()
                .unwrap_or_default()
            {
                let Some(pull_request) = summary_from(&node) else {
                    continue;
                };
                let entry = by_key.entry(pull_request.key.clone()).or_insert_with(|| {
                    order.push(pull_request.key.clone());
                    pull_request
                });
                if let Some(inbox) = inbox
                    && !entry.inboxes.contains(&inbox)
                {
                    entry.inboxes.push(inbox);
                }
            }
        }
        Ok(order
            .into_iter()
            .filter_map(|key| by_key.remove(&key))
            .collect())
    }

    fn detail(&self, key: &PullRequestKey) -> Result<PullRequestDetail, ApiError> {
        let (data, files) = std::thread::scope(|scope| {
            let files = scope.spawn(|| self.files(key));
            let data = self.pull_request_query(
                DETAIL_QUERY,
                DETAIL_EXTRAS,
                json!({
                    "owner": key.repo().owner(),
                    "name": key.repo().name(),
                    "number": key.number(),
                }),
            );
            (
                data,
                files
                    .join()
                    .unwrap_or_else(|_| Err(ApiError::new("Loading files failed."))),
            )
        });
        let data = data?;
        let viewer = data["viewer"]["login"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let node = &data["repository"]["pullRequest"];
        let summary = summary_from(node).ok_or_else(|| {
            ApiError::new(format!("{}#{} wasn’t found.", key.repo(), key.number()))
        })?;
        let mut detail = PullRequestDetail::new(
            summary.clone(),
            node["body"].as_str().unwrap_or_default(),
            timestamp(&node["createdAt"]),
        );
        detail.viewer = (!viewer.is_empty()).then(|| viewer.clone());
        detail.merge = merge_info(&data["repository"], node);
        detail.stack = stack_from(key.repo(), node);
        detail.commits = node["commits"]["nodes"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|commit| {
                let commit = &commit["commit"];
                Commit::new(
                    commit["oid"].as_str().unwrap_or_default(),
                    commit["messageHeadline"].as_str().unwrap_or_default(),
                    ci_status(&commit["statusCheckRollup"]),
                )
            })
            .collect();
        detail.files = files?;
        detail.threads = node["reviewThreads"]["nodes"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|thread| {
                let line = thread["line"]
                    .as_u64()
                    .or_else(|| thread["originalLine"].as_u64())? as u32;
                let comments: Vec<Comment> = thread["comments"]["nodes"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .map(|comment| {
                        Comment::new(
                            comment["databaseId"]
                                .as_u64()
                                .unwrap_or_default()
                                .to_string(),
                            person(&comment["author"]),
                            comment["body"].as_str().unwrap_or_default(),
                            timestamp(&comment["createdAt"]),
                        )
                    })
                    .collect();
                let mut review_thread = ReviewThread::new(
                    thread["id"].as_str().unwrap_or_default(),
                    thread["path"].as_str().unwrap_or_default(),
                    line,
                    comments,
                );
                review_thread.is_resolved = thread["isResolved"].as_bool().unwrap_or(false);
                Some(review_thread)
            })
            .collect();

        let mut reviewers: Vec<Reviewer> = node["latestReviews"]["nodes"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|review| {
                let state = match review["state"].as_str() {
                    Some("APPROVED") => ReviewerState::Approved,
                    Some("CHANGES_REQUESTED") => ReviewerState::ChangesRequested,
                    _ => ReviewerState::Commented,
                };
                let actor = person(&review["author"]);
                let reviewer = Reviewer::new(actor.clone(), state);
                if actor.login == viewer {
                    reviewer.viewer()
                } else {
                    reviewer
                }
            })
            .collect();
        for request in node["reviewRequests"]["nodes"]
            .as_array()
            .cloned()
            .unwrap_or_default()
        {
            let requested = person(&request["requestedReviewer"]);
            if reviewers.iter().any(|r| r.actor.login == requested.login) {
                continue;
            }
            let is_viewer = requested.login == viewer;
            let reviewer = Reviewer::new(requested, ReviewerState::Pending);
            reviewers.push(if is_viewer {
                reviewer.viewer()
            } else {
                reviewer
            });
        }
        detail.reviewers = reviewers;

        detail.timeline = vec![TimelineItem::Description];
        let mut pending_push: Option<(Actor, Vec<String>, Timestamp)> = None;
        let items = node["timelineItems"]["nodes"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for item in &items {
            if item["__typename"] != "PullRequestCommit"
                && let Some((actor, commits, at)) = pending_push.take()
            {
                detail
                    .timeline
                    .push(TimelineItem::Pushed { actor, commits, at });
            }
            match item["__typename"].as_str() {
                Some("PullRequestCommit") => {
                    let commit = &item["commit"];
                    let author = commit["author"]["user"]["login"]
                        .as_str()
                        .or_else(|| commit["author"]["name"].as_str())
                        .unwrap_or("someone")
                        .to_string();
                    let oid = commit["oid"].as_str().unwrap_or_default().to_string();
                    let at = timestamp(&commit["committedDate"]);
                    match &mut pending_push {
                        Some((actor, commits, last)) if actor.login == author => {
                            commits.push(oid);
                            *last = at;
                        }
                        _ => {
                            if let Some((actor, commits, at)) = pending_push.take() {
                                detail
                                    .timeline
                                    .push(TimelineItem::Pushed { actor, commits, at });
                            }
                            let name = commit["author"]["user"]["name"].as_str();
                            pending_push =
                                Some((Actor::person(author).with_name(name), vec![oid], at));
                        }
                    }
                }
                Some("IssueComment") => detail.timeline.push(TimelineItem::Commented {
                    comment: Comment::new(
                        item["databaseId"].as_u64().unwrap_or_default().to_string(),
                        person(&item["author"]),
                        item["body"].as_str().unwrap_or_default(),
                        timestamp(&item["createdAt"]),
                    ),
                }),
                Some("PullRequestReview") => {
                    let verdict = match item["state"].as_str() {
                        Some("APPROVED") => ReviewVerdict::Approve,
                        Some("CHANGES_REQUESTED") => ReviewVerdict::RequestChanges,
                        Some("COMMENTED") => ReviewVerdict::Comment,
                        _ => continue,
                    };
                    let at = timestamp(&item["submittedAt"]);
                    let actor = person(&item["author"]);
                    detail.timeline.push(TimelineItem::Reviewed {
                        actor: actor.clone(),
                        verdict,
                        at,
                    });
                    if let Some(body) = item["body"].as_str().filter(|body| !body.trim().is_empty())
                    {
                        detail.timeline.push(TimelineItem::Commented {
                            comment: Comment::new(format!("review-{}", at.unix()), actor, body, at),
                        });
                    }
                }
                Some("ReviewRequestedEvent") => {
                    let actor = person(&item["actor"]);
                    let reviewer = person(&item["requestedReviewer"]);
                    let at = timestamp(&item["createdAt"]);
                    match detail.timeline.last_mut() {
                        Some(TimelineItem::ReviewRequested {
                            actor: last,
                            reviewers,
                            ..
                        }) if last == &actor => {
                            if !reviewers.contains(&reviewer) {
                                reviewers.push(reviewer);
                            }
                        }
                        _ => detail.timeline.push(TimelineItem::ReviewRequested {
                            actor,
                            reviewers: vec![reviewer],
                            at,
                        }),
                    }
                }
                _ => {}
            }
        }
        if let Some((actor, commits, at)) = pending_push.take() {
            detail
                .timeline
                .push(TimelineItem::Pushed { actor, commits, at });
        }
        // GitHub has no per-push diffs, so the current diff of every file follows the
        // latest push.
        let after_last_push = detail
            .timeline
            .iter()
            .rposition(|item| matches!(item, TimelineItem::Pushed { .. }))
            .map(|ix| ix + 1)
            .unwrap_or(detail.timeline.len());
        let files: Vec<TimelineItem> = detail
            .files
            .iter()
            .map(|file| TimelineItem::File {
                path: file.path.clone(),
            })
            .collect();
        detail
            .timeline
            .splice(after_last_push..after_last_push, files);

        detail.depends_on = dependency_numbers(&detail.body)
            .into_iter()
            .map(|number| {
                Dependency::new(
                    PullRequestKey::new(key.repo().clone(), number),
                    format!("{}#{number}", key.repo().name()),
                    format!("Lands after #{number} is merged."),
                )
            })
            .collect();
        Ok(detail)
    }

    fn checks(&self, key: &PullRequestKey) -> Result<Vec<CheckRun>, ApiError> {
        self.head_checks(key)
    }

    fn check_log(&self, key: &PullRequestKey, check_id: &str) -> Result<Vec<LogLine>, ApiError> {
        let text = self.rest_text(&format!(
            "repos/{}/actions/jobs/{check_id}/logs",
            key.repo()
        ))?;
        Ok(tail_log(&text, LOG_TAIL))
    }

    fn submit_review(
        &self,
        key: &PullRequestKey,
        submission: &ReviewSubmission,
    ) -> Result<(), ApiError> {
        let event = match submission.verdict {
            ReviewVerdict::Approve => "APPROVE",
            ReviewVerdict::RequestChanges => "REQUEST_CHANGES",
            ReviewVerdict::Comment => "COMMENT",
        };
        for reply in &submission.replies {
            self.api(
                &[
                    "--method",
                    "POST",
                    &format!(
                        "repos/{}/pulls/{}/comments/{}/replies",
                        key.repo(),
                        key.number(),
                        reply.reply_to
                    ),
                ],
                Some(&json!({ "body": reply.body })),
            )?;
        }
        let comments: Vec<Value> = submission
            .new_comments
            .iter()
            .map(|comment| json!({ "path": comment.path, "line": comment.line, "side": "RIGHT", "body": comment.body }))
            .collect();
        let mut body = json!({ "event": event, "comments": comments });
        if !submission.summary.trim().is_empty() {
            body["body"] = json!(submission.summary);
        }
        self.api(
            &[
                "--method",
                "POST",
                &format!("repos/{}/pulls/{}/reviews", key.repo(), key.number()),
            ],
            Some(&body),
        )?;
        Ok(())
    }

    fn rerun_failed_jobs(&self, key: &PullRequestKey, check_id: &str) -> Result<(), ApiError> {
        let job = self.api(
            &[&format!("repos/{}/actions/jobs/{check_id}", key.repo())],
            None,
        )?;
        let run = job["run_id"].as_u64().ok_or_else(|| {
            ApiError::new("Only GitHub Actions jobs can be re-run from Switchfold.")
        })?;
        self.api(
            &[
                "--method",
                "POST",
                &format!("repos/{}/actions/runs/{run}/rerun-failed-jobs", key.repo()),
            ],
            None,
        )?;
        Ok(())
    }

    fn merge(
        &self,
        key: &PullRequestKey,
        method: MergeMethod,
        title: &str,
        message: &str,
        head_sha: &str,
    ) -> Result<(), ApiError> {
        self.api(
            &[
                "--method",
                "PUT",
                &format!("repos/{}/pulls/{}/merge", key.repo(), key.number()),
            ],
            Some(&merge_body(method, title, message, head_sha)),
        )?;
        Ok(())
    }

    fn set_auto_merge(&self, node_id: &str, method: Option<MergeMethod>) -> Result<(), ApiError> {
        match method {
            Some(method) => self.graphql(
                "mutation($id: ID!, $method: PullRequestMergeMethod!) { enablePullRequestAutoMerge(input: {pullRequestId: $id, mergeMethod: $method}) { clientMutationId } }",
                json!({ "id": node_id, "method": method.graphql_name() }),
            )?,
            None => self.graphql(
                "mutation($id: ID!) { disablePullRequestAutoMerge(input: {pullRequestId: $id}) { clientMutationId } }",
                json!({ "id": node_id }),
            )?,
        };
        Ok(())
    }

    fn update_branch(&self, node_id: &str, head_sha: &str, rebase: bool) -> Result<(), ApiError> {
        self.graphql(
            "mutation($id: ID!, $head: GitObjectID!, $method: PullRequestBranchUpdateMethod!) { updatePullRequestBranch(input: {pullRequestId: $id, expectedHeadOid: $head, updateMethod: $method}) { clientMutationId } }",
            json!({ "id": node_id, "head": head_sha, "method": if rebase { "REBASE" } else { "MERGE" } }),
        )?;
        Ok(())
    }

    fn merge_statuses(&self, keys: &[PullRequestKey]) -> Result<Vec<MergeStatus>, ApiError> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let (query, variables) = merge_status_query(keys);
        Ok(parse_merge_statuses(
            keys,
            &self.graphql(&query, variables)?,
        ))
    }

    fn set_open(&self, key: &PullRequestKey, open: bool) -> Result<(), ApiError> {
        self.api(
            &[
                "--method",
                "PATCH",
                &format!("repos/{}/pulls/{}", key.repo(), key.number()),
            ],
            Some(&json!({ "state": if open { "open" } else { "closed" } })),
        )?;
        Ok(())
    }

    fn request_reviewers(&self, key: &PullRequestKey, logins: &[String]) -> Result<(), ApiError> {
        self.api(
            &[
                "--method",
                "POST",
                &format!(
                    "repos/{}/pulls/{}/requested_reviewers",
                    key.repo(),
                    key.number()
                ),
            ],
            Some(&json!({ "reviewers": logins })),
        )?;
        Ok(())
    }

    fn commit_files(
        &self,
        repo: &RepoSlug,
        sha: &str,
    ) -> Result<Vec<diff_model::FileDiff>, ApiError> {
        let commit = self.api(&[&format!("repos/{repo}/commits/{sha}")], None)?;
        Ok(commit["files"]
            .as_array()
            .map(|files| files.iter().map(file_diff).collect())
            .unwrap_or_default())
    }

    fn assignable_users(&self, repo: &RepoSlug) -> Result<Vec<Actor>, ApiError> {
        let data = self.graphql(
            ASSIGNABLE_QUERY,
            json!({ "owner": repo.owner(), "name": repo.name() }),
        )?;
        Ok(data["repository"]["assignableUsers"]["nodes"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(person)
            .collect())
    }
}

const ASSIGNABLE_QUERY: &str = r#"
query($owner: String!, $name: String!) {
  repository(owner: $owner, name: $name) {
    assignableUsers(first: 100) { nodes { login name } }
  }
}"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_dependency_references() {
        assert_eq!(
            dependency_numbers("Closes #431, depends on #477"),
            vec![477]
        );
        assert_eq!(
            dependency_numbers("Blocked by #12 and lands after #13."),
            vec![13, 12]
        );
        assert!(dependency_numbers("depends on the cache").is_empty());
    }

    #[test]
    fn log_tails_drop_timestamps_and_mark_errors() {
        let log = "2026-10-06T10:00:00.0000000Z running\n2026-10-06T10:00:01.0000000Z ##[error]Process completed with exit code 1.";
        let lines = tail_log(log, 40);
        assert_eq!(lines[0].text, "running");
        assert_eq!(lines[1].kind, LogLineKind::Error);
        assert_eq!(lines[1].text, "Process completed with exit code 1.");
    }

    #[test]
    fn summaries_map_github_states() {
        let node = json!({
            "number": 7, "title": "T", "isDraft": true, "state": "OPEN",
            "updatedAt": "2026-10-06T10:00:01Z",
            "repository": { "nameWithOwner": "o/r" },
            "author": { "login": "a" }, "headRefName": "h", "baseRefName": "main",
            "reviewDecision": "CHANGES_REQUESTED",
            "comments": { "totalCount": 2 }, "reviewThreads": { "totalCount": 3 },
            "labels": { "nodes": [{ "name": "bug" }] },
            "milestone": { "title": "v1.0" },
            "commits": { "nodes": [{ "commit": { "statusCheckRollup": { "state": "FAILURE" } } }] }
        });
        let pr = summary_from(&node).unwrap();
        assert_eq!(pr.state, PullRequestState::Draft);
        assert_eq!(pr.review, ReviewDecision::ChangesRequested);
        assert_eq!(pr.ci, Some(CheckStatus::Failed));
        assert_eq!(pr.comments, 5);
        assert!(pr.has_label("bug"));
        assert_eq!(pr.milestone.as_deref(), Some("v1.0"));
    }

    #[test]
    fn a_summary_reads_its_stack_position_and_queue_state() {
        let node = json!({
            "number": 579, "title": "t", "repository": { "nameWithOwner": "o/r" },
            "stack": { "number": 580, "size": 2 },
            "stackEntry": { "position": 2 },
            "mergeQueueEntry": { "state": "AWAITING_CHECKS" }
        });
        let pr = summary_from(&node).unwrap();
        assert_eq!(pr.stack, Some(StackPosition::new(580, 2, 2)));
        assert_eq!(pr.merge_queue, Some(MergeQueueState::AwaitingChecks));
    }

    #[test]
    fn a_summary_outside_a_stack_and_queue_has_neither() {
        let node = json!({
            "number": 7, "title": "t", "repository": { "nameWithOwner": "o/r" },
            "stack": null, "stackEntry": null, "mergeQueueEntry": null
        });
        let pr = summary_from(&node).unwrap();
        assert_eq!(pr.stack, None);
        assert_eq!(pr.merge_queue, None);
        let without_the_fields =
            json!({ "number": 7, "title": "t", "repository": { "nameWithOwner": "o/r" } });
        assert_eq!(summary_from(&without_the_fields).unwrap().stack, None);
    }

    #[test]
    fn a_stack_lists_its_members_in_merge_order_and_skips_unreadable_ones() {
        let repo = RepoSlug::new("o", "r");
        let node = json!({ "stack": {
            "number": 577, "size": 3, "baseRefName": "main",
            "entries": { "nodes": [
                { "position": 3, "pullRequest": { "number": 576, "title": "c", "state": "OPEN", "isDraft": true } },
                { "position": 1, "pullRequest": { "number": 574, "title": "a", "state": "MERGED", "isDraft": false } },
                { "position": 2, "pullRequest": null }
            ] }
        } });
        let stack = stack_from(&repo, &node).unwrap();
        assert_eq!(stack.number, 577);
        assert_eq!(stack.base_ref, "main");
        let members: Vec<_> = stack
            .entries
            .iter()
            .map(|entry| (entry.position, entry.key.number(), entry.state))
            .collect();
        assert_eq!(
            members,
            [
                (1, 574, PullRequestState::Merged),
                (3, 576, PullRequestState::Draft)
            ]
        );
        assert_eq!(stack_from(&repo, &json!({ "stack": null })), None);
    }

    #[test]
    fn only_a_missing_pull_request_field_drops_the_newer_fields() {
        assert!(lacks_pull_request_field(
            "GitHub: Field 'stack' doesn't exist on type 'PullRequest'"
        ));
        assert!(lacks_pull_request_field(
            "GitHub: Field 'mergeQueueEntry' doesn't exist on type 'PullRequest'"
        ));
        assert!(!lacks_pull_request_field(
            "GitHub: Field 'viewerDefaultMergeMethod' doesn't exist on type 'Repository'"
        ));
        assert!(!lacks_pull_request_field("GitHub: API rate limit exceeded"));
        for (template, fields) in [
            (SEARCH_QUERY, SUMMARY_EXTRAS),
            (DETAIL_QUERY, DETAIL_EXTRAS),
        ] {
            assert!(template.replace(NEWER_FIELDS, fields).contains(fields));
            let without = template.replace(NEWER_FIELDS, "");
            assert!(!without.contains("stack") && !without.contains("mergeQueueEntry"));
        }
    }

    #[test]
    fn a_summary_without_a_milestone_has_none() {
        let node = json!({
            "number": 7, "title": "T", "state": "OPEN",
            "repository": { "nameWithOwner": "o/r" },
            "milestone": null
        });
        assert_eq!(summary_from(&node).unwrap().milestone, None);
    }

    fn queued(number: u64) -> PullRequestKey {
        PullRequestKey::new(RepoSlug::new("acme", "app"), number)
    }

    /// One `pN` alias of the merge status query, with `contexts` as its head's checks.
    fn alias(state: &str, review: &str, contexts: Value) -> Value {
        json!({
            "squashMergeAllowed": true, "viewerDefaultMergeMethod": "SQUASH",
            "pullRequest": {
                "id": "N1", "state": state, "isDraft": false, "headRefOid": "h1",
                "mergeStateStatus": "BLOCKED", "reviewDecision": review,
                "viewerCanUpdateBranch": true,
                "commits": { "nodes": [{ "commit": { "statusCheckRollup": {
                    "contexts": { "nodes": contexts }
                } } }] }
            }
        })
    }

    fn check(conclusion: Option<&str>, required: bool) -> Value {
        json!({
            "__typename": "CheckRun",
            "status": if conclusion.is_some() { "COMPLETED" } else { "IN_PROGRESS" },
            "conclusion": conclusion, "isRequired": required
        })
    }

    fn statuses(alias: Value) -> Vec<review_model::MergeStatus> {
        parse_merge_statuses(&[queued(1)], &json!({ "p0": alias }))
    }

    #[test]
    fn failed_required_check_fails() {
        let data = alias("OPEN", "APPROVED", json!([check(Some("FAILURE"), true)]));
        assert_eq!(statuses(data)[0].required_checks, CheckStatus::Failed);
    }

    #[test]
    fn failed_optional_check_is_ignored() {
        let contexts = json!([check(Some("FAILURE"), false), check(Some("SUCCESS"), true)]);
        let data = alias("OPEN", "APPROVED", contexts);
        assert_eq!(statuses(data)[0].required_checks, CheckStatus::Passed);
    }

    #[test]
    fn failed_required_status_context_fails() {
        let contexts =
            json!([{ "__typename": "StatusContext", "state": "ERROR", "isRequired": true }]);
        let data = alias("OPEN", "APPROVED", contexts);
        assert_eq!(statuses(data)[0].required_checks, CheckStatus::Failed);
    }

    #[test]
    fn running_required_check_runs() {
        let data = alias("OPEN", "APPROVED", json!([check(None, true)]));
        assert_eq!(statuses(data)[0].required_checks, CheckStatus::Running);
    }

    #[test]
    fn merged_state_is_read() {
        let data = alias("MERGED", "APPROVED", json!([]));
        assert_eq!(statuses(data)[0].state, PullRequestState::Merged);
    }

    #[test]
    fn review_required_needs_review() {
        let data = alias("OPEN", "REVIEW_REQUIRED", json!([]));
        assert!(statuses(data)[0].needs_review);
    }

    #[test]
    fn merge_info_comes_from_the_alias() {
        let data = alias("OPEN", "APPROVED", json!([]));
        let status = &statuses(data)[0];
        assert_eq!(status.merge.head_sha, "h1");
        assert_eq!(status.merge.state, MergeState::Blocked);
        assert_eq!(status.merge.default_method, Some(MergeMethod::Squash));
    }

    #[test]
    fn missing_alias_is_left_out() {
        let data = json!({
            "p0": alias("OPEN", "APPROVED", json!([])),
            "p1": { "pullRequest": null }
        });
        let found = parse_merge_statuses(&[queued(1), queued(2)], &data);
        assert_eq!(
            found
                .iter()
                .map(|status| status.key.clone())
                .collect::<Vec<_>>(),
            [queued(1)]
        );
    }

    #[test]
    fn merge_without_a_title_keeps_githubs_default() {
        assert_eq!(
            merge_body(MergeMethod::Squash, "", "", "h1"),
            json!({ "merge_method": "squash", "sha": "h1" })
        );
    }

    #[test]
    fn merge_with_a_title_sends_it() {
        assert_eq!(
            merge_body(MergeMethod::Squash, "T", "M", "h1"),
            json!({ "merge_method": "squash", "sha": "h1", "commit_title": "T", "commit_message": "M" })
        );
    }

    #[test]
    fn query_keeps_names_out_of_the_text() {
        let (query, variables) = merge_status_query(&[queued(7)]);
        assert!(!query.contains("acme"));
        assert_eq!(variables, json!({ "o0": "acme", "n0": "app", "p0": 7 }));
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// Reads the signed-in `gh` account's inboxes and one detail, timing both. Run with
    /// `cargo test -p github_api -- --ignored live --nocapture`; set `PR=owner/name#N`
    /// to time that pull request's detail instead.
    #[test]
    #[ignore = "talks to GitHub with the gh CLI's credentials"]
    fn live_inboxes_and_detail() {
        let source = GhSource::new("github.com");
        if let Some(key) = std::env::var("PR").ok().and_then(|pr| {
            let (repo, number) = pr.split_once('#')?;
            Some(PullRequestKey::new(
                RepoSlug::parse(repo)?,
                number.parse().ok()?,
            ))
        }) {
            let started = std::time::Instant::now();
            let detail = source.detail(&key).expect("detail loads");
            eprintln!(
                "{}: {} files in {:.1?}",
                key.id(),
                detail.files.len(),
                started.elapsed()
            );
            let started = std::time::Instant::now();
            let checks = source.checks(&key).expect("checks load");
            eprintln!("{} checks in {:.1?}", checks.len(), started.elapsed());
            return;
        }
        let started = std::time::Instant::now();
        let pull_requests = source.pull_requests().expect("inboxes load");
        eprintln!("inboxes in {:.1?}", started.elapsed());
        let requested: Vec<_> = pull_requests
            .iter()
            .filter(|pr| pr.inboxes.contains(&Inbox::ReviewRequested))
            .collect();
        eprintln!(
            "{} pull requests, {} review requested",
            pull_requests.len(),
            requested.len()
        );
        let first = requested
            .first()
            .copied()
            .or(pull_requests.first())
            .expect("at least one pull request");
        let started = std::time::Instant::now();
        let detail = source.detail(&first.key).expect("detail loads");
        eprintln!("detail in {:.1?}", started.elapsed());
        eprintln!(
            "{}: {} files, {} threads, {} checks, {} timeline items, {} reviewers",
            first.key.id(),
            detail.files.len(),
            detail.threads.len(),
            detail.checks.len(),
            detail.timeline.len(),
            detail.reviewers.len()
        );
        assert!(!detail.files.is_empty());
    }

    /// Asks github.com for a pull request field it doesn't have, as an older GitHub
    /// Enterprise Server would be asked for `stack`: the rows still load, and the source
    /// stops asking.
    #[test]
    #[ignore = "talks to GitHub with the gh CLI's credentials"]
    fn live_unknown_field_falls_back() {
        let source = GhSource::new("github.com");
        let variables = json!({ "query": "repo:github/gh-stack is:pr is:merged", "first": 3 });
        let data = source
            .pull_request_query(SEARCH_QUERY, "fieldNoHostHas", variables.clone())
            .expect("the search runs without the unknown field");
        let rows = data["search"]["nodes"].as_array().expect("nodes");
        assert!(rows.iter().filter_map(summary_from).count() > 0);
        assert!(source.without_newer_fields.load(Ordering::Relaxed));
        let data = source
            .pull_request_query(SEARCH_QUERY, SUMMARY_EXTRAS, variables)
            .expect("later queries run");
        assert!(
            data["search"]["nodes"][0].get("stack").is_none(),
            "later queries leave the newer fields out"
        );
    }

    /// Reads stack 580 of `github/gh-stack`, two merged pull requests, through the list
    /// query and the detail query.
    #[test]
    #[ignore = "talks to GitHub with the gh CLI's credentials"]
    fn live_stack() {
        let source = GhSource::new("github.com");
        let repo = RepoSlug::new("github", "gh-stack");
        let data = source
            .pull_request_query(
                SEARCH_QUERY,
                SUMMARY_EXTRAS,
                json!({
                    "query": "repo:github/gh-stack is:pr is:merged in:title Reduce redundant Git and worktree lookups",
                    "first": 5,
                }),
            )
            .expect("the search runs");
        let summaries: Vec<PullRequest> = data["search"]["nodes"]
            .as_array()
            .expect("the search has nodes")
            .iter()
            .filter_map(summary_from)
            .collect();
        let summary = summaries
            .iter()
            .find(|pr| pr.key.number() == 579)
            .expect("#579 is found");
        assert_eq!(summary.stack, Some(StackPosition::new(580, 2, 2)));
        assert!(!source.without_newer_fields.load(Ordering::Relaxed));

        let detail = source
            .detail(&PullRequestKey::new(repo, 579))
            .expect("detail loads");
        let stack = detail.stack.expect("#579 is in a stack");
        assert_eq!(stack.number, 580);
        assert_eq!(stack.base_ref, "main");
        let members: Vec<_> = stack
            .entries
            .iter()
            .map(|entry| (entry.position, entry.key.number(), entry.state))
            .collect();
        assert_eq!(
            members,
            [
                (1, 578, PullRequestState::Merged),
                (2, 579, PullRequestState::Merged)
            ]
        );
    }
}

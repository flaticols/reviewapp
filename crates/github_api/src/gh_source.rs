//! A [`PullRequestSource`] backed by GitHub through the `gh` CLI, which supplies the
//! signed-in user's token. Every call runs `gh api` and blocks.

use std::collections::HashMap;

use diff_model::FileChange;
use pr_model::{
    CheckStatus, Inbox, PullRequest, PullRequestKey, PullRequestState, RepoSlug, ReviewDecision,
    Timestamp,
};
use review_model::{
    Actor, CheckRun, CheckStep, Comment, Commit, Dependency, LogLine, LogLineKind,
    PullRequestDetail, ReviewThread, ReviewVerdict, Reviewer, ReviewerState, TimelineItem,
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
}

impl GhSource {
    /// A source for `host`, normally `github.com`.
    pub fn new(host: impl Into<String>) -> Self {
        Self { host: host.into() }
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
        commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }
      }
    }
  }
}"#;

const DETAIL_QUERY: &str = r#"
query($owner: String!, $name: String!, $number: Int!) {
  viewer { login }
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      number title body isDraft state updatedAt createdAt
      author { login ... on User { name } }
      headRefName baseRefName reviewDecision
      repository { nameWithOwner }
      comments { totalCount }
      labels(first: 10) { nodes { name } }
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
    pull_request.state = match (node["state"].as_str(), node["isDraft"].as_bool()) {
        (Some("MERGED"), _) => PullRequestState::Merged,
        (Some("CLOSED"), _) => PullRequestState::Closed,
        (_, Some(true)) => PullRequestState::Draft,
        _ => PullRequestState::Open,
    };
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
    fn checks(&self, repo: &RepoSlug, sha: &str) -> Vec<CheckRun> {
        let Ok(response) = self.api(
            &[&format!(
                "repos/{repo}/commits/{sha}/check-runs?per_page=100"
            )],
            None,
        ) else {
            return Vec::new();
        };
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
        let failed_jobs: Vec<u64> = runs
            .iter()
            .filter(|run| {
                run["status"].as_str() == Some("completed")
                    && matches!(run["conclusion"].as_str(), Some("failure" | "timed_out"))
            })
            .filter_map(|run| run["id"].as_u64())
            .filter(|id| steps_by_job.contains_key(id))
            .collect();
        let logs: HashMap<u64, Vec<LogLine>> = std::thread::scope(|scope| {
            let handles: Vec<_> = failed_jobs
                .iter()
                .map(|job| {
                    scope.spawn(move || {
                        let log = self
                            .rest_text(&format!("repos/{repo}/actions/jobs/{job}/logs"))
                            .ok()
                            .map(|text| tail_log(&text, LOG_TAIL));
                        (*job, log)
                    })
                })
                .collect();
            handles
                .into_iter()
                .filter_map(|handle| handle.join().ok())
                .filter_map(|(job, log)| Some((job, log?)))
                .collect()
        });
        runs.iter()
            .map(|run| {
                let status = check_status(run["status"].as_str(), run["conclusion"].as_str());
                let elapsed = elapsed_secs(&run["started_at"], &run["completed_at"]);
                let id = run["id"].as_u64().unwrap_or_default();
                let steps = steps_by_job
                    .get(&id)
                    .map(|steps| job_steps(steps, logs.get(&id).cloned()))
                    .unwrap_or_default();
                CheckRun::new(
                    id.to_string(),
                    run["name"].as_str().unwrap_or("check"),
                    status,
                    elapsed,
                    sha,
                    steps,
                )
                .with_url(run["html_url"].as_str().unwrap_or_default())
            })
            .collect()
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

/// A job's steps; the job's log tail goes to its first failed step.
fn job_steps(steps: &[Value], mut log: Option<Vec<LogLine>>) -> Vec<CheckStep> {
    steps
        .iter()
        .map(|step| {
            let status = check_status(step["status"].as_str(), step["conclusion"].as_str());
            let duration = step["completed_at"]
                .as_str()
                .map(|_| elapsed_secs(&step["started_at"], &step["completed_at"]));
            let lines = if status == CheckStatus::Failed {
                log.take().unwrap_or_default()
            } else {
                Vec::new()
            };
            CheckStep::new(
                step["name"].as_str().unwrap_or("step"),
                status,
                duration,
                lines,
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
                        self.graphql(SEARCH_QUERY, json!({ "query": query, "first": first }))
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
            let data = self.graphql(
                DETAIL_QUERY,
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

        if let Some(head) = detail.commits.last() {
            detail.checks = self.checks(key.repo(), &head.sha);
            let failing_or_running = detail
                .checks
                .iter()
                .filter(|check| matches!(check.status, CheckStatus::Failed | CheckStatus::Running));
            let ids: Vec<String> = failing_or_running.map(|check| check.id.clone()).collect();
            detail
                .timeline
                .extend(ids.into_iter().map(|id| TimelineItem::Check { id }));
        }

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
            "commits": { "nodes": [{ "commit": { "statusCheckRollup": { "state": "FAILURE" } } }] }
        });
        let pr = summary_from(&node).unwrap();
        assert_eq!(pr.state, PullRequestState::Draft);
        assert_eq!(pr.review, ReviewDecision::ChangesRequested);
        assert_eq!(pr.ci, Some(CheckStatus::Failed));
        assert_eq!(pr.comments, 5);
        assert!(pr.has_label("bug"));
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
                "{}: {} files, {} checks in {:.1?}",
                key.id(),
                detail.files.len(),
                detail.checks.len(),
                started.elapsed()
            );
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
}

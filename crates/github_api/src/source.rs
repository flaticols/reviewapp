use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use pr_model::{PullRequest, PullRequestKey, RepoSlug};
use review_model::{Actor, CheckRun, MergeMethod, PullRequestDetail, ReviewVerdict};

/// A failed GitHub request, with a message suitable for the interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiError(String);

impl ApiError {
    /// An error carrying `message`.
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ApiError {}

/// A new line comment posted with a review.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DraftComment {
    pub path: String,
    /// Line on the head side.
    pub line: u32,
    pub body: String,
}

impl DraftComment {
    /// A comment on `path` at head-side `line`.
    pub fn new(path: impl Into<String>, line: u32, body: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            line,
            body: body.into(),
        }
    }
}

/// A reply to an existing review thread.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Reply {
    /// The id of the thread's first comment.
    pub reply_to: String,
    pub body: String,
}

impl Reply {
    /// A reply to the thread whose first comment is `reply_to`.
    pub fn new(reply_to: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            reply_to: reply_to.into(),
            body: body.into(),
        }
    }
}

/// A review the viewer submits, with the pending comments and findings it posts.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ReviewSubmission {
    pub verdict: ReviewVerdict,
    pub summary: String,
    pub new_comments: Vec<DraftComment>,
    pub replies: Vec<Reply>,
}

impl ReviewSubmission {
    /// Builds a submission.
    pub fn new(
        verdict: ReviewVerdict,
        summary: impl Into<String>,
        new_comments: Vec<DraftComment>,
        replies: Vec<Reply>,
    ) -> Self {
        Self {
            verdict,
            summary: summary.into(),
            new_comments,
            replies,
        }
    }
}

/// Where pull requests come from. Implementations block; call them off the UI thread.
pub trait PullRequestSource: Send + Sync {
    /// Every pull request in the viewer's inboxes.
    fn pull_requests(&self) -> Result<Vec<PullRequest>, ApiError>;

    /// The full review data for one pull request.
    fn detail(&self, key: &PullRequestKey) -> Result<PullRequestDetail, ApiError>;

    /// Posts a review.
    fn submit_review(
        &self,
        key: &PullRequestKey,
        submission: &ReviewSubmission,
    ) -> Result<(), ApiError>;

    /// Re-runs the failed jobs of a check run.
    fn rerun_failed_jobs(&self, key: &PullRequestKey, check_id: &str) -> Result<(), ApiError>;

    /// Closes the pull request, or reopens it when `open`.
    fn set_open(&self, key: &PullRequestKey, open: bool) -> Result<(), ApiError>;

    /// Merges `key` with `method`, as long as its head is still `head_sha`. Title and
    /// message are the commit's, for methods that write one.
    fn merge(
        &self,
        key: &PullRequestKey,
        method: MergeMethod,
        title: &str,
        message: &str,
        head_sha: &str,
    ) -> Result<(), ApiError>;

    /// Turns auto-merge on with `method`, or off with `None`, for the pull request with
    /// GraphQL id `node_id`.
    fn set_auto_merge(&self, node_id: &str, method: Option<MergeMethod>) -> Result<(), ApiError>;

    /// Brings the pull request's branch up to date with its base, by merging the base in
    /// or rebasing onto it, as long as its head is still `head_sha`.
    fn update_branch(&self, node_id: &str, head_sha: &str, rebase: bool) -> Result<(), ApiError>;

    /// Asks `logins` to review the pull request.
    fn request_reviewers(&self, key: &PullRequestKey, logins: &[String]) -> Result<(), ApiError>;

    /// People who can be asked to review in `repo`, with their profile names.
    fn assignable_users(&self, repo: &RepoSlug) -> Result<Vec<Actor>, ApiError>;

    /// The changes one commit makes, against its parent.
    fn commit_files(
        &self,
        repo: &RepoSlug,
        sha: &str,
    ) -> Result<Vec<diff_model::FileDiff>, ApiError>;
}

/// A check that finishes some time after it is first served or re-run.
struct CheckProgression {
    key: PullRequestKey,
    finished: CheckRun,
    after: Duration,
    started: Instant,
}

/// Serves the sample workspace from memory. Submissions are recorded, not posted, and
/// running checks can be scripted to finish after a delay.
pub struct FixtureSource {
    pull_requests: Vec<PullRequest>,
    details: HashMap<PullRequestKey, PullRequestDetail>,
    submitted: Mutex<Vec<(PullRequestKey, ReviewSubmission)>>,
    progressions: Mutex<Vec<CheckProgression>>,
    /// Closes, reopens and review requests, as `close #482`, `request #482 tanya`.
    actions: Mutex<Vec<String>>,
}

impl FixtureSource {
    /// A source serving `pull_requests` and their `details`.
    pub fn new(pull_requests: Vec<PullRequest>, details: Vec<PullRequestDetail>) -> Self {
        Self {
            pull_requests,
            details: details
                .into_iter()
                .map(|detail| (detail.summary.key.clone(), detail))
                .collect(),
            submitted: Mutex::new(Vec::new()),
            progressions: Mutex::new(Vec::new()),
            actions: Mutex::new(Vec::new()),
        }
    }

    /// Serves `finished` in place of the check with the same id once `after` has passed
    /// since construction or since its last re-run.
    pub fn with_check_progression(
        self,
        key: PullRequestKey,
        finished: CheckRun,
        after: Duration,
    ) -> Self {
        if let Ok(mut progressions) = self.progressions.lock() {
            progressions.push(CheckProgression {
                key,
                finished,
                after,
                started: Instant::now(),
            });
        }
        self
    }

    /// Closes, reopens and review requests so far, oldest first.
    pub fn actions(&self) -> Vec<String> {
        self.actions
            .lock()
            .map(|actions| actions.clone())
            .unwrap_or_default()
    }

    fn record(&self, action: String) -> Result<(), ApiError> {
        self.actions
            .lock()
            .map_err(|_| ApiError::new("Couldn’t record the change."))?
            .push(action);
        Ok(())
    }

    /// Reviews submitted so far, oldest first.
    pub fn submitted(&self) -> Vec<(PullRequestKey, ReviewSubmission)> {
        self.submitted
            .lock()
            .map(|submitted| submitted.clone())
            .unwrap_or_default()
    }
}

impl PullRequestSource for FixtureSource {
    fn pull_requests(&self) -> Result<Vec<PullRequest>, ApiError> {
        Ok(self.pull_requests.clone())
    }

    fn detail(&self, key: &PullRequestKey) -> Result<PullRequestDetail, ApiError> {
        let mut detail = self.details.get(key).cloned().ok_or_else(|| {
            ApiError::new(format!("{}#{} wasn’t found.", key.repo(), key.number()))
        })?;
        let progressions = self
            .progressions
            .lock()
            .map_err(|_| ApiError::new("Couldn’t read check progress."))?;
        for progression in progressions.iter().filter(|p| &p.key == key) {
            let Some(check) = detail
                .checks
                .iter_mut()
                .find(|check| check.id == progression.finished.id)
            else {
                continue;
            };
            let elapsed = progression.started.elapsed();
            if elapsed >= progression.after {
                *check = progression.finished.clone();
            } else {
                check.elapsed_secs += elapsed.as_secs() as u32;
            }
        }
        Ok(detail)
    }

    fn submit_review(
        &self,
        key: &PullRequestKey,
        submission: &ReviewSubmission,
    ) -> Result<(), ApiError> {
        self.submitted
            .lock()
            .map_err(|_| ApiError::new("Couldn’t record the review."))?
            .push((key.clone(), submission.clone()));
        Ok(())
    }

    fn rerun_failed_jobs(&self, key: &PullRequestKey, check_id: &str) -> Result<(), ApiError> {
        let detail = self.detail(key)?;
        detail
            .check(check_id)
            .ok_or_else(|| ApiError::new(format!("Check {check_id} wasn’t found.")))?;
        if let Ok(mut progressions) = self.progressions.lock() {
            for progression in progressions
                .iter_mut()
                .filter(|p| &p.key == key && p.finished.id == check_id)
            {
                progression.started = Instant::now();
            }
        }
        Ok(())
    }

    fn merge(
        &self,
        key: &PullRequestKey,
        method: MergeMethod,
        _title: &str,
        _message: &str,
        _head_sha: &str,
    ) -> Result<(), ApiError> {
        self.record(format!("merge {key:?} {}", method.api_name()))
    }

    fn set_auto_merge(&self, node_id: &str, method: Option<MergeMethod>) -> Result<(), ApiError> {
        self.record(format!(
            "auto-merge {node_id} {}",
            method.map_or("off", MergeMethod::api_name)
        ))
    }

    fn update_branch(&self, node_id: &str, _head_sha: &str, rebase: bool) -> Result<(), ApiError> {
        self.record(format!(
            "update-branch {node_id} {}",
            if rebase { "rebase" } else { "merge" }
        ))
    }

    fn set_open(&self, key: &PullRequestKey, open: bool) -> Result<(), ApiError> {
        let verb = if open { "reopen" } else { "close" };
        self.record(format!("{verb} #{}", key.number()))
    }

    fn request_reviewers(&self, key: &PullRequestKey, logins: &[String]) -> Result<(), ApiError> {
        self.record(format!("request #{} {}", key.number(), logins.join(" ")))
    }

    fn commit_files(
        &self,
        repo: &RepoSlug,
        sha: &str,
    ) -> Result<Vec<diff_model::FileDiff>, ApiError> {
        // The sample has no history of its own: a commit shows the files of the pull
        // request it belongs to, minus the last, so the view visibly changes.
        let detail = self
            .details
            .values()
            .find(|detail| {
                detail.summary.key.repo() == repo
                    && detail.commits.iter().any(|commit| commit.sha == sha)
            })
            .ok_or_else(|| ApiError::new(format!("Commit {sha} wasn’t found.")))?;
        let mut files = detail.files.clone();
        if files.len() > 1 {
            files.pop();
        }
        Ok(files)
    }

    fn assignable_users(&self, repo: &RepoSlug) -> Result<Vec<Actor>, ApiError> {
        let mut people: Vec<Actor> = self
            .details
            .values()
            .filter(|detail| detail.summary.key.repo() == repo)
            .flat_map(|detail| detail.reviewers.iter().map(|r| r.actor.clone()))
            .filter(|actor| !actor.is_agent)
            .collect();
        people.sort_by(|a, b| a.login.cmp(&b.login));
        people.dedup_by(|a, b| a.login == b.login);
        Ok(people)
    }
}

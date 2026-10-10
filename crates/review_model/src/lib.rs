//! Everything the review screen shows about one pull request: conversation, review
//! threads, agent findings, reviewers, checks and the timeline. No UI or I/O.

use diff_model::FileDiff;
use pr_model::{CheckStatus, PullRequest, PullRequestKey, Timestamp};

/// Who wrote something: a person or an agent acting over ACP.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Actor {
    /// The GitHub handle, or the agent's name.
    pub login: String,
    /// The person's full name from their GitHub profile, when they set one.
    pub name: Option<String>,
    pub is_agent: bool,
}

impl Actor {
    /// A GitHub user.
    pub fn person(login: impl Into<String>) -> Self {
        Self {
            login: login.into(),
            name: None,
            is_agent: false,
        }
    }

    /// An agent such as Claude Code.
    pub fn agent(name: impl Into<String>) -> Self {
        Self {
            login: name.into(),
            name: None,
            is_agent: true,
        }
    }

    /// Adds the person's full name. A blank name, or one equal to the handle, is ignored.
    pub fn with_name(mut self, name: Option<impl Into<String>>) -> Self {
        self.name = name
            .map(Into::into)
            .map(|name: String| name.trim().to_string())
            .filter(|name| !name.is_empty() && *name != self.login);
        self
    }

    /// The full name when known, else the handle.
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.login)
    }

    /// Up to two uppercase initials, as avatars show them: `denis` → `DE`,
    /// `mark-v` → `MV`, `Claude Code` → `CC`.
    pub fn initials(&self) -> String {
        initials(&self.login)
    }
}

/// Up to two uppercase initials of a name, splitting on spaces, dashes and underscores.
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name
        .split([' ', '-', '_'])
        .filter(|word| !word.is_empty())
        .collect();
    let letters: String = match words.as_slice() {
        [] => String::new(),
        [single] => single.chars().take(2).collect(),
        [first, second, ..] => first
            .chars()
            .take(1)
            .chain(second.chars().take(1))
            .collect(),
    };
    letters.to_uppercase()
}

/// One comment in a conversation or review thread.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[non_exhaustive]
pub struct Comment {
    pub id: String,
    pub author: Actor,
    pub body: String,
    pub created_at: Timestamp,
    /// Set when the comment belongs to the viewer's pending review and is not posted yet.
    pub is_pending: bool,
}

impl Comment {
    /// Builds a posted comment.
    pub fn new(
        id: impl Into<String>,
        author: Actor,
        body: impl Into<String>,
        created_at: Timestamp,
    ) -> Self {
        Self {
            id: id.into(),
            author,
            body: body.into(),
            created_at,
            is_pending: false,
        }
    }

    /// Marks the comment as part of the viewer's pending review.
    pub fn pending(mut self) -> Self {
        self.is_pending = true;
        self
    }
}

/// A conversation anchored to a line of a file in the diff.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[non_exhaustive]
pub struct ReviewThread {
    pub id: String,
    pub path: String,
    /// Line on the head side the thread is anchored to.
    pub line: u32,
    pub comments: Vec<Comment>,
    pub is_resolved: bool,
}

impl ReviewThread {
    /// Builds an unresolved thread.
    pub fn new(
        id: impl Into<String>,
        path: impl Into<String>,
        line: u32,
        comments: Vec<Comment>,
    ) -> Self {
        Self {
            id: id.into(),
            path: path.into(),
            line,
            comments,
            is_resolved: false,
        }
    }

    /// The author of the first comment.
    pub fn starter(&self) -> Option<&Actor> {
        self.comments.first().map(|comment| &comment.author)
    }

    /// `thread.rs:142`, the anchor as the outline shows it.
    pub fn anchor_label(&self) -> String {
        let file = self.path.rsplit('/').next().unwrap_or(&self.path);
        format!("{file}:{}", self.line)
    }
}

/// Where an agent's finding stands in the viewer's review.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindingState {
    /// Proposed by the agent; nothing is posted.
    Draft,
    /// Added to the viewer's pending review; posts when the review is submitted.
    Added,
    /// Dismissed by the viewer.
    Dismissed,
}

/// A problem an agent found while reviewing.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[non_exhaustive]
pub struct AgentFinding {
    pub id: String,
    pub agent: Actor,
    pub path: String,
    pub line: u32,
    pub body: String,
    pub state: FindingState,
}

impl AgentFinding {
    /// Builds a draft finding.
    pub fn new(
        id: impl Into<String>,
        agent: Actor,
        path: impl Into<String>,
        line: u32,
        body: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            agent,
            path: path.into(),
            line,
            body: body.into(),
            state: FindingState::Draft,
        }
    }

    /// `hunk.rs:31`, the anchor as findings show it.
    pub fn anchor_label(&self) -> String {
        let file = self.path.rsplit('/').next().unwrap_or(&self.path);
        format!("{file}:{}", self.line)
    }
}

/// A reviewer's verdict on the pull request.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewVerdict {
    Comment,
    Approve,
    RequestChanges,
}

impl ReviewVerdict {
    /// Every verdict in the order the submit dialog lists them.
    pub const ALL: [ReviewVerdict; 3] = [Self::Comment, Self::Approve, Self::RequestChanges];

    /// The option title in the submit dialog.
    pub fn title(self) -> &'static str {
        match self {
            Self::Comment => "Comment",
            Self::Approve => "Approve",
            Self::RequestChanges => "Request changes",
        }
    }

    /// The option description in the submit dialog.
    pub fn description(self) -> &'static str {
        match self {
            Self::Comment => "Post your comments without approving or blocking.",
            Self::Approve => "Post your comments and approve merging these changes.",
            Self::RequestChanges => {
                "Post your comments and block merging until they are addressed."
            }
        }
    }

    /// What the timeline says after the reviewer's name.
    pub fn timeline_text(self) -> &'static str {
        match self {
            Self::Comment => "reviewed these changes",
            Self::Approve => "approved these changes",
            Self::RequestChanges => "requested changes",
        }
    }
}

/// Where a requested reviewer stands.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewerState {
    Approved,
    Commented,
    ChangesRequested,
    /// An agent review is running.
    Reviewing,
    /// The reviewer has not reviewed yet.
    Pending,
}

/// A reviewer listed in the inspector.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[non_exhaustive]
pub struct Reviewer {
    pub actor: Actor,
    pub state: ReviewerState,
    pub is_viewer: bool,
}

impl Reviewer {
    /// Builds a reviewer entry.
    pub fn new(actor: Actor, state: ReviewerState) -> Self {
        Self {
            actor,
            state,
            is_viewer: false,
        }
    }

    /// Marks the entry as the signed-in user.
    pub fn viewer(mut self) -> Self {
        self.is_viewer = true;
        self
    }
}

/// How a line of check output should read.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLineKind {
    Normal,
    Error,
    Added,
    Removed,
}

/// One numbered line of a check step's log.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[non_exhaustive]
pub struct LogLine {
    pub number: u32,
    pub text: String,
    pub kind: LogLineKind,
}

impl LogLine {
    /// Builds a log line.
    pub fn new(number: u32, text: impl Into<String>, kind: LogLineKind) -> Self {
        Self {
            number,
            text: text.into(),
            kind,
        }
    }
}

/// One step of a CI job.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[non_exhaustive]
pub struct CheckStep {
    pub name: String,
    pub status: CheckStatus,
    pub duration_secs: Option<u32>,
    pub log: Vec<LogLine>,
}

impl CheckStep {
    /// Builds a step.
    pub fn new(
        name: impl Into<String>,
        status: CheckStatus,
        duration_secs: Option<u32>,
        log: Vec<LogLine>,
    ) -> Self {
        Self {
            name: name.into(),
            status,
            duration_secs,
            log,
        }
    }
}

/// One CI job reported on the head commit.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[non_exhaustive]
pub struct CheckRun {
    pub id: String,
    pub name: String,
    pub status: CheckStatus,
    pub elapsed_secs: u32,
    pub head_sha: String,
    pub url: String,
    pub steps: Vec<CheckStep>,
}

impl CheckRun {
    /// Builds a check run.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        status: CheckStatus,
        elapsed_secs: u32,
        head_sha: impl Into<String>,
        steps: Vec<CheckStep>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            status,
            elapsed_secs,
            head_sha: head_sha.into(),
            url: String::default(),
            steps,
        }
    }

    /// Sets the GitHub URL of the run.
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = url.into();
        self
    }
}

/// A commit in the pull request.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[non_exhaustive]
pub struct Commit {
    pub sha: String,
    pub message: String,
    pub ci: Option<CheckStatus>,
}

impl Commit {
    /// Builds a commit.
    pub fn new(
        sha: impl Into<String>,
        message: impl Into<String>,
        ci: Option<CheckStatus>,
    ) -> Self {
        Self {
            sha: sha.into(),
            message: message.into(),
            ci,
        }
    }

    /// The first seven characters of the hash.
    pub fn short_sha(&self) -> &str {
        &self.sha[..self.sha.len().min(7)]
    }
}

/// A pull request this one depends on.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[non_exhaustive]
pub struct Dependency {
    pub key: PullRequestKey,
    pub title: String,
    pub note: String,
}

impl Dependency {
    /// Builds a dependency entry.
    pub fn new(key: PullRequestKey, title: impl Into<String>, note: impl Into<String>) -> Self {
        Self {
            key,
            title: title.into(),
            note: note.into(),
        }
    }
}

/// A summary of the most recent agent review.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[non_exhaustive]
pub struct AgentReviewSummary {
    pub agent: Actor,
    pub files: u32,
    pub findings: u32,
    pub added: u32,
    pub finished_at: Timestamp,
}

impl AgentReviewSummary {
    /// Builds a summary.
    pub fn new(
        agent: Actor,
        files: u32,
        findings: u32,
        added: u32,
        finished_at: Timestamp,
    ) -> Self {
        Self {
            agent,
            files,
            findings,
            added,
            finished_at,
        }
    }
}

/// One entry of the single-scroll review timeline, in the order things happened.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum TimelineItem {
    /// The pull request description, written by the author when opening it.
    Description,
    ReviewRequested {
        actor: Actor,
        reviewers: Vec<Actor>,
        at: Timestamp,
    },
    Pushed {
        actor: Actor,
        commits: Vec<String>,
        at: Timestamp,
    },
    /// The diff of one file, by path.
    File {
        path: String,
    },
    AgentReviewed(AgentReviewSummary),
    /// An agent finding, by id.
    Finding {
        id: String,
    },
    Reviewed {
        actor: Actor,
        verdict: ReviewVerdict,
        at: Timestamp,
    },
    /// A check run on the head commit, by id.
    Check {
        id: String,
    },
    Commented {
        comment: Comment,
    },
}

/// How a pull request's commits land on its base branch.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

impl MergeMethod {
    /// Every method in GitHub's menu order.
    pub const ALL: [MergeMethod; 3] = [Self::Merge, Self::Squash, Self::Rebase];

    /// The button label, as GitHub words it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Merge => "Create a merge commit",
            Self::Squash => "Squash and merge",
            Self::Rebase => "Rebase and merge",
        }
    }

    /// The REST API's `merge_method`.
    pub fn api_name(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
        }
    }

    /// The GraphQL `PullRequestMergeMethod`.
    pub fn graphql_name(self) -> &'static str {
        match self {
            Self::Merge => "MERGE",
            Self::Squash => "SQUASH",
            Self::Rebase => "REBASE",
        }
    }

    /// The method for a GraphQL `PullRequestMergeMethod`.
    pub fn from_graphql(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|method| method.graphql_name() == name)
    }

    /// Whether the reviewer writes a commit title and message for this method.
    pub fn has_message(self) -> bool {
        !matches!(self, Self::Rebase)
    }
}

/// Whether a pull request can merge now, as GitHub's `mergeStateStatus` says.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MergeState {
    /// Mergeable, and every requirement is met.
    Clean,
    /// Mergeable, but some checks are failing.
    Unstable,
    /// Mergeable, with pre-receive hooks to run.
    HasHooks,
    /// Behind the base branch, which the repository requires to be current.
    Behind,
    /// Required reviews or checks are missing.
    Blocked,
    /// Has conflicts with the base branch.
    Dirty,
    Draft,
    /// GitHub is still working it out.
    #[default]
    Unknown,
}

impl MergeState {
    /// The state for a GraphQL `MergeStateStatus`.
    pub fn from_graphql(name: &str) -> Self {
        match name {
            "CLEAN" => Self::Clean,
            "UNSTABLE" => Self::Unstable,
            "HAS_HOOKS" => Self::HasHooks,
            "BEHIND" => Self::Behind,
            "BLOCKED" => Self::Blocked,
            "DIRTY" => Self::Dirty,
            "DRAFT" => Self::Draft,
            _ => Self::Unknown,
        }
    }

    /// Whether merging now would go through.
    pub fn can_merge(self) -> bool {
        matches!(self, Self::Clean | Self::Unstable | Self::HasHooks)
    }
}

/// Auto-merge waiting for a pull request's requirements.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct AutoMerge {
    pub method: MergeMethod,
    pub enabled_by: String,
}

impl AutoMerge {
    /// Auto-merge with `method`, turned on by `enabled_by`.
    pub fn new(method: MergeMethod, enabled_by: impl Into<String>) -> Self {
        Self {
            method,
            enabled_by: enabled_by.into(),
        }
    }
}

/// What merging a pull request involves: whether it can merge, how, and what the
/// viewer may do about it.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MergeInfo {
    /// The pull request's GraphQL id, which the auto-merge and update mutations take.
    pub node_id: String,
    /// The head commit merging expects, so a push in between makes it fail.
    pub head_sha: String,
    pub state: MergeState,
    /// The methods the repository allows, in menu order.
    pub methods: Vec<MergeMethod>,
    /// The method GitHub suggests to this viewer.
    pub default_method: Option<MergeMethod>,
    pub auto_merge: Option<AutoMerge>,
    pub can_enable_auto_merge: bool,
    pub can_disable_auto_merge: bool,
    pub can_update_branch: bool,
    /// The repository deletes head branches after merging.
    pub deletes_branch: bool,
    /// The head branch lives in a fork, which Switchfold can't push to.
    pub from_fork: bool,
}

impl MergeInfo {
    /// Merge facts for head `head_sha`, in `state`, allowing `methods`.
    pub fn new(
        node_id: impl Into<String>,
        head_sha: impl Into<String>,
        state: MergeState,
        methods: Vec<MergeMethod>,
    ) -> Self {
        let default_method = methods.first().copied();
        Self {
            node_id: node_id.into(),
            head_sha: head_sha.into(),
            state,
            methods,
            default_method,
            auto_merge: None,
            can_enable_auto_merge: false,
            can_disable_auto_merge: false,
            can_update_branch: false,
            deletes_branch: false,
            from_fork: false,
        }
    }
}

/// Everything the review screen shows about one pull request.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[non_exhaustive]
pub struct PullRequestDetail {
    pub summary: PullRequest,
    pub body: String,
    pub opened_at: Timestamp,
    pub commits: Vec<Commit>,
    pub files: Vec<FileDiff>,
    pub threads: Vec<ReviewThread>,
    pub findings: Vec<AgentFinding>,
    pub timeline: Vec<TimelineItem>,
    pub reviewers: Vec<Reviewer>,
    pub checks: Vec<CheckRun>,
    pub depends_on: Vec<Dependency>,
    pub last_agent_review: Option<AgentReviewSummary>,
    /// Paths the viewer marked as viewed.
    pub viewed: Vec<String>,
    /// How the pull request merges, when the source knows.
    #[serde(default)]
    pub merge: Option<MergeInfo>,
    /// The signed-in viewer's login, when the source knows.
    #[serde(default)]
    pub viewer: Option<String>,
}

impl PullRequestDetail {
    /// Builds a detail with only a summary and description; the rest starts empty.
    pub fn new(summary: PullRequest, body: impl Into<String>, opened_at: Timestamp) -> Self {
        Self {
            summary,
            body: body.into(),
            opened_at,
            commits: Vec::new(),
            files: Vec::new(),
            threads: Vec::new(),
            findings: Vec::new(),
            timeline: vec![TimelineItem::Description],
            reviewers: Vec::new(),
            checks: Vec::new(),
            depends_on: Vec::new(),
            last_agent_review: None,
            viewed: Vec::new(),
            merge: None,
            viewer: None,
        }
    }

    /// The file diff at `path`.
    pub fn file(&self, path: &str) -> Option<&FileDiff> {
        self.files.iter().find(|file| file.path.as_str() == path)
    }

    /// The check run with `id`.
    pub fn check(&self, id: &str) -> Option<&CheckRun> {
        self.checks.iter().find(|check| check.id.as_str() == id)
    }

    /// The finding with `id`.
    pub fn finding(&self, id: &str) -> Option<&AgentFinding> {
        self.findings
            .iter()
            .find(|finding| finding.id.as_str() == id)
    }

    /// Whether the viewer marked `path` as viewed.
    pub fn is_viewed(&self, path: &str) -> bool {
        self.viewed.iter().any(|viewed| viewed.as_str() == path)
    }

    /// Threads that are still open.
    pub fn unresolved_threads(&self) -> impl Iterator<Item = &ReviewThread> {
        self.threads.iter().filter(|thread| !thread.is_resolved)
    }

    /// Checks that passed, out of all checks.
    pub fn passed_checks(&self) -> usize {
        self.checks
            .iter()
            .filter(|check| check.status == CheckStatus::Passed)
            .count()
    }

    /// `denis wants to merge 4 commits into main from feat/thread-view`.
    pub fn merge_summary(&self) -> String {
        let count = self.commits.len();
        let noun = if count == 1 { "commit" } else { "commits" };
        format!(
            "{} wants to merge {count} {noun} into {} from {}",
            self.summary.author, self.summary.base_ref, self.summary.head_ref
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_take_one_letter_per_word_or_two_of_one_word() {
        assert_eq!(initials("mark-v").as_str(), "MV");
        assert_eq!(initials("Claude Code").as_str(), "CC");
        assert_eq!(initials("tanya").as_str(), "TA");
        assert_eq!(initials("denis-work").as_str(), "DW");
    }
}

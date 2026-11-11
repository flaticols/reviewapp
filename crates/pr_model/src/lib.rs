//! Pull request summaries as lists show them: repositories, keys, states, review
//! decisions, CI status, inboxes and saved filters. No UI or I/O.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// A GitHub repository, `owner/name`.
#[derive(
    serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
pub struct RepoSlug {
    owner: String,
    name: String,
}

impl RepoSlug {
    /// Builds a slug from its owner and repository name.
    pub fn new(owner: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            name: name.into(),
        }
    }

    /// Parses `owner/name`; returns `None` for anything else.
    pub fn parse(text: &str) -> Option<Self> {
        let (owner, name) = text.trim().split_once('/')?;
        let name = name.trim_end_matches(".git");
        if owner.is_empty() || name.is_empty() || name.contains('/') {
            return None;
        }
        Some(Self::new(owner.to_string(), name.to_string()))
    }

    /// The account or organisation that owns the repository.
    pub fn owner(&self) -> &String {
        &self.owner
    }

    /// The repository name without its owner.
    pub fn name(&self) -> &String {
        &self.name
    }
}

impl fmt::Display for RepoSlug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

/// Identifies one pull request across repositories.
#[derive(
    serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
pub struct PullRequestKey {
    repo: RepoSlug,
    number: u64,
}

impl PullRequestKey {
    /// Builds a key from a repository and pull request number.
    pub fn new(repo: RepoSlug, number: u64) -> Self {
        Self { repo, number }
    }

    /// The repository the pull request belongs to.
    pub fn repo(&self) -> &RepoSlug {
        &self.repo
    }

    /// The pull request number within its repository.
    pub fn number(&self) -> u64 {
        self.number
    }

    /// A string unique across repositories, suitable for element identity.
    pub fn id(&self) -> String {
        format!("{}#{}", self.repo, self.number)
    }
}

/// Lifecycle state of a pull request.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PullRequestState {
    Open,
    Draft,
    Merged,
    Closed,
}

impl PullRequestState {
    /// Every state in the order the list's state tabs show them.
    pub const ALL: [PullRequestState; 4] = [Self::Open, Self::Draft, Self::Merged, Self::Closed];

    /// The label used for the state.
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Draft => "Draft",
            Self::Merged => "Merged",
            Self::Closed => "Closed",
        }
    }
}

/// The aggregate review outcome GitHub shows for a pull request.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReviewDecision {
    ReviewRequested,
    ChangesRequested,
    Approved,
    Commented,
    NoReviews,
}

impl ReviewDecision {
    /// Every decision in filter-menu order.
    pub const ALL: [ReviewDecision; 5] = [
        Self::ReviewRequested,
        Self::ChangesRequested,
        Self::Approved,
        Self::Commented,
        Self::NoReviews,
    ];

    /// The label used in lists and filters.
    pub fn label(self) -> &'static str {
        match self {
            Self::ReviewRequested => "Review requested",
            Self::ChangesRequested => "Changes requested",
            Self::Approved => "Approved",
            Self::Commented => "Commented",
            Self::NoReviews => "No reviews",
        }
    }
}

/// Outcome of a CI check, or of all checks combined.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CheckStatus {
    Passed,
    Failed,
    Running,
    Queued,
    Skipped,
}

impl CheckStatus {
    /// The label used in filters and accessible names.
    pub fn label(self) -> &'static str {
        match self {
            Self::Passed => "Passing",
            Self::Failed => "Failing",
            Self::Running => "Running",
            Self::Queued => "Queued",
            Self::Skipped => "Skipped",
        }
    }

    /// Whether the check has stopped running.
    pub fn is_finished(self) -> bool {
        matches!(self, Self::Passed | Self::Failed | Self::Skipped)
    }
}

/// A personal inbox the sidebar groups pull requests into.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Inbox {
    ReviewRequested,
    AssignedToMe,
    CreatedByMe,
    Mentions,
    AllOpen,
}

impl Inbox {
    /// Every inbox in sidebar order.
    pub const ALL: [Inbox; 5] = [
        Self::ReviewRequested,
        Self::AssignedToMe,
        Self::CreatedByMe,
        Self::Mentions,
        Self::AllOpen,
    ];

    /// The sidebar and header label.
    pub fn label(self) -> &'static str {
        match self {
            Self::ReviewRequested => "Review requested",
            Self::AssignedToMe => "Assigned to me",
            Self::CreatedByMe => "Created by me",
            Self::Mentions => "Mentions",
            Self::AllOpen => "All open",
        }
    }

    /// A stable key for element identity and persistence.
    pub fn key(self) -> &'static str {
        match self {
            Self::ReviewRequested => "review-requested",
            Self::AssignedToMe => "assigned",
            Self::CreatedByMe => "created",
            Self::Mentions => "mentions",
            Self::AllOpen => "all-open",
        }
    }
}

/// A pull request as it appears in lists.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[non_exhaustive]
pub struct PullRequest {
    pub key: PullRequestKey,
    pub title: String,
    pub author: String,
    pub head_ref: String,
    pub base_ref: String,
    pub state: PullRequestState,
    pub review: ReviewDecision,
    pub ci: Option<CheckStatus>,
    pub comments: u32,
    pub updated_at: Timestamp,
    pub labels: Vec<String>,
    pub inboxes: Vec<Inbox>,
    pub agent_reviewed: bool,
    /// The start of the description, for triage in the list.
    #[serde(default)]
    pub body_excerpt: String,
    #[serde(default)]
    pub changed_files: u32,
    #[serde(default)]
    pub additions: u32,
    #[serde(default)]
    pub deletions: u32,
    /// The first changed paths.
    #[serde(default)]
    pub paths: Vec<String>,
}

impl PullRequest {
    /// Builds an open pull request with no review, checks, comments or labels; set the
    /// remaining public fields directly.
    pub fn new(
        key: PullRequestKey,
        title: impl Into<String>,
        author: impl Into<String>,
        head_ref: impl Into<String>,
        base_ref: impl Into<String>,
        updated_at: Timestamp,
    ) -> Self {
        Self {
            key,
            title: title.into(),
            author: author.into(),
            head_ref: head_ref.into(),
            base_ref: base_ref.into(),
            state: PullRequestState::Open,
            review: ReviewDecision::NoReviews,
            ci: None,
            comments: 0,
            updated_at,
            labels: Vec::new(),
            inboxes: Vec::new(),
            agent_reviewed: false,
            body_excerpt: String::new(),
            changed_files: 0,
            additions: 0,
            deletions: 0,
            paths: Vec::new(),
        }
    }

    /// Whether the pull request belongs to `inbox`. Every open or draft pull request
    /// is in [`Inbox::AllOpen`].
    pub fn is_in(&self, inbox: Inbox) -> bool {
        match inbox {
            Inbox::AllOpen => {
                matches!(self.state, PullRequestState::Open | PullRequestState::Draft)
            }
            _ => self.inboxes.contains(&inbox),
        }
    }

    /// Whether the pull request carries `label`.
    pub fn has_label(&self, label: &str) -> bool {
        self.labels.iter().any(|candidate| candidate == label)
    }
}

/// A saved combination of filters shown in the sidebar.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[non_exhaustive]
pub struct SavedFilter {
    pub id: String,
    pub name: String,
    pub rule: SavedFilterRule,
}

/// What a saved filter matches.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub enum SavedFilterRule {
    /// Open pull requests whose CI fails and that an agent reviewed.
    FailingCiAgentReviewed,
    /// Open pull requests whose CI fails.
    FailingCi,
    /// Open pull requests carrying a label.
    Label(String),
}

impl SavedFilter {
    /// Builds a saved filter.
    pub fn new(id: impl Into<String>, name: impl Into<String>, rule: SavedFilterRule) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            rule,
        }
    }

    /// Whether `pull_request` matches the filter.
    pub fn matches(&self, pull_request: &PullRequest) -> bool {
        if !pull_request.is_in(Inbox::AllOpen) {
            return false;
        }
        match &self.rule {
            SavedFilterRule::FailingCiAgentReviewed => {
                pull_request.ci == Some(CheckStatus::Failed) && pull_request.agent_reviewed
            }
            SavedFilterRule::FailingCi => pull_request.ci == Some(CheckStatus::Failed),
            SavedFilterRule::Label(label) => pull_request.has_label(label),
        }
    }
}

/// Seconds since the Unix epoch.
#[derive(
    serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub struct Timestamp(i64);

impl Timestamp {
    /// Builds a timestamp from seconds since the Unix epoch.
    pub const fn from_unix(seconds: i64) -> Self {
        Self(seconds)
    }

    /// The current wall-clock time.
    pub fn now() -> Self {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs() as i64)
            .unwrap_or_default();
        Self(seconds)
    }

    /// A timestamp `seconds` before `self`.
    pub fn minus(self, seconds: i64) -> Self {
        Self(self.0 - seconds)
    }

    /// Seconds since the Unix epoch.
    pub fn unix(self) -> i64 {
        self.0
    }

    /// Compact age used in lists, such as `2h` or `4d`.
    pub fn short_age(self, now: Timestamp) -> String {
        let seconds = (now.0 - self.0).max(0);
        match seconds {
            0..60 => "now".into(),
            60..3_600 => format!("{}m", seconds / 60),
            3_600..86_400 => format!("{}h", seconds / 3_600),
            _ => format!("{}d", seconds / 86_400),
        }
    }

    /// Age used in prose, such as `2h ago`, `40m ago` or `2 days ago`.
    pub fn long_age(self, now: Timestamp) -> String {
        let seconds = (now.0 - self.0).max(0);
        match seconds {
            0..60 => "just now".into(),
            60..3_600 => format!("{}m ago", seconds / 60),
            3_600..86_400 => format!("{}h ago", seconds / 3_600),
            86_400..172_800 => "yesterday".into(),
            _ => format!("{} days ago", seconds / 86_400),
        }
    }

    /// Age used after "last run" or "Fetched", such as `12 min ago`.
    pub fn minutes_age(self, now: Timestamp) -> String {
        let seconds = (now.0 - self.0).max(0);
        match seconds {
            0..60 => "just now".into(),
            60..3_600 => format!("{} min ago", seconds / 60),
            _ => self.long_age(now),
        }
    }
}

/// Formats a duration the way checks report it: `19s`, `1m 02s`.
pub fn format_duration(seconds: u32) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_round_down_to_the_largest_unit() {
        let now = Timestamp::from_unix(1_000_000);
        assert_eq!(now.minus(2 * 3_600 + 59).short_age(now), "2h");
        assert_eq!(now.minus(4 * 86_400).short_age(now), "4d");
        assert_eq!(now.minus(40 * 60).long_age(now), "40m ago");
        assert_eq!(now.minus(2 * 86_400).long_age(now), "2 days ago");
        assert_eq!(now.minus(12 * 60).minutes_age(now), "12 min ago");
    }

    #[test]
    fn durations_pad_seconds_after_a_minute() {
        assert_eq!(format_duration(19), "19s");
        assert_eq!(format_duration(62), "1m 02s");
    }
}

//! Sample data reproducing the Sketch design's content, so the running app can be
//! compared with the design frame by frame. Ages are relative to the `now` passed in.

mod markdown;
mod patches;
mod triage;

pub use markdown::KITCHEN_SINK;
pub use triage::SampleTriage;

use std::path::Path;

use diff_model::FileChange;
use git_diff::parse_patch;
use pr_model::{
    CheckStatus, Inbox, PullRequest, PullRequestKey, PullRequestState, RepoSlug, ReviewDecision,
    SavedFilter, SavedFilterRule, Timestamp,
};
use review_model::{
    Actor, AgentFinding, AgentReviewSummary, CheckRun, CheckStep, Comment, Commit, Dependency,
    FindingState, LogLine, LogLineKind, PullRequestDetail, ReviewThread, ReviewVerdict, Reviewer,
    ReviewerState, TimelineItem,
};
use settings_model::{
    Account, AccountSource, CloneStatus, Preferences, RepositoryMapping, SettingsFile,
    default_agents,
};

const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

/// The id of the check that is still running in the design and fails a little later.
pub const SNAPSHOT_CHECK_ID: &str = "ci-snapshot";

/// Everything the sample workspace contains.
#[non_exhaustive]
pub struct SampleData {
    pub viewer: Account,
    pub pull_requests: Vec<PullRequest>,
    pub details: Vec<PullRequestDetail>,
    pub saved_filters: Vec<SavedFilter>,
    pub settings: SettingsFile,
    /// Clone status per `owner/name`, as the design shows it.
    pub clone_statuses: Vec<(String, CloneStatus)>,
}

/// Builds the sample workspace with ages relative to `now` and paths under `home`.
pub fn sample(now: Timestamp, home: &Path) -> SampleData {
    let pull_requests = pull_requests(now);
    let details = pull_requests
        .iter()
        .map(|pull_request| detail(pull_request, now))
        .collect();
    let viewer =
        Account::new("flaticols", "github.com", AccountSource::GhCli).with_name("Denis Panfilov");
    SampleData {
        settings: SettingsFile::new(
            vec![
                viewer.clone(),
                Account::new(
                    "denis-work",
                    "ghe.example-corp.com",
                    AccountSource::Token {
                        expires_in_days: Some(9),
                    },
                ),
            ],
            vec![
                RepositoryMapping::new(&switchfold(), Some(home.join("src/switchfold"))),
                RepositoryMapping::new(&gathering_app(), Some(home.join("src/gathering-app"))),
                RepositoryMapping::new(&zed_extensions(), None),
                RepositoryMapping::new(&acp_playground(), Some(home.join("old/acp-playground"))),
            ],
            default_agents(Some(home)),
            Preferences::default(),
        ),
        clone_statuses: vec![
            (
                switchfold().to_string(),
                CloneStatus::Fetched {
                    at: now.minus(2 * MINUTE),
                    worktrees: 3,
                },
            ),
            (gathering_app().to_string(), CloneStatus::Fetching),
            (zed_extensions().to_string(), CloneStatus::NotCloned),
            (acp_playground().to_string(), CloneStatus::FolderNotFound),
        ],
        viewer,
        pull_requests,
        details,
        saved_filters: vec![
            SavedFilter::new(
                "failing-ci-agent-reviewed",
                "Failing CI, agent reviewed",
                SavedFilterRule::FailingCiAgentReviewed,
            ),
            SavedFilter::new(
                "release-blockers",
                "Release blockers",
                SavedFilterRule::Label("release-blocker".into()),
            ),
        ],
    }
}

fn switchfold() -> RepoSlug {
    RepoSlug::new("flaticols", "switchfold")
}

fn gathering_app() -> RepoSlug {
    RepoSlug::new("flaticols", "gathering-app")
}

fn zed_extensions() -> RepoSlug {
    RepoSlug::new("flaticols", "zed-extensions")
}

fn acp_playground() -> RepoSlug {
    RepoSlug::new("tanya", "acp-playground")
}

struct Row {
    repo: RepoSlug,
    number: u64,
    title: &'static str,
    author: &'static str,
    head: &'static str,
    state: PullRequestState,
    review: ReviewDecision,
    ci: Option<CheckStatus>,
    comments: u32,
    age: i64,
    labels: &'static [&'static str],
    inboxes: &'static [Inbox],
    agent_reviewed: bool,
}

impl Row {
    fn into_pull_request(self, now: Timestamp) -> PullRequest {
        let mut pull_request = PullRequest::new(
            PullRequestKey::new(self.repo, self.number),
            self.title,
            self.author,
            self.head,
            "main",
            now.minus(self.age),
        );
        pull_request.state = self.state;
        pull_request.review = self.review;
        pull_request.ci = self.ci;
        pull_request.comments = self.comments;
        pull_request.labels = self.labels.iter().map(|label| label.to_string()).collect();
        pull_request.inboxes = self.inboxes.to_vec();
        pull_request.agent_reviewed = self.agent_reviewed;
        pull_request
    }
}

fn pull_requests(now: Timestamp) -> Vec<PullRequest> {
    use CheckStatus::*;
    use Inbox::*;
    use PullRequestState::*;
    use ReviewDecision::*;

    let featured = [
        Row {
            repo: switchfold(),
            number: 482,
            title: "Render review thread as a single scroll with inline diffs",
            author: "denis",
            head: "feat/thread-view",
            state: Open,
            review: ReviewDecision::ReviewRequested,
            ci: Some(Passed),
            comments: 8,
            age: 2 * HOUR,
            labels: &["release-blocker"],
            inboxes: &[Inbox::ReviewRequested, CreatedByMe],
            agent_reviewed: true,
        },
        Row {
            repo: switchfold(),
            number: 479,
            title: "ACP: stream agent findings as draft comments",
            author: "tanya",
            head: "feat/acp-drafts",
            state: Open,
            review: ChangesRequested,
            ci: Some(Running),
            comments: 14,
            age: 5 * HOUR,
            labels: &["release-blocker"],
            inboxes: &[Inbox::ReviewRequested, AssignedToMe, Mentions],
            agent_reviewed: true,
        },
        Row {
            repo: gathering_app(),
            number: 121,
            title: "Sync company calendar over CalDAV",
            author: "mark-v",
            head: "feat/caldav",
            state: Open,
            review: ReviewDecision::ReviewRequested,
            ci: Some(Failed),
            comments: 3,
            age: DAY + 2 * HOUR,
            labels: &[],
            inboxes: &[Inbox::ReviewRequested, AssignedToMe],
            agent_reviewed: true,
        },
        Row {
            repo: switchfold(),
            number: 477,
            title: "Map GitHub repositories to local worktrees",
            author: "denis",
            head: "feat/repo-mapping",
            state: Draft,
            review: NoReviews,
            ci: Some(Passed),
            comments: 0,
            age: DAY + 5 * HOUR,
            labels: &["release-blocker"],
            inboxes: &[Inbox::ReviewRequested, CreatedByMe],
            agent_reviewed: false,
        },
        Row {
            repo: gathering_app(),
            number: 118,
            title: "Bump gpui-kit to 0.9 and migrate Sidebar",
            author: "renovate[bot]",
            head: "deps/gpui-kit",
            state: Open,
            review: Approved,
            ci: Some(Passed),
            comments: 1,
            age: 2 * DAY,
            labels: &["dependencies"],
            inboxes: &[Inbox::ReviewRequested],
            agent_reviewed: false,
        },
        Row {
            repo: switchfold(),
            number: 475,
            title: "Settings: add personal access token flow",
            author: "tanya",
            head: "feat/pat-auth",
            state: Open,
            review: ReviewDecision::ReviewRequested,
            ci: Some(Passed),
            comments: 5,
            age: 2 * DAY + 3 * HOUR,
            labels: &["release-blocker"],
            inboxes: &[Inbox::ReviewRequested, Mentions],
            agent_reviewed: false,
        },
        Row {
            repo: zed_extensions(),
            number: 42,
            title: "Highlight .ron files with tree-sitter-ron",
            author: "kai",
            head: "feat/ron",
            state: Open,
            review: Commented,
            ci: Some(Passed),
            comments: 2,
            age: 3 * DAY,
            labels: &[],
            inboxes: &[Inbox::ReviewRequested],
            agent_reviewed: false,
        },
        Row {
            repo: switchfold(),
            number: 470,
            title: "Fix hunk expansion off-by-one at end of file",
            author: "denis",
            head: "fix/hunk-eof",
            state: Open,
            review: ChangesRequested,
            ci: Some(Failed),
            comments: 6,
            age: 4 * DAY,
            labels: &["release-blocker"],
            inboxes: &[Inbox::ReviewRequested, AssignedToMe, CreatedByMe],
            agent_reviewed: true,
        },
        Row {
            repo: switchfold(),
            number: 466,
            title: "Show check logs in a sheet",
            author: "tanya",
            head: "feat/check-sheet",
            state: Merged,
            review: Approved,
            ci: Some(Passed),
            comments: 4,
            age: 6 * DAY,
            labels: &[],
            inboxes: &[Inbox::ReviewRequested],
            agent_reviewed: false,
        },
        Row {
            repo: gathering_app(),
            number: 112,
            title: "Invite guests by email",
            author: "mark-v",
            head: "feat/invites",
            state: Merged,
            review: Approved,
            ci: Some(Passed),
            comments: 9,
            age: 8 * DAY,
            labels: &[],
            inboxes: &[Inbox::ReviewRequested],
            agent_reviewed: false,
        },
        Row {
            repo: switchfold(),
            number: 459,
            title: "Experiment: tabbed conversation view",
            author: "denis",
            head: "spike/tabs",
            state: Closed,
            review: Commented,
            ci: None,
            comments: 3,
            age: 12 * DAY,
            labels: &[],
            inboxes: &[Inbox::ReviewRequested, CreatedByMe],
            agent_reviewed: false,
        },
    ];

    let mut pull_requests: Vec<PullRequest> = featured
        .into_iter()
        .map(|row| row.into_pull_request(now))
        .collect();
    pull_requests.extend(backlog(now));
    pull_requests
}

/// Older open pull requests that fill the All open inbox and repository counts.
fn backlog(now: Timestamp) -> Vec<PullRequest> {
    const SWITCHFOLD: [&str; 16] = [
        "Persist sidebar width per window",
        "Keyboard navigation in the file outline",
        "Collapse resolved threads by default",
        "Cache syntax highlighting per blob",
        "Open in Zed at the commented line",
        "Show force-push markers in the timeline",
        "Retry GitHub sync with backoff",
        "Notification when a review is requested",
        "Split diff mode for wide windows",
        "Mark files viewed with the space bar",
        "Group check runs by workflow",
        "Render suggested changes as diffs",
        "Copy permalink to a line",
        "Show co-authors on commits",
        "Respect .gitattributes linguist-generated",
        "Expand context above a hunk",
    ];
    const GATHERING_APP: [&str; 6] = [
        "Recurring events with exceptions",
        "Time zone picker in event editor",
        "Export attendees as CSV",
        "Room booking conflicts",
        "Dark mode for the agenda",
        "Reminders before an event starts",
    ];
    const ACP_PLAYGROUND: [&str; 6] = [
        "Stream tool calls into the transcript",
        "Session resume after restart",
        "MCP server per session",
        "Plan updates as a checklist",
        "Cancel a prompt mid-turn",
        "Terminal output in tool calls",
    ];
    const AUTHORS: [&str; 4] = ["tanya", "mark-v", "kai", "denis"];

    let groups: [(RepoSlug, &[&str], u64); 3] = [
        (switchfold(), &SWITCHFOLD, 440),
        (gathering_app(), &GATHERING_APP, 90),
        (acp_playground(), &ACP_PLAYGROUND, 20),
    ];
    let mut pull_requests = Vec::new();
    for (repo, titles, first_number) in groups {
        for (ix, title) in titles.iter().enumerate() {
            let ix64 = ix as i64;
            let ci = match ix % 5 {
                0 => Some(CheckStatus::Failed),
                3 => None,
                _ => Some(CheckStatus::Passed),
            };
            let review = match ix % 4 {
                0 => ReviewDecision::NoReviews,
                1 => ReviewDecision::Commented,
                2 => ReviewDecision::ReviewRequested,
                _ => ReviewDecision::Approved,
            };
            let labels = if ix == 2 || ix == 9 {
                vec!["release-blocker".to_string()]
            } else {
                Vec::new()
            };
            let mut pull_request = PullRequest::new(
                PullRequestKey::new(repo.clone(), first_number - ix as u64),
                *title,
                AUTHORS[ix % AUTHORS.len()],
                format!("feat/{}", slug(title)),
                "main",
                now.minus(5 * DAY + ix64 * 19 * HOUR),
            );
            pull_request.review = review;
            pull_request.ci = ci;
            pull_request.comments = (ix as u32 * 3) % 11;
            pull_request.labels = labels;
            if ix == 4 {
                pull_request.inboxes = vec![Inbox::AssignedToMe];
            }
            pull_request.agent_reviewed = ix == 5;
            pull_requests.push(pull_request);
        }
    }
    pull_requests
}

fn slug(title: &str) -> String {
    title
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join("-")
}

fn detail(pull_request: &PullRequest, now: Timestamp) -> PullRequestDetail {
    if pull_request.key == PullRequestKey::new(switchfold(), 482) {
        return thread_view_detail(pull_request.clone(), now);
    }
    generic_detail(pull_request)
}

fn thread_view_detail(summary: PullRequest, now: Timestamp) -> PullRequestDetail {
    let denis = Actor::person("denis");
    let tanya = Actor::person("tanya").with_name(Some("Tanya Orlova"));
    let mark = Actor::person("mark-v").with_name(Some("Mark Vance"));
    let viewer = Actor::person("flaticols").with_name(Some("Denis Panfilov"));
    let claude = Actor::agent("Claude Code");
    const DIR: &str = "crates/review/src";

    let mut detail = PullRequestDetail::new(
        summary,
        "Replaces the tabbed Conversation / Files view with one scroll: description, \
         commits, diffs, comments, agent findings and checks in the order they happened.\n\n\
         - Diffs render inline per push, keyed by file + hunk hash\n\
         - Threads stay anchored when lines move\n\
         - Closes #431, depends on #477",
        now.minus(2 * DAY),
    );
    detail.commits = vec![
        Commit::new(
            "5d1c0e2a9b",
            "Replace tabbed view with one scroll",
            Some(CheckStatus::Passed),
        ),
        Commit::new(
            "91be7f3c40",
            "Render diff blocks inline",
            Some(CheckStatus::Passed),
        ),
        Commit::new(
            "a3f9c21d77",
            "Key diff blocks by file + hunk hash",
            Some(CheckStatus::Passed),
        ),
        Commit::new(
            "7be0d4e5a1",
            "Keep thread anchors when hunks move",
            Some(CheckStatus::Running),
        ),
    ];
    detail.files = vec![
        parse_patch(
            &format!("{DIR}/thread.rs"),
            FileChange::Modified,
            patches::THREAD_RS,
        ),
        parse_patch(
            &format!("{DIR}/diff_view.rs"),
            FileChange::Added,
            patches::DIFF_VIEW_RS,
        ),
        parse_patch(
            &format!("{DIR}/hunk.rs"),
            FileChange::Modified,
            patches::HUNK_RS,
        ),
        parse_patch(
            &format!("{DIR}/render.rs"),
            FileChange::Modified,
            patches::RENDER_RS,
        ),
        parse_patch(
            &format!("{DIR}/mod.rs"),
            FileChange::Modified,
            patches::MOD_RS,
        ),
        parse_patch(
            "crates/review/Cargo.toml",
            FileChange::Modified,
            patches::CARGO_TOML,
        ),
        parse_patch("README.md", FileChange::Modified, patches::README_MD),
    ];
    detail.viewed = vec![
        format!("{DIR}/mod.rs"),
        "crates/review/Cargo.toml".to_string(),
    ];
    detail.threads = vec![
        ReviewThread::new(
            "thread-tanya-142",
            format!("{DIR}/thread.rs"),
            142,
            vec![
                Comment::new(
                    "c-tanya-1",
                    tanya.clone(),
                    "Should reanchor_threads run before entry().or_insert? Right now a moved \
                     hunk keeps its old anchor until the next push.",
                    now.minus(2 * HOUR),
                ),
                Comment::new(
                    "c-viewer-1",
                    viewer.clone(),
                    "Agree — a moved hunk should pick up its thread in the same push.",
                    now.minus(20 * MINUTE),
                )
                .pending(),
            ],
        ),
        ReviewThread::new(
            "thread-mark-88",
            format!("{DIR}/render.rs"),
            88,
            vec![
                Comment::new(
                    "c-mark-1",
                    mark.clone(),
                    "This re-renders the whole thread on every push. Could we key diff blocks \
                     by file + hunk hash so unchanged hunks keep their scroll position and \
                     inline threads?",
                    now.minus(DAY),
                ),
                Comment::new(
                    "c-denis-1",
                    denis.clone(),
                    "Done in a3f9c21 — blocks are keyed by path and hunk hash now.",
                    now.minus(HOUR),
                ),
                Comment::new(
                    "c-viewer-2",
                    viewer,
                    "Collapsed state should use the same key, otherwise it resets on push.",
                    now.minus(15 * MINUTE),
                )
                .pending(),
            ],
        ),
    ];
    let mut added = AgentFinding::new(
        "finding-render-104",
        claude.clone(),
        format!("{DIR}/render.rs"),
        104,
        "`collapsed` is looked up by key but never cleared when a file leaves the diff, so \
         the set grows across pushes.",
    );
    added.state = FindingState::Added;
    let mut dismissed = AgentFinding::new(
        "finding-readme-18",
        claude.clone(),
        "README.md",
        18,
        "Consider documenting the collapse threshold as a setting.",
    );
    dismissed.state = FindingState::Dismissed;
    detail.findings = vec![
        AgentFinding::new(
            "finding-hunk-31",
            claude.clone(),
            format!("{DIR}/hunk.rs"),
            31,
            "Possible race: `apply_push` mutates `self.hunks` while `render` holds an iterator \
             from the previous frame. Clone the hunk ids before awaiting `fetch_blobs`, or move \
             the fetch out of the render path.",
        ),
        added,
        dismissed,
    ];
    let agent_summary = AgentReviewSummary::new(claude.clone(), 7, 3, 1, now.minus(12 * MINUTE));
    detail.last_agent_review = Some(agent_summary.clone());
    detail.reviewers = vec![
        Reviewer::new(mark.clone(), ReviewerState::Approved),
        Reviewer::new(tanya.clone(), ReviewerState::Commented),
        Reviewer::new(claude.clone(), ReviewerState::Reviewing),
        Reviewer::new(
            Actor::person("flaticols").with_name(Some("Denis Panfilov")),
            ReviewerState::Pending,
        )
        .viewer(),
    ];
    detail.checks = vec![
        passed_check("ci-fmt", "ci / fmt + clippy", 48),
        passed_check("ci-test-macos", "ci / test (macos-14)", 171),
        passed_check("ci-test-ubuntu", "ci / test (ubuntu-latest)", 134),
        snapshot_check(CheckStatus::Running, 62),
    ];
    detail.depends_on = vec![Dependency::new(
        PullRequestKey::new(switchfold(), 477),
        "Map GitHub repositories to local worktrees",
        "Lands after #477 is merged.",
    )];
    detail.timeline = vec![
        TimelineItem::Description,
        TimelineItem::ReviewRequested {
            actor: denis.clone(),
            reviewers: vec![tanya, mark.clone(), claude],
            at: now.minus(2 * DAY),
        },
        TimelineItem::Pushed {
            actor: denis,
            commits: vec!["a3f9c21d77".into(), "7be0d4e5a1".into()],
            at: now.minus(HOUR),
        },
        TimelineItem::File {
            path: format!("{DIR}/thread.rs"),
        },
        TimelineItem::AgentReviewed(agent_summary),
        TimelineItem::Finding {
            id: "finding-hunk-31".into(),
        },
        TimelineItem::File {
            path: format!("{DIR}/diff_view.rs"),
        },
        TimelineItem::Reviewed {
            actor: mark,
            verdict: ReviewVerdict::Approve,
            at: now.minus(40 * MINUTE),
        },
        TimelineItem::Check {
            id: SNAPSHOT_CHECK_ID.into(),
        },
    ];
    detail
}

fn passed_check(id: &str, name: &str, elapsed: u32) -> CheckRun {
    CheckRun::new(
        id,
        name,
        CheckStatus::Passed,
        elapsed,
        "7be0d4e5a1",
        vec![
            CheckStep::new("Set up job", CheckStatus::Passed, Some(2), Vec::new()),
            CheckStep::new(
                "Check out 7be0d4e",
                CheckStatus::Passed,
                Some(1),
                Vec::new(),
            ),
            CheckStep::new(
                name.trim_start_matches("ci / "),
                CheckStatus::Passed,
                Some(elapsed.saturating_sub(3)),
                Vec::new(),
            ),
        ],
    )
    .with_url("https://github.com/flaticols/switchfold/actions")
}

/// The snapshot-test job from design 04. `Running` shows the first three steps done and
/// the test step in progress; `Failed` shows the complete failing log.
pub fn snapshot_check(status: CheckStatus, elapsed: u32) -> CheckRun {
    let failed = status == CheckStatus::Failed;
    let test_status = if failed { CheckStatus::Failed } else { status };
    let mut log = vec![
        LogLine::new(41, "running 18 tests", LogLineKind::Normal),
        LogLine::new(
            42,
            "test thread::renders_single_scroll ... ok",
            LogLineKind::Normal,
        ),
        LogLine::new(
            43,
            "test thread::keeps_anchor_after_force_push ... ok",
            LogLineKind::Normal,
        ),
    ];
    if failed {
        log.extend([
            LogLine::new(
                44,
                "test diff_view::collapses_large_files ... FAILED",
                LogLineKind::Error,
            ),
            LogLine::new(45, "", LogLineKind::Normal),
            LogLine::new(46, "-old snapshot", LogLineKind::Removed),
            LogLine::new(47, "+new results", LogLineKind::Added),
            LogLine::new(
                48,
                "    ▸ crates/review/src/diff_view.rs  +120 -0",
                LogLineKind::Normal,
            ),
            LogLine::new(49, "-   [collapsed: 120 lines]", LogLineKind::Removed),
            LogLine::new(50, "+   [collapsed: 120 lines] Expand", LogLineKind::Added),
            LogLine::new(51, "", LogLineKind::Normal),
            LogLine::new(
                52,
                "test result: FAILED. 17 passed; 1 failed",
                LogLineKind::Error,
            ),
            LogLine::new(
                53,
                "Error: Process completed with exit code 1.",
                LogLineKind::Error,
            ),
        ]);
    }
    CheckRun::new(
        SNAPSHOT_CHECK_ID,
        "ci / snapshot tests",
        status,
        elapsed,
        "7be0d4e5a1",
        vec![
            CheckStep::new("Set up job", CheckStatus::Passed, Some(2), Vec::new()),
            CheckStep::new(
                "Check out 7be0d4e",
                CheckStatus::Passed,
                Some(1),
                Vec::new(),
            ),
            CheckStep::new(
                "Install Rust 1.82 + cache",
                CheckStatus::Passed,
                Some(38),
                Vec::new(),
            ),
            CheckStep::new(
                "cargo insta test --review",
                test_status,
                failed.then_some(19),
                log,
            ),
            CheckStep::new(
                "Upload snapshot diffs",
                if failed {
                    CheckStatus::Skipped
                } else {
                    CheckStatus::Queued
                },
                None,
                Vec::new(),
            ),
        ],
    )
    .with_url("https://github.com/flaticols/switchfold/actions/runs/1")
}

fn generic_detail(summary: &PullRequest) -> PullRequestDetail {
    let author = Actor::person(summary.author.clone());
    let opened = summary.updated_at.minus(DAY);
    let mut detail = PullRequestDetail::new(
        summary.clone(),
        format!(
            "{}.\n\n- Covered by unit tests\n- No migration needed",
            summary.title
        ),
        opened,
    );
    detail.commits = vec![Commit::new("4c2e9a1b00", summary.title.clone(), summary.ci)];
    let path = format!("crates/{}/src/lib.rs", summary.key.repo().name());
    detail.files = vec![parse_patch(
        &path,
        FileChange::Modified,
        &patches::generic_patch(&summary.title),
    )];
    if let Some(ci) = summary.ci {
        detail.checks = vec![CheckRun::new(
            "ci-test",
            "ci / test",
            ci,
            95,
            "4c2e9a1b00",
            vec![CheckStep::new(
                "cargo test",
                ci,
                ci.is_finished().then_some(90),
                Vec::new(),
            )],
        )];
    }
    detail.reviewers =
        vec![Reviewer::new(Actor::person("flaticols"), ReviewerState::Pending).viewer()];
    detail.timeline = vec![
        TimelineItem::Description,
        TimelineItem::Pushed {
            actor: author,
            commits: vec!["4c2e9a1b00".into()],
            at: opened,
        },
        TimelineItem::File { path },
    ];
    if summary.ci.is_some() {
        detail.timeline.push(TimelineItem::Check {
            id: "ci-test".into(),
        });
    }
    detail
}

#[cfg(test)]
mod tests {
    use super::*;
    use diff_model::LineKind;

    fn data() -> SampleData {
        sample(Timestamp::from_unix(2_000_000_000), Path::new("/Users/me"))
    }

    #[test]
    fn hunk_headers_match_their_lines() {
        for detail in data().details {
            for file in &detail.files {
                for hunk in &file.hunks {
                    let old = hunk
                        .lines
                        .iter()
                        .filter(|l| l.kind != LineKind::Added)
                        .count();
                    let new = hunk
                        .lines
                        .iter()
                        .filter(|l| l.kind != LineKind::Removed)
                        .count();
                    assert_eq!(
                        (old as u32, new as u32),
                        (hunk.old_len, hunk.new_len),
                        "{} {}",
                        file.path,
                        hunk.header()
                    );
                }
            }
        }
    }

    #[test]
    fn featured_pull_request_matches_the_design() {
        let data = data();
        let detail = data
            .details
            .iter()
            .find(|d| d.summary.key.number() == 482)
            .unwrap();
        assert_eq!(detail.files.len(), 7);
        assert_eq!(detail.files[1].additions(), 120);
        assert_eq!(detail.viewed.len(), 2);
        assert_eq!(detail.unresolved_threads().count(), 2);
        let line_142 = detail.files[0].hunks[0]
            .lines
            .iter()
            .find(|line| line.new_line == Some(142))
            .unwrap();
        assert_eq!(line_142.text.trim(), "self.reanchor_threads(cx);");
        assert_eq!(
            detail.merge_summary(),
            "denis wants to merge 4 commits into main from feat/thread-view"
        );
    }

    #[test]
    fn every_timeline_reference_resolves() {
        for detail in data().details {
            for item in &detail.timeline {
                match item {
                    TimelineItem::File { path } => assert!(detail.file(path).is_some(), "{path}"),
                    TimelineItem::Check { id } => assert!(detail.check(id).is_some(), "{id}"),
                    TimelineItem::Finding { id } => assert!(detail.finding(id).is_some(), "{id}"),
                    _ => {}
                }
            }
        }
    }
}

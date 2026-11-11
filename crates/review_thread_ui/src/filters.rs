use review_model::{FindingState, PullRequestDetail, TimelineItem};
use serde::{Deserialize, Serialize};

/// What the thread shows, chosen in the toolbar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimelineFilter {
    /// The whole conversation in the order it happened.
    #[default]
    Everything,
    /// Open threads and agent findings that still need a decision.
    Unresolved,
    /// Every changed file.
    Changes,
    /// Every check run.
    Checks,
    /// Agent reviews of this session with their progress, then every review posted.
    Reviews,
}

impl TimelineFilter {
    /// Every filter in toolbar order.
    pub const ALL: [TimelineFilter; 5] = [
        Self::Everything,
        Self::Unresolved,
        Self::Changes,
        Self::Checks,
        Self::Reviews,
    ];

    /// The toolbar label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Everything => "Everything",
            Self::Unresolved => "Unresolved",
            Self::Changes => "Changes",
            Self::Checks => "Checks",
            Self::Reviews => "Reviews",
        }
    }
}

/// Open threads plus draft findings: what the Unresolved filter counts.
pub fn unresolved_count(detail: &PullRequestDetail) -> usize {
    detail.unresolved_threads().count()
        + detail
            .findings
            .iter()
            .filter(|finding| finding.state == FindingState::Draft)
            .count()
}

/// The items the thread shows under `filter`, in display order.
pub fn items(detail: &PullRequestDetail, filter: TimelineFilter) -> Vec<TimelineItem> {
    match filter {
        TimelineFilter::Everything => detail.timeline.clone(),
        TimelineFilter::Changes => detail
            .files
            .iter()
            .map(|file| TimelineItem::File {
                path: file.path.clone(),
            })
            .collect(),
        TimelineFilter::Checks => detail
            .checks
            .iter()
            .map(|check| TimelineItem::Check {
                id: check.id.clone(),
            })
            .collect(),
        TimelineFilter::Reviews => detail
            .timeline
            .iter()
            .filter(|item| {
                matches!(
                    item,
                    TimelineItem::Reviewed { .. } | TimelineItem::AgentReviewed(_)
                )
            })
            .rev()
            .cloned()
            .collect(),
        TimelineFilter::Unresolved => detail
            .findings
            .iter()
            .filter(|finding| finding.state == FindingState::Draft)
            .map(|finding| TimelineItem::Finding {
                id: finding.id.clone(),
            })
            .collect(),
    }
}

/// Files that start collapsed: those over the size threshold, and those already viewed.
pub fn initially_collapsed(detail: &PullRequestDetail, threshold: usize) -> Vec<String> {
    detail
        .files
        .iter()
        .filter(|file| file.additions() + file.deletions() > threshold)
        .map(|file| file.path.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pr_model::{PullRequestKey, RepoSlug, Timestamp};

    fn detail() -> PullRequestDetail {
        fixtures::sample(
            Timestamp::from_unix(2_000_000_000),
            std::path::Path::new("/u"),
        )
        .details
        .into_iter()
        .find(|d| {
            d.summary.key == PullRequestKey::new(RepoSlug::new("flaticols", "switchfold"), 482)
        })
        .unwrap()
    }

    #[test]
    fn unresolved_counts_open_threads_and_draft_findings() {
        assert_eq!(unresolved_count(&detail()), 3);
    }

    #[test]
    fn changes_lists_every_file_and_checks_every_run() {
        let detail = detail();
        assert_eq!(items(&detail, TimelineFilter::Changes).len(), 7);
        assert_eq!(items(&detail, TimelineFilter::Checks).len(), 4);
    }

    #[test]
    fn the_new_large_file_starts_collapsed() {
        assert_eq!(
            initially_collapsed(&detail(), 100),
            vec!["crates/review/src/diff_view.rs".to_string()]
        );
    }
}

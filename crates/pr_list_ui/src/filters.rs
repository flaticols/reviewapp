use pr_model::{
    CheckStatus, Inbox, PullRequest, PullRequestState, RepoSlug, ReviewDecision, SavedFilter,
};

/// Which collection the list shows, chosen in the sidebar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    Inbox(Inbox),
    SavedFilter(String),
    Repository(RepoSlug),
}

/// The state tabs above the list. `Open` includes drafts, as GitHub's `is:open` does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StateTab {
    #[default]
    Open,
    Draft,
    Merged,
    Closed,
}

impl StateTab {
    /// Every tab in display order.
    pub const ALL: [StateTab; 4] = [Self::Open, Self::Draft, Self::Merged, Self::Closed];

    /// The tab label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Draft => "Draft",
            Self::Merged => "Merged",
            Self::Closed => "Closed",
        }
    }

    /// Whether `state` belongs under this tab.
    pub fn includes(self, state: PullRequestState) -> bool {
        match self {
            Self::Open => matches!(state, PullRequestState::Open | PullRequestState::Draft),
            Self::Draft => state == PullRequestState::Draft,
            Self::Merged => state == PullRequestState::Merged,
            Self::Closed => state == PullRequestState::Closed,
        }
    }
}

/// How the list is ordered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortOrder {
    #[default]
    RecentlyUpdated,
    LeastRecentlyUpdated,
    MostCommented,
}

impl SortOrder {
    /// Every order in menu order.
    pub const ALL: [SortOrder; 3] = [
        Self::RecentlyUpdated,
        Self::LeastRecentlyUpdated,
        Self::MostCommented,
    ];

    /// The menu label.
    pub fn label(self) -> &'static str {
        match self {
            Self::RecentlyUpdated => "Recently updated",
            Self::LeastRecentlyUpdated => "Least recently updated",
            Self::MostCommented => "Most commented",
        }
    }
}

/// The CI filter options.
pub const CI_FILTERS: [CheckStatus; 3] = [
    CheckStatus::Passed,
    CheckStatus::Failed,
    CheckStatus::Running,
];

/// Everything that narrows and orders the list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListFilters {
    pub tab: StateTab,
    pub repository: Option<RepoSlug>,
    pub author: Option<String>,
    pub label: Option<String>,
    pub ci: Option<CheckStatus>,
    pub review: Option<ReviewDecision>,
    pub text: String,
    pub sort: SortOrder,
}

impl ListFilters {
    /// Whether any filter beyond the state tab and sort is set.
    pub fn is_filtering(&self) -> bool {
        self.repository.is_some()
            || self.author.is_some()
            || self.label.is_some()
            || self.ci.is_some()
            || self.review.is_some()
            || !self.text.trim().is_empty()
    }

    /// Clears every filter, keeping the tab and sort.
    pub fn clear(&mut self) {
        *self = Self {
            tab: self.tab,
            sort: self.sort,
            ..Self::default()
        };
    }

    /// The active filters as a sentence fragment, such as
    /// `author is mark-v, CI is failing, text contains "caldav sync"`.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(repo) = &self.repository {
            parts.push(format!("repository is {}", repo.name()));
        }
        if let Some(author) = &self.author {
            parts.push(format!("author is {author}"));
        }
        if let Some(label) = &self.label {
            parts.push(format!("label is {label}"));
        }
        if let Some(ci) = self.ci {
            parts.push(format!("CI is {}", ci.label().to_lowercase()));
        }
        if let Some(review) = self.review {
            parts.push(format!("review is {}", review.label().to_lowercase()));
        }
        let text = self.text.trim();
        if !text.is_empty() {
            parts.push(format!("text contains \u{201c}{text}\u{201d}"));
        }
        parts.join(", ")
    }

    fn matches(&self, pull_request: &PullRequest) -> bool {
        self.tab.includes(pull_request.state)
            && self
                .repository
                .as_ref()
                .is_none_or(|repo| pull_request.key.repo() == repo)
            && self
                .author
                .as_ref()
                .is_none_or(|author| &pull_request.author == author)
            && self
                .label
                .as_ref()
                .is_none_or(|label| pull_request.has_label(label))
            && self.ci.is_none_or(|ci| pull_request.ci == Some(ci))
            && self
                .review
                .is_none_or(|review| pull_request.review == review)
            && matches_text(pull_request, self.text.trim())
    }
}

fn matches_text(pull_request: &PullRequest, text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    let text = text.to_lowercase();
    let number = text.trim_start_matches('#');
    pull_request.title.to_lowercase().contains(&text)
        || pull_request.author.to_lowercase().contains(&text)
        || pull_request.head_ref.to_lowercase().contains(&text)
        || pull_request.key.number().to_string() == number
}

/// Whether `pull_request` is in `scope`, before any filter.
pub fn in_scope(pull_request: &PullRequest, scope: &Scope, saved_filters: &[SavedFilter]) -> bool {
    match scope {
        Scope::Inbox(inbox) => pull_request.is_in(*inbox),
        Scope::SavedFilter(id) => saved_filters
            .iter()
            .find(|filter| &filter.id == id)
            .is_some_and(|filter| filter.matches(pull_request)),
        Scope::Repository(repo) => pull_request.key.repo() == repo,
    }
}

/// Pull requests in `scope` that pass `filters`, in the filters' order.
pub fn visible<'a>(
    pull_requests: &'a [PullRequest],
    scope: &Scope,
    saved_filters: &[SavedFilter],
    filters: &ListFilters,
) -> Vec<&'a PullRequest> {
    let mut visible: Vec<&PullRequest> = pull_requests
        .iter()
        .filter(|pr| in_scope(pr, scope, saved_filters) && filters.matches(pr))
        .collect();
    match filters.sort {
        SortOrder::RecentlyUpdated => visible.sort_by_key(|pr| std::cmp::Reverse(pr.updated_at)),
        SortOrder::LeastRecentlyUpdated => visible.sort_by_key(|pr| pr.updated_at),
        SortOrder::MostCommented => visible.sort_by_key(|pr| std::cmp::Reverse(pr.comments)),
    }
    visible
}

/// Pull requests in `scope` under `tab`, ignoring other filters; the tab's count.
pub fn tab_count(
    pull_requests: &[PullRequest],
    scope: &Scope,
    saved_filters: &[SavedFilter],
    tab: StateTab,
) -> usize {
    pull_requests
        .iter()
        .filter(|pr| in_scope(pr, scope, saved_filters) && tab.includes(pr.state))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pr_model::Timestamp;

    fn data() -> (Vec<PullRequest>, Vec<SavedFilter>) {
        let data = fixtures::sample(
            Timestamp::from_unix(2_000_000_000),
            std::path::Path::new("/u"),
        );
        (data.pull_requests, data.saved_filters)
    }

    #[test]
    fn review_requested_open_tab_matches_the_design_rows() {
        let (prs, saved) = data();
        let rows = visible(
            &prs,
            &Scope::Inbox(Inbox::ReviewRequested),
            &saved,
            &ListFilters::default(),
        );
        let numbers: Vec<u64> = rows.iter().map(|pr| pr.key.number()).collect();
        assert_eq!(numbers, vec![482, 479, 121, 477, 118, 475, 42, 470]);
    }

    #[test]
    fn filters_combine_and_describe_themselves() {
        let (prs, saved) = data();
        let filters = ListFilters {
            author: Some("mark-v".into()),
            ci: Some(CheckStatus::Failed),
            text: "caldav sync".into(),
            ..ListFilters::default()
        };
        assert_eq!(
            filters.describe(),
            "author is mark-v, CI is failing, text contains \u{201c}caldav sync\u{201d}"
        );
        assert!(visible(&prs, &Scope::Inbox(Inbox::AllOpen), &saved, &filters).is_empty());
        let caldav = ListFilters {
            text: "caldav".into(),
            ..filters
        };
        assert_eq!(
            visible(&prs, &Scope::Inbox(Inbox::AllOpen), &saved, &caldav).len(),
            1
        );
    }

    #[test]
    fn a_number_finds_its_pull_request() {
        let (prs, saved) = data();
        let filters = ListFilters {
            text: "#477".into(),
            ..ListFilters::default()
        };
        let rows = visible(&prs, &Scope::Inbox(Inbox::AllOpen), &saved, &filters);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key.number(), 477);
    }

    #[test]
    fn draft_tab_is_a_subset_of_open() {
        let (prs, saved) = data();
        let scope = Scope::Inbox(Inbox::ReviewRequested);
        assert_eq!(tab_count(&prs, &scope, &saved, StateTab::Open), 8);
        assert_eq!(tab_count(&prs, &scope, &saved, StateTab::Draft), 1);
        assert_eq!(tab_count(&prs, &scope, &saved, StateTab::Merged), 2);
    }
}

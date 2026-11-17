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

    /// Clears the field filters (repository, author, label, CI and review), keeping
    /// the state, text and sort.
    pub fn clear_fields(&mut self) {
        self.repository = None;
        self.author = None;
        self.label = None;
        self.ci = None;
        self.review = None;
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

/// How the list arranges its rows under headers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Grouping {
    /// No headers.
    #[default]
    None,
    /// Under the GitHub milestone each pull request is assigned to.
    Milestone,
    /// Under the GitHub stack each pull request belongs to.
    Stack,
}

/// One painted item of the list, in display order.
#[derive(Clone, Copy, Debug)]
pub enum ListEntry<'a> {
    /// The header of a milestone's group: the milestone's title, or `None` for pull
    /// requests without one, and how many pull requests follow it.
    Milestone {
        title: Option<&'a str>,
        count: usize,
    },
    /// The header of a stack's group: the stack's repository and number, or `None` for
    /// pull requests outside a stack, how many pull requests follow it, and how many
    /// the whole stack holds, which is more when some members aren't listed.
    Stack {
        stack: Option<(&'a RepoSlug, u64)>,
        count: usize,
        size: usize,
    },
    /// A pull request row.
    PullRequest(&'a PullRequest),
}

impl<'a> ListEntry<'a> {
    /// The pull request of a row; `None` for a header.
    pub fn pull_request(self) -> Option<&'a PullRequest> {
        match self {
            Self::PullRequest(pull_request) => Some(pull_request),
            Self::Milestone { .. } | Self::Stack { .. } => None,
        }
    }

    /// Whether the entry is a group's header.
    pub fn is_header(self) -> bool {
        self.pull_request().is_none()
    }
}

/// What the list paints for `rows`, which are already filtered and sorted: the rows
/// alone, or under headers by `grouping`.
pub fn entries<'a>(rows: Vec<&'a PullRequest>, grouping: Grouping) -> Vec<ListEntry<'a>> {
    match grouping {
        Grouping::None => rows.into_iter().map(ListEntry::PullRequest).collect(),
        Grouping::Milestone => by_milestone(rows),
        Grouping::Stack => by_stack(rows),
    }
}

/// Each milestone's header before its pull requests: milestones in title order
/// ignoring case, pull requests without one last, and `rows`' order kept inside each
/// group. Milestones are matched by title, so the same title in two repositories is
/// one group.
fn by_milestone<'a>(rows: Vec<&'a PullRequest>) -> Vec<ListEntry<'a>> {
    let mut titles: Vec<Option<&str>> = rows.iter().map(|pr| pr.milestone.as_deref()).collect();
    titles.sort_by_key(|title| (title.is_none(), title.map(str::to_lowercase), *title));
    titles.dedup();
    let mut entries = Vec::with_capacity(rows.len() + titles.len());
    for title in titles {
        let members = rows.iter().filter(|pr| pr.milestone.as_deref() == title);
        entries.push(ListEntry::Milestone {
            title,
            count: members.clone().count(),
        });
        entries.extend(members.map(|pr| ListEntry::PullRequest(pr)));
    }
    entries
}

/// Each stack's header before its pull requests. A stack is a repository and a stack
/// number. Stacks come in the order `rows` first reaches them, so the sort orders the
/// groups; inside a stack the pull requests are in merge order, position 1 first,
/// because that order is what a stack means. Pull requests outside a stack go last, in
/// `rows`' order.
fn by_stack<'a>(rows: Vec<&'a PullRequest>) -> Vec<ListEntry<'a>> {
    let stack_of = |pr: &'a PullRequest| pr.stack.map(|stack| (pr.key.repo(), stack.number()));
    let mut stacks: Vec<(&RepoSlug, u64)> = Vec::new();
    for stack in rows.iter().filter_map(|pr| stack_of(pr)) {
        if !stacks.contains(&stack) {
            stacks.push(stack);
        }
    }
    let mut entries = Vec::with_capacity(rows.len() + stacks.len() + 1);
    for stack in stacks {
        let mut members: Vec<&PullRequest> = rows
            .iter()
            .copied()
            .filter(|pr| stack_of(pr) == Some(stack))
            .collect();
        members.sort_by_key(|pr| pr.stack.map(|stack| stack.position()));
        let size = members
            .first()
            .and_then(|pr| pr.stack)
            .map_or(members.len(), |stack| stack.size() as usize);
        entries.push(ListEntry::Stack {
            stack: Some(stack),
            count: members.len(),
            size,
        });
        entries.extend(members.into_iter().map(ListEntry::PullRequest));
    }
    let unstacked: Vec<&PullRequest> = rows
        .iter()
        .copied()
        .filter(|pr| pr.stack.is_none())
        .collect();
    if !unstacked.is_empty() {
        entries.push(ListEntry::Stack {
            stack: None,
            count: unstacked.len(),
            size: unstacked.len(),
        });
        entries.extend(unstacked.into_iter().map(ListEntry::PullRequest));
    }
    entries
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

    /// Each entry as text: `[title count]` for a header, the number for a row.
    fn outline(entries: &[ListEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| match entry {
                ListEntry::Milestone { title, count } => {
                    format!("[{} {count}]", title.unwrap_or("none"))
                }
                ListEntry::Stack { stack, count, size } => match stack {
                    Some((repo, number)) => format!("[{}#{number} {count}/{size}]", repo.name()),
                    None => format!("[none {count}]"),
                },
                ListEntry::PullRequest(pr) => pr.key.number().to_string(),
            })
            .collect()
    }

    #[test]
    fn stacks_group_in_sort_order_with_members_in_merge_order() {
        let (prs, saved) = data();
        let rows = visible(
            &prs,
            &Scope::Inbox(Inbox::ReviewRequested),
            &saved,
            &ListFilters::default(),
        );
        // Recently updated puts #482, the top of the stack, first; its group still
        // reads from the base up, and #468 has merged, so three of four are listed.
        assert_eq!(
            outline(&entries(rows, Grouping::Stack)),
            [
                "[switchfold#12 3/4]",
                "470",
                "477",
                "482",
                "[none 5]",
                "479",
                "121",
                "118",
                "475",
                "42"
            ]
        );
    }

    #[test]
    fn the_same_stack_number_in_two_repositories_is_two_groups() {
        use pr_model::{PullRequestKey, StackPosition};
        let stacked = |repo: &str, number, position| {
            let mut pr = PullRequest::new(
                PullRequestKey::new(RepoSlug::new("o", repo), number),
                "t",
                "a",
                "h",
                "main",
                Timestamp::from_unix(0),
            );
            pr.stack = Some(StackPosition::new(1, position, 2));
            pr
        };
        let prs = [
            stacked("b", 20, 2),
            stacked("a", 10, 1),
            stacked("b", 21, 1),
            stacked("a", 11, 2),
        ];
        assert_eq!(
            outline(&entries(prs.iter().collect(), Grouping::Stack)),
            ["[b#1 2/2]", "21", "20", "[a#1 2/2]", "10", "11"],
            "no \"Not in a stack\" header when every row is stacked"
        );
    }

    #[test]
    fn milestones_group_rows_in_title_order_with_none_last() {
        let (prs, saved) = data();
        let rows = visible(
            &prs,
            &Scope::Inbox(Inbox::ReviewRequested),
            &saved,
            &ListFilters::default(),
        );
        assert_eq!(
            outline(&entries(rows, Grouping::Milestone)),
            [
                "[v0.9 3]", "482", "479", "477", "[v1.0 3]", "121", "475", "470", "[none 2]",
                "118", "42"
            ]
        );
    }

    #[test]
    fn the_sort_orders_rows_inside_each_milestone() {
        let (prs, saved) = data();
        let filters = ListFilters {
            sort: SortOrder::LeastRecentlyUpdated,
            ..ListFilters::default()
        };
        let rows = visible(
            &prs,
            &Scope::Inbox(Inbox::ReviewRequested),
            &saved,
            &filters,
        );
        assert_eq!(
            outline(&entries(rows, Grouping::Milestone)),
            [
                "[v0.9 3]", "477", "479", "482", "[v1.0 3]", "470", "475", "121", "[none 2]", "42",
                "118"
            ]
        );
    }

    #[test]
    fn milestone_titles_order_ignoring_case() {
        let now = Timestamp::from_unix(2_000_000_000);
        let with = |number, milestone: Option<&str>| {
            let mut pr = PullRequest::new(
                pr_model::PullRequestKey::new(RepoSlug::new("o", "r"), number),
                "T",
                "a",
                "h",
                "main",
                now,
            );
            pr.milestone = milestone.map(str::to_string);
            pr
        };
        let prs = [
            with(1, None),
            with(2, Some("beta")),
            with(3, Some("Alpha")),
            with(4, Some("beta")),
        ];
        assert_eq!(
            outline(&entries(prs.iter().collect(), Grouping::Milestone)),
            ["[Alpha 1]", "3", "[beta 2]", "2", "4", "[none 1]", "1"]
        );
    }

    #[test]
    fn ungrouped_entries_are_the_rows() {
        let (prs, saved) = data();
        let rows = visible(
            &prs,
            &Scope::Inbox(Inbox::ReviewRequested),
            &saved,
            &ListFilters::default(),
        );
        assert_eq!(
            outline(&entries(rows, Grouping::None)),
            ["482", "479", "121", "477", "118", "475", "42", "470"]
        );
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

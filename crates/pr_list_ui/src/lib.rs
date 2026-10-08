//! The pull request list: inboxes, saved filters and repositories in the sidebar, state
//! tabs and filters above, and a keyboard-navigable list of pull requests.

mod filters;
mod row;
mod sidebar;

use std::ops::Range;

use app_state_ui::actions::{
    OpenSelected, OpenSettings, Refresh, SelectNext, SelectPrevious, ToggleSidebar,
};
use app_state_ui::{ListState, PullRequestStore, StoresExt as _};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Selectable as _, Sizable as _,
    button::Button,
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem},
    separator::Separator,
    sidebar::{Sidebar, SidebarCollapsible, SidebarItem as _, SidebarMenu, SidebarMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Render, ScrollStrategy,
    SharedString, StyleRefinement, Styled as _, Subscription, UniformListScrollHandle, Window, div,
    rems, uniform_list,
};
use pr_model::{CheckStatus, Inbox, PullRequest, PullRequestKey, RepoSlug, ReviewDecision};
use theme_ui::TypographyExt as _;
use widgets_ui::{Counter, EmptyState, FilterTabs};

pub use filters::{CI_FILTERS, ListFilters, Scope, SortOrder, StateTab, tab_count, visible};
pub use row::{PullRequestRow, SkeletonRow, review_decision_icon};

const CONTEXT: &str = "PullRequestList";

/// Registers the list's key bindings. Call once at startup.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", SelectNext, Some(CONTEXT)),
        KeyBinding::new("j", SelectNext, Some(CONTEXT)),
        KeyBinding::new("up", SelectPrevious, Some(CONTEXT)),
        KeyBinding::new("k", SelectPrevious, Some(CONTEXT)),
        KeyBinding::new("enter", OpenSelected, Some(CONTEXT)),
    ]);
}

/// What the list asks its owner to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PullRequestListEvent {
    /// Open the review screen for a pull request.
    Open(PullRequestKey),
    /// Open a pull request at its Review with an agent form.
    ReviewWithAgent(PullRequestKey),
    /// Open Settings at the repositories page to add one.
    AddRepository,
}

/// The pull request list screen. Reads pull requests from the [`PullRequestStore`] and
/// owns the scope, filters and selection.
pub struct PullRequestList {
    focus_handle: FocusHandle,
    filter_input: Entity<InputState>,
    scope: Scope,
    filters: ListFilters,
    selected: Option<PullRequestKey>,
    sidebar_collapsed: bool,
    scroll_handle: UniformListScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PullRequestListEvent> for PullRequestList {}

impl Focusable for PullRequestList {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl PullRequestList {
    /// Creates the list showing the Review requested inbox.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter_input = cx
            .new(|cx| InputState::new(window, cx).placeholder("Filter by title, #number, author…"));
        let store = cx.stores().pull_requests().clone();
        let subscriptions = vec![
            cx.observe(&store, |_, _, cx| cx.notify()),
            cx.subscribe(
                &filter_input,
                |this: &mut Self, input, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.filters.text = input.read(cx).value().to_string();
                        cx.notify();
                    }
                },
            ),
        ];
        Self {
            focus_handle: cx.focus_handle().tab_stop(true),
            filter_input,
            scope: Scope::Inbox(Inbox::ReviewRequested),
            filters: ListFilters::default(),
            selected: None,
            sidebar_collapsed: false,
            scroll_handle: UniformListScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    /// Whether the sidebar is hidden.
    pub fn is_sidebar_collapsed(&self) -> bool {
        self.sidebar_collapsed
    }

    /// Shows or hides the sidebar.
    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_collapsed = !self.sidebar_collapsed;
        cx.notify();
    }

    /// The pull request the list currently selects.
    pub fn selected(&self) -> Option<&PullRequestKey> {
        self.selected.as_ref()
    }

    /// Selects `key`, for when the user returns from its review.
    pub fn select(&mut self, key: PullRequestKey, cx: &mut Context<Self>) {
        self.selected = Some(key);
        cx.notify();
    }

    /// The repository the selection or scope points at, for the title bar.
    pub fn current_repository(&self, cx: &App) -> Option<RepoSlug> {
        if let Scope::Repository(repo) = &self.scope {
            return Some(repo.clone());
        }
        self.selected
            .as_ref()
            .map(|key| key.repo().clone())
            .or_else(|| self.visible_keys(cx).first().map(|key| key.repo().clone()))
    }

    fn set_scope(&mut self, scope: Scope, cx: &mut Context<Self>) {
        self.scope = scope;
        self.selected = None;
        self.scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    fn clear_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filters.clear();
        self.filter_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        cx.notify();
    }

    fn visible_keys(&self, cx: &App) -> Vec<PullRequestKey> {
        let store = cx.stores().pull_requests().read(cx);
        visible(
            store.pull_requests(),
            &self.scope,
            store.saved_filters(),
            &self.filters,
        )
        .into_iter()
        .map(|pr| pr.key.clone())
        .collect()
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let keys = self.visible_keys(cx);
        if keys.is_empty() {
            return;
        }
        let current = self
            .selected
            .as_ref()
            .and_then(|key| keys.iter().position(|candidate| candidate == key));
        let next = match current {
            Some(ix) => (ix as isize + delta).clamp(0, keys.len() as isize - 1) as usize,
            None if delta >= 0 => 0,
            None => keys.len() - 1,
        };
        self.selected = Some(keys[next].clone());
        self.scroll_handle
            .scroll_to_item(next, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn open(&mut self, key: PullRequestKey, cx: &mut Context<Self>) {
        self.selected = Some(key.clone());
        cx.emit(PullRequestListEvent::Open(key));
    }

    /// The heading of the current scope, such as `Review requested`.
    pub fn title(&self, cx: &App) -> SharedString {
        match &self.scope {
            Scope::Inbox(inbox) => inbox.label().into(),
            Scope::Repository(repo) => repo.name().clone().into(),
            Scope::SavedFilter(id) => cx
                .stores()
                .pull_requests()
                .read(cx)
                .saved_filters()
                .iter()
                .find(|filter| &filter.id == id)
                .map(|filter| SharedString::from(filter.name.clone()))
                .unwrap_or_default(),
        }
    }

    fn render_sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let store = cx.stores().pull_requests().read(cx);
        let settings = cx.stores().settings().read(cx);
        let prs = store.pull_requests();
        let saved_filters = store.saved_filters();

        let inbox_menu = SidebarMenu::new().children(Inbox::ALL.map(|inbox| {
            let count = tab_count(prs, &Scope::Inbox(inbox), saved_filters, StateTab::Open);
            let icon = match inbox {
                Inbox::ReviewRequested => IconName::Inbox,
                Inbox::AssignedToMe => IconName::User,
                Inbox::CreatedByMe => IconName::GitPullRequest,
                Inbox::Mentions => IconName::MessageSquare,
                Inbox::AllOpen => IconName::GitPullRequest,
            };
            let view = view.clone();
            SidebarMenuItem::new(inbox.label())
                .label_style(label_style())
                .icon(Icon::new(icon))
                .active(self.scope == Scope::Inbox(inbox))
                .suffix(move |_, _| Counter::new(count))
                .on_click(move |_, _, cx| {
                    view.update(cx, |this, cx| this.set_scope(Scope::Inbox(inbox), cx))
                })
        }));

        let saved_menu = SidebarMenu::new().children(saved_filters.iter().map(|filter| {
            let scope = Scope::SavedFilter(filter.id.clone());
            let count = tab_count(prs, &scope, saved_filters, StateTab::Open);
            let view = view.clone();
            let active = self.scope == scope;
            SidebarMenuItem::new(filter.name.clone())
                .label_style(label_style())
                .icon(Icon::new(IconName::Bookmark))
                .active(active)
                .suffix(move |_, _| Counter::new(count))
                .on_click(move |_, _, cx| {
                    let scope = scope.clone();
                    view.update(cx, |this, cx| this.set_scope(scope, cx))
                })
        }));

        let repositories = sidebar_repositories(prs, settings.repositories(), saved_filters);
        let repo_menu =
            SidebarMenu::new().children(repositories.into_iter().map(|(repo, count)| {
                let scope = Scope::Repository(repo.clone());
                let view = view.clone();
                let active = self.scope == scope;
                SidebarMenuItem::new(repo.name().clone())
                    .label_style(label_style())
                    .icon(Icon::new(IconName::FolderGit2))
                    .active(active)
                    .suffix(move |_, _| Counter::new(count))
                    .on_click(move |_, _, cx| {
                        let scope = scope.clone();
                        view.update(cx, |this, cx| this.set_scope(scope, cx))
                    })
            }));

        let add_repository = view.clone();
        Sidebar::new("pr-sidebar")
            .collapsible(SidebarCollapsible::Offcanvas)
            .collapsed(self.sidebar_collapsed)
            .w(rems(15.5))
            .child(sidebar::NavSection::new("Pull requests", inbox_menu))
            .child(sidebar::NavSection::new("Saved filters", saved_menu))
            .child(
                sidebar::NavSection::new("Repositories", repo_menu).add_action(
                    "add-repository",
                    "Add repository…",
                    move |_, _, cx| {
                        add_repository
                            .update(cx, |_, cx| cx.emit(PullRequestListEvent::AddRepository))
                    },
                ),
            )
            .footer(
                div().w_full().child(
                    SidebarMenu::new()
                        .child(
                            SidebarMenuItem::new("Settings…")
                                .label_style(label_style())
                                .icon(Icon::new(IconName::Settings))
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(OpenSettings), cx)
                                }),
                        )
                        .render("sidebar-footer", window, cx),
                ),
            )
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let store = cx.stores().pull_requests().read(cx);
        let count = tab_count(
            store.pull_requests(),
            &self.scope,
            store.saved_filters(),
            StateTab::Open,
        );
        h_flex()
            .flex_shrink_0()
            .h(rems(3.25))
            .px_4()
            .gap_2()
            .child(div().text_heading().child(self.title(cx)))
            .child(Counter::new(count))
            .child(div().flex_1())
            .child(
                Input::new(&self.filter_input)
                    .small()
                    .w(rems(18.75))
                    .cleanable(true)
                    .prefix(Icon::new(IconName::Search).small()),
            )
            .child(
                Button::new("refresh")
                    .outline()
                    .small()
                    .icon(IconName::RefreshCw)
                    .tooltip_with_action("Refresh", &Refresh, None)
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(Refresh), cx)),
            )
    }

    fn render_filter_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let store = cx.stores().pull_requests().read(cx);
        let prs = store.pull_requests();
        let saved = store.saved_filters();
        let in_scope: Vec<&PullRequest> = prs
            .iter()
            .filter(|pr| filters::in_scope(pr, &self.scope, saved))
            .collect();

        let selected_tab = StateTab::ALL
            .iter()
            .position(|tab| *tab == self.filters.tab)
            .unwrap_or_default();
        let tabs = StateTab::ALL
            .iter()
            .fold(FilterTabs::new("state-tabs"), |tabs, tab| {
                let count = tab_count(prs, &self.scope, saved, *tab);
                let show_count = matches!(tab, StateTab::Open | StateTab::Draft) && count > 0;
                tabs.tab(tab.label(), show_count.then_some(count))
            });

        let mut repositories: Vec<RepoSlug> =
            in_scope.iter().map(|pr| pr.key.repo().clone()).collect();
        repositories.sort();
        repositories.dedup();
        let mut authors: Vec<String> = in_scope.iter().map(|pr| pr.author.clone()).collect();
        authors.sort();
        authors.dedup();
        let mut labels: Vec<String> = in_scope.iter().flat_map(|pr| pr.labels.clone()).collect();
        labels.sort();
        labels.dedup();

        let tab_view = view.clone();
        h_flex()
            .flex_shrink_0()
            .h_11()
            .px_4()
            .gap_2()
            .child(tabs.selected(selected_tab).on_select(move |ix, _, cx| {
                let tab = StateTab::ALL[*ix];
                tab_view.update(cx, |this, cx| {
                    this.filters.tab = tab;
                    this.selected = None;
                    cx.notify();
                })
            }))
            .child(Separator::vertical().h_4())
            .child(filter_menu(
                "filter-repository",
                "Repository",
                self.filters
                    .repository
                    .as_ref()
                    .map(|repo| repo.name().clone()),
                repositories
                    .into_iter()
                    .map(|repo| (repo.name().clone(), repo))
                    .collect(),
                view.clone(),
                |filters, value| filters.repository = value,
            ))
            .child(filter_menu(
                "filter-author",
                "Author",
                self.filters.author.clone(),
                authors.into_iter().map(|a| (a.clone(), a)).collect(),
                view.clone(),
                |filters, value| filters.author = value,
            ))
            .child(filter_menu(
                "filter-label",
                "Labels",
                self.filters.label.clone(),
                labels.into_iter().map(|l| (l.clone(), l)).collect(),
                view.clone(),
                |filters, value| filters.label = value,
            ))
            .child(filter_menu(
                "filter-ci",
                "CI",
                self.filters.ci.map(|ci| ci.label().to_string()),
                CI_FILTERS
                    .iter()
                    .map(|ci| (ci.label().to_string(), *ci))
                    .collect::<Vec<(String, CheckStatus)>>(),
                view.clone(),
                |filters, value| filters.ci = value,
            ))
            .child(filter_menu(
                "filter-review",
                "Review",
                self.filters.review.map(|r| r.label().to_string()),
                ReviewDecision::ALL
                    .iter()
                    .map(|r| (r.label().to_string(), *r))
                    .collect::<Vec<(String, ReviewDecision)>>(),
                view.clone(),
                |filters, value| filters.review = value,
            ))
            .child(div().flex_1())
            .child({
                let current = self.filters.sort;
                let view = view.clone();
                Button::new("sort")
                    .outline()
                    .small()
                    .dropdown_caret(true)
                    .label(current.label())
                    .dropdown_menu_with_anchor(
                        gpui_kit::Anchor::TopRight,
                        move |menu: PopupMenu, _, _| {
                            SortOrder::ALL.iter().fold(menu, |menu, order| {
                                let view = view.clone();
                                let order = *order;
                                menu.item(
                                    PopupMenuItem::new(order.label())
                                        .checked(order == current)
                                        .on_click(move |_, _, cx| {
                                            view.update(cx, |this, cx| {
                                                this.filters.sort = order;
                                                cx.notify();
                                            })
                                        }),
                                )
                            })
                        },
                    )
            })
    }

    fn render_rows(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let store = cx.stores().pull_requests().read(cx);
        match store.list_state() {
            ListState::Loading => {
                return v_flex()
                    .p_2()
                    .gap_0p5()
                    .children([22., 25., 28., 30.].map(SkeletonRow::new))
                    .into_any_element();
            }
            ListState::Failed(error) => {
                return EmptyState::new(
                    IconName::CircleAlert,
                    "Couldn’t load pull requests",
                    format!("{error} Check your connection and try again."),
                )
                .action(
                    Button::new("retry")
                        .small()
                        .icon(IconName::RotateCcw)
                        .label("Try again")
                        .on_click(|_, window, cx| window.dispatch_action(Box::new(Refresh), cx)),
                )
                .into_any_element();
            }
            ListState::Loaded => {}
        }

        let count = visible(
            store.pull_requests(),
            &self.scope,
            store.saved_filters(),
            &self.filters,
        )
        .len();
        if count == 0 {
            return self.render_empty(window, cx);
        }

        uniform_list(
            "pull-requests",
            count,
            cx.processor(|this, range: Range<usize>, _, cx| this.row_elements(range, cx)),
        )
        .track_scroll(&self.scroll_handle)
        .size_full()
        .into_any_element()
    }

    fn row_elements(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let view = cx.entity();
        let store = cx.stores().pull_requests().read(cx);
        let now = store.now();
        visible(
            store.pull_requests(),
            &self.scope,
            store.saved_filters(),
            &self.filters,
        )
        .into_iter()
        .enumerate()
        .skip(range.start)
        .take(range.len())
        .map(|(_, pr)| {
            let key = pr.key.clone();
            let view = view.clone();
            let target = pr_actions_ui::PullRequestTarget::of(pr);
            let agent_view = view.clone();
            div()
                .id(SharedString::from(format!("row-{}", pr.key.id())))
                .px_2()
                .pt_0p5()
                .context_menu(move |menu, _, _| {
                    let agent_view = agent_view.clone();
                    let agent_key = target.key.clone();
                    pr_actions_ui::pull_request_menu(
                        menu,
                        &target,
                        Some(std::rc::Rc::new(move |_, cx| {
                            agent_view.update(cx, |_, cx| {
                                cx.emit(PullRequestListEvent::ReviewWithAgent(agent_key.clone()))
                            })
                        })),
                    )
                })
                .child(
                    PullRequestRow::new(pr.clone(), now)
                        .selected(self.selected.as_ref() == Some(&pr.key))
                        .on_click(move |_, window, cx| {
                            let key = key.clone();
                            view.update(cx, |this, cx| {
                                window.focus(&this.focus_handle, cx);
                                this.open(key, cx)
                            })
                        }),
                )
                .into_any_element()
        })
        .collect()
    }

    fn render_empty(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let _ = window;
        if self.filters.is_filtering() {
            return EmptyState::new(
                IconName::Search,
                "No pull requests match these filters",
                format!("Filters: {}.", self.filters.describe()),
            )
            .action(
                Button::new("clear-filters")
                    .outline()
                    .small()
                    .icon(IconName::X)
                    .label("Clear filters")
                    .on_click(cx.listener(|this, _, window, cx| this.clear_filters(window, cx))),
            )
            .into_any_element();
        }
        let (icon, title, description) = match (&self.scope, self.filters.tab) {
            (Scope::Inbox(Inbox::ReviewRequested), StateTab::Open) => (
                IconName::Inbox,
                "No reviews requested",
                "Nobody is waiting on you. New review requests show up here as soon as they sync.",
            ),
            (Scope::Inbox(Inbox::Mentions), StateTab::Open) => (
                IconName::MessageSquare,
                "No mentions",
                "Pull requests that mention you show up here.",
            ),
            (_, StateTab::Draft) => (
                IconName::GitPullRequestDraft,
                "No drafts",
                "Draft pull requests in this list show up here.",
            ),
            (_, StateTab::Merged) => (
                IconName::GitMerge,
                "Nothing merged yet",
                "Merged pull requests in this list show up here.",
            ),
            (_, StateTab::Closed) => (
                IconName::GitPullRequestClosed,
                "Nothing closed",
                "Closed pull requests in this list show up here.",
            ),
            _ => (
                IconName::Inbox,
                "No open pull requests",
                "Open pull requests in this list show up here.",
            ),
        };
        let state = EmptyState::new(icon, title, description);
        if self.scope == Scope::Inbox(Inbox::AllOpen) {
            return state.into_any_element();
        }
        state
            .action(
                Button::new("show-all-open")
                    .small()
                    .icon(IconName::GitPullRequest)
                    .label("Show all open")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.filters.tab = StateTab::Open;
                        this.set_scope(Scope::Inbox(Inbox::AllOpen), cx)
                    })),
            )
            .into_any_element()
    }
}

/// Repositories to list in the sidebar with their open pull request counts: every mapped
/// repository plus any with open pull requests, busiest first.
fn sidebar_repositories(
    pull_requests: &[PullRequest],
    mappings: &[settings_model::RepositoryMapping],
    saved_filters: &[pr_model::SavedFilter],
) -> Vec<(RepoSlug, usize)> {
    let mut repos: Vec<RepoSlug> = mappings
        .iter()
        .filter_map(|mapping| mapping.repo())
        .collect();
    for pr in pull_requests.iter().filter(|pr| pr.is_in(Inbox::AllOpen)) {
        if !repos.contains(pr.key.repo()) {
            repos.push(pr.key.repo().clone());
        }
    }
    let mut counted: Vec<(RepoSlug, usize)> = repos
        .into_iter()
        .map(|repo| {
            let count = tab_count(
                pull_requests,
                &Scope::Repository(repo.clone()),
                saved_filters,
                StateTab::Open,
            );
            (repo, count)
        })
        .collect();
    counted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    counted
}

/// Sidebar labels at the design's 13 px chrome size; the component defaults to 14 px.
fn label_style() -> StyleRefinement {
    StyleRefinement::default().text_ui()
}

/// A filter dropdown: the trigger names the filter, or its value when set, and stays
/// selected while a value is applied.
fn filter_menu<T: Clone + PartialEq + 'static>(
    id: &'static str,
    name: &'static str,
    current: Option<impl Into<SharedString>>,
    options: Vec<(impl Into<SharedString>, T)>,
    view: Entity<PullRequestList>,
    apply: fn(&mut ListFilters, Option<T>),
) -> impl IntoElement {
    let current: Option<SharedString> = current.map(Into::into);
    let options: Vec<(SharedString, T)> = options
        .into_iter()
        .map(|(label, value)| (label.into(), value))
        .collect();
    let label: SharedString = match &current {
        Some(value) => format!("{name}: {value}").into(),
        None => name.into(),
    };
    Button::new(id)
        .outline()
        .small()
        .dropdown_caret(true)
        .selected(current.is_some())
        .label(label)
        .dropdown_menu(move |menu: PopupMenu, _, _| {
            let any_view = view.clone();
            let menu = menu.item(
                PopupMenuItem::new(format!("Any {}", name.to_lowercase()))
                    .checked(current.is_none())
                    .on_click(move |_, _, cx| {
                        any_view.update(cx, |this, cx| {
                            apply(&mut this.filters, None);
                            cx.notify();
                        })
                    }),
            );
            let menu = if options.is_empty() {
                menu
            } else {
                menu.separator()
            };
            options.iter().fold(menu, |menu, (label, value)| {
                let view = view.clone();
                let value = value.clone();
                menu.item(
                    PopupMenuItem::new(label.clone())
                        .checked(current.as_ref() == Some(label))
                        .on_click(move |_, _, cx| {
                            let value = value.clone();
                            view.update(cx, |this, cx| {
                                apply(&mut this.filters, Some(value));
                                cx.notify();
                            })
                        }),
                )
            })
        })
}

impl Render for PullRequestList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .size_full()
            .items_stretch()
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_sidebar(cx)))
            .on_action(cx.listener(|_, _: &Refresh, _, cx| {
                cx.stores()
                    .pull_requests()
                    .clone()
                    .update(cx, |store: &mut PullRequestStore, cx| store.refresh(cx))
            }))
            .child(self.render_sidebar(window, cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .bg(cx.theme().background)
                    .child(self.render_header(cx))
                    .child(self.render_filter_bar(cx))
                    .child(
                        div()
                            .id("pull-request-rows")
                            .key_context(CONTEXT)
                            .track_focus(&self.focus_handle)
                            .flex_1()
                            .min_h_0()
                            .pb_2()
                            .on_action(
                                cx.listener(|this, _: &SelectNext, _, cx| {
                                    this.move_selection(1, cx)
                                }),
                            )
                            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| {
                                this.move_selection(-1, cx)
                            }))
                            .on_action(cx.listener(|this, _: &OpenSelected, _, cx| {
                                if let Some(key) = this.selected.clone() {
                                    this.open(key, cx);
                                }
                            }))
                            .child(self.render_rows(window, cx)),
                    ),
            )
    }
}

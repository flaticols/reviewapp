//! The pull request list: inboxes, saved filters and repositories in the sidebar, state
//! tabs and filters above, and a keyboard-navigable list of pull requests.

mod filters;
mod row;
mod sidebar;
mod toolbar;

use std::ops::Range;

use app_state_ui::actions::{
    FocusFilter, OpenSelected, Refresh, SelectNext, SelectPrevious, ToggleSidebar,
};
use app_state_ui::{ListState, PullRequestStore, StoresExt as _};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{InputEvent, InputState},
    menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem},
    sidebar::{Sidebar, SidebarCollapsible, SidebarMenu, SidebarMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Render, ScrollStrategy,
    SharedString, StatefulInteractiveElement as _, StyleRefinement, Styled as _, Subscription,
    TestSupportExt as _, UniformListScrollHandle, Window, div, rems, uniform_list,
};
use pr_model::{Inbox, PullRequest, PullRequestKey, RepoSlug};
use theme_ui::TypographyExt as _;
use widgets_ui::{Counter, EmptyState};

pub use filters::{CI_FILTERS, ListFilters, Scope, SortOrder, StateTab, tab_count, visible};
pub use row::{PullRequestRow, SkeletonRow, review_decision_icon};
pub use toolbar::{ListActions, ListTitle};

const CONTEXT: &str = "PullRequestList";

/// Registers the list's key bindings. Call once at startup.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", SelectNext, Some(CONTEXT)),
        KeyBinding::new("j", SelectNext, Some(CONTEXT)),
        KeyBinding::new("up", SelectPrevious, Some(CONTEXT)),
        KeyBinding::new("k", SelectPrevious, Some(CONTEXT)),
        KeyBinding::new("enter", OpenSelected, Some(CONTEXT)),
        KeyBinding::new("cmd-f", FocusFilter, Some(CONTEXT)),
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
        let filter_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter pull requests"));
        let store = cx.stores().pull_requests().clone();
        let mut subscriptions = vec![
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
        if let Some(triage) = triage_ui::TriageStoreExt::triage(&**cx) {
            subscriptions.push(cx.observe(&triage, |_, _, cx| cx.notify()));
        }
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

    /// The filters in effect.
    pub fn filters(&self) -> &ListFilters {
        &self.filters
    }

    /// Changes the filters, as the title bar's Filter menu does.
    pub fn update_filters(&mut self, edit: impl FnOnce(&mut ListFilters), cx: &mut Context<Self>) {
        edit(&mut self.filters);
        self.selected = None;
        cx.notify();
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

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
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

        let preferences = settings.preferences().clone();
        let mut repositories = sidebar_repositories(prs, settings.repositories(), saved_filters);
        let mut pinned: Vec<(RepoSlug, usize)> = Vec::new();
        for slug in &preferences.pinned_repositories {
            if let Some(ix) = repositories
                .iter()
                .position(|(repo, _)| &repo.to_string() == slug)
            {
                pinned.push(repositories.remove(ix));
            }
        }
        let item = |repo: RepoSlug, count: usize, cx: &App| {
            let scope = Scope::Repository(repo.clone());
            let active = self.scope == scope;
            let slug = repo.to_string();
            let color = preferences
                .repository_color(&slug)
                .map(|color| theme_ui::repository_color(color, cx));
            let is_pinned = preferences.is_pinned(&slug);
            let view = view.clone();
            let menu_repo = repo.clone();
            let menu_view = view.clone();
            SidebarMenuItem::new(repo.name().clone())
                .label_style(label_style())
                .icon(match color {
                    Some(color) => Icon::new(IconName::FolderGit2).text_color(color),
                    None => Icon::new(IconName::FolderGit2),
                })
                .active(active)
                .suffix(move |_, _| Counter::new(count))
                .on_click(move |_, _, cx| {
                    let scope = scope.clone();
                    view.update(cx, |this, cx| this.set_scope(scope, cx))
                })
                .context_menu(move |menu, _, cx| {
                    repository_menu(menu, &menu_repo, is_pinned, menu_view.clone(), cx)
                })
        };
        let pinned_menu = SidebarMenu::new().children(
            pinned
                .iter()
                .map(|(repo, count)| item(repo.clone(), *count, cx)),
        );
        let repo_menu = SidebarMenu::new().children(
            repositories
                .iter()
                .map(|(repo, count)| item(repo.clone(), *count, cx)),
        );

        let add_repository = view.clone();
        let mut sidebar = Sidebar::new("pr-sidebar")
            .collapsible(SidebarCollapsible::Offcanvas)
            .collapsed(self.sidebar_collapsed)
            .w(rems(15.5))
            .child(sidebar::NavSection::new("Pull requests", inbox_menu));
        if !pinned.is_empty() {
            sidebar = sidebar.child(
                sidebar::NavSection::new("Pinned", pinned_menu)
                    .foldable(preferences.is_folded("Pinned")),
            );
        }
        sidebar
            .child(sidebar::NavSection::new("Saved filters", saved_menu))
            .child(
                sidebar::NavSection::new("Repositories", repo_menu)
                    .foldable(preferences.is_folded("Repositories"))
                    .add_action("add-repository", "Add repository…", move |_, _, cx| {
                        add_repository
                            .update(cx, |_, cx| cx.emit(PullRequestListEvent::AddRepository))
                    }),
            )
    }

    /// Field filters in effect, as chips that each remove their filter. Hidden when
    /// none are on, so the filter state stays visible without a permanent bar.
    fn render_active_filters(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let filters = &self.filters;
        let chips: Vec<Chip> = [
            filters.repository.as_ref().map(|r| {
                (
                    "Repository",
                    r.name().clone(),
                    (|f: &mut ListFilters| f.repository = None) as fn(&mut ListFilters),
                )
            }),
            filters.author.clone().map(|a| {
                (
                    "Author",
                    a,
                    (|f: &mut ListFilters| f.author = None) as fn(&mut ListFilters),
                )
            }),
            filters.label.clone().map(|l| {
                (
                    "Label",
                    l,
                    (|f: &mut ListFilters| f.label = None) as fn(&mut ListFilters),
                )
            }),
            filters.ci.map(|c| {
                (
                    "CI",
                    c.label().to_string(),
                    (|f: &mut ListFilters| f.ci = None) as fn(&mut ListFilters),
                )
            }),
            filters.review.map(|r| {
                (
                    "Review",
                    r.label().to_string(),
                    (|f: &mut ListFilters| f.review = None) as fn(&mut ListFilters),
                )
            }),
        ]
        .into_iter()
        .flatten()
        .collect();
        if chips.is_empty() {
            return None;
        }
        let view = cx.entity();
        let clear_view = view.clone();
        Some(
            h_flex()
                .flex_shrink_0()
                .flex_wrap()
                .px_4()
                .pt_2()
                .gap_1()
                .children(chips.into_iter().map(move |(name, value, clear)| {
                    let view = view.clone();
                    Button::new(SharedString::from(format!("chip-{name}")))
                        .outline()
                        .xsmall()
                        .label(format!("{name}: {value}"))
                        .icon(IconName::X)
                        .tooltip(format!("Stop filtering by {}", name.to_lowercase()))
                        .on_click(move |_, _, cx| {
                            view.update(cx, |this, cx| {
                                clear(&mut this.filters);
                                cx.notify();
                            })
                        })
                }))
                .child(
                    Button::new("chips-clear")
                        .ghost()
                        .xsmall()
                        .label("Clear")
                        .on_click(move |_, _, cx| {
                            clear_view.update(cx, |this, cx| {
                                this.filters.clear_fields();
                                cx.notify();
                            })
                        }),
                ),
        )
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
                        .triage(
                            triage_ui::TriageStoreExt::triage(&**cx)
                                .and_then(|store| store.read(cx).pull_request(&pr.key)),
                        )
                        .repo_color(
                            cx.stores()
                                .settings()
                                .read(cx)
                                .preferences()
                                .repository_color(&pr.key.repo().to_string())
                                .map(|color| theme_ui::repository_color(color, cx)),
                        )
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

/// Actions on a repository in the sidebar: pin, color, open, copy, and its settings.
fn repository_menu(
    menu: PopupMenu,
    repo: &RepoSlug,
    pinned: bool,
    view: Entity<PullRequestList>,
    cx: &App,
) -> PopupMenu {
    let slug = repo.to_string();
    let current = cx
        .stores()
        .settings()
        .read(cx)
        .preferences()
        .repository_color(&slug);
    let url = format!("https://github.com/{slug}");
    let cloned = cx.stores().settings().read(cx).clone_path(repo).is_some();
    let set = |edit: Box<dyn Fn(&mut settings_model::Preferences)>| {
        move |_: &gpui_kit::ClickEvent, _: &mut Window, cx: &mut App| {
            cx.stores().settings().clone().update(cx, |settings, cx| {
                settings.update_preferences(cx, |p| edit(p))
            })
        }
    };
    let pin_slug = slug.clone();
    let menu = menu.item(
        PopupMenuItem::new(if pinned { "Unpin" } else { "Pin to sidebar" })
            .icon(Icon::new(if pinned {
                IconName::PinOff
            } else {
                IconName::Pin
            }))
            .on_click(set(Box::new(move |p| p.set_pinned(&pin_slug, !pinned)))),
    );
    let grid_slug = slug.clone();
    let menu = menu
        .separator()
        .label("Color")
        .item(PopupMenuItem::element(move |_, cx| {
            color_grid(&grid_slug, current, cx)
        }));
    let github = url.clone();
    let zed_repo = repo.clone();
    let finder_repo = repo.clone();
    let copy_name = slug.clone();
    let copy_url = url.clone();
    menu.separator()
        .item(
            PopupMenuItem::new("Open on GitHub")
                .icon(Icon::new(IconName::Globe))
                .on_click(move |_, _, cx| cx.open_url(&github)),
        )
        .item(
            PopupMenuItem::new("Open in Zed")
                .icon(Icon::new(IconName::ExternalLink))
                .disabled(!cloned)
                .on_click(move |_, window, cx| {
                    pr_actions_ui::open_repository_in_zed(&zed_repo, window, cx)
                }),
        )
        .item(
            PopupMenuItem::new("Reveal in Finder")
                .icon(Icon::new(IconName::Folder))
                .disabled(!cloned)
                .on_click(move |_, window, cx| {
                    pr_actions_ui::reveal_repository(&finder_repo, window, cx)
                }),
        )
        .separator()
        .item(
            PopupMenuItem::new("Copy name")
                .icon(Icon::new(IconName::Copy))
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(copy_name.clone()))
                }),
        )
        .item(
            PopupMenuItem::new("Copy link")
                .icon(Icon::new(IconName::Link))
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(copy_url.clone()))
                }),
        )
        .separator()
        .item(
            PopupMenuItem::new("Repository settings…")
                .icon(Icon::new(IconName::Settings))
                .on_click(move |_, _, cx| {
                    view.update(cx, |_, cx| cx.emit(PullRequestListEvent::AddRepository))
                }),
        )
}

/// The repository colors as two rows of circles, the first "None". Clicking one sets
/// it; the menu row around the grid then closes the menu.
fn color_grid(
    slug: &str,
    current: Option<settings_model::RepositoryColor>,
    cx: &App,
) -> impl IntoElement + use<> {
    let options: Vec<Option<settings_model::RepositoryColor>> = std::iter::once(None)
        .chain(settings_model::RepositoryColor::ALL.into_iter().map(Some))
        .collect();
    let swatch = |option: Option<settings_model::RepositoryColor>, cx: &App| {
        let selected = option == current;
        let slug = slug.to_string();
        let key = option.map_or("none", |color| color.label());
        let tooltip = option.map_or("No color", |color| color.label());
        div()
            .id(SharedString::from(format!("repo-color-{key}")))
            .test_support()
            .size_6()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_2()
            .border_color(if selected {
                cx.theme().foreground
            } else {
                cx.theme().transparent
            })
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tooltip).build(window, cx)
            })
            .child(match option {
                Some(color) => div()
                    .size_4()
                    .rounded_full()
                    .bg(theme_ui::repository_color(color, cx))
                    .into_any_element(),
                None => div()
                    .size_4()
                    .rounded_full()
                    .border_1()
                    .border_color(cx.theme().muted_foreground)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::new(IconName::X)
                            .xsmall()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .into_any_element(),
            })
            .on_click(move |_, _, cx| {
                let slug = slug.clone();
                cx.stores().settings().clone().update(cx, |settings, cx| {
                    settings.update_preferences(cx, |p| p.set_repository_color(&slug, option))
                })
            })
    };
    let (first, second) = options.split_at(4);
    v_flex()
        .py_1()
        .gap_1()
        .child(
            h_flex()
                .gap_1()
                .children(first.iter().map(|option| swatch(*option, cx))),
        )
        .child(
            h_flex()
                .gap_1()
                .children(second.iter().map(|option| swatch(*option, cx))),
        )
}

/// An active filter as a chip: its name, its value, and how to clear it.
type Chip = (&'static str, String, fn(&mut ListFilters));

/// Sidebar labels at the design's 13 px chrome size; the component defaults to 14 px.
fn label_style() -> StyleRefinement {
    StyleRefinement::default().text_ui()
}

impl Render for PullRequestList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .size_full()
            .items_stretch()
            .key_context(CONTEXT)
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_sidebar(cx)))
            .on_action(cx.listener(|this, _: &FocusFilter, window, cx| {
                this.filter_input
                    .update(cx, |input, cx| input.focus(window, cx))
            }))
            .on_action(cx.listener(|_, _: &Refresh, _, cx| {
                cx.stores()
                    .pull_requests()
                    .clone()
                    .update(cx, |store: &mut PullRequestStore, cx| store.refresh(cx))
            }))
            .child(self.render_sidebar(cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .bg(cx.theme().background)
                    .children(self.render_active_filters(cx))
                    .child(
                        div()
                            .id("pull-request-rows")
                            .key_context(CONTEXT)
                            .track_focus(&self.focus_handle)
                            .flex_1()
                            .min_h_0()
                            .pt_1()
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

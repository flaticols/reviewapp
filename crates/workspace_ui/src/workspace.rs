mod zed;

use app_state_ui::StoresExt as _;
use app_state_ui::actions::{
    CloseWindow, FocusSearch, GoBack, OpenSettings, Quit, Refresh, ResetZoom, ZoomIn, ZoomOut,
};
use chrome_ui::{AppStatusBar, AppTitleBar};
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, KeyBinding, ParentElement as _, Render, SharedString, Styled as _, Subscription,
    Window, div, prelude::FluentBuilder as _,
};
use pr_list_ui::{PullRequestList, PullRequestListEvent};
use pr_model::RepoSlug;
use review_thread_ui::{ReviewThreadEvent, ReviewThreadView};
use settings_model::{DEFAULT_FONT_SIZE, FONT_SIZES, display_path};
use settings_ui::{SettingsEvent, SettingsPageKind, SettingsView};
use sign_in_ui::{SignInEvent, SignInView};

const CONTEXT: &str = "Workspace";

/// Registers window-level key bindings. Call once at startup, before menus.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-k", FocusSearch, None),
        KeyBinding::new("cmd-r", Refresh, None),
        KeyBinding::new("cmd-[", GoBack, Some(CONTEXT)),
        KeyBinding::new("cmd-=", ZoomIn, None),
        KeyBinding::new("cmd-+", ZoomIn, None),
        KeyBinding::new("cmd--", ZoomOut, None),
        KeyBinding::new("cmd-0", ResetZoom, None),
        KeyBinding::new("cmd-b", app_state_ui::actions::ToggleSidebar, Some(CONTEXT)),
        KeyBinding::new(
            "cmd-alt-b",
            app_state_ui::actions::ToggleInspector,
            Some(CONTEXT),
        ),
        KeyBinding::new(
            "cmd-shift-f",
            app_state_ui::actions::ToggleFocusMode,
            Some(CONTEXT),
        ),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
}

/// The screen the window shows.
enum Route {
    SignIn(Entity<SignInView>),
    PullRequests,
    Review(Entity<ReviewThreadView>),
    Settings(Entity<SettingsView>),
}

/// The window's root view: chrome around whichever screen is current.
pub struct Workspace {
    route: Route,
    pull_request_list: Entity<PullRequestList>,
    route_subscription: Option<Subscription>,
    entity: gpui_kit::WeakEntity<Self>,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    /// Creates the workspace showing the pull request list.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pull_request_list = cx.new(|cx| PullRequestList::new(window, cx));
        let pull_requests = cx.stores().pull_requests().clone();
        let settings = cx.stores().settings().clone();
        let observed = vec![
            cx.observe(&pull_requests, |_, _, cx| cx.notify()),
            cx.observe_in(&settings, window, |this, settings, window, cx| {
                let signed_out = settings.read(cx).accounts().is_empty();
                match (&this.route, signed_out) {
                    (Route::SignIn(_), false) => {
                        cx.stores()
                            .pull_requests()
                            .clone()
                            .update(cx, |store, cx| store.refresh(cx));
                        this.show_list(window, cx)
                    }
                    (Route::PullRequests | Route::Review(_), true) => this.show_sign_in(window, cx),
                    _ => cx.notify(),
                }
            }),
            cx.subscribe_in(
                &pull_request_list,
                window,
                |this, _, event: &PullRequestListEvent, window, cx| match event {
                    PullRequestListEvent::Open(key) => this.open_review(key.clone(), window, cx),
                    PullRequestListEvent::ReviewWithAgent(key) => {
                        this.open_review(key.clone(), window, cx);
                        if let Some(review) = this.review_view() {
                            review.update(cx, |review, cx| review.open_agent_review(window, cx));
                        }
                    }
                    PullRequestListEvent::AddRepository => {
                        this.open_settings(SettingsPageKind::Repositories, window, cx)
                    }
                },
            ),
        ];
        window.focus(&pull_request_list.focus_handle(cx), cx);
        let mut workspace = Self {
            route: Route::PullRequests,
            pull_request_list,
            route_subscription: None,
            entity: cx.entity().downgrade(),
            _subscriptions: observed,
        };
        if settings.read(cx).accounts().is_empty() {
            workspace.show_sign_in(window, cx);
        }
        workspace
    }

    fn show_sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let sign_in = cx.new(SignInView::new);
        self.route_subscription = Some(cx.subscribe_in(
            &sign_in,
            window,
            |this, _, event: &SignInEvent, window, cx| match event {
                SignInEvent::AddToken => {
                    this.open_settings(SettingsPageKind::Accounts, window, cx);
                    if let Route::Settings(settings) = &this.route {
                        settings.update(cx, |settings, cx| settings.add_token(window, cx));
                    }
                }
            },
        ));
        self.route = Route::SignIn(sign_in);
        cx.notify();
    }

    /// Opens Settings at `page`.
    pub fn open_settings(
        &mut self,
        page: SettingsPageKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Route::Settings(settings) = &self.route {
            settings.update(cx, |settings, cx| settings.open_page(page, cx));
            return;
        }
        let settings = cx.new(|cx| SettingsView::new(page, window, cx));
        self.route_subscription = Some(cx.subscribe_in(
            &settings,
            window,
            |this, _, event: &SettingsEvent, window, cx| match event {
                SettingsEvent::Back => {
                    let signed_out = cx.stores().settings().read(cx).accounts().is_empty();
                    if signed_out {
                        this.show_sign_in(window, cx)
                    } else {
                        this.show_list(window, cx)
                    }
                }
            },
        ));
        self.route = Route::Settings(settings);
        cx.notify();
    }

    fn current_repository(&self, cx: &App) -> Option<RepoSlug> {
        match &self.route {
            Route::PullRequests | Route::Settings(_) => {
                self.pull_request_list.read(cx).current_repository(cx)
            }
            Route::Review(review) => Some(review.read(cx).key().repo().clone()),
            Route::SignIn(_) => None,
        }
    }

    /// Lists review requests and mentions, newest first, in the palette.
    fn open_notifications(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use command_palette_ui::{IconName, PaletteEntry, PaletteGroup};
        let store = cx.stores().pull_requests().read(cx);
        let now = store.now();
        let mut waiting: Vec<&pr_model::PullRequest> = store
            .pull_requests()
            .iter()
            .filter(|pr| {
                pr.is_in(pr_model::Inbox::AllOpen)
                    && (pr.is_in(pr_model::Inbox::ReviewRequested)
                        || pr.is_in(pr_model::Inbox::Mentions))
            })
            .collect();
        waiting.sort_by_key(|pr| std::cmp::Reverse(pr.updated_at));
        let entries: Vec<PaletteEntry> = waiting
            .into_iter()
            .map(|pr| {
                let why = if pr.is_in(pr_model::Inbox::ReviewRequested) {
                    "Review requested"
                } else {
                    "Mentioned"
                };
                PaletteEntry::key(
                    format!(
                        "{why} · {} · #{} · {}",
                        pr.title,
                        pr.key.number(),
                        pr.updated_at.short_age(now)
                    ),
                    if pr.is_in(pr_model::Inbox::ReviewRequested) {
                        IconName::Eye
                    } else {
                        IconName::MessageSquare
                    },
                    format!("pr:{}", pr.key.id()),
                )
            })
            .collect();
        let workspace = cx.entity();
        command_palette_ui::open(
            vec![PaletteGroup::new("Waiting on you", entries)],
            move |key, window, cx| {
                workspace.update(cx, |this, cx| this.confirm_palette(&key, window, cx))
            },
            window,
            cx,
        );
    }

    fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use command_palette_ui::{IconName, PaletteEntry, PaletteGroup};
        if matches!(self.route, Route::SignIn(_)) {
            return;
        }
        let store = cx.stores().pull_requests().read(cx);
        let pull_requests: Vec<PaletteEntry> = store
            .pull_requests()
            .iter()
            .filter(|pr| pr.is_in(pr_model::Inbox::AllOpen))
            .map(|pr| {
                PaletteEntry::key(
                    format!(
                        "{} · #{} {}",
                        pr.title,
                        pr.key.number(),
                        pr.key.repo().name()
                    ),
                    IconName::GitPullRequest,
                    format!("pr:{}", pr.key.id()),
                )
                .keywords([pr.author.clone(), pr.head_ref.clone()])
            })
            .collect();
        let files: Vec<PaletteEntry> = match &self.route {
            Route::Review(review) => review
                .read(cx)
                .detail()
                .map(|detail| {
                    detail
                        .files
                        .iter()
                        .map(|file| {
                            PaletteEntry::key(
                                file.path.clone(),
                                IconName::FileCode,
                                format!("file:{}", file.path),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let mut commands = vec![
            PaletteEntry::action("Refresh", IconName::RefreshCw, Box::new(Refresh)),
            PaletteEntry::action("Settings…", IconName::Settings, Box::new(OpenSettings)),
            PaletteEntry::action(
                "Make the interface larger",
                IconName::Plus,
                Box::new(ZoomIn),
            ),
            PaletteEntry::action(
                "Make the interface smaller",
                IconName::Minus,
                Box::new(ZoomOut),
            ),
        ];
        if matches!(self.route, Route::Review(_)) {
            commands.splice(
                0..0,
                [
                    PaletteEntry::action(
                        "Submit review…",
                        IconName::Send,
                        Box::new(app_state_ui::actions::SubmitReview),
                    ),
                    PaletteEntry::action(
                        "Review with agent…",
                        IconName::Sparkles,
                        Box::new(app_state_ui::actions::ReviewWithAgent),
                    ),
                    PaletteEntry::action(
                        "Open in Zed",
                        IconName::ExternalLink,
                        Box::new(app_state_ui::actions::OpenInZed),
                    ),
                    PaletteEntry::action(
                        "Back to pull requests",
                        IconName::ArrowLeft,
                        Box::new(GoBack),
                    ),
                ],
            );
        }
        let workspace = cx.entity();
        command_palette_ui::open(
            vec![
                PaletteGroup::new("Files", files),
                PaletteGroup::new("Pull requests", pull_requests),
                PaletteGroup::new("Commands", commands),
            ],
            move |key, window, cx| {
                workspace.update(cx, |this, cx| this.confirm_palette(&key, window, cx));
            },
            window,
            cx,
        );
    }

    fn confirm_palette(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = key.strip_prefix("pr:") {
            let found = cx
                .stores()
                .pull_requests()
                .read(cx)
                .pull_requests()
                .iter()
                .find(|pr| pr.key.id() == id)
                .map(|pr| pr.key.clone());
            if let Some(found) = found {
                self.open_pull_request(found, window, cx);
            }
        } else if let Some(path) = key.strip_prefix("file:")
            && let Route::Review(review) = &self.route
        {
            review.update(cx, |review, cx| review.reveal_file(path, cx));
        }
    }

    /// The open review screen, if any.
    pub fn review_view(&self) -> Option<Entity<ReviewThreadView>> {
        match &self.route {
            Route::Review(review) => Some(review.clone()),
            _ => None,
        }
    }

    /// The pull request whose review is open, if any.
    pub fn reviewing(&self, cx: &App) -> Option<pr_model::PullRequestKey> {
        match &self.route {
            Route::Review(review) => Some(review.read(cx).key().clone()),
            _ => None,
        }
    }

    /// The pull request list.
    pub fn pull_request_list(&self) -> &Entity<PullRequestList> {
        &self.pull_request_list
    }

    /// Selects `key` in the list and opens its review.
    pub fn open_pull_request(
        &mut self,
        key: pr_model::PullRequestKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pull_request_list
            .update(cx, |list, cx| list.select(key.clone(), cx));
        self.open_review(key, window, cx);
    }

    fn open_review(
        &mut self,
        key: pr_model::PullRequestKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let back_label = self.pull_request_list.read(cx).title(cx);
        let review = cx.new(|cx| ReviewThreadView::new(key, back_label, window, cx));
        let subscription = cx.subscribe_in(
            &review,
            window,
            |this, _, event: &ReviewThreadEvent, window, cx| match event {
                ReviewThreadEvent::Back => this.show_list(window, cx),
                ReviewThreadEvent::Open(key) => {
                    let key = key.clone();
                    this.pull_request_list
                        .update(cx, |list, cx| list.select(key.clone(), cx));
                    this.open_review(key, window, cx)
                }
            },
        );
        self.route_subscription = Some(subscription);
        window.focus(&review.focus_handle(cx), cx);
        self.route = Route::Review(review);
        cx.notify();
    }

    fn show_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if cx.stores().settings().read(cx).accounts().is_empty() {
            return self.show_sign_in(window, cx);
        }
        self.route = Route::PullRequests;
        self.route_subscription = None;
        window.focus(&self.pull_request_list.focus_handle(cx), cx);
        cx.notify();
    }

    fn zoom(&mut self, step: Option<isize>, window: &mut Window, cx: &mut Context<Self>) {
        let settings = cx.stores().settings().clone();
        let current = settings.read(cx).preferences().font_size;
        let next = match step {
            None => DEFAULT_FONT_SIZE,
            Some(step) => {
                let ix = FONT_SIZES
                    .iter()
                    .position(|size| *size >= current)
                    .unwrap_or(FONT_SIZES.len() - 1) as isize;
                FONT_SIZES[(ix + step).clamp(0, FONT_SIZES.len() as isize - 1) as usize]
            }
        };
        settings.update(cx, |settings, cx| {
            settings.update_preferences(cx, |preferences| preferences.font_size = next)
        });
        let preferences = settings.read(cx).preferences().clone();
        theme_ui::apply(&preferences, Some(window), cx);
    }

    fn render_title_bar(&self, cx: &App) -> AppTitleBar {
        let settings = cx.stores().settings().read(cx);
        let location: SharedString = match self.current_repository(cx) {
            Some(repo) => format!("{} / {}", repo.owner(), repo.name()).into(),
            None => "Switchfold".into(),
        };
        let workspace = self.entity.clone();
        let mut title_bar = AppTitleBar::new(location).on_notifications(move |_, window, cx| {
            if let Some(workspace) = workspace.upgrade() {
                workspace.update(cx, |this, cx| this.open_notifications(window, cx));
            }
        });
        match &self.route {
            Route::PullRequests => {
                let open = !self.pull_request_list.read(cx).is_sidebar_collapsed();
                title_bar = title_bar
                    .sidebar_open(open)
                    .title(pr_list_ui::ListTitle::new(self.pull_request_list.clone()))
                    .actions(pr_list_ui::ListActions::new(self.pull_request_list.clone()));
            }
            Route::SignIn(_) => {
                return title_bar.without_sidebar_toggle().without_account();
            }
            Route::Review(view) => {
                title_bar = title_bar
                    .sidebar_open(review_thread_ui::ReviewLayout::get(cx).outline)
                    .title(review_thread_ui::PullRequestTitle::new(view.clone()))
                    .actions(review_thread_ui::ReviewActions::new(view.clone()))
            }
            Route::Settings(_) => title_bar = title_bar.without_sidebar_toggle(),
        }
        match settings.viewer() {
            Some(viewer) => title_bar.viewer(viewer.display_name().to_string()),
            None => title_bar,
        }
    }

    fn render_status_bar(&self, cx: &App) -> AppStatusBar {
        let stores = cx.stores();
        let settings = stores.settings().read(cx);
        let pull_requests = stores.pull_requests().read(cx);
        let mut bar = AppStatusBar::new();
        if let Some(repo) = self.current_repository(cx)
            && let Some(path) = settings.clone_path(&repo)
        {
            bar = bar.clone_path(display_path(path, settings.home()));
        }
        let selected = match &self.route {
            Route::Review(review) => Some(review.read(cx).key().clone()),
            _ => self.pull_request_list.read(cx).selected().cloned(),
        };
        if let Some(pr) = selected.and_then(|key| pull_requests.pull_request(&key)) {
            bar = bar.branch(pr.head_ref.clone());
        }
        if let Some(synced) = pull_requests.last_synced() {
            bar = bar.sync(format!(
                "Synced {}",
                synced.minutes_age(pull_requests.now())
            ));
        }
        if let Some(agent) = settings.default_agent() {
            let state = match settings.agent_availability(&agent.id) {
                Some(availability) if availability.is_launchable() => "ready",
                Some(_) => "not installed",
                None => "checking…",
            };
            bar = bar.agent(format!("{} · {state}", agent.name));
        }
        if let Some(viewer) = settings.viewer() {
            bar = bar.account(viewer.login.clone());
        }
        bar
    }

    fn render_route(&self) -> AnyElement {
        match &self.route {
            Route::SignIn(sign_in) => sign_in.clone().into_any_element(),
            Route::PullRequests => self.pull_request_list.clone().into_any_element(),
            Route::Review(review) => review.clone().into_any_element(),
            Route::Settings(settings) => settings.clone().into_any_element(),
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .key_context(CONTEXT)
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(
                cx.listener(|this, _: &app_state_ui::actions::ToggleSidebar, _, cx| {
                    // The list handles its own sidebar before the action reaches here.
                    if matches!(this.route, Route::Review(_)) {
                        review_thread_ui::ReviewLayout::toggle_outline(cx);
                    }
                }),
            )
            .on_action(
                cx.listener(|this, _: &app_state_ui::actions::ToggleInspector, _, cx| {
                    if matches!(this.route, Route::Review(_)) {
                        review_thread_ui::ReviewLayout::toggle_inspector(cx);
                    }
                }),
            )
            .on_action(
                cx.listener(|this, _: &app_state_ui::actions::ToggleFocusMode, _, cx| {
                    if matches!(this.route, Route::Review(_)) {
                        review_thread_ui::ReviewLayout::toggle_focus(cx);
                    }
                }),
            )
            .on_action(cx.listener(|this, _: &ZoomIn, window, cx| this.zoom(Some(1), window, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, window, cx| this.zoom(Some(-1), window, cx)))
            .on_action(cx.listener(|this, _: &ResetZoom, window, cx| this.zoom(None, window, cx)))
            .on_action(|_: &CloseWindow, window, _| window.remove_window())
            .on_action(
                cx.listener(|this, _: &FocusSearch, window, cx| this.open_palette(window, cx)),
            )
            .on_action(cx.listener(|this, _: &OpenSettings, window, cx| {
                this.open_settings(SettingsPageKind::Accounts, window, cx)
            }))
            .on_action(cx.listener(|this, _: &GoBack, window, cx| {
                if matches!(this.route, Route::Settings(_)) {
                    this.show_list(window, cx);
                }
            }))
            .child(self.render_title_bar(cx))
            .child(div().flex_1().min_h_0().child(self.render_route()))
            .when(!matches!(self.route, Route::SignIn(_)), |this| {
                this.child(self.render_status_bar(cx))
            })
    }
}

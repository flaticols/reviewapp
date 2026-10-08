use std::rc::Rc;

use app_state_ui::actions::{FocusSearch, ToggleSidebar};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Selectable as _, Sizable as _, TitleBar,
    button::{Button, ButtonVariants as _},
    h_flex,
    kbd::Kbd,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, IntoElement, Keystroke, ParentElement as _, RenderOnce,
    SharedString, Styled as _, TitlebarOptions, Window, WindowOptions, div, point,
    prelude::FluentBuilder as _, px, rems, size,
};
use theme_ui::TypographyExt as _;
use widgets_ui::InitialsAvatar;

/// Window options for a Switchfold window: the title bar draws itself, and the
/// traffic lights sit centered in its 44 px height.
pub fn window_options(cx: &App) -> WindowOptions {
    let base = TitleBar::window_options();
    let bounds = gpui_kit::Bounds::centered(None, size(px(1440.), px(900.)), cx);
    WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some("Switchfold".into()),
            // Window geometry in points from the top-left corner. 14.5 was measured in
            // a real window: it puts the traffic lights' centers level with the title
            // bar's icons and text.
            traffic_light_position: Some(point(px(16.), px(14.5))),
            ..TitleBar::title_bar_options()
        }),
        window_bounds: Some(gpui_kit::WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(960.), px(600.))),
        ..base
    }
}

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The title bar: sidebar toggle, the current repository, a search trigger, the
/// current screen's actions and the signed-in account.
#[derive(IntoElement)]
pub struct AppTitleBar {
    location: SharedString,
    show_search: bool,
    show_sidebar_toggle: bool,
    sidebar_open: bool,
    show_account: bool,
    viewer_name: Option<SharedString>,
    title: Option<AnyElement>,
    actions: Option<AnyElement>,
    on_notifications: Option<ClickHandler>,
}

impl AppTitleBar {
    /// A title bar naming `location`, such as `flaticols / switchfold`.
    pub fn new(location: impl Into<SharedString>) -> Self {
        Self {
            location: location.into(),
            show_search: true,
            show_sidebar_toggle: true,
            sidebar_open: false,
            show_account: true,
            viewer_name: None,
            title: None,
            actions: None,
            on_notifications: None,
        }
    }

    /// Hides the search trigger, for screens with nothing to search yet.
    pub fn without_search(mut self) -> Self {
        self.show_search = false;
        self
    }

    /// Hides notifications and the account, for screens shown before signing in.
    pub fn without_account(mut self) -> Self {
        self.show_account = false;
        self
    }

    /// Hides the sidebar toggle, for screens without a sidebar.
    pub fn without_sidebar_toggle(mut self) -> Self {
        self.show_sidebar_toggle = false;
        self
    }

    /// Marks the sidebar toggle as on while the sidebar is showing.
    pub fn sidebar_open(mut self, open: bool) -> Self {
        self.sidebar_open = open;
        self
    }

    /// Shows the signed-in account's avatar.
    pub fn viewer(mut self, name: impl Into<SharedString>) -> Self {
        self.viewer_name = Some(name.into());
        self
    }

    /// Shows `title` in place of the location, such as the pull request under review.
    pub fn title(mut self, title: impl IntoElement) -> Self {
        self.title = Some(title.into_any_element());
        self
    }

    /// Shows the current screen's actions, such as Approve, before notifications.
    pub fn actions(mut self, actions: impl IntoElement) -> Self {
        self.actions = Some(actions.into_any_element());
        self
    }

    /// Handles the notifications button.
    pub fn on_notifications(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_notifications = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for AppTitleBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        // Leading and trailing share the free space equally so search stays centered;
        // when the trailing actions need more than half, search slides left and shrinks
        // rather than overlapping them.
        let leading = h_flex()
            .flex_1()
            .min_w_0()
            .gap_2()
            .when(self.show_sidebar_toggle, |this| {
                this.child(
                    Button::new("toggle-sidebar")
                        .ghost()
                        .small()
                        .icon(IconName::PanelLeft)
                        .selected(self.sidebar_open)
                        .tooltip_with_action(
                            if self.sidebar_open {
                                "Hide the sidebar"
                            } else {
                                "Show the sidebar"
                            },
                            &ToggleSidebar,
                            None,
                        )
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(ToggleSidebar), cx)
                        }),
                )
            })
            .child(match self.title {
                Some(title) => title,
                None => h_flex()
                    .min_w_0()
                    .gap_1p5()
                    .text_ui()
                    .text_color(cx.theme().muted_foreground)
                    .child(Icon::new(IconName::FolderGit2).small())
                    .child(div().min_w_0().truncate().child(self.location))
                    .into_any_element(),
            });

        let search = self.show_search.then(|| {
            // Button pins itself to flex-shrink 0; the wrapper is what gives way to the
            // title bar's actions on a narrow window.
            let button = Button::new("search")
                .outline()
                .small()
                .w_full()
                .accessibility_label("Search pull requests, files and commands")
                .on_click(|_, window, cx| window.dispatch_action(Box::new(FocusSearch), cx))
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .overflow_hidden()
                        .gap_2()
                        .text_ui()
                        .text_color(cx.theme().muted_foreground)
                        .child(Icon::new(IconName::Search).small())
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child("Search PRs, files, commands…"),
                        )
                        .child(
                            Kbd::new(Keystroke::parse("cmd-k").expect("valid keystroke")).outline(),
                        ),
                );
            div()
                .flex_shrink_1()
                .min_w(rems(4.5))
                .w(rems(22.5))
                .child(button)
        });

        // No-wrap keeps the actions' min-content width honest; otherwise button labels
        // count as wrappable and the actions overflow off the right edge.
        let trailing = h_flex()
            .flex_1()
            .flex_shrink_0()
            .whitespace_nowrap()
            .justify_end()
            .gap_2()
            .pr_3()
            .when_some(self.actions, |this, actions| this.child(actions))
            .when(self.show_account, |this| {
                this.child(
                    Button::new("notifications")
                        .ghost()
                        .small()
                        .icon(IconName::Bell)
                        .tooltip("Notifications")
                        .when_some(self.on_notifications, |this, handler| {
                            this.on_click(move |event, window, cx| handler(event, window, cx))
                        }),
                )
            })
            .when_some(
                self.viewer_name.filter(|_| self.show_account),
                |this, name| this.child(InitialsAvatar::new(&name)),
            );

        TitleBar::new()
            .h(rems(theme_ui::TITLE_BAR_REMS))
            .bg(cx.theme().title_bar)
            // TitleBar's bar never shrinks below its content's intrinsic width, and two
            // equal flex halves make that width twice the wider half. Absolute layout keeps
            // the row out of that sum so it always fits the window.
            .child(
                div().relative().flex_1().h_full().child(
                    h_flex()
                        .absolute()
                        .inset_0()
                        .gap_4()
                        .child(leading)
                        .children(search)
                        .child(trailing),
                ),
            )
    }
}

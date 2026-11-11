use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, status_bar::StatusBar};
use gpui_kit::{
    App, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _, Window,
    prelude::FluentBuilder as _,
};
use theme_ui::TypographyExt as _;

/// The status bar: where the current pull request lives on disk, its branch, sync
/// state, the default agent and the account in use.
#[derive(IntoElement, Default)]
pub struct AppStatusBar {
    clone_path: Option<SharedString>,
    branch: Option<SharedString>,
    sync: Option<SharedString>,
    agent: Option<SharedString>,
    account: Option<SharedString>,
}

impl AppStatusBar {
    /// An empty status bar.
    pub fn new() -> Self {
        Self::default()
    }

    /// The local clone, such as `~/src/switchfold`.
    pub fn clone_path(mut self, path: impl Into<SharedString>) -> Self {
        self.clone_path = Some(path.into());
        self
    }

    /// The branch being reviewed.
    pub fn branch(mut self, branch: impl Into<SharedString>) -> Self {
        self.branch = Some(branch.into());
        self
    }

    /// The sync state, such as `Synced 2 min ago`.
    pub fn sync(mut self, sync: impl Into<SharedString>) -> Self {
        self.sync = Some(sync.into());
        self
    }

    /// The default agent and its state, such as `Claude Code · ready`.
    pub fn agent(mut self, agent: impl Into<SharedString>) -> Self {
        self.agent = Some(agent.into());
        self
    }

    /// The account in use.
    pub fn account(mut self, account: impl Into<SharedString>) -> Self {
        self.account = Some(account.into());
        self
    }
}

fn item(icon: IconName, text: SharedString) -> impl IntoElement {
    h_flex()
        .gap_1p5()
        .child(Icon::new(icon).small())
        .child(text)
}

impl RenderOnce for AppStatusBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        StatusBar::new()
            .h_7()
            .px_4()
            .text_ui()
            .bg(cx.theme().status_bar)
            .left(
                h_flex()
                    .gap_5()
                    .when_some(self.clone_path, |this, path| {
                        this.child(item(IconName::FolderGit2, path))
                    })
                    .when_some(self.branch, |this, branch| {
                        this.child(item(IconName::GitBranch, branch))
                    })
                    .when_some(self.sync, |this, sync| {
                        this.child(item(IconName::RefreshCw, sync))
                    }),
            )
            .right(
                h_flex()
                    .gap_5()
                    .when_some(self.agent, |this, agent| {
                        this.child(item(IconName::Bot, agent))
                    })
                    .when_some(self.account, |this, account| {
                        this.child(item(IconName::Github, account))
                    }),
            )
    }
}

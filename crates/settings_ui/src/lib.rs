//! Settings: navigation between the Accounts, Repositories, Agents & editor,
//! Appearance and Keyboard pages.

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    sidebar::{Sidebar, SidebarCollapsible, SidebarGroup, SidebarMenu, SidebarMenuItem},
};
use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, EventEmitter, IntoElement, ParentElement as _,
    Render, Styled as _, Window, div, rems,
};
use settings_accounts_ui::AccountsPage;
use settings_agents_ui::AgentsPage;
use settings_appearance_ui::AppearancePage;
use settings_keyboard_ui::KeyboardPage;
use settings_repositories_ui::RepositoriesPage;

/// The pages of Settings, in navigation order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsPageKind {
    #[default]
    Accounts,
    Repositories,
    AgentsAndEditor,
    Appearance,
    Keyboard,
}

impl SettingsPageKind {
    /// Every page in navigation order.
    pub const ALL: [SettingsPageKind; 5] = [
        Self::Accounts,
        Self::Repositories,
        Self::AgentsAndEditor,
        Self::Appearance,
        Self::Keyboard,
    ];

    /// The navigation label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Accounts => "Accounts",
            Self::Repositories => "Repositories",
            Self::AgentsAndEditor => "Agents & editor",
            Self::Appearance => "Appearance",
            Self::Keyboard => "Keyboard",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Accounts => IconName::User,
            Self::Repositories => IconName::FolderGit2,
            Self::AgentsAndEditor => IconName::Bot,
            Self::Appearance => IconName::Eye,
            Self::Keyboard => IconName::Command,
        }
    }
}

/// What Settings asks its owner to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsEvent {
    /// Return to the pull request list.
    Back,
}

/// The Settings screen.
pub struct SettingsView {
    page: SettingsPageKind,
    accounts: Entity<AccountsPage>,
    repositories: Entity<RepositoriesPage>,
    agents: Entity<AgentsPage>,
    appearance: Entity<AppearancePage>,
}

impl EventEmitter<SettingsEvent> for SettingsView {}

impl SettingsView {
    /// Opens Settings at `page`.
    pub fn new(page: SettingsPageKind, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            page,
            accounts: cx.new(|cx| AccountsPage::new(window, cx)),
            repositories: cx.new(RepositoriesPage::new),
            agents: cx.new(|cx| AgentsPage::new(window, cx)),
            appearance: cx.new(|cx| AppearancePage::new(window, cx)),
        }
    }

    /// Shows `page`.
    pub fn open_page(&mut self, page: SettingsPageKind, cx: &mut Context<Self>) {
        self.page = page;
        cx.notify();
    }

    /// Shows Accounts with the personal access token form open.
    pub fn add_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.page = SettingsPageKind::Accounts;
        self.accounts
            .update(cx, |accounts, cx| accounts.show_token_form(window, cx));
        cx.notify();
    }

    fn render_page(&self) -> AnyElement {
        match self.page {
            SettingsPageKind::Accounts => self.accounts.clone().into_any_element(),
            SettingsPageKind::Repositories => self.repositories.clone().into_any_element(),
            SettingsPageKind::AgentsAndEditor => self.agents.clone().into_any_element(),
            SettingsPageKind::Appearance => self.appearance.clone().into_any_element(),
            SettingsPageKind::Keyboard => KeyboardPage::new().into_any_element(),
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let pages = SidebarMenu::new().children(SettingsPageKind::ALL.map(|page| {
            let view = view.clone();
            SidebarMenuItem::new(page.label())
                .icon(Icon::new(page.icon()))
                .active(self.page == page)
                .on_click(move |_, _, cx| view.update(cx, |this, cx| this.open_page(page, cx)))
        }));
        let back = view.clone();
        h_flex()
            .size_full()
            .items_stretch()
            .bg(cx.theme().background)
            .child(
                Sidebar::new("settings-sidebar")
                    .collapsible(SidebarCollapsible::None)
                    .w(rems(15.5))
                    .header(
                        Button::new("back-to-pull-requests")
                            .ghost()
                            .small()
                            .icon(IconName::ArrowLeft)
                            .label("Back to pull requests")
                            .on_click(move |_, _, cx| {
                                back.update(cx, |_, cx| cx.emit(SettingsEvent::Back))
                            }),
                    )
                    .child(SidebarGroup::new("Settings").child(pages)),
            )
            .child(div().flex_1().min_w_0().child(self.render_page()))
    }
}

//! Settings › Accounts: the GitHub accounts Switchfold loads pull requests with.

use app_state_ui::StoresExt as _;
use github_auth_ui::{TokenForm, TokenFormEvent};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Icon, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariant, ButtonVariants as _},
    dialog::DialogButtonProps,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
};
use gpui_kit::{
    AppContext as _, Context, Entity, FontWeight, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Subscription, Window, div,
};
use settings_model::{Account, AccountSource};
use widgets_ui::{
    AvatarSize, InitialsAvatar, SettingsPage, SettingsRow, SettingsSection, StatusTag, Tone,
};

/// The Accounts page. Owns the token form while it is open.
pub struct AccountsPage {
    token_form: Option<Entity<TokenForm>>,
    _subscriptions: Vec<Subscription>,
}

impl AccountsPage {
    /// Creates the page.
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = cx.stores().settings().clone();
        Self {
            token_form: None,
            _subscriptions: vec![cx.observe(&settings, |_, _, cx| cx.notify())],
        }
    }

    /// Opens the Add personal access token form.
    pub fn show_token_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.token_form.is_some() {
            return;
        }
        let form = cx.new(|cx| TokenForm::new(window, cx));
        self._subscriptions
            .push(cx.subscribe(&form, |this, _, event: &TokenFormEvent, cx| {
                if *event == TokenFormEvent::Done {
                    this.token_form = None;
                    cx.notify();
                }
            }));
        self.token_form = Some(form);
        cx.notify();
    }

    fn render_account(
        &self,
        account: &Account,
        is_default: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let id = format!("{}@{}", account.login, account.host);
        let login = account.login.clone();
        let host = account.host.clone();
        let mut row = SettingsRow::with_title_element(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(SharedString::from(account.login.clone())),
        )
        .leading(InitialsAvatar::new(account.display_name()).size(AvatarSize::Large))
        .description(account.description());
        if is_default {
            row = row.trailing(StatusTag::new(Tone::Neutral, "Default").icon(IconName::Check));
        }
        if account.is_expiring_soon() {
            row = row.trailing(StatusTag::new(Tone::Warning, "Expires soon").icon(IconName::Clock));
        }
        let primary = match account.source {
            AccountSource::Token { .. } if account.is_expiring_soon() => {
                let view = cx.entity();
                Button::new(SharedString::from(format!("renew-{id}")))
                    .outline()
                    .small()
                    .label("Renew…")
                    .on_click(move |_, window, cx| {
                        view.update(cx, |this, cx| this.show_token_form(window, cx))
                    })
            }
            _ => {
                let (login, host) = (login.clone(), host.clone());
                Button::new(SharedString::from(format!("sign-out-{id}")))
                    .outline()
                    .small()
                    .label("Sign out")
                    .on_click(move |_, window, cx| {
                        confirm_sign_out(login.clone(), host.clone(), window, cx)
                    })
            }
        };
        row.trailing(primary).trailing(
            Button::new(SharedString::from(format!("account-more-{id}")))
                .ghost()
                .small()
                .icon(IconName::Ellipsis)
                .accessibility_label("More account actions")
                .dropdown_menu_with_anchor(
                    gpui_kit::Anchor::TopRight,
                    move |menu: PopupMenu, _, _| {
                        let (default_login, default_host) = (login.clone(), host.clone());
                        let (sign_out_login, sign_out_host) = (login.clone(), host.clone());
                        menu.item(
                            PopupMenuItem::new("Make default")
                                .disabled(is_default)
                                .on_click(move |_, _, cx| {
                                    let (login, host) =
                                        (default_login.clone(), default_host.clone());
                                    cx.stores().settings().clone().update(cx, |settings, cx| {
                                        settings.make_default_account(&login, &host, cx)
                                    });
                                }),
                        )
                        .separator()
                        .item(
                            PopupMenuItem::new("Sign out…")
                                .icon(Icon::new(IconName::LogOut))
                                .on_click(move |_, window, cx| {
                                    confirm_sign_out(
                                        sign_out_login.clone(),
                                        sign_out_host.clone(),
                                        window,
                                        cx,
                                    )
                                }),
                        )
                    },
                ),
        )
    }
}

fn confirm_sign_out(login: String, host: String, window: &mut Window, cx: &mut gpui_kit::App) {
    window.open_alert_dialog(cx, move |alert, _, _| {
        let (login, host) = (login.clone(), host.clone());
        alert
            .title(format!("Sign out of {login}?"))
            .description(format!(
                "Pull requests from {host} stop loading for this account. A stored token is deleted from the Keychain."
            ))
            .confirm()
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Sign out")
                    .ok_variant(ButtonVariant::Danger)
                    .cancel_text("Cancel"),
            )
            .on_ok(move |_, _, cx| {
                let (login, host) = (login.clone(), host.clone());
                cx.stores()
                    .settings()
                    .clone()
                    .update(cx, |settings, cx| settings.sign_out(&login, &host, cx));
                true
            })
    });
}

impl Render for AccountsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accounts: Vec<Account> = cx.stores().settings().read(cx).accounts().to_vec();
        let rows: Vec<_> = accounts
            .iter()
            .enumerate()
            .map(|(ix, account)| self.render_account(account, ix == 0, cx))
            .collect();
        let view = cx.entity();
        let page = SettingsPage::new(
            "accounts-page",
            "Accounts",
            "GitHub accounts used to load pull requests. The first one is the default for new repositories.",
        )
        .child(
            SettingsSection::new()
                .rows(rows)
                .after(
                    Button::new("sign-in-github")
                        .outline()
                        .small()
                        .icon(IconName::Github)
                        .label("Sign in with GitHub…")
                        .on_click(|_, window, cx| github_auth_ui::sign_in_with_github(window, cx)),
                )
                .after(
                    Button::new("add-token")
                        .ghost()
                        .small()
                        .icon(IconName::KeyRound)
                        .label("Add token…")
                        .on_click(move |_, window, cx| view.update(cx, |this, cx| this.show_token_form(window, cx))),
                ),
        );
        match &self.token_form {
            Some(form) => page.child(form.clone()),
            None => page,
        }
    }
}

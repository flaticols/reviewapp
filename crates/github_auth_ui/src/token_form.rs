use app_state_ui::StoresExt as _;
use github_api::{REQUIRED_SCOPES, TokenCheck, check_token, is_token_shaped};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    label::Label,
    link::Link,
    notification::Notification,
    v_flex,
};
use gpui_kit::{
    AppContext as _, Context, Entity, EventEmitter, FontWeight, IntoElement, ParentElement as _,
    Render, Styled as _, Subscription, Window, div, prelude::FluentBuilder as _,
};
use settings_model::{Account, AccountSource};
use theme_ui::TypographyExt as _;
use widgets_ui::EditMenu;

/// What the form reports to its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenFormEvent {
    /// The user cancelled or the account was added.
    Done,
}

/// The Add personal access token form: host, token, live validation, and Add account.
pub struct TokenForm {
    host: Entity<InputState>,
    token: Entity<InputState>,
    check: Option<TokenCheck>,
    checking: bool,
    /// Bumped on every edit so an older check can't overwrite a newer one.
    revision: u64,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<TokenFormEvent> for TokenForm {}

impl TokenForm {
    /// An empty form for github.com.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let host = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value("github.com")
                .context_menu(false)
        });
        let token = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("ghp_…")
                .context_menu(false)
        });
        let subscription = cx.subscribe(&token, |this: &mut Self, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.recheck(cx);
            }
        });
        Self {
            host,
            token,
            check: None,
            checking: false,
            revision: 0,
            _subscriptions: vec![subscription],
        }
    }

    fn recheck(&mut self, cx: &mut Context<Self>) {
        self.revision += 1;
        let token = self.token.read(cx).unmask_value().trim().to_string();
        let host = self.host.read(cx).value().trim().to_string();
        self.check = None;
        self.checking = false;
        if token.is_empty() {
            cx.notify();
            return;
        }
        if !is_token_shaped(&token) {
            self.check = Some(TokenCheck::Malformed);
            cx.notify();
            return;
        }
        self.checking = true;
        let revision = self.revision;
        let task = cx.background_spawn(async move { check_token(&host, &token) });
        cx.spawn(async move |this, cx| {
            let check = task.await;
            this.update(cx, |this, cx| {
                if this.revision == revision {
                    this.check = Some(check);
                    this.checking = false;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(TokenCheck::Valid { login, .. }) = self.check.clone() else {
            return;
        };
        let host = self.host.read(cx).value().trim().to_string();
        let token = self.token.read(cx).unmask_value().to_string();
        let account_login = login.clone();
        let account_host = host.clone();
        let task = cx.background_spawn(async move {
            keychain::store_token(&account_login, &account_host, &token)
        });
        cx.spawn_in(window, async move |this, cx| {
            let stored = task.await;
            this.update_in(cx, |_, window, cx| match stored {
                Ok(()) => {
                    let account = Account::new(
                        login.clone(),
                        host.clone(),
                        AccountSource::Token {
                            expires_in_days: None,
                        },
                    );
                    cx.stores()
                        .settings()
                        .clone()
                        .update(cx, |settings, cx| settings.add_account(account, cx));
                    window.push_notification(
                        Notification::success(format!("Added {login} on {host}.")),
                        cx,
                    );
                    cx.emit(TokenFormEvent::Done);
                }
                Err(error) => window.push_notification(Notification::error(error.to_string()), cx),
            })
            .ok();
        })
        .detach();
    }
}

impl Render for TokenForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let valid = matches!(self.check, Some(TokenCheck::Valid { .. }));
        let checking = self.checking.then(|| {
            (
                IconName::LoaderCircle,
                cx.theme().muted_foreground,
                "Checking with GitHub…".to_string(),
            )
        });
        let status = self.check.as_ref().map(|check| match check {
            TokenCheck::Valid { login, scopes } if scopes.is_empty() => (
                IconName::CircleCheck,
                cx.theme().success,
                format!("Valid · signed in as {login} · fine-grained token"),
            ),
            TokenCheck::Valid { login, scopes } => (
                IconName::CircleCheck,
                cx.theme().success,
                format!(
                    "Valid · signed in as {login} · scopes {}",
                    scopes.join(", ")
                ),
            ),
            TokenCheck::Rejected => (
                IconName::CircleX,
                cx.theme().danger,
                "GitHub rejected this token. It may be expired or revoked.".to_string(),
            ),
            TokenCheck::Unreachable(message) => (
                IconName::CircleAlert,
                cx.theme().warning,
                format!("Couldn’t check the token: {message}"),
            ),
            TokenCheck::Malformed => (
                IconName::CircleX,
                cx.theme().danger,
                "This isn’t a GitHub token. Tokens start with ghp_ or github_pat_.".to_string(),
            ),
            TokenCheck::MissingScopes { missing, .. } => (
                IconName::CircleAlert,
                cx.theme().warning,
                format!("The token works but lacks {}.", missing.join(" and ")),
            ),
        });
        v_flex()
            .gap_3()
            .p_4()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius_lg)
            .child(div().text_body().font_weight(FontWeight::MEDIUM).child("Add personal access token"))
            .child(
                v_flex()
                    .gap_1p5()
                    .child(Label::new("Host").text_ui().font_weight(FontWeight::MEDIUM))
                    .child(EditMenu::new(
                        "host-edit-menu",
                        &self.host,
                        Input::new(&self.host).prefix(Icon::new(IconName::Github).small()),
                    )),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(Label::new("Token").text_ui().font_weight(FontWeight::MEDIUM))
                    .child(EditMenu::new(
                        "token-edit-menu",
                        &self.token,
                        Input::new(&self.token)
                            .mask_toggle()
                            .prefix(Icon::new(IconName::KeyRound).small()),
                    ))
                    .when_some(status.or(checking), |this, (icon, color, text)| {
                        this.child(
                            h_flex()
                                .gap_1p5()
                                .text_ui()
                                .text_color(color)
                                .child(Icon::new(icon).small())
                                .child(text),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap_2()
                    .text_ui()
                    .text_color(cx.theme().muted_foreground)
                    .child(div().flex_1().child(format!(
                        "Needs {}. Add workflow to re-run checks from Switchfold.",
                        REQUIRED_SCOPES.join(" and ")
                    )))
                    .child(
                        Link::new("create-token")
                            .href("https://github.com/settings/tokens/new?scopes=repo,read:org,workflow&description=Switchfold")
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child("Create a token on GitHub")
                                    .child(Icon::new(IconName::ArrowUpRight).xsmall()),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("cancel-token")
                            .ghost()
                            .small()
                            .label("Cancel")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(TokenFormEvent::Done))),
                    )
                    .child(
                        Button::new("add-account")
                            .small()
                            .label("Add account")
                            .disabled(!valid)
                            .on_click(cx.listener(|this, _, window, cx| this.add(window, cx))),
                    ),
            )
    }
}

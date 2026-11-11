//! The sign-in screen shown when no GitHub account is configured.

use github_api::GhSession;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::{
    AppContext as _, Context, EventEmitter, FontWeight, IntoElement, ParentElement as _, Render,
    Styled as _, Window, div,
};
use theme_ui::TypographyExt as _;

/// What the screen asks its owner to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignInEvent {
    /// Open Settings › Accounts with the token form.
    AddToken,
}

/// Whether the GitHub CLI session has been looked up yet.
enum GhState {
    Checking,
    SignedIn(GhSession),
    SignedOut,
}

/// The sign-in screen.
pub struct SignInView {
    gh: GhState,
}

impl EventEmitter<SignInEvent> for SignInView {}

impl SignInView {
    /// Creates the screen and looks up the GitHub CLI session in the background.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let lookup = cx.background_spawn(async { github_api::gh_session() });
        cx.spawn(async move |this, cx| {
            let session = lookup.await;
            this.update(cx, |this, cx| {
                this.gh = match session {
                    Some(session) => GhState::SignedIn(session),
                    None => GhState::SignedOut,
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
        Self {
            gh: GhState::Checking,
        }
    }
}

impl Render for SignInView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (gh_note, gh_available) = match &self.gh {
            GhState::Checking => ("Checking the GitHub CLI…".to_string(), false),
            GhState::SignedIn(session) => (
                format!("gh is signed in as {} on {}", session.login, session.host),
                true,
            ),
            GhState::SignedOut => ("gh isn’t signed in on this Mac".to_string(), false),
        };
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_6()
            .bg(cx.theme().background)
            .child(
                v_flex()
                    .items_center()
                    .gap_3()
                    .child(
                        h_flex()
                            .size_12()
                            .justify_center()
                            .rounded(cx.theme().radius_lg)
                            .bg(cx.theme().muted)
                            .child(Icon::new(IconName::GitPullRequest).size_5()),
                    )
                    .child(div().text_title().font_weight(FontWeight::MEDIUM).child("Review pull requests locally"))
                    .child(
                        div()
                            .max_w(gpui_kit::rems(22.))
                            .text_center()
                            .text_ui()
                            .text_color(cx.theme().muted_foreground)
                            .child("Switchfold reads diffs from your local clones, opens files in Zed and lets agents review over ACP."),
                    ),
            )
            .child(
                v_flex()
                    .items_center()
                    .gap_3()
                    .child(
                        Button::new("sign-in-github")
                            .primary()
                            .icon(IconName::Github)
                            .label("Sign in with GitHub")
                            .on_click(|_, window, cx| github_auth_ui::sign_in_with_github(window, cx)),
                    )
                    .child(
                        v_flex()
                            .items_center()
                            .gap_1p5()
                            .child(
                                Button::new("use-gh")
                                    .outline()
                                    .icon(IconName::Terminal)
                                    .label("Use GitHub CLI session")
                                    .disabled(!gh_available)
                                    .on_click(|_, window, cx| github_auth_ui::use_gh_session(window, cx)),
                            )
                            .child(
                                div()
                                    .text_meta()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(gh_note),
                            ),
                    )
                    .child(
                        Button::new("add-token")
                            .ghost()
                            .icon(IconName::KeyRound)
                            .label("Add a personal access token…")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(SignInEvent::AddToken))),
                    )
                    .child(
                        div()
                            .text_meta()
                            .text_color(cx.theme().muted_foreground)
                            .child("Credentials are stored in the macOS Keychain."),
                    ),
            )
    }
}

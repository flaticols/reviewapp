//! Adding GitHub accounts: device sign-in through the GitHub CLI, adopting an existing
//! `gh` session, and personal access tokens kept in the Keychain.

mod token_form;

use app_state_ui::StoresExt as _;
use github_api::{GhSession, start_gh_login};
use gpui_kit::component::{
    ActiveTheme as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::{DialogClose, DialogFooter},
    notification::Notification,
    spinner::Spinner,
    v_flex,
};
use gpui_kit::{
    App, AppContext as _, ClipboardItem, FontWeight, ParentElement as _, SharedString, Styled as _,
    Window, div,
};
use settings_model::{Account, AccountSource};
use theme_ui::TypographyExt as _;

pub use token_form::{TokenForm, TokenFormEvent};

/// Adds the account `gh` is signed in with, or explains why it can't.
pub fn use_gh_session(window: &mut Window, cx: &mut App) {
    let task = cx.background_spawn(async { github_api::gh_session() });
    window
        .spawn(cx, async move |cx| {
            let session = task.await;
            cx.update(|window, cx| match session {
                Some(session) => adopt(session, window, cx),
                None => window.push_notification(
                    Notification::warning(
                        "The GitHub CLI isn’t signed in. Run `gh auth login` or sign in with GitHub.",
                    ),
                    cx,
                ),
            })
            .ok();
        })
        .detach();
}

fn adopt(session: GhSession, window: &mut Window, cx: &mut App) {
    let account = Account::new(
        session.login.clone(),
        session.host.clone(),
        AccountSource::GhCli,
    );
    cx.stores()
        .settings()
        .clone()
        .update(cx, |settings, cx| settings.add_account(account, cx));
    window.push_notification(
        Notification::success(format!(
            "Signed in as {} on {}.",
            session.login, session.host
        )),
        cx,
    );
}

/// Signs in to github.com in the browser with a one-time code, through the GitHub CLI.
/// Shows the code in a dialog and adds the account once the user approves it.
pub fn sign_in_with_github(window: &mut Window, cx: &mut App) {
    let task = cx.background_spawn(async { start_gh_login("github.com") });
    window
        .spawn(cx, async move |cx| {
            let started = task.await;
            let login = match started {
                Ok((code, login)) => {
                    cx.update(|window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(code.code.clone()));
                        cx.open_url(&code.url);
                        open_code_dialog(code.code.clone(), code.url.clone(), window, cx);
                    })
                    .ok();
                    login
                }
                Err(error) => {
                    cx.update(|window, cx| {
                        window.push_notification(
                            Notification::error(format!("Couldn’t start GitHub sign-in: {error}")),
                            cx,
                        )
                    })
                    .ok();
                    return;
                }
            };
            let finished = cx
                .background_executor()
                .spawn(async move { login.wait() })
                .await;
            cx.update(|window, cx| {
                window.close_dialog(cx);
                match finished {
                    Ok(session) => adopt(session, window, cx),
                    Err(error) => {
                        window.push_notification(Notification::error(error.to_string()), cx)
                    }
                }
            })
            .ok();
        })
        .detach();
}

fn open_code_dialog(code: String, url: String, window: &mut Window, cx: &mut App) {
    let code: SharedString = code.into();
    let url: SharedString = url.into();
    window.open_dialog(cx, move |dialog, _, cx| {
        let url_for_button = url.clone();
        dialog
            .title("Sign in with GitHub")
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_body()
                            .child("Enter this code on the GitHub page that opened in your browser. It’s on your clipboard."),
                    )
                    .child(
                        div()
                            .py_3()
                            .text_center()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().muted)
                            .text_code(cx)
                            .text_size(gpui_kit::rems(1.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(code.clone()),
                    )
                    .child(
                        gpui_kit::component::h_flex()
                            .gap_2()
                            .text_ui()
                            .text_color(cx.theme().muted_foreground)
                            .child(Spinner::new())
                            .child("Waiting for GitHub…"),
                    ),
            )
            .footer(
                DialogFooter::new()
                    .gap_2()
                    .child(DialogClose::new().child(Button::new("cancel-sign-in").ghost().label("Cancel")))
                    .child(
                        Button::new("open-github")
                            .label("Open GitHub")
                            .on_click(move |_, _, cx| cx.open_url(&url_for_button)),
                    ),
            )
    });
}

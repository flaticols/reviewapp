use std::rc::Rc;

use app_state_ui::StoresExt as _;
use github_api::ApiError;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Icon, WindowExt as _,
    button::ButtonVariant,
    dialog::DialogButtonProps,
    menu::{PopupMenu, PopupMenuItem},
    notification::Notification,
};
use gpui_kit::{App, ClipboardItem, Task, Window};
use pr_model::{PullRequest, PullRequestKey, PullRequestState};

/// A pull request, for its context menu.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct PullRequestTarget {
    pub key: PullRequestKey,
    pub title: String,
    pub head_ref: String,
    pub state: PullRequestState,
}

impl PullRequestTarget {
    /// The target for `pull_request`.
    pub fn of(pull_request: &PullRequest) -> Self {
        Self {
            key: pull_request.key.clone(),
            title: pull_request.title.clone(),
            head_ref: pull_request.head_ref.clone(),
            state: pull_request.state,
        }
    }

    /// The pull request on GitHub.
    pub fn url(&self) -> String {
        format!(
            "https://github.com/{}/pull/{}",
            self.key.repo(),
            self.key.number()
        )
    }

    /// `[Title (#42)](url)`, which Slack and most chat tools turn into a link.
    pub fn markdown_link(&self) -> String {
        format!("[{} (#{})]({})", self.title, self.key.number(), self.url())
    }

    /// The command that asks Claude Code or Codex to review it.
    pub fn review_command(&self) -> String {
        format!("/review {}", self.url())
    }
}

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

/// Adds every action on `target` to `menu`. `review_with_agent` opens the pull request's
/// Review with an agent form, when the caller can.
pub fn pull_request_menu(
    menu: PopupMenu,
    target: &PullRequestTarget,
    review_with_agent: Option<Handler>,
) -> PopupMenu {
    let url = target.url();
    let zed_key = target.key.clone();
    let checkout_key = target.key.clone();
    let diff_key = target.key.clone();
    let head_ref = target.head_ref.clone();
    let approve_target = target.clone();
    let request_key = target.key.clone();
    let state_target = target.clone();
    let open = target.state == PullRequestState::Closed;
    let copy_url = url.clone();
    let markdown = target.markdown_link();
    let command = target.review_command();
    let branch = target.head_ref.clone();

    let menu = menu
        .item(
            PopupMenuItem::new("Open on GitHub")
                .icon(Icon::new(IconName::Globe))
                .on_click(move |_, _, cx| cx.open_url(&url)),
        )
        .item(
            PopupMenuItem::new("Open in Zed")
                .icon(Icon::new(IconName::ExternalLink))
                .on_click(move |_, window, cx| {
                    crate::open_in_zed(&zed_key, None, None, window, cx)
                }),
        )
        .item(
            PopupMenuItem::new("Diff in Zed")
                .icon(Icon::new(IconName::FileDiff))
                .on_click(move |_, window, cx| crate::diff_in_zed(&diff_key, window, cx)),
        )
        .item(
            PopupMenuItem::new("Check out")
                .icon(Icon::new(IconName::GitBranch))
                .on_click(move |_, window, cx| {
                    crate::check_out(&checkout_key, &head_ref, window, cx)
                }),
        );
    let menu = match review_with_agent {
        Some(review) => menu.item(
            PopupMenuItem::new("Review with agent…")
                .icon(Icon::new(IconName::Sparkles))
                .on_click(move |_, window, cx| review(window, cx)),
        ),
        None => menu,
    };
    let menu = menu.separator();
    let menu = if target.state == PullRequestState::Merged {
        menu
    } else {
        menu.item(
            PopupMenuItem::new("Approve…")
                .icon(Icon::new(IconName::Check))
                .on_click(move |_, window, cx| confirm_approve(&approve_target, window, cx)),
        )
        .item(
            PopupMenuItem::new("Request review…")
                .icon(Icon::new(IconName::UserPlus))
                .on_click(move |_, window, cx| {
                    crate::open_request_review(request_key.clone(), window, cx)
                }),
        )
        .item(
            PopupMenuItem::new(if open {
                "Reopen pull request…"
            } else {
                "Close pull request…"
            })
            .icon(Icon::new(if open {
                IconName::GitPullRequest
            } else {
                IconName::GitPullRequestClosed
            }))
            .on_click(move |_, window, cx| confirm_set_open(&state_target, open, window, cx)),
        )
    };
    menu.separator()
        .item(
            PopupMenuItem::new("Copy link")
                .icon(Icon::new(IconName::Link))
                .on_click(move |_, _, cx| copy(&copy_url, cx)),
        )
        .item(
            PopupMenuItem::new("Copy Markdown link (Slack)")
                .icon(Icon::new(IconName::Copy))
                .on_click(move |_, _, cx| copy(&markdown, cx)),
        )
        .item(
            PopupMenuItem::new("Copy /review command for agents")
                .icon(Icon::new(IconName::Terminal))
                .on_click(move |_, _, cx| copy(&command, cx)),
        )
        .item(
            PopupMenuItem::new("Copy branch name")
                .icon(Icon::new(IconName::GitBranch))
                .on_click(move |_, _, cx| copy(&branch, cx)),
        )
}

fn copy(text: &str, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
}

/// Shows `task`'s outcome as a notification: `done` when it succeeded, the error
/// otherwise.
fn report(task: Task<Result<(), ApiError>>, done: String, window: &mut Window, cx: &mut App) {
    window
        .spawn(cx, async move |cx| {
            let result = task.await;
            cx.update(|window, cx| match result {
                Ok(()) => window.push_notification(Notification::success(done), cx),
                Err(error) => window.push_notification(Notification::error(error.to_string()), cx),
            })
            .ok();
        })
        .detach();
}

fn confirm_approve(target: &PullRequestTarget, window: &mut Window, cx: &mut App) {
    let key = target.key.clone();
    let title = format!("Approve #{}?", key.number());
    let description = format!(
        "Posts your approval of “{}” to GitHub now, with any pending comments.",
        target.title
    );
    window.open_alert_dialog(cx, move |alert, _, _| {
        let key = key.clone();
        alert
            .title(title.clone())
            .description(description.clone())
            .confirm()
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Approve")
                    .ok_variant(ButtonVariant::Success)
                    .cancel_text("Cancel"),
            )
            .on_ok(move |_, window, cx| {
                let viewer = cx.stores().settings().read(cx).viewer_actor();
                let store = cx.stores().pull_requests().clone();
                let task = store.update(cx, |store, cx| store.approve(&key, &viewer, cx));
                report(task, format!("Approved #{}.", key.number()), window, cx);
                true
            })
    });
}

fn confirm_set_open(target: &PullRequestTarget, open: bool, window: &mut Window, cx: &mut App) {
    let key = target.key.clone();
    let (title, verb, done) = if open {
        (
            format!("Reopen #{}?", key.number()),
            "Reopen",
            format!("Reopened #{}.", key.number()),
        )
    } else {
        (
            format!("Close #{}?", key.number()),
            "Close pull request",
            format!("Closed #{}.", key.number()),
        )
    };
    let description = if open {
        format!("“{}” opens again for review.", target.title)
    } else {
        format!(
            "“{}” closes without merging. You can reopen it later.",
            target.title
        )
    };
    window.open_alert_dialog(cx, move |alert, _, _| {
        let key = key.clone();
        let done = done.clone();
        alert
            .title(title.clone())
            .description(description.clone())
            .confirm()
            .button_props(
                DialogButtonProps::default()
                    .ok_text(verb)
                    .ok_variant(if open {
                        ButtonVariant::Primary
                    } else {
                        ButtonVariant::Danger
                    })
                    .cancel_text("Cancel"),
            )
            .on_ok(move |_, window, cx| {
                let store = cx.stores().pull_requests().clone();
                let task = store.update(cx, |store, cx| store.set_open(&key, open, cx));
                report(task, done.clone(), window, cx);
                true
            })
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use pr_model::RepoSlug;

    #[test]
    fn links_and_commands_name_the_pull_request() {
        let target = PullRequestTarget {
            key: PullRequestKey::new(RepoSlug::new("octocat", "hello-world"), 42),
            title: "fix: update greeting".into(),
            head_ref: "fix/update-greeting".into(),
            state: PullRequestState::Open,
        };
        assert_eq!(
            target.markdown_link(),
            "[fix: update greeting (#42)](https://github.com/octocat/hello-world/pull/42)"
        );
        assert_eq!(
            target.review_command(),
            "/review https://github.com/octocat/hello-world/pull/42"
        );
    }
}

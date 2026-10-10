use std::path::PathBuf;

use app_state_ui::StoresExt as _;
use gpui_kit::component::{WindowExt as _, notification::Notification};
use gpui_kit::{App, AppContext as _, Window};
use pr_model::{PullRequestKey, RepoSlug};

/// The mapped clone of `key`'s repository, or a notification saying how to map one.
pub(crate) fn clone_of(key: &PullRequestKey, window: &mut Window, cx: &mut App) -> Option<PathBuf> {
    clone_of_repo(key.repo(), window, cx)
}

/// The mapped clone of `repo`, or a notification saying how to map one.
pub(crate) fn clone_of_repo(repo: &RepoSlug, window: &mut Window, cx: &mut App) -> Option<PathBuf> {
    let clone = cx
        .stores()
        .settings()
        .read(cx)
        .clone_path(repo)
        .map(|path| path.to_path_buf());
    match clone {
        Some(clone) if clone.is_dir() => Some(clone),
        Some(clone) => {
            window.push_notification(
                Notification::warning(format!(
                    "Couldn’t find {}. Locate the clone in Settings › Repositories.",
                    clone.display()
                )),
                cx,
            );
            None
        }
        None => {
            window.push_notification(
                Notification::warning(format!(
                    "{repo} isn’t cloned. Map it to a local clone in Settings › Repositories."
                )),
                cx,
            );
            None
        }
    }
}

/// Checks out `head_ref` of `key` in its clone, in a worktree when the preferences ask
/// for one, and reports where.
pub fn check_out(key: &PullRequestKey, head_ref: &str, window: &mut Window, cx: &mut App) {
    let Some(clone) = clone_of(key, window, cx) else {
        return;
    };
    let settings = cx.stores().settings().read(cx);
    let worktree = settings.preferences().worktree_per_pull_request;
    let home = settings.home().map(|home| home.to_path_buf());
    let request = git_worktree::Checkout::new(clone, head_ref.to_string(), key.number());
    let request = if worktree {
        request.in_worktree(home.unwrap_or_default().join(".switchfold/worktrees"))
    } else {
        request
    };
    let task = cx.background_spawn(async move { git_worktree::check_out(&request) });
    window
        .spawn(cx, async move |cx| {
            let result = task.await;
            cx.update(|window, cx| match result {
                Ok(path) => window.push_notification(
                    Notification::success(format!("Checked out into {}.", path.display())),
                    cx,
                ),
                Err(error) => window.push_notification(Notification::error(error.to_string()), cx),
            })
            .ok();
        })
        .detach();
}

/// Shows the clone of `repo` in Finder.
pub fn reveal_repository(repo: &RepoSlug, window: &mut Window, cx: &mut App) {
    if let Some(clone) = clone_of_repo(repo, window, cx) {
        cx.reveal_path(&clone);
    }
}

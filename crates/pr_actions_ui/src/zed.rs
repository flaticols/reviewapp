use std::path::PathBuf;
use std::time::Duration;

use app_state_ui::{DetailState, StoresExt as _};
use gpui_kit::component::{WindowExt as _, notification::Notification};
use gpui_kit::{App, AppContext as _, Task, Window};
use pr_model::PullRequestKey;
use review_model::PullRequestDetail;
use settings_model::OpenFilesAt;

/// How long Zed gets to open a project before a link aimed at its active window.
const ZED_SETTLE: Duration = Duration::from_millis(1200);

/// `key`'s review data, loading it first when needed. `None` when it fails to load.
pub fn detail_when_loaded(key: &PullRequestKey, cx: &mut App) -> Task<Option<PullRequestDetail>> {
    let store = cx.stores().pull_requests().clone();
    if let Some(detail) = store.read(cx).loaded_detail(key) {
        return Task::ready(Some(detail.clone()));
    }
    store.update(cx, |store, cx| store.load_detail(key, cx));
    let key = key.clone();
    cx.spawn(async move |cx| {
        loop {
            let state = cx.update(|cx| match store.read(cx).detail(&key) {
                Some(DetailState::Loaded(detail)) => Some(Some((**detail).clone())),
                Some(DetailState::Loading) => None,
                _ => Some(None),
            });
            match state {
                Some(done) => return done,
                None => {
                    cx.background_executor()
                        .timer(Duration::from_millis(100))
                        .await
                }
            }
        }
    })
}

/// Where Zed should open `key`'s code: the clone when the preferences ask for the
/// working tree or it already has the head commit checked out, else the review
/// worktree at the head commit. Runs git; resolve it off the UI thread.
struct CheckoutPlan {
    clone: PathBuf,
    review_tree: Option<git_worktree::ReviewTree>,
}

impl CheckoutPlan {
    fn resolve(self) -> Result<PathBuf, String> {
        let Some(tree) = self.review_tree else {
            return Ok(self.clone);
        };
        if git_worktree::head_of(&self.clone).as_deref() == Some(tree.head_sha()) {
            return Ok(self.clone);
        }
        tree.ensure().map_err(|error| error.to_string())
    }
}

fn plan(
    key: &PullRequestKey,
    detail: &PullRequestDetail,
    window: &mut Window,
    cx: &mut App,
) -> Option<CheckoutPlan> {
    let clone = crate::local::clone_of(key, window, cx)?;
    let settings = cx.stores().settings().read(cx);
    let at_head = settings.preferences().open_files_at == OpenFilesAt::PullRequestHead;
    let review_tree = match (at_head, detail.commits.last(), settings.home()) {
        (true, Some(head), Some(home)) => Some(git_worktree::ReviewTree::new(
            clone.clone(),
            key.number(),
            head.sha.clone(),
            detail.summary.base_ref.clone(),
            home.join(".switchfold/review-worktrees"),
        )),
        _ => None,
    };
    Some(CheckoutPlan { clone, review_tree })
}

fn zed_command(cx: &App) -> String {
    cx.stores()
        .settings()
        .read(cx)
        .preferences()
        .zed_command
        .clone()
}

fn notify_error(window: &mut Window, cx: &mut App, message: String) {
    window.push_notification(Notification::error(message), cx);
}

/// Runs `then` with the checkout of `key` Zed should open, once the review data has
/// loaded and the checkout is ready.
fn with_checkout(
    key: &PullRequestKey,
    window: &mut Window,
    cx: &mut App,
    then: impl FnOnce(PathBuf, PullRequestDetail, &mut Window, &mut App) + 'static,
) {
    let detail = detail_when_loaded(key, cx);
    let key = key.clone();
    window
        .spawn(cx, async move |cx| {
            let Some(detail) = detail.await else {
                cx.update(|window, cx| {
                    notify_error(
                        window,
                        cx,
                        format!("Couldn’t load #{} to find its code.", key.number()),
                    )
                })
                .ok();
                return;
            };
            let Ok(Some(plan)) = cx.update(|window, cx| plan(&key, &detail, window, cx)) else {
                return;
            };
            let root = cx.background_spawn(async move { plan.resolve() }).await;
            cx.update(|window, cx| match root {
                Ok(root) => then(root, detail, window, cx),
                Err(error) => notify_error(
                    window,
                    cx,
                    format!("Couldn’t check out #{} for Zed: {error}", key.number()),
                ),
            })
            .ok();
        })
        .detach();
}

/// Runs a Zed launch off the UI thread and reports a failure.
fn launch(
    run: impl FnOnce() -> Result<(), zed_bridge::LaunchError> + Send + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Task<bool> {
    let task = cx.background_spawn(async move { run() });
    window.spawn(cx, async move |cx| {
        let result = task.await;
        let launched = result.is_ok();
        if let Err(error) = result {
            cx.update(|window, cx| {
                notify_error(
                    window,
                    cx,
                    format!("{error}. Check the Zed command in Settings › Agents & editor."),
                )
            })
            .ok();
        }
        launched
    })
}

/// Opens `key`'s code in Zed as a project, at `path` (relative to the repository root)
/// and `line` when given.
pub fn open_in_zed(
    key: &PullRequestKey,
    path: Option<&str>,
    line: Option<u32>,
    window: &mut Window,
    cx: &mut App,
) {
    let path = path.map(str::to_string);
    with_checkout(key, window, cx, move |root, _, window, cx| {
        let zed = zed_command(cx);
        launch(
            move || zed_bridge::open_project(&zed, &root, path.as_deref().map(|p| (p, line))),
            window,
            cx,
        )
        .detach();
    });
}

/// Shows the whole of `key`'s diff in one Zed multi-file diff, from copies of both
/// sides written under `~/.switchfold/zed-diffs`.
pub fn diff_in_zed(key: &PullRequestKey, window: &mut Window, cx: &mut App) {
    let home = cx
        .stores()
        .settings()
        .read(cx)
        .home()
        .map(|h| h.to_path_buf());
    let number = key.number();
    with_checkout(key, window, cx, move |root, detail, window, cx| {
        let Some(head) = detail.commits.last().map(|c| c.sha.clone()) else {
            return;
        };
        let name = root
            .file_name()
            .map(|n| {
                n.to_string_lossy()
                    .trim_end_matches(&format!("-pr-{number}"))
                    .to_string()
            })
            .unwrap_or_else(|| "repo".into());
        let folder = home
            .unwrap_or_else(std::env::temp_dir)
            .join(".switchfold/zed-diffs")
            .join(format!("{name}-pr-{number}"));
        let base = format!("origin/{}", detail.summary.base_ref);
        let zed = zed_command(cx);
        let export = cx.background_spawn(async move {
            std::fs::remove_dir_all(&folder).ok();
            let (base_dir, head_dir) = (folder.join("base"), folder.join("head"));
            let clone = git_diff::LocalClone::open(&root).map_err(|e| e.to_string())?;
            let changed = clone
                .export_sides(&base, &head, &base_dir, &head_dir)
                .map_err(|e| e.to_string())?;
            // `zed --diff` compares folders, so both sides must exist even when one is
            // empty because every file was added or deleted.
            std::fs::create_dir_all(&base_dir).map_err(|e| e.to_string())?;
            std::fs::create_dir_all(&head_dir).map_err(|e| e.to_string())?;
            Ok::<_, String>((changed, base_dir, head_dir))
        });
        window
            .spawn(cx, async move |cx| {
                let result = export.await;
                cx.update(|window, cx| match result {
                    Ok((0, _, _)) => window.push_notification(
                        Notification::info(format!("#{number} changes no files.")),
                        cx,
                    ),
                    Ok((_, base_dir, head_dir)) => launch(
                        move || zed_bridge::open_diff(&zed, &base_dir, &head_dir),
                        window,
                        cx,
                    )
                    .detach(),
                    Err(error) => notify_error(
                        window,
                        cx,
                        format!("Couldn’t write #{number}’s diff for Zed: {error}"),
                    ),
                })
                .ok();
            })
            .detach();
    });
}

/// Opens commit `sha` of `key` in Zed's commit view.
pub fn open_commit_in_zed(key: &PullRequestKey, sha: &str, window: &mut Window, cx: &mut App) {
    let sha = sha.to_string();
    with_checkout(key, window, cx, move |root, _, window, cx| {
        let url = zed_bridge::commit_url(&root, &sha);
        launch(move || zed_bridge::open_url(&url), window, cx).detach();
    });
}

/// Opens `path` at `line` of `key` in Zed and starts a thread in Zed's agent panel
/// about `subject`, such as a review comment or an agent finding. The prompt is only
/// filled in: the reviewer reads it and presses Send in Zed.
pub fn ask_zed_agent(
    key: &PullRequestKey,
    path: &str,
    line: u32,
    subject: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let path = path.to_string();
    let subject = subject.trim().to_string();
    let key_label = format!("{}#{}", key.repo(), key.number());
    with_checkout(key, window, cx, move |root, detail, window, cx| {
        let zed = zed_command(cx);
        let prompt = format!(
            "This is about pull request {key_label}, “{}”, at {path}:{line}. The pull request is checked out at {}.\n\n{subject}\n\nRead the code around that line and suggest how to address this.",
            detail.summary.title,
            root.display(),
        );
        let url = zed_bridge::agent_prompt_url(&prompt);
        let opened = launch(
            move || zed_bridge::open_project(&zed, &root, Some((&path, Some(line)))),
            window,
            cx,
        );
        window
            .spawn(cx, async move |cx| {
                if !opened.await {
                    return;
                }
                // The agent link goes to Zed's active window, so the project has to be
                // open and focused first.
                cx.background_executor().timer(ZED_SETTLE).await;
                let task = cx
                    .update(|window, cx| launch(move || zed_bridge::open_url(&url), window, cx))
                    .ok();
                if let Some(task) = task {
                    task.await;
                }
            })
            .detach();
    });
}

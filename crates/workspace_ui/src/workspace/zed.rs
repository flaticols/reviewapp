//! What Zed asks of the workspace: `dev.switchfold.review://` links and tool calls from
//! Zed's agent through Switchfold's MCP server.

use std::path::Path;
use std::time::Duration;

use app_state_ui::StoresExt as _;
use gpui_kit::component::{WindowExt as _, notification::Notification};
use gpui_kit::{AppContext as _, Context, Entity, Task, Window};
use pr_model::{PullRequestKey, RepoSlug};
use review_model::{FindingState, PullRequestDetail};
use review_thread_ui::ReviewThreadView;
use serde_json::{Value, json};
use zed_bridge::mcp::{BridgeListener, BridgeReply, BridgeRequest, tool};
use zed_bridge::{Located, SwitchfoldLink};

use super::Workspace;

/// How long a link or tool call waits for a pull request's review data.
const LOAD_TIMEOUT: Duration = Duration::from_secs(45);

type Pending = (BridgeRequest, async_channel::Sender<BridgeReply>);

impl Workspace {
    /// Answers Zed's agent on `socket` for as long as the workspace lives.
    pub fn listen_for_zed(&mut self, socket: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let listener = match BridgeListener::bind(socket) {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!("switchfold: not answering Zed: {error}");
                return;
            }
        };
        let (requests, incoming) = async_channel::unbounded::<Pending>();
        listener.serve(move |request| {
            let (reply, answer) = async_channel::bounded(1);
            if requests.send_blocking((request, reply)).is_err() {
                return BridgeReply::error("Switchfold is closing.");
            }
            answer
                .recv_blocking()
                .unwrap_or_else(|_| BridgeReply::error("Switchfold didn’t answer."))
        });
        cx.spawn_in(window, async move |this, cx| {
            while let Ok((request, reply)) = incoming.recv().await {
                let Ok(answer) =
                    this.update_in(cx, |this, window, cx| this.answer_zed(request, window, cx))
                else {
                    break;
                };
                cx.background_spawn(async move {
                    reply.send(answer.await).await.ok();
                })
                .detach();
            }
        })
        .detach();
    }

    /// Follows a `dev.switchfold.review://` link.
    pub fn open_link(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(link) = SwitchfoldLink::parse(url) else {
            window.push_notification(
                Notification::warning(format!("Switchfold doesn’t know the link {url}")),
                cx,
            );
            return;
        };
        window.activate_window();
        match link {
            SwitchfoldLink::Open { key, file, line } => {
                let view = self.show_pull_request(key, window, cx);
                if let Some(file) = file {
                    self.when_review_loaded(view, window, cx, move |view, window, cx| match line {
                        Some(line) => {
                            view.comment_on_line(&file, line, window, cx);
                        }
                        None => view.reveal_file(&file, cx),
                    });
                }
            }
            SwitchfoldLink::Comment { file, line } => {
                self.comment_from_zed(&file, line, window, cx)
            }
        }
    }

    fn comment_from_zed(
        &mut self,
        file: &Path,
        line: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(located) = zed_bridge::locate(file) else {
            window.push_notification(
                Notification::warning(format!("{} isn’t in a git repository.", file.display())),
                cx,
            );
            return;
        };
        let Some(key) = self.pull_request_for(&located, cx) else {
            let branch = located.branch.as_deref().unwrap_or("this checkout");
            window.push_notification(
                Notification::warning(format!(
                    "No pull request in your inboxes comes from {branch} in {}.",
                    located.clone.display()
                )),
                cx,
            );
            return;
        };
        let view = self.show_pull_request(key, window, cx);
        let path = located.relative.clone();
        self.when_review_loaded(view, window, cx, move |view, window, cx| {
            if !view.comment_on_line(&path, line, window, cx) {
                window.push_notification(
                    Notification::info(format!(
                        "Line {line} of {path} isn’t in this pull request’s diff, so GitHub won’t take a comment there."
                    )),
                    cx,
                );
            }
        });
    }

    /// The pull request a file in `located` belongs to: the one a Switchfold worktree is
    /// named after, else the one whose head branch is checked out, else the one open now
    /// when it's in the same repository.
    fn pull_request_for(&self, located: &Located, cx: &Context<Self>) -> Option<PullRequestKey> {
        let repo = self.repository_of(&located.clone, cx)?;
        if let Some(number) = located.number_hint {
            return Some(PullRequestKey::new(repo, number));
        }
        let store = cx.stores().pull_requests().read(cx);
        if let Some(branch) = &located.branch
            && let Some(pr) = store
                .pull_requests()
                .iter()
                .find(|pr| pr.key.repo() == &repo && &pr.head_ref == branch)
        {
            return Some(pr.key.clone());
        }
        self.current_review(cx)
            .map(|view| view.read(cx).key().clone())
            .filter(|key| key.repo() == &repo)
    }

    /// The repository the settings map to `clone`, else the GitHub remote of the clone.
    fn repository_of(&self, clone: &Path, cx: &Context<Self>) -> Option<RepoSlug> {
        let canonical = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let clone = canonical(clone);
        let settings = cx.stores().settings().read(cx);
        settings
            .repositories()
            .iter()
            .find(|mapping| {
                mapping
                    .local_path
                    .as_deref()
                    .is_some_and(|path| canonical(path) == clone)
            })
            .and_then(|mapping| RepoSlug::parse(&mapping.slug))
            .or_else(|| git_diff::LocalClone::open(&clone).ok()?.github_slug())
    }

    fn current_review(&self, cx: &Context<Self>) -> Option<Entity<ReviewThreadView>> {
        self.review_view(cx)
    }

    /// Shows `key`, reusing the open review when it's the same pull request.
    fn show_pull_request(
        &mut self,
        key: PullRequestKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<ReviewThreadView>> {
        let current = self.current_review(cx);
        if current
            .as_ref()
            .is_none_or(|view| view.read(cx).key() != &key)
        {
            self.open_pull_request(key, window, cx);
        }
        self.current_review(cx)
    }

    /// Runs `then` on `view` once its review data has loaded.
    fn when_review_loaded(
        &self,
        view: Option<Entity<ReviewThreadView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut ReviewThreadView, &mut Window, &mut Context<ReviewThreadView>) + 'static,
    ) {
        let Some(view) = view else {
            return;
        };
        cx.spawn_in(window, async move |_, cx| {
            let started = std::time::Instant::now();
            while started.elapsed() < LOAD_TIMEOUT {
                let ready = cx
                    .update(|_, cx| view.read(cx).detail().is_some())
                    .unwrap_or(false);
                if ready {
                    view.update_in(cx, |view, window, cx| then(view, window, cx))
                        .ok();
                    return;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
            }
        })
        .detach();
    }

    /// The pull request a tool call names, or the one open in Switchfold.
    fn requested_key(
        &self,
        request: &BridgeRequest,
        cx: &Context<Self>,
    ) -> Result<PullRequestKey, BridgeReply> {
        match request.text("pull_request") {
            Some(text) => parse_key(&text).ok_or_else(|| {
                BridgeReply::error(format!(
                    "“{text}” isn’t a pull request. Use owner/name#number."
                ))
            }),
            None => self
                .current_review(cx)
                .map(|view| view.read(cx).key().clone())
                .ok_or_else(|| {
                    BridgeReply::error(
                        "No pull request is open in Switchfold. Pass pull_request as owner/name#number, or call list_pull_requests.",
                    )
                }),
        }
    }

    /// Answers one tool call from Zed's agent.
    pub fn answer_zed(
        &mut self,
        request: BridgeRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<BridgeReply> {
        match request.tool.as_str() {
            tool::LIST => Task::ready(self.list_for_zed(cx)),
            tool::SHOW => {
                let key = match self.requested_key(&request, cx) {
                    Ok(key) => key,
                    Err(reply) => return Task::ready(reply),
                };
                window.activate_window();
                let view = self.show_pull_request(key.clone(), window, cx);
                if let Some(path) = request.text("path") {
                    self.when_review_loaded(view, window, cx, move |view, _, cx| {
                        view.reveal_file(&path, cx)
                    });
                }
                Task::ready(BridgeReply::ok(format!(
                    "Showing {}#{} in Switchfold.",
                    key.repo(),
                    key.number()
                )))
            }
            _ => {
                let key = match self.requested_key(&request, cx) {
                    Ok(key) => key,
                    Err(reply) => return Task::ready(reply),
                };
                let detail = pr_actions_ui::detail_when_loaded(&key, cx);
                cx.spawn(async move |this, cx| {
                    let Some(detail) = detail.await else {
                        return BridgeReply::error(format!(
                            "Couldn’t load {}#{} from GitHub.",
                            key.repo(),
                            key.number()
                        ));
                    };
                    this.update(cx, |this, cx| {
                        this.answer_with_detail(&request, &key, &detail, cx)
                    })
                    .unwrap_or_else(|_| BridgeReply::error("Switchfold is closing."))
                })
            }
        }
    }

    fn answer_with_detail(
        &mut self,
        request: &BridgeRequest,
        key: &PullRequestKey,
        detail: &PullRequestDetail,
        cx: &mut Context<Self>,
    ) -> BridgeReply {
        let name = format!("{}#{}", key.repo(), key.number());
        match request.tool.as_str() {
            tool::CURRENT => BridgeReply::json(&self.describe(key, detail, cx)),
            tool::THREADS => {
                let include_resolved = request.flag("include_resolved");
                let threads: Vec<Value> = detail
                    .threads
                    .iter()
                    .filter(|thread| include_resolved || !thread.is_resolved)
                    .map(|thread| {
                        json!({
                            "thread_id": thread.id,
                            "path": thread.path,
                            "line": thread.line,
                            "resolved": thread.is_resolved,
                            "comments": thread.comments.iter().map(|comment| json!({
                                "author": comment.author.login,
                                "body": comment.body,
                                "pending": comment.is_pending,
                            })).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                BridgeReply::json(&json!({ "pull_request": name, "threads": threads }))
            }
            tool::FINDINGS => {
                let findings: Vec<Value> = detail
                    .findings
                    .iter()
                    .map(|finding| {
                        json!({
                            "id": finding.id,
                            "agent": finding.agent.login,
                            "path": finding.path,
                            "line": finding.line,
                            "state": match finding.state {
                                FindingState::Draft => "draft",
                                FindingState::Added => "added_to_review",
                                FindingState::Dismissed => "dismissed",
                            },
                            "body": finding.body,
                        })
                    })
                    .collect();
                BridgeReply::json(&json!({ "pull_request": name, "findings": findings }))
            }
            tool::COMMENT => {
                let (Some(path), Some(line), Some(body)) = (
                    request.text("path"),
                    request
                        .number("line")
                        .and_then(|line| u32::try_from(line).ok()),
                    request.text("body"),
                ) else {
                    return BridgeReply::error("path, line and body are all required.");
                };
                let Some(file) = detail.file(&path) else {
                    return BridgeReply::error(format!(
                        "{path} isn’t changed in {name}. Comments go on files in the diff; current_pull_request lists them."
                    ));
                };
                let in_diff = file
                    .hunks
                    .iter()
                    .flat_map(|hunk| &hunk.lines)
                    .any(|diff_line| diff_line.new_line == Some(line));
                if !in_diff {
                    return BridgeReply::error(format!(
                        "Line {line} of {path} isn’t in {name}’s diff. GitHub only takes comments on changed lines and the context around them."
                    ));
                }
                let viewer = cx.stores().settings().read(cx).viewer_actor();
                cx.stores().pull_requests().clone().update(cx, |store, cx| {
                    store.start_thread(key, &path, line, &viewer, body, cx)
                });
                BridgeReply::ok(format!(
                    "Drafted a pending comment on {path}:{line} of {name}. It posts when the reviewer submits their review in Switchfold."
                ))
            }
            tool::REPLY => {
                let (Some(thread_id), Some(body)) =
                    (request.text("thread_id"), request.text("body"))
                else {
                    return BridgeReply::error("thread_id and body are both required.");
                };
                let Some(thread) = detail.threads.iter().find(|t| t.id == thread_id) else {
                    return BridgeReply::error(format!(
                        "{name} has no thread {thread_id}; list_review_threads lists them."
                    ));
                };
                let anchor = format!("{}:{}", thread.path, thread.line);
                let viewer = cx.stores().settings().read(cx).viewer_actor();
                cx.stores().pull_requests().clone().update(cx, |store, cx| {
                    store.reply(key, &thread_id, &viewer, body, cx)
                });
                BridgeReply::ok(format!(
                    "Drafted a pending reply on the thread at {anchor}. It posts when the reviewer submits their review in Switchfold."
                ))
            }
            other => BridgeReply::error(format!("Switchfold doesn’t handle {other}.")),
        }
    }

    fn describe(
        &self,
        key: &PullRequestKey,
        detail: &PullRequestDetail,
        cx: &Context<Self>,
    ) -> Value {
        let settings = cx.stores().settings().read(cx);
        let clone = settings.clone_path(key.repo()).map(Path::to_path_buf);
        let review_tree = match (&clone, settings.home()) {
            (Some(clone), Some(home)) => clone.file_name().map(|name| {
                home.join(".switchfold/review-worktrees").join(format!(
                    "{}-pr-{}",
                    name.to_string_lossy(),
                    key.number()
                ))
            }),
            _ => None,
        }
        .filter(|tree| tree.is_dir());
        let pending = detail
            .threads
            .iter()
            .flat_map(|thread| &thread.comments)
            .filter(|comment| comment.is_pending)
            .count();
        let summary = &detail.summary;
        json!({
            "pull_request": format!("{}#{}", key.repo(), key.number()),
            "url": format!("https://github.com/{}/pull/{}", key.repo(), key.number()),
            "title": summary.title,
            "author": summary.author,
            "state": summary.state.label(),
            "head_branch": summary.head_ref,
            "base_branch": summary.base_ref,
            "head_commit": detail.commits.last().map(|commit| commit.sha.clone()),
            "description": detail.body,
            "files": detail.files.iter().map(|file| json!({
                "path": file.path,
                "additions": file.additions(),
                "deletions": file.deletions(),
            })).collect::<Vec<_>>(),
            "checkout_at_head": review_tree.map(|tree| tree.display().to_string()),
            "clone": clone.map(|clone| clone.display().to_string()),
            "unresolved_threads": detail.threads.iter().filter(|t| !t.is_resolved).count(),
            "pending_comments": pending,
        })
    }

    fn list_for_zed(&self, cx: &Context<Self>) -> BridgeReply {
        let store = cx.stores().pull_requests().read(cx);
        let open = self
            .current_review(cx)
            .map(|view| view.read(cx).key().clone());
        let pull_requests: Vec<Value> = store
            .pull_requests()
            .iter()
            .map(|pr| {
                json!({
                    "pull_request": format!("{}#{}", pr.key.repo(), pr.key.number()),
                    "title": pr.title,
                    "author": pr.author,
                    "state": pr.state.label(),
                    "open_in_switchfold": open.as_ref() == Some(&pr.key),
                    "inboxes": pr.inboxes.iter().map(|inbox| inbox.key()).collect::<Vec<_>>(),
                })
            })
            .collect();
        BridgeReply::json(&json!({ "pull_requests": pull_requests }))
    }
}

/// `owner/name#number` or a GitHub pull request URL.
fn parse_key(text: &str) -> Option<PullRequestKey> {
    let text = text.trim();
    if let Some(rest) = text
        .strip_prefix("https://github.com/")
        .or_else(|| text.strip_prefix("github.com/"))
    {
        let mut parts = rest.split('/');
        let (owner, name, pull, number) =
            (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
        if pull != "pull" {
            return None;
        }
        return Some(PullRequestKey::new(
            RepoSlug::new(owner, name),
            number.parse().ok()?,
        ));
    }
    let (repo, number) = text.split_once('#')?;
    Some(PullRequestKey::new(
        RepoSlug::parse(repo)?,
        number.parse().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_pull_requests_by_slug_or_url() {
        let key = PullRequestKey::new(RepoSlug::new("flaticols", "switchfold"), 482);
        assert_eq!(parse_key("flaticols/switchfold#482"), Some(key.clone()));
        assert_eq!(
            parse_key("https://github.com/flaticols/switchfold/pull/482/files"),
            Some(key)
        );
        assert_eq!(
            parse_key("https://github.com/flaticols/switchfold/issues/482"),
            None
        );
        assert_eq!(parse_key("482"), None);
    }
}

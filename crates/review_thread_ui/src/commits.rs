use std::rc::Rc;

use app_state_ui::StoresExt as _;
use diff_model::FileDiff;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    spinner::Spinner,
};
use gpui_kit::{
    AppContext as _, Context, IntoElement, ParentElement as _, SharedString, Styled as _, div,
};
use theme_ui::TypographyExt as _;

use crate::ReviewThreadView;

/// One commit of the pull request, shown on its own in place of the whole change.
pub(crate) struct CommitView {
    pub sha: String,
    pub short_sha: String,
    pub message: String,
    /// The commit's files once loaded, or why they couldn't be.
    pub files: Option<Result<Vec<Rc<FileDiff>>, String>>,
}

impl ReviewThreadView {
    /// Shows only the changes commit `sha` makes. Diffs come from the mapped clone when
    /// it has the commit, else from GitHub.
    pub fn show_commit(&mut self, sha: String, cx: &mut Context<Self>) {
        let Some(commit) = self
            .detail
            .as_ref()
            .and_then(|detail| detail.commits.iter().find(|c| c.sha == sha).cloned())
        else {
            return;
        };
        self.commit = Some(CommitView {
            sha: sha.clone(),
            short_sha: commit.short_sha().to_string(),
            message: commit.message.clone(),
            files: None,
        });
        self.rows = Rc::default();
        self.list_state.reset(0);
        cx.notify();

        let clone = cx
            .stores()
            .settings()
            .read(cx)
            .clone_path(self.key.repo())
            .filter(|path| path.is_dir())
            .map(|path| path.to_path_buf());
        let local_sha = sha.clone();
        let local = cx.background_spawn(async move {
            let clone = git_diff::LocalClone::open(&clone?).ok()?;
            if !clone.has_revision(&local_sha) {
                return None;
            }
            clone.diff_commit(&local_sha).ok()
        });
        let repo = self.key.repo().clone();
        let store = self.store(cx);
        cx.spawn(async move |this, cx| {
            let files = match local.await {
                Some(files) => Ok(files),
                None => store
                    .update(cx, |store, cx| store.commit_files(&repo, &sha, cx))
                    .await
                    .map_err(|error| error.to_string()),
            };
            this.update(cx, |this, cx| {
                let Some(view) = this.commit.as_mut().filter(|view| view.sha == sha) else {
                    return;
                };
                let files: Result<Vec<Rc<FileDiff>>, String> =
                    files.map(|files| files.into_iter().map(Rc::new).collect());
                if let Ok(files) = &files {
                    this.files = files
                        .iter()
                        .map(|file| (file.path.clone(), file.clone()))
                        .collect();
                    this.highlights.clear();
                }
                view.files = Some(files);
                this.list_state.remeasure();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Goes back to the whole pull request.
    pub fn show_all_commits(&mut self, cx: &mut Context<Self>) {
        if self.commit.take().is_none() {
            return;
        }
        if let Some(detail) = &self.detail {
            self.files = detail
                .files
                .iter()
                .map(|file| (file.path.clone(), Rc::new(file.clone())))
                .collect();
        }
        self.highlights.clear();
        self.rows = Rc::default();
        self.list_state.reset(0);
        cx.notify();
    }

    /// Paths of the files the timeline diffs: the picked commit's once loaded, else the
    /// whole pull request's.
    pub fn shown_files(&self) -> Vec<String> {
        match &self.commit {
            Some(commit) => match &commit.files {
                Some(Ok(files)) => files.iter().map(|file| file.path.clone()).collect(),
                _ => Vec::new(),
            },
            None => self
                .detail
                .as_ref()
                .map(|detail| detail.files.iter().map(|file| file.path.clone()).collect())
                .unwrap_or_default(),
        }
    }

    /// The banner above a single commit's changes.
    pub(crate) fn render_commit_banner(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(view) = &self.commit else {
            return div().into_any_element();
        };
        let status = match &view.files {
            None => h_flex()
                .gap_1p5()
                .child(Spinner::new().xsmall())
                .child("Loading the commit…")
                .into_any_element(),
            Some(Ok(files)) => div()
                .child(match files.len() {
                    1 => "1 file".to_string(),
                    n => format!("{n} files"),
                })
                .into_any_element(),
            Some(Err(error)) => div()
                .text_color(cx.theme().danger)
                .child(format!("Couldn’t load it: {error}"))
                .into_any_element(),
        };
        h_flex()
            .gap_2()
            .px_3()
            .py_2()
            .rounded(cx.theme().radius_lg)
            .bg(cx.theme().muted)
            .text_ui()
            .child(Icon::new(IconName::GitCommitHorizontal).small())
            .child(
                div()
                    .text_code(cx)
                    .text_color(cx.theme().muted_foreground)
                    .child(SharedString::from(view.short_sha.clone())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(SharedString::from(view.message.clone())),
            )
            .child(div().text_color(cx.theme().muted_foreground).child(status))
            .child({
                let sha = view.sha.clone();
                Button::new("open-commit-in-zed")
                    .ghost()
                    .small()
                    .icon(IconName::ExternalLink)
                    .label("Open in Zed")
                    .tooltip("Show this commit in Zed’s commit view")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        pr_actions_ui::open_commit_in_zed(&this.key, &sha, window, cx)
                    }))
            })
            .child(
                Button::new("show-all-commits")
                    .ghost()
                    .small()
                    .label("Show all commits")
                    .on_click(cx.listener(|this, _, _, cx| this.show_all_commits(cx))),
            )
            .into_any_element()
    }
}

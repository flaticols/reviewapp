use std::path::{Path, PathBuf};

use app_state_ui::StoresExt as _;
use gpui_kit::component::{
    ActiveTheme as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::DialogFooter,
    input::{Input, InputEvent, InputState},
    notification::Notification,
    v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, TestSupportExt as _,
    Window, div,
};
use pr_model::PullRequestKey;
use theme_ui::TypographyExt as _;
use widgets_ui::EditMenu;

/// The Check out in new worktree dialog's state: the folder to create, prefilled with
/// the default beside the clone.
struct NewWorktreeForm {
    key: PullRequestKey,
    clone: PathBuf,
    head_ref: String,
    path: Entity<InputState>,
}

impl NewWorktreeForm {
    fn new(
        key: PullRequestKey,
        clone: PathBuf,
        head_ref: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let default = git_worktree::NewWorktree::default_path(&clone, &head_ref);
        let path = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Folder for the worktree")
                .default_value(default.to_string_lossy().into_owned())
                .context_menu(false)
        });
        cx.subscribe_in(
            &path,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                _ => cx.notify(),
            },
        )
        .detach();
        Self {
            key,
            clone,
            head_ref,
            path,
        }
    }

    /// The folder typed in, with a leading `~` meaning the home folder.
    fn destination(&self, cx: &App) -> PathBuf {
        let typed = self.path.read(cx).value().trim().to_string();
        let home = cx
            .stores()
            .settings()
            .read(cx)
            .home()
            .map(Path::to_path_buf);
        match (
            typed.strip_prefix("~/").or((typed == "~").then_some("")),
            home,
        ) {
            (Some(rest), Some(home)) => home.join(rest),
            _ => PathBuf::from(typed),
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let request =
            git_worktree::NewWorktree::new(self.clone.clone(), &self.head_ref, self.key.number())
                .at(self.destination(cx));
        let branch = self.head_ref.clone();
        let task = cx.background_spawn(async move { request.create() });
        window
            .spawn(cx, async move |cx| {
                let result = task.await;
                cx.update(|window, cx| match result {
                    Ok(path) => window.push_notification(
                        Notification::success(format!(
                            "Checked out {branch} into {}.",
                            path.display()
                        )),
                        cx,
                    ),
                    Err(error) => {
                        window.push_notification(Notification::error(error.to_string()), cx)
                    }
                })
                .ok();
            })
            .detach();
        window.close_dialog(cx);
    }
}

impl Render for NewWorktreeForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let branch = format!("Branch {}", self.head_ref);
        let destination = format!("Creates {}", self.destination(cx).display());
        v_flex()
            .gap_3()
            .child(
                div()
                    .id("new-worktree-branch")
                    .aria_label(branch.clone())
                    .test_support()
                    .text_ui()
                    .text_color(cx.theme().muted_foreground)
                    .child(branch),
            )
            .child(EditMenu::new(
                "new-worktree-path",
                &self.path,
                Input::new(&self.path),
            ))
            .child(
                div()
                    .id("new-worktree-destination")
                    .aria_label(destination.clone())
                    .test_support()
                    .text_ui()
                    .text_color(cx.theme().muted_foreground)
                    .child(destination),
            )
    }
}

/// Opens the Check out in new worktree dialog for `key`, whose head branch is `head_ref`,
/// creating the worktree from the clone at `clone`.
pub fn open_new_worktree(
    key: PullRequestKey,
    head_ref: String,
    clone: PathBuf,
    window: &mut Window,
    cx: &mut App,
) {
    let form = cx.new(|cx| NewWorktreeForm::new(key.clone(), clone, head_ref, window, cx));
    let width = window.rem_size() * 32.;
    let path = form.read(cx).path.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        let submit_form = form.clone();
        dialog
            .title(format!("Check out #{} in a new worktree", key.number()))
            .w(width)
            .child(form.clone())
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("new-worktree-cancel")
                            .ghost()
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("new-worktree-create")
                            .primary()
                            .label("Create worktree")
                            .on_click(move |_, window, cx| {
                                submit_form.update(cx, |form, cx| form.submit(window, cx))
                            }),
                    ),
            )
    });
    // The dialog takes focus as it opens, and a context menu that opened it hands focus
    // back as it closes, so the field is focused once both have happened.
    window.defer(cx, move |window, cx| {
        path.update(cx, |path, cx| path.focus(window, cx))
    });
}

/// Opens the Check out in new worktree dialog for `key` when its repository has a clone,
/// or says how to map one.
pub fn check_out_in_new_worktree(
    key: &PullRequestKey,
    head_ref: &str,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(clone) = crate::local::clone_of(key, window, cx) {
        open_new_worktree(key.clone(), head_ref.to_string(), clone, window, cx);
    }
}

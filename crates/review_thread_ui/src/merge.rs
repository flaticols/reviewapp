//! Merging from the title bar: a Merge button that opens a panel with whether the pull
//! request can merge, the method, the commit title and message, auto-merge, updating
//! the branch, and for the author's own pull request, squashing its commits.

use app_state_ui::StoresExt as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::DialogButtonProps,
    h_flex,
    input::{Input, InputState, Textarea, TextareaState},
    notification::Notification,
    popover::{Popover, PopoverState},
    radio::RadioGroup,
    v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Window, div, prelude::FluentBuilder as _, rems,
};
use pr_model::{PullRequestKey, PullRequestState};
use review_model::{MergeInfo, MergeMethod, MergeState, PullRequestDetail};
use theme_ui::TypographyExt as _;

use crate::ReviewThreadView;
use crate::review_actions::bordered;

/// The merge panel's state: the chosen method, the commit title and message, and
/// whether a request is in flight.
pub(crate) struct MergeForm {
    key: PullRequestKey,
    method: Option<MergeMethod>,
    title: Entity<InputState>,
    message: Entity<TextareaState>,
    busy: bool,
    error: Option<SharedString>,
    popover: Option<Entity<PopoverState>>,
}

/// What the panel says about whether the pull request can merge, and how loudly.
fn readiness(info: &MergeInfo, base: &str) -> (IconName, &'static str, String) {
    match info.state {
        MergeState::Clean => (IconName::CircleCheck, "success", "Ready to merge.".into()),
        MergeState::HasHooks => (
            IconName::CircleCheck,
            "success",
            "Ready to merge; hooks run when it does.".into(),
        ),
        MergeState::Unstable => (
            IconName::CircleAlert,
            "warning",
            "Some checks are failing, but the branch can merge.".into(),
        ),
        MergeState::Behind => (
            IconName::CircleAlert,
            "warning",
            format!("This branch is behind {base}. Update it before merging."),
        ),
        MergeState::Blocked => (
            IconName::CircleX,
            "danger",
            "Merging is blocked: required reviews or checks are missing.".into(),
        ),
        MergeState::Dirty => (
            IconName::CircleX,
            "danger",
            format!("This branch has conflicts with {base} to resolve first."),
        ),
        MergeState::Draft => (
            IconName::CircleX,
            "danger",
            "Draft pull requests can’t merge.".into(),
        ),
        MergeState::Unknown => (
            IconName::LoaderCircle,
            "muted",
            "GitHub is still checking whether this can merge.".into(),
        ),
    }
}

/// The commit title and message GitHub would suggest for `method`.
fn suggested_commit(detail: &PullRequestDetail, method: MergeMethod) -> (String, String) {
    let number = detail.summary.key.number();
    match method {
        MergeMethod::Squash => (
            format!("{} (#{number})", detail.summary.title),
            detail
                .commits
                .iter()
                .map(|commit| format!("* {}", commit.message))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        MergeMethod::Merge => (
            format!(
                "Merge pull request #{number} from {}/{}",
                detail.summary.key.repo().owner(),
                detail.summary.head_ref
            ),
            detail.summary.title.clone(),
        ),
        MergeMethod::Rebase => (String::new(), String::new()),
    }
}

impl MergeForm {
    pub(crate) fn new(key: PullRequestKey, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let title = cx.new(|cx| InputState::new(window, cx).placeholder("Commit title"));
        let message = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Commit message")
                .auto_grow(3, 8)
        });
        let mut form = Self {
            key,
            method: None,
            title,
            message,
            busy: false,
            error: None,
            popover: None,
        };
        let method = form.detail(cx).and_then(|detail| {
            let info = detail.merge.as_ref()?;
            info.default_method
                .or_else(|| info.methods.first().copied())
        });
        if let Some(method) = method {
            form.choose(method, window, cx);
        }
        form
    }

    fn detail(&self, cx: &App) -> Option<PullRequestDetail> {
        cx.stores()
            .pull_requests()
            .read(cx)
            .loaded_detail(&self.key)
            .cloned()
    }

    fn choose(&mut self, method: MergeMethod, window: &mut Window, cx: &mut Context<Self>) {
        self.method = Some(method);
        if let Some(detail) = self.detail(cx) {
            let (title, message) = suggested_commit(&detail, method);
            self.title
                .update(cx, |input, cx| input.set_value(title, window, cx));
            self.message
                .update(cx, |input, cx| input.set_value(message, window, cx));
        }
        cx.notify();
    }

    fn dismiss(&self, window: &mut Window, cx: &mut App) {
        if let Some(popover) = &self.popover {
            popover.update(cx, |state, cx| state.dismiss(window, cx));
        }
    }

    /// Runs a store request, shows its failure in the panel, and on success closes the
    /// panel and says `done`.
    fn run(
        &mut self,
        task: gpui_kit::Task<Result<(), github_api::ApiError>>,
        done: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.busy = true;
        self.error = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(()) => {
                        this.dismiss(window, cx);
                        window.push_notification(Notification::success(done), cx);
                    }
                    Err(error) => this.error = Some(error.to_string().into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn merge(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(method) = self.method else {
            return;
        };
        let title = self.title.read(cx).value().trim().to_string();
        let message = self.message.read(cx).value().to_string();
        let key = self.key.clone();
        let task = cx.stores().pull_requests().clone().update(cx, |store, cx| {
            store.merge(&key, method, title, message, cx)
        });
        self.run(task, format!("Merged #{}.", key.number()), window, cx);
    }

    fn auto_merge(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        let method = if on { self.method } else { None };
        if on && method.is_none() {
            return;
        }
        let key = self.key.clone();
        let task = cx
            .stores()
            .pull_requests()
            .clone()
            .update(cx, |store, cx| store.set_auto_merge(&key, method, cx));
        let done = if on {
            format!(
                "Auto-merge is on: #{} merges once its requirements are met.",
                key.number()
            )
        } else {
            format!("Auto-merge is off for #{}.", key.number())
        };
        self.run(task, done, window, cx);
    }

    fn update_branch(&mut self, rebase: bool, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.key.clone();
        let task = cx
            .stores()
            .pull_requests()
            .clone()
            .update(cx, |store, cx| store.update_branch(&key, rebase, cx));
        self.run(
            task,
            format!("Updating #{}’s branch with its base.", key.number()),
            window,
            cx,
        );
    }

    fn squash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(detail) = self.detail(cx) else {
            return;
        };
        self.dismiss(window, cx);
        open_squash_dialog(&detail, window, cx);
    }
}

/// Asks to squash the author's own pull request into one commit and force-push it.
fn open_squash_dialog(detail: &PullRequestDetail, window: &mut Window, cx: &mut App) {
    let key = detail.summary.key.clone();
    let Some(info) = detail.merge.clone() else {
        return;
    };
    let clone = cx
        .stores()
        .settings()
        .read(cx)
        .clone_path(key.repo())
        .map(|path| path.to_path_buf());
    let Some(clone) = clone.filter(|path| path.is_dir()) else {
        window.push_notification(
            Notification::warning(format!(
                "{} isn’t cloned. Map it to a local clone in Settings › Repositories to squash.",
                key.repo()
            )),
            cx,
        );
        return;
    };
    let count = detail.commits.len();
    let head_ref = detail.summary.head_ref.clone();
    let base_ref = detail.summary.base_ref.clone();
    let suggested = suggested_commit(detail, MergeMethod::Squash);
    let message = cx.new(|cx| {
        TextareaState::new(window, cx)
            .auto_grow(4, 10)
            .default_value(format!("{}\n\n{}", suggested.0, suggested.1))
    });
    let title = format!("Squash {count} commits on {head_ref} into one?");
    let description = format!(
        "Rewrites {head_ref} on GitHub with a force push. Anyone who pulled it will need to reset to the new commit. If someone pushed since Switchfold loaded it, nothing is overwritten."
    );
    window.open_alert_dialog(cx, move |alert, _, _| {
        let message = message.clone();
        let clone = clone.clone();
        let key = key.clone();
        let info = info.clone();
        let head_ref = head_ref.clone();
        let base_ref = base_ref.clone();
        alert
            .title(title.clone())
            .description(description.clone())
            .child(div().pt_2().child(Textarea::new(&message).small()))
            .confirm()
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Squash and force-push")
                    .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                    .cancel_text("Cancel"),
            )
            .on_ok(move |_, window, cx| {
                let text = message.read(cx).value().to_string();
                let squash = git_worktree::Squash::new(
                    clone.clone(),
                    key.number(),
                    info.head_sha.clone(),
                    head_ref.clone(),
                    base_ref.clone(),
                    text,
                );
                let task = cx.background_spawn(async move { squash.run() });
                let key = key.clone();
                window
                    .spawn(cx, async move |cx| {
                        let result = task.await;
                        cx.update(|window, cx| match result {
                            Ok(sha) => {
                                window.push_notification(
                                    Notification::success(format!(
                                        "Squashed #{} into {}.",
                                        key.number(),
                                        &sha[..7.min(sha.len())]
                                    )),
                                    cx,
                                );
                                cx.stores()
                                    .pull_requests()
                                    .clone()
                                    .update(cx, |store, cx| store.reload_detail(&key, cx));
                            }
                            Err(error) => window.push_notification(
                                Notification::error(format!("Couldn’t squash: {error}")),
                                cx,
                            ),
                        })
                        .ok();
                    })
                    .detach();
                true
            })
    });
}

impl Render for MergeForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(detail) = self.detail(cx) else {
            return div().into_any_element();
        };
        let Some(info) = detail.merge.clone() else {
            return div()
                .w(rems(22.))
                .text_ui()
                .text_color(cx.theme().muted_foreground)
                .child("Switchfold doesn’t know yet how this pull request merges.")
                .into_any_element();
        };
        let number = detail.summary.key.number();
        let (icon, tone, status) = readiness(&info, &detail.summary.base_ref);
        let status_color = match tone {
            "success" => cx.theme().success,
            "warning" => cx.theme().warning,
            "danger" => cx.theme().danger,
            _ => cx.theme().muted_foreground,
        };
        let method = self.method;
        let own = detail.viewer.as_deref() == Some(detail.summary.author.as_str());
        let can_squash_own = own && !info.from_fork && detail.commits.len() > 1;
        let busy = self.busy;
        let methods = info.methods.clone();
        let selected = method.and_then(|m| methods.iter().position(|x| *x == m));

        v_flex()
            .w(rems(24.))
            .gap_3()
            // The title bar's actions don't wrap, and the popover inherits that.
            .whitespace_normal()
            .child(div().text_heading().child(format!("Merge #{number}")))
            .child(
                h_flex()
                    .items_start()
                    .gap_2()
                    .text_ui()
                    .child(Icon::new(icon).small().text_color(status_color))
                    .child(div().flex_1().child(status)),
            )
            .when(
                info.state == MergeState::Behind && info.can_update_branch,
                |this| {
                    this.child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new("update-branch-merge")
                                    .small()
                                    .outline()
                                    .label("Update with merge")
                                    .disabled(busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.update_branch(false, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("update-branch-rebase")
                                    .small()
                                    .ghost()
                                    .label("Update with rebase")
                                    .disabled(busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.update_branch(true, window, cx)
                                    })),
                            ),
                    )
                },
            )
            .when_some(info.auto_merge.clone(), |this, auto| {
                this.child(
                    div()
                        .text_ui()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!(
                            "Auto-merge is on ({}), turned on by {}.",
                            auto.method.label().to_lowercase(),
                            auto.enabled_by
                        )),
                )
            })
            .when(methods.len() > 1, |this| {
                let choices = methods.clone();
                this.child(
                    RadioGroup::vertical("merge-method")
                        .children(choices.iter().map(|m| m.label()))
                        .selected_index(selected)
                        .on_change(cx.listener(move |this, ix: &usize, window, cx| {
                            if let Some(method) = choices.get(*ix) {
                                this.choose(*method, window, cx);
                            }
                        })),
                )
            })
            .when(method.is_some_and(MergeMethod::has_message), |this| {
                this.child(
                    v_flex()
                        .gap_1()
                        .child(Input::new(&self.title).small())
                        .child(Textarea::new(&self.message).small()),
                )
            })
            .when(info.deletes_branch, |this| {
                this.child(
                    div()
                        .text_meta()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!(
                            "{} is deleted after merging.",
                            detail.summary.head_ref
                        )),
                )
            })
            .when_some(self.error.clone(), |this, error| {
                this.child(div().text_ui().text_color(cx.theme().danger).child(error))
            })
            .child(
                h_flex()
                    .gap_2()
                    .when(can_squash_own, |this| {
                        this.child(
                            Button::new("squash-own")
                                .ghost()
                                .small()
                                .label(format!("Squash {} commits…", detail.commits.len()))
                                .tooltip("Rewrite your branch as one commit, with a force push")
                                .disabled(busy)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.squash(window, cx)),
                                ),
                        )
                    })
                    .child(div().flex_1())
                    .when(
                        info.auto_merge.is_some() && info.can_disable_auto_merge,
                        |this| {
                            this.child(
                                Button::new("auto-merge-off")
                                    .small()
                                    .ghost()
                                    .label("Turn off auto-merge")
                                    .disabled(busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.auto_merge(false, window, cx)
                                    })),
                            )
                        },
                    )
                    .when(
                        info.auto_merge.is_none()
                            && info.can_enable_auto_merge
                            && !info.state.can_merge(),
                        |this| {
                            this.child(
                                Button::new("auto-merge-on")
                                    .small()
                                    .outline()
                                    .label("Enable auto-merge")
                                    .disabled(busy || method.is_none())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.auto_merge(true, window, cx)
                                    })),
                            )
                        },
                    )
                    .child(
                        Button::new("merge-confirm")
                            .small()
                            .success()
                            .label(method.map_or("Merge", MergeMethod::label))
                            .loading(busy)
                            .disabled(busy || !info.state.can_merge() || method.is_none())
                            .on_click(cx.listener(|this, _, window, cx| this.merge(window, cx))),
                    ),
            )
            .into_any_element()
    }
}

impl ReviewThreadView {
    /// The merge panel, created the first time it is shown.
    pub(crate) fn merge_form(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<MergeForm> {
        if let Some(form) = &self.merge_form {
            return form.clone();
        }
        let key = self.key.clone();
        let form = cx.new(|cx| MergeForm::new(key, window, cx));
        self.merge_form = Some(form.clone());
        form
    }
}

/// The title bar's Merge button: green when the pull request can merge now, quiet
/// otherwise, and "Auto-merge" while that waits. Hidden unless the pull request is open.
pub(crate) fn merge_button(
    detail: &PullRequestDetail,
    form: Entity<MergeForm>,
    cx: &App,
) -> Option<impl IntoElement + use<>> {
    let info = detail.merge.as_ref()?;
    if detail.summary.state != PullRequestState::Open {
        return None;
    }
    let ready = info.state.can_merge();
    let (border, text) = if info.auto_merge.is_some() {
        (cx.theme().info, cx.theme().info)
    } else if ready {
        (cx.theme().success, cx.theme().success)
    } else {
        (cx.theme().border, cx.theme().muted_foreground)
    };
    let label = if info.auto_merge.is_some() {
        "Auto-merge"
    } else {
        "Merge"
    };
    let trigger = bordered(Button::new("merge"), border, text)
        .small()
        .icon(IconName::GitMerge)
        .label(label)
        .dropdown_caret(true);
    Some(
        Popover::new("merge-popover")
            .anchor(gpui_kit::Anchor::TopRight)
            .arrow(true)
            .trigger(trigger)
            .content(move |_, _, cx| {
                let popover = cx.entity();
                form.update(cx, |form, _| form.popover = Some(popover));
                form.clone()
            }),
    )
}

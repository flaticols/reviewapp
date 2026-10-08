//! The check log sheet: a check run's steps with their durations, the log of the
//! failing or running step, a link to the run on GitHub, and Re-run failed jobs.

use std::collections::HashSet;

use app_state_ui::{PullRequestStore, StoresExt as _};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, WindowExt as _, button::Button, h_flex,
    link::Link, list::ListItem, v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, div, prelude::FluentBuilder as _, rems,
};
use pr_model::{CheckStatus, PullRequestKey, format_duration};
use review_model::{CheckRun, CheckStep, LogLineKind};
use theme_ui::TypographyExt as _;
use widgets_ui::CheckIcon;

/// The sheet's content: one check run, kept current as polling updates it.
pub struct CheckLogView {
    key: PullRequestKey,
    check_id: String,
    expanded: HashSet<String>,
    expanded_initialized: bool,
    _store: Subscription,
}

impl CheckLogView {
    /// A view of check `check_id` on pull request `key`.
    pub fn new(key: PullRequestKey, check_id: String, cx: &mut Context<Self>) -> Self {
        let store = cx.stores().pull_requests().clone();
        let subscription = cx.observe(&store, |_, _, cx| cx.notify());
        Self {
            key,
            check_id,
            expanded: HashSet::new(),
            expanded_initialized: false,
            _store: subscription,
        }
    }

    fn check(&self, cx: &App) -> Option<CheckRun> {
        cx.stores()
            .pull_requests()
            .read(cx)
            .loaded_detail(&self.key)?
            .check(&self.check_id)
            .cloned()
    }

    fn render_title(check: &CheckRun) -> impl IntoElement {
        h_flex().gap_2().child(CheckIcon::new(check.status)).child(
            div()
                .text_heading()
                .child(SharedString::from(check.name.clone())),
        )
    }

    fn render_step(&self, step: &CheckStep, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let expanded = self.expanded.contains(&step.name);
        let has_log = !step.log.is_empty();
        let name = step.name.clone();
        let failed = step.status == CheckStatus::Failed;
        v_flex()
            .child(
                ListItem::new(SharedString::from(format!("step-{}", step.name)))
                    .h(rems(1.875))
                    .px_2()
                    .rounded(cx.theme().radius)
                    .selected(expanded && has_log)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.expanded.remove(&name) {
                            this.expanded.insert(name.clone());
                        }
                        cx.notify();
                    }))
                    .child(
                        h_flex()
                            .gap_2()
                            .text_ui()
                            .child(
                                Icon::new(if expanded {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .small()
                                .text_color(cx.theme().muted_foreground),
                            )
                            .child(CheckIcon::new(step.status))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(SharedString::from(step.name.clone())),
                            )
                            .child(div().text_color(cx.theme().muted_foreground).child(
                                match (step.duration_secs, step.status) {
                                    (Some(seconds), _) => format_duration(seconds),
                                    (None, CheckStatus::Skipped) => "skipped".into(),
                                    (None, CheckStatus::Running) => "running".into(),
                                    (None, _) => String::new(),
                                },
                            )),
                    ),
            )
            .when(expanded, |this| {
                this.child(if has_log {
                    v_flex()
                        .id(SharedString::from(format!("log-{}", step.name)))
                        .mx_1()
                        .my_1()
                        .py_2()
                        .px_2()
                        .rounded(cx.theme().radius)
                        .bg(cx.theme().muted.opacity(0.5))
                        .overflow_x_scroll()
                        .children(step.log.iter().map(|line| {
                            let color = match line.kind {
                                LogLineKind::Normal => cx.theme().foreground,
                                LogLineKind::Error | LogLineKind::Removed => cx.theme().danger,
                                LogLineKind::Added => cx.theme().success,
                            };
                            h_flex()
                                .gap_3()
                                .text_code(cx)
                                .whitespace_nowrap()
                                .child(
                                    div()
                                        .w_6()
                                        .flex_shrink_0()
                                        .text_right()
                                        .text_gutter(cx)
                                        .text_color(cx.theme().muted_foreground)
                                        .child(line.number.to_string()),
                                )
                                .child(
                                    div()
                                        .text_color(color)
                                        .child(SharedString::from(line.text.clone())),
                                )
                        }))
                        .into_any_element()
                } else {
                    div()
                        .pl_10()
                        .py_1()
                        .text_meta()
                        .text_color(cx.theme().muted_foreground)
                        .child(if failed {
                            "This step failed without output."
                        } else {
                            "No output."
                        })
                        .into_any_element()
                })
            })
    }
}

impl Render for CheckLogView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(check) = self.check(cx) else {
            return div()
                .text_ui()
                .text_color(cx.theme().muted_foreground)
                .child("This check isn’t available any more.")
                .into_any_element();
        };
        if !self.expanded_initialized {
            self.expanded = check
                .steps
                .iter()
                .filter(|step| matches!(step.status, CheckStatus::Failed | CheckStatus::Running))
                .map(|step| step.name.clone())
                .collect();
            self.expanded_initialized = true;
        }
        let sha = &check.head_sha[..check.head_sha.len().min(7)];
        let status = match check.status {
            CheckStatus::Passed => format!("Passed in {}", format_duration(check.elapsed_secs)),
            CheckStatus::Failed => format!("Failed in {}", format_duration(check.elapsed_secs)),
            CheckStatus::Running => format!("Running for {}", format_duration(check.elapsed_secs)),
            CheckStatus::Queued => "Queued".into(),
            CheckStatus::Skipped => "Skipped".into(),
        };
        let key = self.key.clone();
        let check_id = check.id.clone();
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .text_ui()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{status} · {sha}")),
                    )
                    .when(!check.url.is_empty(), |this| {
                        this.child(
                            Link::new("check-on-github").href(check.url.clone()).child(
                                h_flex()
                                    .gap_1()
                                    .text_ui()
                                    .child("GitHub")
                                    .child(Icon::new(IconName::ArrowUpRight).xsmall()),
                            ),
                        )
                    })
                    .child(
                        Button::new("rerun-failed")
                            .outline()
                            .small()
                            .icon(IconName::RotateCcw)
                            .label("Re-run failed jobs")
                            .disabled(check.status != CheckStatus::Failed)
                            .on_click(move |_, _, cx| {
                                let store: Entity<PullRequestStore> =
                                    cx.stores().pull_requests().clone();
                                store
                                    .update(cx, |store, cx| store.rerun_check(&key, &check_id, cx));
                            }),
                    ),
            )
            .child(div().h_px().bg(cx.theme().border))
            .child({
                let steps: Vec<_> = check
                    .steps
                    .iter()
                    .map(|step| self.render_step(step, cx))
                    .collect();
                v_flex().gap_0p5().children(steps)
            })
            .into_any_element()
    }
}

/// Opens the sheet for check `check_id` of pull request `key`.
pub fn open(key: PullRequestKey, check_id: String, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| CheckLogView::new(key, check_id, cx));
    window.open_sheet(cx, move |sheet, window, cx| {
        let sheet = sheet
            .size(window.rem_size() * 35.)
            .overlay(false)
            .child(view.clone());
        match view.read(cx).check(cx) {
            Some(check) => sheet.title(CheckLogView::render_title(&check)),
            None => sheet,
        }
    });
}

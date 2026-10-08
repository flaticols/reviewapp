//! The Submit review dialog: choose Comment, Approve or Request changes, write a summary,
//! and post it with the pending comments and added agent findings.

use app_state_ui::StoresExt as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::DialogFooter,
    h_flex,
    input::{Textarea, TextareaState},
    label::Label,
    notification::Notification,
    radio::Radio,
    v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FontWeight, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use pr_model::PullRequestKey;
use review_model::{FindingState, ReviewVerdict};
use theme_ui::TypographyExt as _;

/// The dialog's retained state: the chosen verdict, the summary and submission progress.
pub struct SubmitReviewForm {
    key: PullRequestKey,
    verdict: ReviewVerdict,
    summary: Entity<TextareaState>,
    is_submitting: bool,
    error: Option<SharedString>,
}

impl SubmitReviewForm {
    /// A form for `key` with `verdict` preselected.
    pub fn new(
        key: PullRequestKey,
        verdict: ReviewVerdict,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let summary = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Summarize your review (optional)")
                .rows(3)
        });
        Self {
            key,
            verdict,
            summary,
            is_submitting: false,
            error: None,
        }
    }

    /// The selected verdict.
    pub fn verdict(&self) -> ReviewVerdict {
        self.verdict
    }

    /// Whether the review is being posted.
    pub fn is_submitting(&self) -> bool {
        self.is_submitting
    }

    /// Posts the review; closes the dialog when GitHub accepts it and keeps it open with
    /// the error otherwise.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_submitting {
            return;
        }
        let summary = self.summary.read(cx).value().trim().to_string();
        if self.verdict == ReviewVerdict::RequestChanges && summary.is_empty() {
            self.error = Some("Explain what needs to change before requesting changes.".into());
            cx.notify();
            return;
        }
        let viewer = cx.stores().settings().read(cx).viewer_actor();
        let key = self.key.clone();
        let verdict = self.verdict;
        let task = cx.stores().pull_requests().clone().update(cx, |store, cx| {
            store.submit_review(&key, &viewer, verdict, summary, cx)
        });
        self.is_submitting = true;
        self.error = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.is_submitting = false;
                match result {
                    Ok(()) => {
                        window.close_dialog(cx);
                        window.push_notification(
                            Notification::success(format!(
                                "Review submitted on #{}.",
                                this.key.number()
                            )),
                            cx,
                        );
                    }
                    Err(error) => {
                        this.error = Some(format!("Couldn’t submit the review: {error}").into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn pending_note(&self, cx: &App) -> Option<String> {
        let store = cx.stores().pull_requests().read(cx);
        let detail = store.loaded_detail(&self.key)?;
        let comments = detail
            .threads
            .iter()
            .flat_map(|thread| &thread.comments)
            .filter(|comment| comment.is_pending)
            .count();
        let findings = detail
            .findings
            .iter()
            .filter(|finding| finding.state == FindingState::Added)
            .count();
        if comments == 0 && findings == 0 {
            return None;
        }
        let comments = match comments {
            1 => "1 comment".to_string(),
            n => format!("{n} comments"),
        };
        let findings = match findings {
            1 => "1 agent finding".to_string(),
            n => format!("{n} agent findings"),
        };
        Some(format!(
            "Also posts {comments} and {findings} from your pending review."
        ))
    }
}

impl Render for SubmitReviewForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let note = self.pending_note(cx);
        v_flex()
            .gap_4()
            .child(v_flex().gap_3().children(ReviewVerdict::ALL.map(|verdict| {
                Radio::new(SharedString::from(format!("verdict-{verdict:?}")))
                    .checked(self.verdict == verdict)
                    .disabled(self.is_submitting)
                    .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                        if *checked {
                            this.verdict = verdict;
                            this.error = None;
                            cx.notify();
                        }
                    }))
                    .child(
                        v_flex()
                            .child(
                                div()
                                    .text_body()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(verdict.title()),
                            )
                            .child(
                                div()
                                    .text_ui()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(verdict.description()),
                            ),
                    )
            })))
            .child(
                v_flex()
                    .gap_1p5()
                    .child(
                        Label::new("Summary")
                            .text_ui()
                            .font_weight(FontWeight::MEDIUM),
                    )
                    .child(
                        Textarea::new(&self.summary)
                            .disabled(self.is_submitting)
                            .h_20(),
                    ),
            )
            .when_some(note, |this, note| {
                this.child(
                    h_flex()
                        .gap_2()
                        .text_meta()
                        .text_color(cx.theme().muted_foreground)
                        .child(Icon::new(IconName::MessageSquarePlus).small())
                        .child(note),
                )
            })
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    h_flex()
                        .gap_2()
                        .text_ui()
                        .text_color(cx.theme().danger)
                        .child(Icon::new(IconName::CircleAlert).small())
                        .child(error),
                )
            })
    }
}

/// The top offset that centers a dialog of roughly `height_rems` in the window, as the
/// design places decision dialogs; never closer to the top than gpui-kit's default.
fn centered_top(window: &Window, height_rems: f32) -> gpui_kit::Pixels {
    let viewport = window.viewport_size().height;
    let height = window.rem_size() * height_rems;
    ((viewport - height) / 2.).max(viewport / 10.)
}

/// Opens the dialog for `key`, preselecting `verdict` or Comment.
pub fn open(
    key: PullRequestKey,
    verdict: Option<ReviewVerdict>,
    window: &mut Window,
    cx: &mut App,
) {
    let verdict = verdict.unwrap_or(ReviewVerdict::Comment);
    let form = cx.new(|cx| SubmitReviewForm::new(key, verdict, window, cx));
    let width = window.rem_size() * 30.;
    window.open_dialog(cx, move |dialog, window, cx| {
        let state = form.read(cx);
        let submit_label = state.verdict().title();
        let submitting = state.is_submitting();
        let submit_form = form.clone();
        let click_form = form.clone();
        dialog
            .title("Submit review")
            .w(width)
            .margin_top(centered_top(window, 24.))
            .child(form.clone())
            .on_ok(move |_, window, cx| {
                submit_form.update(cx, |form, cx| form.submit(window, cx));
                false
            })
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("cancel")
                            .ghost()
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("submit")
                            .primary()
                            .loading(submitting)
                            .label(submit_label)
                            .on_click(move |_, window, cx| {
                                click_form.update(cx, |form, cx| form.submit(window, cx))
                            }),
                    ),
            )
    });
}

#[cfg(test)]
mod tests {
    use review_model::ReviewVerdict;

    #[test]
    fn the_confirm_button_names_the_verdict() {
        assert_eq!(ReviewVerdict::RequestChanges.title(), "Request changes");
        assert_eq!(ReviewVerdict::ALL.len(), 3);
    }
}

use std::rc::Rc;

use acp_client::RunOutcome;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    spinner::Spinner,
    v_flex,
};
use gpui_kit::{
    App, Entity, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    SharedString, StatefulInteractiveElement as _, Styled as _, TestSupportExt as _, Window, div,
    prelude::FluentBuilder as _,
};
use theme_ui::TypographyExt as _;
use widgets_ui::{AvatarSize, InitialsAvatar};

use crate::{AgentQuestion, RunState};

type CommentHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;
type DismissHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// A question to an agent about diff lines, under those lines: who was asked about
/// what, the answer as it streams, the permission prompt when the agent asks to run
/// something, and the comment it suggests with Add to review and Edit.
#[derive(IntoElement)]
pub struct AgentQuestionCard {
    question: Entity<AgentQuestion>,
    on_add_comment: Option<CommentHandler>,
    on_edit_comment: Option<CommentHandler>,
    on_dismiss: Option<DismissHandler>,
}

impl AgentQuestionCard {
    /// A card for `question`.
    pub fn new(question: Entity<AgentQuestion>) -> Self {
        Self {
            question,
            on_add_comment: None,
            on_edit_comment: None,
            on_dismiss: None,
        }
    }

    /// Handles Add to review with the suggested comment.
    pub fn on_add_comment(
        mut self,
        handler: impl Fn(&str, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_add_comment = Some(Rc::new(handler));
        self
    }

    /// Handles Edit with the suggested comment, to open it as a draft.
    pub fn on_edit_comment(
        mut self,
        handler: impl Fn(&str, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_edit_comment = Some(Rc::new(handler));
        self
    }

    /// Runs after Dismiss removed the question.
    pub fn on_dismiss(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(handler));
        self
    }

    fn header(&self, cx: &App) -> impl IntoElement + use<> {
        let question = self.question.read(cx);
        let state = question.state().clone();
        let status = match &state {
            RunState::Preparing => "checking out the pull request…",
            RunState::Starting => "starting…",
            RunState::Running if question.answer().is_empty() => "reading the code…",
            RunState::Running => "answering…",
            RunState::AwaitingPermission => "waiting for you",
            RunState::Stopping => "stopping…",
            RunState::Finished(RunOutcome::Completed) => "answered",
            RunState::Finished(RunOutcome::Truncated) => "ran out of room",
            RunState::Finished(_) | RunState::Failed(_) => "stopped",
        };
        let subtitle = format!("{} · {status}", question.label());
        let agent = question.agent().login.clone();
        let entity = self.question.clone();
        let on_dismiss = self.on_dismiss.clone();
        h_flex()
            .gap_2()
            .text_ui()
            .child(InitialsAvatar::icon(IconName::Bot).size(AvatarSize::Medium))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(agent)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(cx.theme().muted_foreground)
                    .child(subtitle),
            )
            .map(|this| {
                if state.is_over() {
                    this.child(
                        Button::new("dismiss-question")
                            .ghost()
                            .small()
                            .label("Dismiss")
                            .on_click(move |_, window, cx| {
                                entity.update(cx, |question, cx| question.dismiss(cx));
                                if let Some(on_dismiss) = &on_dismiss {
                                    on_dismiss(window, cx);
                                }
                            }),
                    )
                } else {
                    this.child(
                        Button::new("stop-question")
                            .ghost()
                            .small()
                            .icon(IconName::X)
                            .label("Stop")
                            .disabled(state == RunState::Stopping)
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |question, cx| question.stop(cx))
                            }),
                    )
                }
            })
    }

    fn suggestion(&self, body: &str, added: bool, cx: &App) -> impl IntoElement + use<> {
        let question = self.question.read(cx);
        let id = self.question.entity_id().as_u64();
        let repo = question.key().repo().clone();
        let muted = cx.theme().muted_foreground;
        let add = self.on_add_comment.clone().map(|handler| {
            let (body, entity) = (body.to_string(), self.question.clone());
            Button::new("add-suggested-comment")
                .small()
                .icon(IconName::MessageSquarePlus)
                .label("Add to review")
                .on_click(move |_, window, cx| {
                    handler(&body, window, cx);
                    entity.update(cx, |question, cx| question.mark_suggestion_added(cx));
                })
        });
        let edit = self.on_edit_comment.clone().map(|handler| {
            let body = body.to_string();
            Button::new("edit-suggested-comment")
                .ghost()
                .small()
                .icon(IconName::Pencil)
                .label("Edit")
                .on_click(move |_, window, cx| handler(&body, window, cx))
        });
        v_flex()
            .pl_8()
            .gap_1p5()
            .child(
                div()
                    .text_meta()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(muted)
                    .child("Suggested comment"),
            )
            .child(
                div()
                    .id("suggested-comment")
                    .aria_label(SharedString::from(body.to_string()))
                    .px_3()
                    .py_2()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().muted)
                    .text_body()
                    .child(markdown_ui::github_markdown(
                        SharedString::from(format!("suggested-comment-{id}")),
                        body,
                        Some(&repo),
                    ))
                    .test_support(),
            )
            .child(if added {
                h_flex()
                    .id("suggested-comment-added")
                    .gap_1p5()
                    .h_6()
                    .text_ui()
                    .text_color(muted)
                    .child(Icon::new(IconName::Check).small())
                    .child("In your review")
                    .test_support()
                    .into_any_element()
            } else {
                h_flex()
                    .gap_1()
                    .children(add)
                    .children(edit)
                    .into_any_element()
            })
    }
}

impl RenderOnce for AgentQuestionCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let question = self.question.read(cx);
        let id = self.question.entity_id().as_u64();
        let repo = question.key().repo().clone();
        let asked = question.question().to_string();
        let answer = question.answer().trim().to_string();
        let doing = (!question.state().is_over())
            .then(|| question.doing().map(str::to_string))
            .flatten();
        let suggestion = question.suggestion().map(str::to_string);
        let added = question.suggestion_added();
        let failure = match question.state() {
            RunState::Failed(message) => Some(message.clone()),
            _ => None,
        };
        let permission = question.permission().map(|prompt| {
            let answering = self.question.clone();
            crate::permission::prompt_card(
                prompt,
                &question.agent().login,
                question.cwd_label(),
                move |decision, cx| {
                    answering.update(cx, |question, cx| question.answer_permission(decision, cx))
                },
                cx,
            )
        });
        let muted = cx.theme().muted_foreground;

        v_flex()
            .id(("agent-question", id))
            .gap_2()
            .p_3()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius_lg)
            .bg(cx.theme().background)
            .child(self.header(cx))
            .child(
                div()
                    .pl_8()
                    .text_ui()
                    .text_color(muted)
                    .child(SharedString::from(asked)),
            )
            .when_some(doing, |this, doing| {
                this.child(
                    h_flex()
                        .gap_2()
                        .pl_8()
                        .text_ui()
                        .text_color(muted)
                        .child(Spinner::new().xsmall())
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(SharedString::from(doing)),
                        ),
                )
            })
            .when(!answer.is_empty(), |this| {
                this.child(
                    div()
                        .id("agent-answer")
                        .aria_label(SharedString::from(answer.clone()))
                        .pl_8()
                        .text_body()
                        .child(markdown_ui::github_markdown(
                            SharedString::from(format!("agent-answer-{id}")),
                            &answer,
                            Some(&repo),
                        ))
                        .test_support(),
                )
            })
            .children(permission)
            .when_some(failure, |this, message| {
                this.child(
                    h_flex()
                        .id("agent-question-failed")
                        .aria_label(SharedString::from(message.clone()))
                        .gap_2()
                        .pl_8()
                        .items_start()
                        .text_ui()
                        .text_color(cx.theme().danger)
                        .child(div().pt_0p5().child(Icon::new(IconName::CircleX).small()))
                        .child(div().flex_1().min_w_0().child(SharedString::from(message)))
                        .test_support(),
                )
            })
            .when_some(suggestion, |this, body| {
                this.child(self.suggestion(&body, added, cx))
            })
    }
}

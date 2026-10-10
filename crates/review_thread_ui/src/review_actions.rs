use app_state_ui::StoresExt as _;
use app_state_ui::actions::{SubmitReview, ToggleFocusMode, ToggleInspector};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariant, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    notification::Notification,
    popover::Popover,
    v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, RenderOnce,
    Styled as _, Window, div, prelude::FluentBuilder as _,
};
use review_model::{Actor, FindingState, PullRequestDetail, ReviewVerdict, ReviewerState};
use theme_ui::{ActiveAppTheme as _, TypographyExt as _};
use widgets_ui::AvatarSize;

use crate::ReviewThreadView;
use crate::timeline::pending_color;

/// Counts of what the viewer's pending review would post.
pub(crate) struct PendingSummary {
    pub comments: usize,
    pub findings: usize,
}

impl PendingSummary {
    pub(crate) fn of(detail: &PullRequestDetail) -> Self {
        Self {
            comments: detail
                .threads
                .iter()
                .flat_map(|thread| &thread.comments)
                .filter(|comment| comment.is_pending)
                .count(),
            findings: detail
                .findings
                .iter()
                .filter(|finding| finding.state == FindingState::Added)
                .count(),
        }
    }

    /// `2 pending comments · 1 agent finding`, for the pending review's tooltip.
    pub(crate) fn describe_pending(&self) -> String {
        format!(
            "{} · {}",
            plural(self.comments, "pending comment", "pending comments"),
            plural(self.findings, "agent finding", "agent findings")
        )
    }
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// The review's actions for the title bar: the pending review's size with Discard,
/// Approve, and Submit review.
#[derive(IntoElement)]
pub struct ReviewActions {
    view: Entity<ReviewThreadView>,
}

impl ReviewActions {
    /// The actions for the review shown in `view`.
    pub fn new(view: Entity<ReviewThreadView>) -> Self {
        Self { view }
    }
}

impl RenderOnce for ReviewActions {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(detail) = self.view.read(cx).detail.clone() else {
            return h_flex();
        };
        let merge_form = self.view.update(cx, |view, cx| view.merge_form(window, cx));
        let summary = PendingSummary::of(&detail);
        let pending = summary.comments + summary.findings;
        let discard_view = self.view.clone();
        let approve_view = self.view.clone();
        let submit_view = self.view.clone();
        h_flex()
            .gap_1()
            .when(pending > 0, |this| {
                this.child(
                    Button::new("discard-review")
                        .ghost()
                        .small()
                        .icon(Icon::new(IconName::MessageSquarePlus).text_color(pending_color(cx)))
                        .label(format!("{pending} pending"))
                        .dropdown_caret(true)
                        .tooltip(summary.describe_pending())
                        .dropdown_menu_with_anchor(
                            gpui_kit::Anchor::TopRight,
                            move |menu: PopupMenu, _, _| {
                                let view = discard_view.clone();
                                menu.item(
                                    PopupMenuItem::new("Discard pending review…")
                                        .icon(Icon::new(IconName::Trash))
                                        .on_click(move |_, window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.confirm_discard(window, cx)
                                            })
                                        }),
                                )
                            },
                        ),
                )
            })
            .child(approve_button(&detail, approve_view, cx))
            .children(crate::merge::merge_button(&detail, merge_form, cx))
            .child(
                bordered(
                    Button::new("submit-review"),
                    cx.theme().primary,
                    cx.theme().primary,
                )
                .small()
                .label("Submit review…")
                .tooltip_with_action("Submit review", &SubmitReview, None)
                .on_click(move |_, window, cx| {
                    submit_view.update(cx, |this, cx| this.open_submit_review(None, window, cx))
                }),
            )
            .child(layout_buttons(cx))
    }
}

/// Show or hide the right sidebar, and focus mode, which hides both sidebars.
fn layout_buttons(cx: &App) -> impl IntoElement + use<> {
    let layout = crate::ReviewLayout::get(cx);
    // On reads as full-strength ink, off as dimmed; neither gets a fill, as in Zed.
    let toggle_icon = |icon: IconName, on: bool| {
        Icon::new(icon).text_color(if on {
            cx.theme().foreground
        } else {
            cx.theme().muted_foreground
        })
    };
    h_flex()
        .gap_0p5()
        .ml_1()
        .child(
            Button::new("toggle-inspector")
                .ghost()
                .small()
                .icon(toggle_icon(IconName::PanelRight, layout.inspector))
                .tooltip_with_action(
                    if layout.inspector {
                        "Hide the right sidebar"
                    } else {
                        "Show the right sidebar"
                    },
                    &ToggleInspector,
                    None,
                )
                .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleInspector), cx)),
        )
        .child(
            Button::new("toggle-focus")
                .ghost()
                .small()
                .icon(toggle_icon(
                    if layout.is_focused() {
                        IconName::Minimize2
                    } else {
                        IconName::Maximize2
                    },
                    layout.is_focused(),
                ))
                .tooltip_with_action(
                    if layout.is_focused() {
                        "Leave focus mode"
                    } else {
                        "Focus mode: hide both sidebars"
                    },
                    &ToggleFocusMode,
                    None,
                )
                .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleFocusMode), cx)),
        )
}

/// A title bar button drawn as a 1 px `border` and `text` in color, with no fill, so
/// the bar stays calm.
pub(crate) fn bordered(button: Button, border: gpui_kit::Hsla, text: gpui_kit::Hsla) -> Button {
    button
        .ghost()
        .border_1()
        .border_color(border)
        .text_color(text)
}

/// Approve: a neutral bordered button until someone approves, then bold green
/// "Approved" without a frame, followed by the approvers' avatars. Clicking opens a popover that lists who approved and asks to confirm;
/// approving again stays possible, since GitHub records each approval.
fn approve_button(
    detail: &PullRequestDetail,
    view: Entity<ReviewThreadView>,
    cx: &App,
) -> impl IntoElement + use<> {
    let approvers: Vec<Actor> = detail
        .reviewers
        .iter()
        .filter(|reviewer| reviewer.state == ReviewerState::Approved)
        .map(|reviewer| reviewer.actor.clone())
        .collect();
    let viewer_login = detail
        .reviewers
        .iter()
        .find(|reviewer| reviewer.is_viewer)
        .map(|reviewer| reviewer.actor.login.clone());
    let viewer_approved = detail
        .reviewers
        .iter()
        .any(|reviewer| reviewer.is_viewer && reviewer.state == ReviewerState::Approved);
    let pending = PendingSummary::of(detail);
    let number = detail.summary.key.number();
    let green = cx.app_theme().approve;

    let trigger = if approvers.is_empty() {
        bordered(
            Button::new("approve"),
            cx.theme().border,
            cx.theme().foreground,
        )
        .small()
        .icon(IconName::Check)
        .label("Approve")
    } else {
        // Approved reads as a state, not a control: green bold text and the approvers'
        // faces, no frame. Overlapping avatars get a ring in the bar's color so each
        // stays distinct.
        let ring = cx.theme().title_bar;
        let shown = approvers.len().min(3);
        let more = approvers.len() - shown;
        let avatars = h_flex()
            .children(approvers.iter().take(shown).enumerate().map(|(ix, actor)| {
                div()
                    .when(ix > 0, |this| this.ml(gpui_kit::px(-5.)))
                    .rounded_full()
                    .border_2()
                    .border_color(ring)
                    .child(crate::cards::actor_avatar(actor, AvatarSize::Small))
            }))
            .when(more > 0, |this| {
                this.child(
                    div()
                        .ml_1()
                        .text_meta()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("+{more}")),
                )
            });
        Button::new("approve")
            .ghost()
            .small()
            .tooltip(if viewer_approved {
                "You approved this. Approve again…"
            } else {
                "Approve…"
            })
            .child(
                h_flex()
                    .gap_1p5()
                    .child(
                        h_flex()
                            .gap_1()
                            .text_color(green)
                            .font_weight(gpui_kit::FontWeight::BOLD)
                            .child(Icon::new(IconName::Check).small())
                            .child("Approved"),
                    )
                    .child(avatars),
            )
    };

    Popover::new("approve-popover")
        .anchor(gpui_kit::Anchor::TopRight)
        .arrow(true)
        .trigger(trigger)
        .content(move |_, _, cx| {
            let posts = if pending.comments + pending.findings == 0 {
                "Posts your approval to GitHub now.".to_string()
            } else {
                let parts: Vec<String> = [
                    (pending.comments, "pending comment", "pending comments"),
                    (pending.findings, "agent finding", "agent findings"),
                ]
                .into_iter()
                .filter(|(count, _, _)| *count > 0)
                .map(|(count, one, many)| plural(count, one, many))
                .collect();
                format!(
                    "Posts your approval to GitHub now, with {}.",
                    parts.join(" and ")
                )
            };
            let popover = cx.entity();
            let approve_view = view.clone();
            let approve_popover = popover.clone();
            // The title bar's actions don't wrap, and the popover inherits that.
            v_flex()
                .w(gpui_kit::rems(20.))
                .whitespace_normal()
                .gap_3()
                .child(
                    v_flex()
                        .gap_1()
                        .child(div().text_heading().child(format!("Approve #{number}?")))
                        .child(
                            div()
                                .text_ui()
                                .text_color(cx.theme().muted_foreground)
                                .child(posts),
                        ),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_meta()
                                .text_color(cx.theme().muted_foreground)
                                .child(match approvers.len() {
                                    0 => "No approvals yet".to_string(),
                                    1 => "Approved by 1 person".to_string(),
                                    n => format!("Approved by {n} people"),
                                }),
                        )
                        .children(approvers.iter().map(|actor| {
                            let is_viewer = viewer_login.as_deref() == Some(actor.login.as_str());
                            h_flex()
                                .gap_2()
                                .h_7()
                                .text_ui()
                                .child(crate::cards::actor_avatar(actor, AvatarSize::Small))
                                .child(widgets_ui::ActorName::new(actor))
                                .when(is_viewer, |this| {
                                    this.child(
                                        div()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("· you"),
                                    )
                                })
                                .child(div().flex_1())
                                .child(
                                    Icon::new(IconName::CircleCheck)
                                        .small()
                                        .text_color(cx.theme().success),
                                )
                        })),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("approve-cancel")
                                .ghost()
                                .small()
                                .label("Cancel")
                                .on_click(move |_, window, cx| {
                                    popover.update(cx, |state, cx| state.dismiss(window, cx))
                                }),
                        )
                        .child(
                            Button::new("approve-confirm")
                                .success()
                                .small()
                                .icon(IconName::Check)
                                .label(if viewer_approved {
                                    "Approve again"
                                } else {
                                    "Approve"
                                })
                                .on_click(move |_, window, cx| {
                                    approve_popover
                                        .update(cx, |state, cx| state.dismiss(window, cx));
                                    approve_view.update(cx, |this, cx| this.approve(window, cx));
                                }),
                        ),
                )
        })
}

impl ReviewThreadView {
    pub(crate) fn open_submit_review(
        &mut self,
        verdict: Option<ReviewVerdict>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        submit_review_ui::open(self.key.clone(), verdict, window, cx);
    }

    /// Shows the Review with an agent form at the top of the Reviews tab.
    pub fn open_agent_review(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.set_filter(crate::TimelineFilter::Reviews);
        self.start_form_open = Some(true);
        self.sync_rows(cx);
        self.list_state.scroll_to(gpui_kit::ListOffset::default());
        cx.notify();
    }

    /// Hides the start form.
    pub(crate) fn hide_agent_review(&mut self, cx: &mut Context<Self>) {
        self.start_form_open = Some(false);
        cx.notify();
    }

    /// The start form, made on first use. Starting a run folds it away.
    pub(crate) fn start_form(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<agent_review_ui::StartReviewForm> {
        if let Some(form) = &self.start_form {
            return form.clone();
        }
        let key = self.key.clone();
        let form = cx.new(|cx| agent_review_ui::StartReviewForm::new(key, window, cx));
        let subscription = cx.subscribe(
            &form,
            |this: &mut Self, _, event: &agent_review_ui::StartReviewEvent, cx| match event {
                agent_review_ui::StartReviewEvent::Started => this.hide_agent_review(cx),
            },
        );
        self._subscriptions.push(subscription);
        self.start_form = Some(form.clone());
        form
    }

    fn approve(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let viewer = self.viewer(cx);
        let key = self.key.clone();
        let task = self.store(cx).update(cx, |store, cx| {
            store.submit_review(&key, &viewer, ReviewVerdict::Approve, String::new(), cx)
        });
        let number = key.number();
        cx.spawn_in(window, async move |_, cx| {
            let result = task.await;
            cx.update(|window, cx| match result {
                Ok(()) => window
                    .push_notification(Notification::success(format!("Approved #{number}.")), cx),
                Err(error) => window.push_notification(
                    Notification::error(format!("Couldn’t approve: {error}")),
                    cx,
                ),
            })
            .ok();
        })
        .detach();
    }

    fn confirm_discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(detail) = self.detail.clone() else {
            return;
        };
        let summary = PendingSummary::of(&detail);
        let key = self.key.clone();
        let store = cx.stores().pull_requests().clone();
        let description = format!(
            "{} and {} will be removed. Findings return to drafts.",
            plural(summary.comments, "pending comment", "pending comments"),
            plural(summary.findings, "added finding", "added findings"),
        );
        window.open_alert_dialog(cx, move |alert, _, _| {
            let key = key.clone();
            let store = store.clone();
            alert
                .title("Discard pending review?")
                .description(description.clone())
                .confirm()
                .button_props(
                    gpui_kit::component::dialog::DialogButtonProps::default()
                        .ok_text("Discard")
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text("Cancel"),
                )
                .on_ok(move |_, _, cx| {
                    store.update(cx, |store, cx| store.discard_pending(&key, cx));
                    true
                })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_the_design_pending_review() {
        let summary = PendingSummary {
            comments: 2,
            findings: 1,
        };
        assert_eq!(
            summary.describe_pending(),
            "2 pending comments · 1 agent finding"
        );
    }
}

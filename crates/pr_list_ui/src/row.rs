use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, h_flex, list::ListItem, skeleton::Skeleton, v_flex,
};
use gpui_kit::{
    App, ClickEvent, FontWeight, Hsla, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div, prelude::FluentBuilder as _, rems,
};
use pr_model::{PullRequest, ReviewDecision, Timestamp};
use theme_ui::{ActiveAppTheme as _, TypographyExt as _};
use triage::{PullRequestRisk, Risk};
use widgets_ui::{CheckIcon, PullRequestStateIcon, StatusTag, Tone};

/// Width of the review-decision lane; fits "Changes requested" with its icon.
const REVIEW_LANE: f32 = 9.;
/// Width of the risk lane, in rem; empty when the risk is low or unknown.
const RISK_LANE: f32 = 7.;

/// The icon and color that identify a review decision.
pub fn review_decision_icon(decision: ReviewDecision, cx: &App) -> (IconName, Hsla) {
    match decision {
        ReviewDecision::ReviewRequested => (IconName::Eye, cx.app_theme().accent),
        ReviewDecision::ChangesRequested => (IconName::CircleX, cx.theme().danger),
        ReviewDecision::Approved => (IconName::CircleCheck, cx.theme().success),
        ReviewDecision::Commented => (IconName::MessageSquare, cx.theme().muted_foreground),
        ReviewDecision::NoReviews => (IconName::Circle, cx.theme().muted_foreground),
    }
}

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// One pull request in the list: state, title, provenance, review decision, CI,
/// comment count and age, each in a fixed lane so rows scan as columns.
#[derive(IntoElement)]
pub struct PullRequestRow {
    pull_request: PullRequest,
    now: Timestamp,
    selected: bool,
    triage: Option<PullRequestRisk>,
    repo_color: Option<Hsla>,
    on_click: Option<ClickHandler>,
}

impl PullRequestRow {
    /// A row for `pull_request`, with ages measured against `now`.
    pub fn new(pull_request: PullRequest, now: Timestamp) -> Self {
        Self {
            pull_request,
            now,
            selected: false,
            triage: None,
            repo_color: None,
            on_click: None,
        }
    }

    /// Shows the row as the list's selection.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Shows the triage provider's kind and risk for the pull request.
    pub fn triage(mut self, triage: Option<PullRequestRisk>) -> Self {
        self.triage = triage;
        self
    }

    /// Marks the row with the color the reviewer gave its repository.
    pub fn repo_color(mut self, color: Option<Hsla>) -> Self {
        self.repo_color = color;
        self
    }

    /// Handles activation.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for PullRequestRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let pr = &self.pull_request;
        let muted = cx.theme().muted_foreground;
        let meta: SharedString = format!(
            "#{} · {} · {} · {} → {}",
            pr.key.number(),
            pr.key.repo().name(),
            pr.author,
            pr.head_ref,
            pr.base_ref
        )
        .into();
        let (review_icon, review_color) = review_decision_icon(pr.review, cx);
        let label: SharedString = format!("#{} {}", pr.key.number(), pr.title).into();

        ListItem::new(SharedString::from(pr.key.id()))
            .selected(self.selected)
            .accessibility_label(label)
            .h(rems(3.5))
            .px_3()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(if self.selected {
                cx.theme().list_active_border
            } else {
                cx.theme().transparent
            })
            .when_some(self.on_click, |this, handler| {
                this.on_click(move |event, window, cx| handler(event, window, cx))
            })
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .child(PullRequestStateIcon::new(pr.state))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .text_ui()
                                            .font_weight(FontWeight::MEDIUM)
                                            .truncate()
                                            .child(pr.title.clone()),
                                    )
                                    .children(self.triage.map(|triage| {
                                        div()
                                            .flex_shrink_0()
                                            .text_meta()
                                            .text_color(muted)
                                            .child(triage.kind.label())
                                    })),
                            )
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .when_some(self.repo_color, |this, color| {
                                        this.child(
                                            div().flex_shrink_0().size_2().rounded_full().bg(color),
                                        )
                                    })
                                    .child(
                                        div()
                                            .min_w_0()
                                            .text_meta()
                                            .text_color(muted)
                                            .truncate()
                                            .child(meta),
                                    ),
                            ),
                    )
                    .child(
                        // Low and trivial risk stay quiet; the lane keeps the columns aligned.
                        h_flex()
                            .flex_shrink_0()
                            .w(rems(RISK_LANE))
                            .children(self.triage.and_then(|triage| {
                                let tone = match triage.risk {
                                    Risk::Trivial | Risk::Low => return None,
                                    Risk::Medium => Tone::Warning,
                                    Risk::High | Risk::Critical => Tone::Danger,
                                };
                                Some(StatusTag::new(tone, triage.risk.label()))
                            })),
                    )
                    .child(
                        h_flex()
                            .flex_shrink_0()
                            .w(rems(REVIEW_LANE))
                            .gap_1p5()
                            .text_ui()
                            .text_color(muted)
                            .child(Icon::new(review_icon).small().text_color(review_color))
                            .child(pr.review.label()),
                    )
                    .child(
                        h_flex()
                            .flex_shrink_0()
                            .w_6()
                            .justify_center()
                            .when_some(pr.ci, |this, ci| this.child(CheckIcon::new(ci))),
                    )
                    .child(
                        h_flex()
                            .flex_shrink_0()
                            .w_10()
                            .gap_1()
                            .text_ui()
                            .text_color(muted)
                            .child(Icon::new(IconName::MessageSquare).small())
                            .child(pr.comments.to_string()),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .w_10()
                            .text_right()
                            .text_ui()
                            .text_color(muted)
                            .child(pr.updated_at.short_age(self.now)),
                    ),
            )
    }
}

/// A placeholder row with the same geometry, shown while the list loads.
#[derive(IntoElement)]
pub struct SkeletonRow {
    title_width: f32,
}

impl SkeletonRow {
    /// A placeholder whose title bar is `title_width` rem wide.
    pub fn new(title_width: f32) -> Self {
        Self { title_width }
    }
}

impl RenderOnce for SkeletonRow {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        h_flex()
            .h(rems(3.5))
            .px_3()
            .gap_3()
            .child(Skeleton::new().size_4().rounded_full())
            .child(
                v_flex()
                    .flex_1()
                    .gap_1p5()
                    .child(Skeleton::new().h_3().w(rems(self.title_width)))
                    .child(Skeleton::new().h_2p5().w(rems(self.title_width * 0.65))),
            )
            .child(Skeleton::new().h_3().w(rems(15.)))
    }
}

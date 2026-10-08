use app_state_ui::actions::{CheckOut, OpenInZed};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenu},
    popover::Popover,
    v_flex,
};
use gpui_kit::{
    App, ClipboardItem, Entity, FontWeight, IntoElement, ParentElement as _, RenderOnce,
    SharedString, Styled as _, Window, div, rems,
};
use pr_model::PullRequestState;
use theme_ui::TypographyExt as _;
use widgets_ui::{PullRequestStateIcon, StatusTag, Tone};

use crate::ReviewThreadView;

/// The pull request under review, in the title bar: state, number and title as a
/// button whose popover holds the full title, who merges what where, and Open in Zed,
/// Check out, Copy link and Open on GitHub.
#[derive(IntoElement)]
pub struct PullRequestTitle {
    view: Entity<ReviewThreadView>,
}

impl PullRequestTitle {
    /// The title for the review shown in `view`.
    pub fn new(view: Entity<ReviewThreadView>) -> Self {
        Self { view }
    }
}

impl RenderOnce for PullRequestTitle {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let review = self.view.read(cx);
        let key = review.key().clone();
        let summary = review
            .detail()
            .map(|detail| detail.summary.clone())
            .or_else(|| {
                use app_state_ui::StoresExt as _;
                cx.stores()
                    .pull_requests()
                    .read(cx)
                    .pull_request(&key)
                    .cloned()
            });
        let merge = review.detail().map(|detail| detail.merge_summary());
        let Some(summary) = summary else {
            return div()
                .text_ui()
                .text_color(cx.theme().muted_foreground)
                .child(format!("{} #{}", key.repo(), key.number()))
                .into_any_element();
        };
        let state = summary.state;
        let title = SharedString::from(summary.title.clone());
        let number = format!("#{}", key.number());
        let repository = key.repo().to_string();
        let url = format!("https://github.com/{}/pull/{}", key.repo(), key.number());

        let trigger = Button::new("pull-request-title").ghost().small().child(
            h_flex()
                .gap_1p5()
                .min_w_0()
                .child(PullRequestStateIcon::new(state).small())
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(number.clone()),
                )
                .child(
                    div()
                        .max_w(rems(28.))
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .child(title.clone()),
                )
                .child(
                    Icon::new(IconName::ChevronDown)
                        .xsmall()
                        .text_color(cx.theme().muted_foreground),
                ),
        );

        let view = self.view.clone();
        let target = pr_actions_ui::PullRequestTarget::of(&summary);
        Popover::new("pull-request-popover")
            .anchor(gpui_kit::Anchor::TopLeft)
            .arrow(true)
            .trigger(trigger)
            .content(move |_, _, cx| {
                let (tone, icon) = match state {
                    PullRequestState::Open => (Tone::Success, IconName::GitPullRequest),
                    PullRequestState::Draft => (Tone::Neutral, IconName::GitPullRequestDraft),
                    PullRequestState::Merged => (Tone::Merged, IconName::GitMerge),
                    PullRequestState::Closed => (Tone::Danger, IconName::GitPullRequestClosed),
                };
                let popover = cx.entity();
                let zed_view = view.clone();
                let zed_popover = popover.clone();
                let checkout_view = view.clone();
                let checkout_popover = popover.clone();
                let diff_view = view.clone();
                let diff_popover = popover.clone();
                let copy = url.clone();
                let copy_popover = popover.clone();
                let open = url.clone();
                v_flex()
                    .w(rems(30.))
                    .gap_3()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_heading().child(format!("{title} {number}")))
                            .child(
                                div()
                                    .text_meta()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(repository.clone()),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_start()
                            .child(StatusTag::new(tone, state.label()).icon(icon))
                            .children(merge.clone().map(|merge| {
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_ui()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(merge)
                            })),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .flex_wrap()
                            .child(
                                Button::new("title-open-in-zed")
                                    .outline()
                                    .small()
                                    .icon(IconName::ExternalLink)
                                    .label("Open in Zed")
                                    .tooltip_with_action(
                                        "Open the pull request’s code in Zed",
                                        &OpenInZed,
                                        None,
                                    )
                                    .on_click(move |_, window, cx| {
                                        zed_popover
                                            .update(cx, |state, cx| state.dismiss(window, cx));
                                        zed_view.update(cx, |this, cx| {
                                            this.open_in_zed(None, None, window, cx)
                                        });
                                    }),
                            )
                            .child(
                                Button::new("title-diff-in-zed")
                                    .ghost()
                                    .small()
                                    .icon(IconName::FileDiff)
                                    .label("Diff in Zed")
                                    .tooltip("Show every change as one diff in Zed")
                                    .on_click(move |_, window, cx| {
                                        diff_popover
                                            .update(cx, |state, cx| state.dismiss(window, cx));
                                        let key = diff_view.read(cx).key().clone();
                                        pr_actions_ui::diff_in_zed(&key, window, cx);
                                    }),
                            )
                            .child(
                                Button::new("title-check-out")
                                    .ghost()
                                    .small()
                                    .icon(IconName::GitBranch)
                                    .label("Check out")
                                    .tooltip_with_action(
                                        "Check out the head branch",
                                        &CheckOut,
                                        None,
                                    )
                                    .on_click(move |_, window, cx| {
                                        checkout_popover
                                            .update(cx, |state, cx| state.dismiss(window, cx));
                                        checkout_view
                                            .update(cx, |this, cx| this.check_out(window, cx));
                                    }),
                            )
                            .child(
                                Button::new("title-copy-link")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Copy)
                                    .label("Copy link")
                                    .on_click(move |_, window, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            copy.clone(),
                                        ));
                                        copy_popover
                                            .update(cx, |state, cx| state.dismiss(window, cx));
                                    }),
                            )
                            .child(
                                Button::new("title-open-github")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Globe)
                                    .label("Open on GitHub")
                                    .on_click(move |_, _, cx| cx.open_url(&open)),
                            )
                            .child({
                                let target = target.clone();
                                let agent_view = view.clone();
                                Button::new("title-more")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Ellipsis)
                                    .accessibility_label("More pull request actions")
                                    .dropdown_menu(move |menu: PopupMenu, _, _| {
                                        let agent_view = agent_view.clone();
                                        pr_actions_ui::pull_request_menu(
                                            menu,
                                            &target,
                                            Some(std::rc::Rc::new(move |window, cx| {
                                                agent_view.update(cx, |this, cx| {
                                                    this.open_agent_review(window, cx)
                                                })
                                            })),
                                        )
                                    })
                            }),
                    )
            })
            .into_any_element()
    }
}

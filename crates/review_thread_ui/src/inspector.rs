use app_state_ui::StoresExt as _;
use app_state_ui::actions::ToggleInspector;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    list::ListItem,
    spinner::Spinner,
    v_flex,
};
use gpui_kit::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, TestSupportExt as _, div,
    prelude::FluentBuilder as _, rems,
};
use review_model::{PullRequestDetail, ReviewerState};
use theme_ui::{ActiveAppTheme as _, TypographyExt as _};
use widgets_ui::{CheckIcon, PullRequestStateIcon, SectionHeader};

use crate::ReviewThreadView;
use crate::cards::actor_avatar;

/// A small button that hides or shows the inspector, like the one in the title bar.
fn inspector_toggle(id: &'static str, tooltip: &'static str) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .icon(IconName::PanelRight)
        .tooltip_with_action(tooltip, &ToggleInspector, None)
        .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleInspector), cx))
}

impl ReviewThreadView {
    /// A narrow column of check icons that stands in for the hidden inspector; each icon
    /// opens its check's log. `None` while the inspector shows, in focus mode, or before
    /// the review data has loaded.
    pub(crate) fn render_check_rail(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let layout = crate::ReviewLayout::get(cx);
        if layout.inspector || layout.is_focused() {
            return None;
        }
        let detail = self.detail.as_ref()?;
        let icons = detail.checks.iter().map(|check| {
            let key = self.key.clone();
            let id = check.id.clone();
            let name = SharedString::from(check.name.clone());
            div()
                .id(SharedString::from(format!(
                    "inspector-check-icon-{}",
                    check.id
                )))
                .aria_label(SharedString::from(format!("{} log", check.name)))
                .test_support()
                .p_0p5()
                .rounded(cx.theme().radius)
                .cursor_pointer()
                .hover(|this| this.bg(cx.theme().list_hover))
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(name.clone()).build(window, cx)
                })
                .on_click(move |_, window, cx| {
                    check_log_ui::open(key.clone(), id.clone(), window, cx)
                })
                .child(CheckIcon::new(check.status))
        });
        Some(
            v_flex()
                .id("check-rail")
                .flex_none()
                .h_full()
                .py_2()
                .px_1()
                .gap_0p5()
                .items_center()
                .overflow_y_scroll()
                .border_l_1()
                .border_color(cx.theme().border)
                .child(inspector_toggle("expand-inspector", "Show the right sidebar"))
                .children(icons)
                .into_any_element(),
        )
    }

    pub(crate) fn render_inspector(
        &self,
        detail: &PullRequestDetail,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let reviewers = detail.reviewers.iter().map(|reviewer| {
            let (suffix, state): (Option<&str>, AnyElement) = match reviewer.state {
                ReviewerState::Approved => (
                    None,
                    Icon::new(IconName::CircleCheck)
                        .small()
                        .text_color(cx.theme().success)
                        .into_any_element(),
                ),
                ReviewerState::Commented => (
                    None,
                    Icon::new(IconName::MessageSquare)
                        .small()
                        .text_color(cx.theme().muted_foreground)
                        .into_any_element(),
                ),
                ReviewerState::ChangesRequested => (
                    None,
                    Icon::new(IconName::CircleX)
                        .small()
                        .text_color(cx.theme().danger)
                        .into_any_element(),
                ),
                ReviewerState::Reviewing => (
                    Some("reviewing"),
                    Spinner::new()
                        .small()
                        .icon(Icon::new(IconName::LoaderCircle))
                        .color(cx.theme().warning)
                        .into_any_element(),
                ),
                ReviewerState::Pending => (
                    Some(if reviewer.is_viewer {
                        "review pending"
                    } else {
                        "pending"
                    }),
                    Icon::new(IconName::Eye)
                        .small()
                        .text_color(cx.app_theme().accent)
                        .into_any_element(),
                ),
            };
            let label = if reviewer.is_viewer && reviewer.state == ReviewerState::Pending {
                div().child("You").into_any_element()
            } else {
                widgets_ui::ActorName::new(&reviewer.actor).into_any_element()
            };
            let label = h_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .overflow_hidden()
                .child(label)
                .when_some(suffix, |this, suffix| {
                    this.child(
                        div()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("· {suffix}")),
                    )
                });
            let avatar = if reviewer.is_viewer {
                let name = cx
                    .stores()
                    .settings()
                    .read(cx)
                    .viewer()
                    .map(|viewer| viewer.display_name().to_string())
                    .unwrap_or_default();
                widgets_ui::InitialsAvatar::new(&name).into_any_element()
            } else if reviewer.actor.is_agent {
                widgets_ui::InitialsAvatar::new(&reviewer.actor.login).into_any_element()
            } else {
                actor_avatar(&reviewer.actor, widgets_ui::AvatarSize::Small).into_any_element()
            };
            h_flex()
                .h_7()
                .px_2()
                .gap_2()
                .text_ui()
                .child(avatar)
                .child(label)
                .child(state)
        });

        let checks = detail.checks.iter().map(|check| {
            let key = self.key.clone();
            let id = check.id.clone();
            ListItem::new(SharedString::from(format!("inspector-check-{}", check.id)))
                .h(rems(1.4375))
                .px_2()
                .rounded(cx.theme().radius)
                .accessibility_label(SharedString::from(format!("{} log", check.name)))
                .on_click(move |_, window, cx| {
                    check_log_ui::open(key.clone(), id.clone(), window, cx)
                })
                .child(
                    h_flex()
                        .gap_2()
                        .text_ui()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(SharedString::from(check.name.clone())),
                        )
                        .child(CheckIcon::new(check.status)),
                )
        });

        let refresh_key = self.key.clone();
        v_flex()
            .id("inspector")
            .test_support()
            .size_full()
            .h_full()
            .overflow_y_scroll()
            .py_4()
            .px_2()
            .gap_5()
            .child(
                v_flex()
                    .gap_0p5()
                    .child(SectionHeader::new("Reviewers").action(inspector_toggle(
                        "collapse-inspector",
                        "Hide the right sidebar",
                    )))
                    .children(reviewers),
            )
            .child(
                v_flex()
                    .gap_0p5()
                    .child(
                        SectionHeader::new(format!(
                            "Checks · {} of {} passed",
                            detail.passed_checks(),
                            detail.checks.len()
                        ))
                        .action(
                            Button::new("refresh-checks")
                                .ghost()
                                .xsmall()
                                .icon(IconName::RefreshCw)
                                .tooltip("Refresh checks")
                                .on_click(move |_, _, cx| {
                                    let store = cx.stores().pull_requests().clone();
                                    let key = refresh_key.clone();
                                    store.update(cx, |store, cx| store.refresh_checks(&key, cx));
                                }),
                        ),
                    )
                    .children(checks),
            )
            .children(self.render_depends(detail, cx))
    }

    fn render_depends(
        &self,
        detail: &PullRequestDetail,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        if detail.depends_on.is_empty() {
            return None;
        }
        let rows: Vec<_> = detail
            .depends_on
            .iter()
            .map(|dependency| {
                let state = cx
                    .stores()
                    .pull_requests()
                    .read(cx)
                    .pull_request(&dependency.key)
                    .map(|pr| pr.state);
                let key = dependency.key.clone();
                v_flex()
                    .child(
                        ListItem::new(SharedString::from(format!(
                            "depends-{}",
                            dependency.key.id()
                        )))
                        .h(rems(1.4375))
                        .px_2()
                        .rounded(cx.theme().radius)
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(crate::ReviewThreadEvent::Open(key.clone()))
                        }))
                        .child(
                            h_flex()
                                .gap_2()
                                .text_ui()
                                .child(div().flex_1().min_w_0().truncate().child(format!(
                                    "#{} {}",
                                    dependency.key.number(),
                                    dependency.title
                                )))
                                .children(
                                    state.map(|state| PullRequestStateIcon::new(state).small()),
                                ),
                        ),
                    )
                    .child(
                        div()
                            .px_2()
                            .text_meta()
                            .text_color(cx.theme().muted_foreground)
                            .child(SharedString::from(dependency.note.clone())),
                    )
            })
            .collect();
        Some(
            v_flex()
                .gap_0p5()
                .child(SectionHeader::new("Depends on"))
                .children(rows),
        )
    }
}

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, v_flex};
use gpui_kit::{
    AnyElement, App, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _,
    Window, div, prelude::FluentBuilder as _,
};
use theme_ui::TypographyExt as _;

/// A centered explanation for a region with nothing to show, with the next action.
#[derive(IntoElement)]
pub struct EmptyState {
    icon: IconName,
    title: SharedString,
    description: SharedString,
    action: Option<AnyElement>,
}

impl EmptyState {
    /// An empty state with an icon, a title and one sentence of explanation.
    pub fn new(
        icon: IconName,
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
    ) -> Self {
        Self {
            icon,
            title: title.into(),
            description: description.into(),
            action: None,
        }
    }

    /// Adds the button that leads out of the empty state.
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.action = Some(action.into_any_element());
        self
    }
}

impl RenderOnce for EmptyState {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        v_flex()
            .items_center()
            .gap_2()
            .py_8()
            .px_4()
            .child(
                Icon::new(self.icon)
                    .size_5()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                div()
                    .text_body()
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .child(self.title),
            )
            .child(
                div()
                    .text_ui()
                    .text_color(cx.theme().muted_foreground)
                    .text_center()
                    .child(self.description),
            )
            .when_some(self.action, |this, action| {
                this.child(div().pt_1().child(action))
            })
    }
}

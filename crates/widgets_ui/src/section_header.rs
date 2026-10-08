use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::{
    AnyElement, App, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _,
    Window, div, prelude::FluentBuilder as _,
};
use theme_ui::TypographyExt as _;

/// A muted label that names a group of rows, with an optional trailing action such as
/// an add button.
#[derive(IntoElement)]
pub struct SectionHeader {
    label: SharedString,
    action: Option<AnyElement>,
}

impl SectionHeader {
    /// A header reading `label`.
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            action: None,
        }
    }

    /// Adds an element at the trailing edge, usually a small ghost icon button.
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.action = Some(action.into_any_element());
        self
    }
}

impl RenderOnce for SectionHeader {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        h_flex()
            .h_7()
            .px_2()
            .gap_2()
            .text_meta()
            .text_color(cx.theme().muted_foreground)
            .child(div().flex_1().min_w_0().truncate().child(self.label))
            .when_some(self.action, |this, action| this.child(action))
    }
}

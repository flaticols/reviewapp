use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::{App, IntoElement, ParentElement as _, RenderOnce, Styled as _, Window, rems};
use theme_ui::TypographyExt as _;

/// A neutral count pill, as sidebars and tabs show next to a label.
#[derive(IntoElement)]
pub struct Counter {
    count: usize,
}

impl Counter {
    /// A pill showing `count`.
    pub fn new(count: usize) -> Self {
        Self { count }
    }
}

impl RenderOnce for Counter {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        h_flex()
            .flex_shrink_0()
            .justify_center()
            .min_w_5()
            .h(rems(1.125))
            .px_1p5()
            .rounded_full()
            .bg(cx.theme().muted)
            .text_meta()
            .text_color(cx.theme().muted_foreground)
            .child(self.count.to_string())
    }
}

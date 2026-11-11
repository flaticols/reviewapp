//! Comments can nest alerts and details as deep as they like; rendering must not
//! follow them all the way down.

use gpui_kit::component::Root;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AppContext as _, Context, IntoElement, ParentElement as _, Render, Styled as _, TestAppContext,
    Window, div, px, size,
};

struct Comment(String);

impl Render for Comment {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(markdown_ui::github_markdown("comment", &self.0, None))
    }
}

/// `levels` open details blocks written as fences, each inside the last, with an alert
/// at every level.
fn nested_fences(levels: usize) -> String {
    let mut text = String::from("bottom");
    for level in 0..levels {
        let fence = "`".repeat(level + 4);
        text = format!(
            "> [!NOTE]\n> level {level}\n\n{fence}switchfold-details-open Level {level}\n{text}\n{fence}"
        );
    }
    text
}

#[gpui_kit::test]
fn deep_nesting_renders_without_exhausting_the_stack(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        theme_ui::apply(&settings_model::Preferences::default(), None, cx);
    });
    let text = nested_fences(200);
    let window = cx.open_window(size(px(800.), px(600.)), move |window, cx| {
        let comment = cx.new(|_| Comment(text));
        Root::new(comment, window, cx)
    });
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
        .unwrap();
}

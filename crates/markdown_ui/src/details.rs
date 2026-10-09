use gpui_kit::assets::IconName;
use gpui_kit::component::text::{MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    App, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, prelude::FluentBuilder as _,
};

use crate::source::{DETAILS_LANG, DETAILS_OPEN_LANG};

const NODE_NAME: &str = "github-details";

#[derive(Clone, Debug)]
struct Details {
    summary: SharedString,
    body: SharedString,
    open: bool,
}

/// `<details>` blocks, which the source pass rewrote as fences: a summary row that
/// shows or hides the body.
pub(crate) struct DetailsPlugin;

impl MarkdownPlugin for DetailsPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        NODE_NAME
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::Code(code) = node else {
            return None;
        };
        let open = match code.lang.as_deref() {
            Some(DETAILS_LANG) => false,
            Some(DETAILS_OPEN_LANG) => true,
            _ => return None,
        };
        let summary = code.meta.clone().unwrap_or_else(|| "Details".into());
        Some(
            MarkdownNode::new(
                NODE_NAME,
                Details {
                    summary: summary.clone().into(),
                    body: code.value.clone().into(),
                    open,
                },
            )
            .text(format!("{summary}\n{}", code.value))
            .markdown(cx.node_source(node).unwrap_or_default()),
        )
    }

    fn render(&self, node: &MarkdownNode, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(details) = node.data::<Details>().cloned() else {
            return v_flex().into_any_element();
        };
        let start = node.source_range().map_or(0, |range| range.start);
        let initially_open = details.open;
        let open = window.use_keyed_state(
            SharedString::from(format!("details-open-{start}")),
            cx,
            move |_, _| initially_open,
        );
        let is_open = *open.read(cx);
        v_flex()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius)
            .child(
                h_flex()
                    .id(SharedString::from(format!("details-summary-{start}")))
                    .gap_1()
                    .px_2()
                    .py_1()
                    .cursor_pointer()
                    .hover(|style| style.bg(cx.theme().muted))
                    .child(
                        Icon::new(if is_open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .small()
                        .text_color(cx.theme().muted_foreground),
                    )
                    .child(details.summary.clone())
                    .on_click(move |_, _, cx| {
                        open.update(cx, |open, cx| {
                            *open = !*open;
                            cx.notify();
                        })
                    }),
            )
            .when(is_open, |this| {
                this.child(v_flex().px_3().pb_2().child(crate::view(
                    SharedString::from(format!("details-body-{start}")),
                    details.body.clone(),
                )))
            })
            .into_any_element()
    }
}

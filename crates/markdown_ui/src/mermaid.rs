use std::sync::Arc;

use gpui_kit::component::text::{MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast};
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::{
    App, Image, ImageFormat, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, img, px,
};
use mermaid_rs_renderer::{RenderOptions, Theme, render_with_options};
use theme_ui::TypographyExt as _;

const NODE_NAME: &str = "mermaid";

/// The largest diagram source drawn. Layout cost grows quickly with size and comments
/// come from anyone, so bigger ones show as source.
const MAX_SOURCE_BYTES: usize = 16 * 1024;
/// The largest drawing kept, in points, so one diagram can't ask for a huge texture.
const MAX_DRAWING_SIDE: f32 = 6000.;

/// One drawing of a diagram, at its natural size in points.
#[derive(Clone, Debug)]
struct Drawing {
    image: Arc<Image>,
    width: f32,
    height: f32,
}

#[derive(Clone, Debug)]
struct Diagram {
    light: Result<Drawing, String>,
    dark: Result<Drawing, String>,
    source: SharedString,
}

/// ` ```mermaid ` fences, drawn as diagrams. Both appearances are drawn while parsing,
/// off the UI thread, so switching between light and dark needs no redraw.
pub(crate) struct MermaidPlugin;

impl MarkdownPlugin for MermaidPlugin {
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
        if !code
            .lang
            .as_deref()
            .is_some_and(|lang| lang.eq_ignore_ascii_case("mermaid"))
        {
            return None;
        }
        let diagram = Diagram {
            light: draw(&code.value, Theme::modern()),
            dark: draw(&code.value, Theme::dark()),
            source: code.value.clone().into(),
        };
        Some(
            MarkdownNode::new(NODE_NAME, diagram)
                .text(code.value.clone())
                .markdown(cx.node_source(node).unwrap_or_default())
                .accessibility_label("Mermaid diagram"),
        )
    }

    fn render(&self, node: &MarkdownNode, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(diagram) = node.data::<Diagram>() else {
            return div().into_any_element();
        };
        let start = node.source_range().map_or(0, |range| range.start);
        let drawing = if cx.theme().is_dark() {
            &diagram.dark
        } else {
            &diagram.light
        };
        match drawing {
            Ok(drawing) => div()
                .id(SharedString::from(format!("mermaid-{start}")))
                .overflow_x_scroll()
                .p_3()
                .border_1()
                .border_color(cx.theme().border)
                .rounded(cx.theme().radius)
                .child(
                    img(drawing.image.clone())
                        .flex_none()
                        .w(px(drawing.width))
                        .h(px(drawing.height)),
                )
                .into_any_element(),
            Err(error) => v_flex()
                .gap_1()
                .child(
                    div()
                        .text_meta()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("Couldn’t draw this Mermaid diagram: {error}")),
                )
                .child(crate::view(
                    SharedString::from(format!("mermaid-source-{start}")),
                    format!("```\n{}\n```", diagram.source),
                ))
                .into_any_element(),
        }
    }
}

/// Draws `source` with `theme` on a transparent background, so it sits on whatever
/// the comment does.
fn draw(source: &str, mut theme: Theme) -> Result<Drawing, String> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(format!(
            "it’s longer than {} KB, the most Switchfold draws",
            MAX_SOURCE_BYTES / 1024
        ));
    }
    theme.background = "transparent".into();
    let options = RenderOptions {
        theme,
        ..RenderOptions::default()
    };
    let svg =
        render_with_options(source, options).map_err(|error| first_line(&error.to_string()))?;
    let (width, height) = natural_size(&svg).ok_or("the drawing has no size")?;
    if !(width.is_finite() && height.is_finite())
        || width > MAX_DRAWING_SIDE
        || height > MAX_DRAWING_SIDE
    {
        return Err("it’s too large to draw".into());
    }
    Ok(Drawing {
        image: Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes())),
        width,
        height,
    })
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or(text).to_string()
}

/// The `width` and `height` of the root `<svg>` element.
fn natural_size(svg: &str) -> Option<(f32, f32)> {
    let root = &svg[svg.find("<svg")?..];
    let root = &root[..root.find('>')?];
    let attribute = |name: &str| -> Option<f32> {
        let start = root.find(&format!(" {name}=\""))? + name.len() + 3;
        let end = start + root[start..].find('"')?;
        root[start..end].trim_end_matches("px").parse().ok()
    };
    Some((attribute("width")?, attribute("height")?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_flowcharts_and_reports_bad_syntax() {
        let drawing = draw("flowchart LR\n  A[Start] --> B[Done]", Theme::modern()).unwrap();
        assert!(drawing.width > 50. && drawing.height > 10.);
        let svg = String::from_utf8(drawing.image.bytes.clone()).unwrap();
        assert!(svg.contains("Start") && !svg.contains("foreignObject"));
        assert!(draw("zzz", Theme::modern()).is_err());
        let huge = format!("flowchart LR\n{}", "  A --> B\n".repeat(3000));
        assert!(draw(&huge, Theme::modern()).is_err());
        assert!(draw("graph TD\n A-->", Theme::modern()).is_err());
    }
}

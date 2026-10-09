use gpui_kit::assets::IconName;
use gpui_kit::component::text::{MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    App, FontWeight, IntoElement, ParentElement as _, SharedString, Styled as _, Window,
};
use theme_ui::ActiveAppTheme as _;

const NODE_NAME: &str = "github-alert";

/// How many `>` markers deep alerts are still drawn; each level renders the next as
/// its own text view, so unbounded nesting from a comment could exhaust the stack.
const MAX_QUOTE_DEPTH: usize = 8;

/// GitHub's five alert kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

impl Kind {
    fn parse(marker: &str) -> Option<Self> {
        let name = marker.trim().strip_prefix("[!")?.strip_suffix(']')?;
        match name.to_ascii_uppercase().as_str() {
            "NOTE" => Some(Self::Note),
            "TIP" => Some(Self::Tip),
            "IMPORTANT" => Some(Self::Important),
            "WARNING" => Some(Self::Warning),
            "CAUTION" => Some(Self::Caution),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Note => "Note",
            Self::Tip => "Tip",
            Self::Important => "Important",
            Self::Warning => "Warning",
            Self::Caution => "Caution",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Note => IconName::Info,
            Self::Tip => IconName::Lightbulb,
            Self::Important => IconName::MessageSquareWarning,
            Self::Warning => IconName::TriangleAlert,
            Self::Caution => IconName::OctagonAlert,
        }
    }
}

#[derive(Clone, Debug)]
struct Alert {
    kind: Kind,
    body: SharedString,
}

/// `> [!NOTE]` and its siblings: a block quote whose first line names the alert.
pub(crate) struct AlertPlugin;

impl MarkdownPlugin for AlertPlugin {
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
        let markdown_ast::Node::Blockquote(_) = node else {
            return None;
        };
        let source = cx.node_source(node)?;
        let (kind, body) = parse_alert(source)?;
        Some(
            MarkdownNode::new(
                NODE_NAME,
                Alert {
                    kind,
                    body: body.clone().into(),
                },
            )
            .text(format!("{}: {body}", kind.label()))
            .markdown(source),
        )
    }

    fn render(&self, node: &MarkdownNode, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(alert) = node.data::<Alert>() else {
            return v_flex().into_any_element();
        };
        let color = match alert.kind {
            Kind::Note => cx.app_theme().accent,
            Kind::Tip => cx.theme().success,
            Kind::Important => cx.app_theme().merged,
            Kind::Warning => cx.theme().warning,
            Kind::Caution => cx.theme().danger,
        };
        let id = SharedString::from(format!(
            "alert-{}",
            node.source_range().map_or(0, |range| range.start)
        ));
        v_flex()
            .gap_1()
            .pl_3()
            .py_0p5()
            .border_l_4()
            .border_color(color)
            .child(
                h_flex()
                    .gap_1p5()
                    .text_color(color)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(Icon::new(alert.kind.icon()).small())
                    .child(alert.kind.label()),
            )
            .child(crate::view(id, alert.body.clone()))
            .into_any_element()
    }
}

/// The alert kind and its body as Markdown, when `source` (a block quote, with its `>`
/// markers) starts with an alert marker on a line of its own.
fn parse_alert(source: &str) -> Option<(Kind, String)> {
    let too_deep = source.lines().any(|line| {
        line.chars()
            .take_while(|c| *c == '>' || c.is_whitespace())
            .filter(|c| *c == '>')
            .count()
            > MAX_QUOTE_DEPTH
    });
    if too_deep {
        return None;
    }
    let mut lines = source.lines().map(|line| {
        let trimmed = line.trim_start();
        match trimmed.strip_prefix('>') {
            Some(rest) => rest.strip_prefix(' ').unwrap_or(rest),
            // A lazy continuation line, without its own marker.
            None => trimmed,
        }
    });
    let kind = Kind::parse(lines.next()?)?;
    let body: Vec<&str> = lines.collect();
    Some((kind, body.join("\n").trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_marker_and_unquotes_the_body() {
        let (kind, body) = parse_alert("> [!warning]\n> Urgent.\n>\n> - one\n  lazy").unwrap();
        assert_eq!(kind, Kind::Warning);
        assert_eq!(body, "Urgent.\n\n- one\nlazy");
        assert!(parse_alert("> [!NOTE] inline text").is_none());
        assert!(parse_alert("> [!FOO]\n> x").is_none());
        assert!(parse_alert("> plain quote").is_none());
        let deep = format!("> [!NOTE]\n{}x", "> ".repeat(20));
        assert!(parse_alert(&deep).is_none());
    }
}

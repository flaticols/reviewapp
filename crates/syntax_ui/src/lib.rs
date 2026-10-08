//! Tree-sitter syntax highlighting for diffs, colored with the design's syntax tokens.
//! Each side of a hunk is highlighted as one text so multi-line constructs keep their
//! context, then split back into per-line spans.

use std::ops::Range;

use diff_model::{FileDiff, LineKind};
use gpui_kit::base::input::HighlightStyleResolver;
use gpui_kit::component::Rope;
use gpui_kit::component::highlighter::SyntaxHighlighter;
use gpui_kit::{FontStyle, HighlightStyle, Hsla};
use theme_ui::AppTheme;

/// Highlight spans for one line, as byte ranges into that line's text.
pub type LineSpans = Vec<(Range<usize>, HighlightStyle)>;

/// Spans for every line of every hunk of a file, indexed `[hunk][line]`.
#[derive(Clone, Debug, Default)]
pub struct FileHighlights {
    hunks: Vec<Vec<LineSpans>>,
}

impl FileHighlights {
    /// The spans of line `line_ix` in hunk `hunk_ix`; empty when unhighlighted.
    pub fn line(&self, hunk_ix: usize, line_ix: usize) -> &[(Range<usize>, HighlightStyle)] {
        self.hunks
            .get(hunk_ix)
            .and_then(|hunk| hunk.get(line_ix))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

/// The tree-sitter language name for a path, or `None` when Switchfold bundles no grammar.
pub fn language_for_path(path: &str) -> Option<&'static str> {
    let file = path.rsplit('/').next().unwrap_or(path);
    match file {
        "Makefile" | "makefile" | "GNUmakefile" => return Some("make"),
        "CMakeLists.txt" => return Some("cmake"),
        "Dockerfile" | ".bashrc" | ".zshrc" | ".profile" | ".envrc" => return Some("bash"),
        "go.mod" | "go.sum" => return None,
        _ => {}
    }
    let extension = file.rsplit_once('.').map(|(_, ext)| ext)?;
    Some(match extension.to_ascii_lowercase().as_str() {
        "rs" => "rust",
        "toml" => "toml",
        "md" | "markdown" | "mdx" => "markdown",
        "json" | "jsonc" | "json5" => "json",
        "yml" | "yaml" => "yaml",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "py" | "pyi" => "python",
        "go" => "go",
        "sh" | "bash" | "zsh" => "bash",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" => "cpp",
        "cs" => "csharp",
        "css" | "scss" => "css",
        "diff" | "patch" => "diff",
        "ex" | "exs" => "elixir",
        "erb" => "erb",
        "ejs" => "ejs",
        "graphql" | "gql" => "graphql",
        "html" | "htm" => "html",
        // XML has no grammar of its own here; HTML's colors its tags, attributes
        // and strings the same way.
        "xml" | "xsd" | "xsl" | "xslt" | "svg" | "plist" | "wsdl" | "csproj" | "pom" => "html",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "lua" => "lua",
        "mk" => "make",
        "cmake" => "cmake",
        "php" => "php",
        "proto" => "proto",
        "rb" | "rake" | "gemspec" => "ruby",
        "scala" | "sc" => "scala",
        "svelte" => "svelte",
        "swift" => "swift",
        "zig" => "zig",
        "astro" => "astro",
        _ => return None,
    })
}

/// Maps tree-sitter capture names onto the six syntax colors the design defines.
struct Palette {
    keyword: Hsla,
    string: Hsla,
    function: Hsla,
    type_: Hsla,
    comment: Hsla,
    number: Hsla,
}

impl Palette {
    fn new(theme: &AppTheme) -> Self {
        Self {
            keyword: theme.syntax_keyword,
            string: theme.syntax_string,
            function: theme.syntax_function,
            type_: theme.syntax_type,
            comment: theme.syntax_comment,
            number: theme.syntax_number,
        }
    }
}

impl HighlightStyleResolver for Palette {
    fn style(&self, name: &str) -> Option<HighlightStyle> {
        let root = name.split('.').next().unwrap_or(name);
        let color = match (root, name) {
            (_, "variable.special") => self.keyword,
            ("keyword" | "preproc" | "tag", _) => self.keyword,
            ("string" | "text", _) => self.string,
            ("function" | "constructor" | "title", _) => self.function,
            ("type" | "enum" | "attribute" | "label", _) => self.type_,
            ("comment", _) => {
                return Some(HighlightStyle {
                    color: Some(self.comment),
                    font_style: Some(FontStyle::Normal),
                    ..HighlightStyle::default()
                });
            }
            ("number" | "boolean" | "constant", _) => self.number,
            _ => return None,
        };
        Some(HighlightStyle {
            color: Some(color),
            ..HighlightStyle::default()
        })
    }
}

/// Highlights `text` as `language`, returning spans in byte offsets of `text`.
pub fn highlight(
    language: &str,
    text: &str,
    theme: &AppTheme,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let rope = Rope::from(text);
    let mut highlighter = SyntaxHighlighter::new(language);
    highlighter.update(None, &rope, None);
    highlighter
        .styles(&(0..text.len()), &Palette::new(theme))
        .into_iter()
        .filter(|(_, style)| style.color.is_some())
        .collect()
}

/// Highlights every hunk of `file`. Unchanged and removed lines are read from the old
/// side, unchanged and added lines from the new side, so each line gets the colors of
/// the version it belongs to.
pub fn highlight_file(file: &FileDiff, theme: &AppTheme) -> FileHighlights {
    let Some(language) = language_for_path(&file.path) else {
        return FileHighlights::default();
    };
    let hunks =
        file.hunks
            .iter()
            .map(|hunk| {
                let old = Side::collect(
                    hunk.lines
                        .iter()
                        .map(|line| (line.kind != LineKind::Added).then_some(line.text.as_str())),
                );
                let new =
                    Side::collect(hunk.lines.iter().map(|line| {
                        (line.kind != LineKind::Removed).then_some(line.text.as_str())
                    }));
                let old_spans = highlight(language, &old.text, theme);
                let new_spans = highlight(language, &new.text, theme);
                hunk.lines
                    .iter()
                    .enumerate()
                    .map(|(ix, line)| match line.kind {
                        LineKind::Removed => old.spans_for(ix, &old_spans),
                        _ => new.spans_for(ix, &new_spans),
                    })
                    .collect()
            })
            .collect();
    FileHighlights { hunks }
}

/// One side of a hunk joined into a single text, remembering where each hunk line
/// starts in it.
struct Side {
    text: String,
    starts: Vec<Option<Range<usize>>>,
}

impl Side {
    fn collect<'a>(lines: impl Iterator<Item = Option<&'a str>>) -> Self {
        let mut text = String::new();
        let mut starts = Vec::new();
        for line in lines {
            match line {
                Some(line) => {
                    let start = text.len();
                    text.push_str(line);
                    starts.push(Some(start..text.len()));
                    text.push('\n');
                }
                None => starts.push(None),
            }
        }
        Self { text, starts }
    }

    fn spans_for(&self, line_ix: usize, spans: &[(Range<usize>, HighlightStyle)]) -> LineSpans {
        let Some(Some(line)) = self.starts.get(line_ix) else {
            return Vec::new();
        };
        spans
            .iter()
            .filter(|(range, _)| range.start < line.end && range.end > line.start)
            .map(|(range, style)| {
                let start = range.start.max(line.start) - line.start;
                let end = range.end.min(line.end) - line.start;
                (start..end, *style)
            })
            .filter(|(range, _)| !range.is_empty())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use diff_model::{DiffLine, FileChange, Hunk};

    #[test]
    fn rust_keywords_get_the_keyword_color() {
        let theme = AppTheme::dark(theme_ui::DarkTheme::GitHub);
        let spans = highlight("rust", "pub fn main() { let x = 1; }", &theme);
        let keyword = spans
            .iter()
            .find(|(range, _)| *range == (0..3))
            .expect("`pub` is highlighted");
        assert_eq!(keyword.1.color, Some(theme.syntax_keyword));
        assert!(
            spans
                .iter()
                .any(|(_, s)| s.color == Some(theme.syntax_number))
        );
    }

    #[test]
    fn lines_take_spans_from_their_own_side() {
        let theme = AppTheme::dark(theme_ui::DarkTheme::GitHub);
        let file = FileDiff::new(
            "a.rs",
            FileChange::Modified,
            vec![Hunk::new(
                1,
                2,
                1,
                2,
                "",
                vec![
                    DiffLine::new(LineKind::Removed, Some(1), None, "let a = 1;"),
                    DiffLine::new(LineKind::Added, None, Some(1), "    let b = \"x\";"),
                    DiffLine::new(LineKind::Context, Some(2), Some(2), "fn f() {}"),
                ],
            )],
        );
        let highlights = highlight_file(&file, &theme);
        assert!(
            highlights
                .line(0, 0)
                .iter()
                .any(|(r, s)| *r == (0..3) && s.color == Some(theme.syntax_keyword))
        );
        assert!(highlights.line(0, 1).iter().any(|(r, _)| r.start == 4));
        assert!(
            highlights
                .line(0, 2)
                .iter()
                .any(|(r, s)| *r == (0..2) && s.color == Some(theme.syntax_keyword))
        );
        assert!(highlights.line(5, 0).is_empty());
    }

    #[test]
    fn go_and_xml_get_colors() {
        let theme = AppTheme::dark(theme_ui::DarkTheme::GitHub);
        let go = highlight("go", "func main() {\n\treturn \"x\"\n}", &theme);
        assert!(
            go.iter()
                .any(|(_, s)| s.color == Some(theme.syntax_keyword))
        );
        let xml = highlight(
            "html",
            "<WeightAllowance unit=\"KGM\">23</WeightAllowance>",
            &theme,
        );
        assert!(
            xml.iter()
                .any(|(_, s)| s.color == Some(theme.syntax_keyword))
        );
        assert!(
            xml.iter()
                .any(|(_, s)| s.color == Some(theme.syntax_string))
        );
    }

    #[test]
    fn unknown_extensions_have_no_language() {
        assert_eq!(language_for_path("crates/a/src/lib.rs"), Some("rust"));
        assert_eq!(language_for_path("Cargo.toml"), Some("toml"));
        assert_eq!(language_for_path("LICENSE"), None);
        assert_eq!(
            language_for_path("bdd/IATA_ServiceListRS.xml"),
            Some("html")
        );
        assert_eq!(language_for_path("Makefile"), Some("make"));
    }
}

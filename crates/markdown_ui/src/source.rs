//! A pass over the Markdown source before parsing, for GitHub conventions the parser
//! doesn't know. Code blocks and code spans are never touched.

use pr_model::RepoSlug;

/// The info string that marks a `<details>` block rewritten as a fence, open when the
/// HTML had `open`. The summary follows it.
pub(crate) const DETAILS_LANG: &str = "switchfold-details";
pub(crate) const DETAILS_OPEN_LANG: &str = "switchfold-details-open";

/// Rewrites what GitHub renders but CommonMark doesn't:
/// - HTML comments disappear, as in pull request templates.
/// - `<details>` blocks become fences the details renderer draws as a disclosure.
/// - `<kbd>` becomes inline code; `<sub>` and `<sup>` become Unicode where they can.
/// - `:shortcode:` becomes its emoji.
/// - With `repo`, `@login` and `#123` become links, as GitHub autolinks them.
pub(crate) fn prepare(source: &str, repo: Option<&RepoSlug>) -> String {
    let source = strip_comments(source);
    let mut out = String::with_capacity(source.len());
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut ix = 0;
    let mut fence: Option<(char, usize)> = None;
    while ix < lines.len() {
        let line = lines[ix];
        if let Some((mark, len)) = fence {
            if closes_fence(line, mark, len) {
                fence = None;
            }
            out.push_str(line);
            ix += 1;
            continue;
        }
        if let Some(opened) = opens_fence(line) {
            fence = Some(opened);
            out.push_str(line);
            ix += 1;
            continue;
        }
        if starts_details(line)
            && let Some((consumed, rewritten)) = details_block(&lines[ix..], repo)
        {
            out.push_str(&rewritten);
            ix += consumed;
            continue;
        }
        out.push_str(&inline(line, repo));
        ix += 1;
    }
    out
}

fn fence_indent_ok(line: &str) -> Option<&str> {
    let trimmed = line.trim_start_matches(' ');
    (line.len() - trimmed.len() <= 3).then_some(trimmed)
}

fn opens_fence(line: &str) -> Option<(char, usize)> {
    let trimmed = fence_indent_ok(line)?;
    let mark = trimmed.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let len = trimmed.chars().take_while(|c| *c == mark).count();
    if len < 3 {
        return None;
    }
    // A backtick fence's info string can't contain backticks.
    if mark == '`' && trimmed[len..].contains('`') {
        return None;
    }
    Some((mark, len))
}

fn closes_fence(line: &str, mark: char, len: usize) -> bool {
    let Some(trimmed) = fence_indent_ok(line) else {
        return false;
    };
    let run = trimmed.chars().take_while(|c| *c == mark).count();
    run >= len && trimmed[run..].trim().is_empty()
}

/// Removes `<!-- … -->` outside code, including comments over several lines.
fn strip_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut fence: Option<(char, usize)> = None;
    let mut in_comment = false;
    for line in source.split_inclusive('\n') {
        if let Some((mark, len)) = fence {
            if closes_fence(line, mark, len) {
                fence = None;
            }
            out.push_str(line);
            continue;
        }
        if !in_comment && let Some(opened) = opens_fence(line) {
            fence = Some(opened);
            out.push_str(line);
            continue;
        }
        let mut rest = line;
        let mut kept = String::new();
        loop {
            if in_comment {
                match rest.find("-->") {
                    Some(end) => {
                        rest = &rest[end + 3..];
                        in_comment = false;
                    }
                    None => break,
                }
            } else {
                match outside_code_find(rest, "<!--") {
                    Some(start) => {
                        kept.push_str(&rest[..start]);
                        rest = &rest[start + 4..];
                        in_comment = true;
                    }
                    None => {
                        kept.push_str(rest);
                        break;
                    }
                }
            }
        }
        // Lines that held only a comment go. Where the comment ends, one blank line
        // stays, so the paragraphs around it stay apart as they do on GitHub.
        if kept.trim().is_empty() && kept.len() != line.len() {
            if !in_comment && !out.is_empty() && !out.ends_with("\n\n") {
                out.push('\n');
            }
            continue;
        }
        out.push_str(&kept);
    }
    out
}

/// The byte offset of `needle` in `line` outside code spans.
fn outside_code_find(line: &str, needle: &str) -> Option<usize> {
    let mut offset = 0;
    for (is_code, segment) in segments(line) {
        if !is_code && let Some(found) = segment.find(needle) {
            return Some(offset + found);
        }
        offset += segment.len();
    }
    None
}

/// Splits `line` into code spans (`true`) and the prose around them (`false`). An
/// unmatched backtick run is prose.
fn segments(line: &str) -> Vec<(bool, &str)> {
    let bytes = line.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0;
    let mut ix = 0;
    while ix < bytes.len() {
        if bytes[ix] != b'`' {
            ix += 1;
            continue;
        }
        let run = bytes[ix..].iter().take_while(|b| **b == b'`').count();
        let after = ix + run;
        let close = (after..bytes.len()).find(|&j| {
            bytes[j] == b'`'
                && (j == 0 || bytes[j - 1] != b'`')
                && bytes[j..].iter().take_while(|b| **b == b'`').count() == run
        });
        match close {
            Some(close) => {
                if start < ix {
                    parts.push((false, &line[start..ix]));
                }
                parts.push((true, &line[ix..close + run]));
                ix = close + run;
                start = ix;
            }
            None => ix = after,
        }
    }
    if start < line.len() {
        parts.push((false, &line[start..]));
    }
    parts
}

fn starts_details(line: &str) -> bool {
    line.trim_start()
        .get(..8)
        .is_some_and(|tag| tag.eq_ignore_ascii_case("<details"))
}

/// Rewrites the `<details>` block at the start of `lines` as a fence, returning how
/// many lines it used. `None` when it never closes, which leaves it as written.
fn details_block(lines: &[&str], repo: Option<&RepoSlug>) -> Option<(usize, String)> {
    let mut depth = 0usize;
    let mut end = None;
    for (ix, line) in lines.iter().enumerate() {
        let lower = line.to_ascii_lowercase();
        depth += lower.matches("<details").count();
        depth = depth.saturating_sub(lower.matches("</details>").count());
        if depth == 0 {
            end = Some(ix);
            break;
        }
    }
    let end = end?;
    let block: String = lines[..=end].concat();
    let lower = block.to_ascii_lowercase();
    let open_end = lower.find('>')?;
    let open = lower[..open_end].contains(" open");
    let mut inner_start = open_end + 1;
    let mut summary = String::from("Details");
    if let Some(start) = lower[inner_start..].find("<summary") {
        let start = inner_start + start;
        let text_start = start + lower[start..].find('>')? + 1;
        let text_end = text_start + lower[text_start..].find("</summary>")?;
        let text = strip_tags(&block[text_start..text_end]);
        if !text.trim().is_empty() {
            summary = text.trim().replace('`', "'");
        }
        inner_start = text_end + "</summary>".len();
    }
    let inner_end = lower.rfind("</details>")?;
    let inner = prepare(block[inner_start..inner_end].trim_matches('\n'), repo);
    let longest = inner
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            let run = trimmed.chars().take_while(|c| *c == '`').count();
            (run >= 3).then_some(run)
        })
        .max()
        .unwrap_or(0);
    let fence = "`".repeat((longest + 1).max(4));
    let lang = if open {
        DETAILS_OPEN_LANG
    } else {
        DETAILS_LANG
    };
    let mut rewritten = format!("{fence}{lang} {summary}\n{}\n{fence}\n", inner.trim_end());
    let tail = &block[inner_end + "</details>".len()..];
    if !tail.trim().is_empty() {
        rewritten.push_str(&inline(tail, repo));
    }
    Some((end + 1, rewritten))
}

fn strip_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// Applies the inline rewrites to the prose of one line.
fn inline(line: &str, repo: Option<&RepoSlug>) -> String {
    let mut out = String::with_capacity(line.len());
    let in_table = line.trim_start().starts_with('|');
    for (is_code, segment) in segments(line) {
        if is_code {
            out.push_str(segment);
        } else {
            let linked = autolink(segment, repo);
            out.push_str(&html_inline(&linked, in_table));
        }
    }
    out
}

/// `<kbd>`, `<sub>`, `<sup>` and `<br>`.
fn html_inline(text: &str, in_table: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let lower = tail.to_ascii_lowercase();
        let paired = |tag: &str| -> Option<(String, usize)> {
            let open = format!("<{tag}>");
            let close = format!("</{tag}>");
            if !lower.starts_with(&open) {
                return None;
            }
            let end = lower.find(&close)?;
            Some((tail[open.len()..end].to_string(), end + close.len()))
        };
        if let Some((inner, used)) = paired("kbd") {
            out.push_str(&format!("`{}`", inner.replace('`', "")));
            rest = &tail[used..];
        } else if let Some((inner, used)) = paired("sub") {
            out.push_str(&shift(&inner, SUBSCRIPTS));
            rest = &tail[used..];
        } else if let Some((inner, used)) = paired("sup") {
            out.push_str(&shift(&inner, SUPERSCRIPTS));
            rest = &tail[used..];
        } else if let Some(used) = ["<br>", "<br/>", "<br />"]
            .iter()
            .find(|tag| lower.starts_with(**tag))
            .map(|tag| tag.len())
        {
            out.push_str(if in_table { " " } else { "  \n" });
            rest = &tail[used..];
        } else {
            out.push('<');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

const SUBSCRIPTS: &[(char, char)] = &[
    ('0', '₀'),
    ('1', '₁'),
    ('2', '₂'),
    ('3', '₃'),
    ('4', '₄'),
    ('5', '₅'),
    ('6', '₆'),
    ('7', '₇'),
    ('8', '₈'),
    ('9', '₉'),
    ('+', '₊'),
    ('-', '₋'),
    ('=', '₌'),
    ('(', '₍'),
    (')', '₎'),
    ('a', 'ₐ'),
    ('e', 'ₑ'),
    ('o', 'ₒ'),
    ('x', 'ₓ'),
    ('h', 'ₕ'),
    ('k', 'ₖ'),
    ('l', 'ₗ'),
    ('m', 'ₘ'),
    ('n', 'ₙ'),
    ('p', 'ₚ'),
    ('s', 'ₛ'),
    ('t', 'ₜ'),
    ('i', 'ᵢ'),
    ('r', 'ᵣ'),
    ('u', 'ᵤ'),
    ('v', 'ᵥ'),
];

const SUPERSCRIPTS: &[(char, char)] = &[
    ('0', '⁰'),
    ('1', '¹'),
    ('2', '²'),
    ('3', '³'),
    ('4', '⁴'),
    ('5', '⁵'),
    ('6', '⁶'),
    ('7', '⁷'),
    ('8', '⁸'),
    ('9', '⁹'),
    ('+', '⁺'),
    ('-', '⁻'),
    ('=', '⁼'),
    ('(', '⁽'),
    (')', '⁾'),
    ('n', 'ⁿ'),
    ('i', 'ⁱ'),
];

/// `text` in sub- or superscript characters when every character has one, else as is.
fn shift(text: &str, table: &[(char, char)]) -> String {
    let shifted: Option<String> = text
        .chars()
        .map(|c| table.iter().find(|(from, _)| *from == c).map(|(_, to)| *to))
        .collect();
    shifted.unwrap_or_else(|| text.to_string())
}

/// Emoji shortcodes, and with `repo`, `@login` and `#123` links. Skips tags, URLs and
/// link syntax, where a rewrite would break the markup.
fn autolink(text: &str, repo: Option<&RepoSlug>) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut ix = 0;
    let mut bracket_depth = 0usize;
    while ix < chars.len() {
        let c = chars[ix];
        let prev = ix.checked_sub(1).map(|p| chars[p]);
        let word_before = prev.is_some_and(|p| {
            p.is_alphanumeric() || matches!(p, '_' | '/' | '.' | '@' | '#' | '-' | '&')
        });
        match c {
            '<' => {
                let end = chars[ix..].iter().position(|c| *c == '>').map(|p| ix + p);
                let end = end.unwrap_or(ix);
                out.extend(&chars[ix..=end]);
                ix = end + 1;
                continue;
            }
            '[' => bracket_depth += 1,
            ']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                if chars.get(ix + 1) == Some(&'(') {
                    let end = chars[ix..].iter().position(|c| *c == ')').map(|p| ix + p);
                    if let Some(end) = end {
                        out.extend(&chars[ix..=end]);
                        ix = end + 1;
                        continue;
                    }
                }
            }
            'h' if !word_before && starts_with(&chars[ix..], "http") => {
                let end = chars[ix..]
                    .iter()
                    .position(|c| c.is_whitespace())
                    .map_or(chars.len(), |p| ix + p);
                out.extend(&chars[ix..end]);
                ix = end;
                continue;
            }
            ':' => {
                let end = chars[ix + 1..]
                    .iter()
                    .position(|c| *c == ':')
                    .map(|p| ix + 1 + p);
                if let Some(end) = end {
                    let name: String = chars[ix + 1..end].iter().collect();
                    let plausible = !name.is_empty()
                        && name
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-'));
                    if plausible && let Some(emoji) = emojis::get_by_shortcode(&name) {
                        out.push_str(emoji.as_str());
                        ix = end + 1;
                        continue;
                    }
                }
            }
            '@' if repo.is_some() && !word_before && bracket_depth == 0 => {
                let login: String = chars[ix + 1..]
                    .iter()
                    .take_while(|c| c.is_ascii_alphanumeric() || **c == '-')
                    .collect();
                let login = login.trim_end_matches('-');
                if !login.is_empty() && login.len() <= 39 {
                    out.push_str(&format!("[@{login}](https://github.com/{login})"));
                    ix += 1 + login.chars().count();
                    continue;
                }
            }
            '#' if !word_before && bracket_depth == 0 => {
                if let Some(repo) = repo {
                    let digits: String = chars[ix + 1..]
                        .iter()
                        .take_while(|c| c.is_ascii_digit())
                        .collect();
                    let after = chars.get(ix + 1 + digits.len());
                    let ends_word = after.is_none_or(|c| !c.is_alphanumeric() && *c != '_');
                    if !digits.is_empty() && ends_word {
                        out.push_str(&format!(
                            "[#{digits}](https://github.com/{repo}/issues/{digits})"
                        ));
                        ix += 1 + digits.len();
                        continue;
                    }
                }
            }
            _ => {}
        }
        out.push(c);
        ix += 1;
    }
    out
}

fn starts_with(chars: &[char], prefix: &str) -> bool {
    prefix
        .chars()
        .enumerate()
        .all(|(ix, c)| chars.get(ix) == Some(&c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> RepoSlug {
        RepoSlug::new("flaticols", "switchfold")
    }

    #[test]
    fn links_mentions_and_references_outside_code() {
        let out = prepare(
            "Thanks @mark-v, see #482 and `#1 @x` :rocket:\n",
            Some(&repo()),
        );
        assert_eq!(
            out,
            "Thanks [@mark-v](https://github.com/mark-v), see [#482](https://github.com/flaticols/switchfold/issues/482) and `#1 @x` 🚀\n"
        );
    }

    #[test]
    fn leaves_links_urls_emails_and_headings_alone() {
        let text = "[see #12](https://x.dev/#3) me@mail.com https://a.dev/@b#4 <img alt=\"@x\">\n## Title\n";
        assert_eq!(prepare(text, Some(&repo())), text);
        assert_eq!(
            prepare("C# and a#1 :not-an-emoji:\n", Some(&repo())),
            "C# and a#1 :not-an-emoji:\n"
        );
    }

    #[test]
    fn code_blocks_are_untouched() {
        let text = "```\n<!-- keep --> :rocket: @x #1\n```\n~~~~\n<details>\n~~~~\n";
        assert_eq!(prepare(text, Some(&repo())), text);
    }

    #[test]
    fn drops_comments_like_github() {
        let text = "Before\n<!-- Describe\nyour change -->\nAfter <!-- inline --> text\n";
        assert_eq!(prepare(text, None), "Before\n\nAfter  text\n");
        assert_eq!(prepare("<!-- template -->\nText\n", None), "Text\n");
    }

    #[test]
    fn details_become_a_fence_longer_than_the_code_inside() {
        let text = "<details open>\n<summary>Logs `x`</summary>\n\n```rust\nfn main() {}\n```\n\n</details>\nafter\n";
        let out = prepare(text, None);
        assert_eq!(
            out,
            "````switchfold-details-open Logs 'x'\n```rust\nfn main() {}\n```\n````\nafter\n"
        );
        let closed = prepare("<details><summary>S</summary>body</details>\n", None);
        assert_eq!(closed, "````switchfold-details S\nbody\n````\n");
        assert_eq!(
            prepare("<details>\nnever closed\n", None),
            "<details>\nnever closed\n"
        );
    }

    #[test]
    fn inline_html_github_renders() {
        assert_eq!(
            prepare(
                "Press <kbd>Cmd</kbd>+<KBD>K</KBD>. H<sub>2</sub>O, x<sup>2</sup>, x<sup>ab</sup>\n",
                None
            ),
            "Press `Cmd`+`K`. H₂O, x², xab\n"
        );
        assert_eq!(prepare("a<br>b\n", None), "a  \nb\n");
        assert_eq!(prepare("| a<br/>b |\n", None), "| a b |\n");
    }
}

//! Quoting and referencing what the reviewer reads in what they write: diff lines and
//! comments go into a comment draft as Markdown quotes, code blocks or links.

use diff_view_ui::{DiffSide, LineMenuTarget};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Icon,
    input::TextareaState,
    menu::{PopupMenu, PopupMenuItem},
};
use gpui_kit::{App, ClipboardItem, Context, Entity, Window, px};
use pr_actions_ui::FileTarget;
use review_model::TimelineItem;

use crate::ReviewThreadView;

/// How a piece of text joins a draft at its cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Placement {
    /// A paragraph of its own, with blank lines around it.
    Block,
    /// Inside the current line, spaced from the word before it.
    Inline,
}

impl ReviewThreadView {
    /// Actions for a diff line in `path`, after its Copy and Copy line: commenting on
    /// it, putting it into a comment, asking an agent about it, and its link.
    pub(crate) fn diff_line_menu(
        view: &Entity<Self>,
        path: &str,
        menu: PopupMenu,
        target: &LineMenuTarget,
        cx: &App,
    ) -> PopupMenu {
        let this = view.read(cx);
        let Some(number) = target.number() else {
            return menu;
        };
        let base = target.side() == DiffSide::Base;
        let link = this.file_target(path).line_url(number, base);
        let quoted = code_block(
            path,
            if target.selected_text().is_empty() {
                &target.line().text
            } else {
                target.selected_text()
            },
        );
        let suggestion = format!("```suggestion\n{}\n```", target.line().text);
        let path = path.to_string();
        let menu = if base {
            menu
        } else {
            let (comment_view, comment_path) = (view.clone(), path.clone());
            let (suggest_view, suggest_path) = (view.clone(), path.clone());
            menu.item(
                PopupMenuItem::new("Comment on line")
                    .icon(Icon::new(IconName::MessageSquarePlus))
                    .on_click(move |_, window, cx| {
                        comment_view.update(cx, |this, cx| {
                            let input = this.line_draft(&comment_path, number, window, cx);
                            input.update(cx, |input, cx| input.focus(window, cx));
                            cx.notify();
                        })
                    }),
            )
            .item(
                PopupMenuItem::new("Suggest change")
                    .icon(Icon::new(IconName::SquarePen))
                    .on_click(move |_, window, cx| {
                        suggest_view.update(cx, |this, cx| {
                            let input = this.line_draft(&suggest_path, number, window, cx);
                            this.insert_into(&input, &suggestion, Placement::Block, window, cx);
                        })
                    }),
            )
        };
        let (quote_view, quote_path) = (view.clone(), path.clone());
        let menu = menu.item(
            PopupMenuItem::new("Quote in comment")
                .icon(Icon::new(IconName::TextQuote))
                .on_click(move |_, window, cx| {
                    quote_view.update(cx, |this, cx| {
                        let input = match this.draft_in_progress(cx) {
                            Some(input) => input,
                            None if base => this.composer.clone(),
                            None => this.line_draft(&quote_path, number, window, cx),
                        };
                        this.insert_into(&input, &quoted, Placement::Block, window, cx);
                    })
                }),
        );
        let (ask_view, ask_path, position) = (view.clone(), path.clone(), target.position());
        let menu = menu.item(
            PopupMenuItem::new("Ask agent…")
                .icon(Icon::new(IconName::Bot))
                .on_click(move |_, window, cx| {
                    ask_view.update(cx, |this, cx| {
                        this.ask_about_line(&ask_path, position, window, cx)
                    })
                }),
        );
        let Some(link) = link else {
            return menu;
        };
        let reference_view = view.clone();
        let reference = link.clone();
        menu.item(
            PopupMenuItem::new("Reference in comment")
                .icon(Icon::new(IconName::Link))
                .on_click(move |_, window, cx| {
                    reference_view.update(cx, |this, cx| {
                        let input = this
                            .draft_in_progress(cx)
                            .unwrap_or_else(|| this.composer.clone());
                        this.insert_into(&input, &reference, Placement::Inline, window, cx);
                    })
                }),
        )
        .item(
            PopupMenuItem::new("Copy link to line")
                .icon(Icon::new(IconName::Link2))
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(link.clone()))
                }),
        )
    }

    /// Quotes `text` from the description or a top-level comment in the comment
    /// composer, as GitHub's Quote reply does.
    pub(crate) fn quote_in_composer(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.composer.clone();
        self.insert_into(&input, &block_quote(text), Placement::Block, window, cx);
    }

    /// A Quote reply handler for a card whose quotes go to the comment composer.
    pub(crate) fn quote_in_composer_handler(
        &self,
        cx: &mut Context<Self>,
    ) -> impl Fn(&str, &mut Window, &mut App) + 'static {
        let view = cx.entity();
        move |text, window, cx| view.update(cx, |this, cx| this.quote_in_composer(text, window, cx))
    }

    /// Quotes `text` from a comment of thread `thread_id` in that thread's reply field.
    pub(crate) fn quote_in_reply(
        &mut self,
        thread_id: &str,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(input) = self.reply_inputs.get(thread_id).cloned() else {
            return;
        };
        self.insert_into(&input, &block_quote(text), Placement::Block, window, cx);
    }

    /// The file at the head commit, for links.
    fn file_target(&self, path: &str) -> FileTarget {
        let head = self
            .detail
            .as_ref()
            .and_then(|detail| detail.commits.last().map(|commit| commit.sha.clone()));
        FileTarget::new(self.key.repo().clone(), self.key.number(), head, path)
    }

    /// The draft the reviewer last typed in, while it still holds text. An empty or
    /// closed draft is not in progress, so a quote goes where it belongs instead of
    /// into a field the reviewer left behind.
    fn draft_in_progress(&self, cx: &App) -> Option<Entity<TextareaState>> {
        let input = self.last_draft.as_ref()?.upgrade()?;
        let open = input == self.composer
            || self.reply_inputs.values().any(|reply| reply == &input)
            || self
                .new_thread
                .as_ref()
                .is_some_and(|(_, _, draft)| draft == &input)
            || self
                .editing_finding
                .as_ref()
                .is_some_and(|(_, draft)| draft == &input);
        (open && !input.read(cx).value().trim().is_empty()).then_some(input)
    }

    /// The line comment being written on `line` of `path`, or a new one there.
    pub(crate) fn line_draft(
        &mut self,
        path: &str,
        line: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TextareaState> {
        if let Some((draft_path, draft_line, input)) = &self.new_thread
            && draft_path == path
            && *draft_line == line
        {
            return input.clone();
        }
        self.start_line_comment(path.to_string(), line, window, cx);
        self.sync_rows(cx);
        self.new_thread
            .as_ref()
            .map(|(_, _, input)| input.clone())
            .unwrap_or_else(|| self.composer.clone())
    }

    /// Puts `text` into `input` at its cursor, then brings the draft on screen and
    /// focuses it so the reviewer keeps writing around it.
    pub(crate) fn insert_into(
        &mut self,
        input: &Entity<TextareaState>,
        text: &str,
        placement: Placement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = {
            let state = input.read(cx);
            let value = state.value();
            let (before, after) = value.split_at(state.cursor().min(value.len()));
            match placement {
                Placement::Block => block_insertion(before, after, text),
                Placement::Inline => inline_insertion(before, text),
            }
        };
        input.update(cx, |input, cx| {
            input.insert(text, window, cx);
            input.focus(window, cx);
        });
        self.last_draft = Some(input.downgrade());
        self.reveal_draft(input, cx);
        cx.notify();
    }

    /// Scrolls the timeline to `input`: the composer at the end, or the line a line
    /// comment or thread reply is under.
    fn reveal_draft(&mut self, input: &Entity<TextareaState>, cx: &mut Context<Self>) {
        if input == &self.composer {
            self.set_filter(crate::TimelineFilter::Everything);
            self.sync_rows(cx);
            self.list_state.scroll_to_end();
            return;
        }
        let anchor = match &self.new_thread {
            Some((path, line, draft)) if draft == input => Some((path.clone(), *line)),
            _ => self
                .reply_inputs
                .iter()
                .find(|(_, reply)| *reply == input)
                .and_then(|(thread_id, _)| {
                    let detail = self.detail.as_ref()?;
                    let thread = detail.threads.iter().find(|t| &t.id == thread_id)?;
                    Some((thread.path.clone(), thread.line))
                }),
        };
        let Some((path, line)) = anchor else {
            return;
        };
        self.sync_rows(cx);
        if let Some(row) = self.threads_row(&path, line) {
            // A few rows of the code above the draft keep the quoted line in view.
            self.list_state.scroll_to(gpui_kit::ListOffset {
                item_ix: row.saturating_sub(4),
                offset_in_item: px(0.),
            });
        }
    }

    /// The timeline row holding the threads under head-side `line` of `path`.
    pub(crate) fn threads_row(&self, path: &str, line: u32) -> Option<usize> {
        self.rows.iter().position(|row| {
            matches!(row, crate::rows::TimelineRow::File { item, row: diff_view_ui::DiffRow::Threads(l), .. }
                if *l == line
                    && matches!(&self.items[*item], TimelineItem::File { path: p } if p == path))
        })
    }
}

/// `text` as a fenced code block, in the language GitHub infers from the extension of
/// `path`. The fence outgrows any backtick run inside the text, so code that holds a
/// fence of its own stays in the block.
fn code_block(path: &str, text: &str) -> String {
    let language = std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    let longest_run = text.split(|ch| ch != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest_run.max(2) + 1);
    format!(
        "{fence}{language}\n{}\n{fence}",
        text.trim_end_matches('\n')
    )
}

/// `text` as a Markdown block quote.
fn block_quote(text: &str) -> String {
    text.trim_end_matches('\n')
        .lines()
        .map(|line| {
            if line.is_empty() {
                ">".to_string()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What to insert at a cursor between `before` and `after` so `block` stands as its own
/// paragraph, with the cursor ending on a fresh line below it.
fn block_insertion(before: &str, after: &str, block: &str) -> String {
    let lead = if before.is_empty() || before.ends_with("\n\n") {
        ""
    } else if before.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let trail = if after.starts_with("\n\n") {
        ""
    } else if after.starts_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    format!("{lead}{block}{trail}")
}

/// What to insert at a cursor after `before` so `text` does not run into the word
/// before it.
fn inline_insertion(before: &str, text: &str) -> String {
    if before.is_empty() || before.ends_with(char::is_whitespace) {
        text.to_string()
    } else {
        format!(" {text}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_blocks_take_the_language_from_the_extension() {
        assert_eq!(
            code_block("src/a.rs", "let x = 1;"),
            "```rs\nlet x = 1;\n```"
        );
        assert_eq!(code_block("Makefile", "all:"), "```\nall:\n```");
    }

    #[test]
    fn a_code_block_fence_outgrows_fences_inside_it() {
        assert_eq!(
            code_block("README.md", "```go\nx\n```"),
            "````md\n```go\nx\n```\n````"
        );
    }

    #[test]
    fn block_quotes_mark_every_line_and_keep_blank_ones() {
        assert_eq!(block_quote("one\n\ntwo\n"), "> one\n>\n> two");
    }

    #[test]
    fn blocks_stand_apart_from_the_text_around_them() {
        assert_eq!(block_insertion("", "", "> q"), "> q\n\n");
        assert_eq!(block_insertion("Hi", "", "> q"), "\n\n> q\n\n");
        assert_eq!(block_insertion("Hi\n", "", "> q"), "\n> q\n\n");
        assert_eq!(block_insertion("Hi\n\n", "\nBye", "> q"), "> q\n");
        assert_eq!(block_insertion("Hi\n\n", "\n\nBye", "> q"), "> q");
    }

    #[test]
    fn inline_text_is_spaced_from_the_word_before() {
        assert_eq!(inline_insertion("", "url"), "url");
        assert_eq!(inline_insertion("see", "url"), " url");
        assert_eq!(inline_insertion("see ", "url"), "url");
    }
}

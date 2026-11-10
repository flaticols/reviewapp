//! One file's diff inside the review timeline, as rows a virtualized list can render
//! one at a time: a header with the path, counts, Viewed and Open in Zed, then hunk
//! headers and lines in unified or split layout with syntax colors, a two-number gutter,
//! and slots for the threads anchored to lines. Consecutive rows of a file draw as one
//! bordered card. Dragging across line numbers selects a range of lines.

mod layout;
mod selectable_code;
mod selection;

use std::ops::Range;
use std::rc::Rc;

use diff_model::{DiffLine, FileDiff, LineKind};
use gpui_kit::assets::IconName;
use gpui_kit::base::TextSelection;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, ClipboardItem, ElementId, FontWeight, HighlightStyle, Hsla,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, RenderOnce,
    SharedString, StatefulInteractiveElement as _, Styled as _, TestSupportExt as _, Window, div,
    prelude::FluentBuilder as _, rems,
};
use selectable_code::SelectableCode;
use syntax_ui::FileHighlights;
use theme_ui::{ActiveAppTheme as _, TypographyExt as _, code_line_height};

pub use layout::{SplitRow, split_rows};
pub use selection::{LineGesture, LinePosition, LineRange};

/// Unified shows one column; split shows the base on the left and the head on the right.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiffMode {
    #[default]
    Unified,
    Split,
}

/// Files with more changed lines than this start collapsed.
pub const COLLAPSE_THRESHOLD: usize = 100;

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type ViewedHandler = Rc<dyn Fn(&bool, &mut Window, &mut App)>;
type LineHandler = Rc<dyn Fn(u32, &mut Window, &mut App)>;
type SelectHandler = Rc<dyn Fn(LinePosition, LineGesture, &mut Window, &mut App)>;
type MenuBuilder = Rc<dyn Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu>;
type LineMenuBuilder = Rc<dyn Fn(PopupMenu, &LineMenuTarget, &mut Window, &mut App) -> PopupMenu>;
type Spans = [(std::ops::Range<usize>, gpui_kit::HighlightStyle)];

/// Where diff lines start in the window's selection order. Text views number themselves
/// from 1 each frame, and the selection takes in everything whose order lies between
/// its two ends, so diff lines sharing those numbers would pull unrelated comments into
/// a selection between two lines.
const DOCUMENT_ORDER_BASE: u64 = 1 << 40;

/// Which version of the file a diff line belongs to: the base a removal is from, or the
/// head that additions and context lines are in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffSide {
    /// The pull request's base: removed lines, and the left column of the split layout.
    Base,
    /// The pull request's head: added and context lines in the unified layout, and the
    /// right column of the split layout.
    Head,
}

/// The line a diff line's context menu was opened on, with the text selected in the
/// window when it opened.
#[derive(Clone, Debug)]
pub struct LineMenuTarget {
    line: DiffLine,
    position: LinePosition,
    side: DiffSide,
    selected_text: String,
}

impl LineMenuTarget {
    /// The line, with its raw text: tabs as written, no sign.
    pub fn line(&self) -> &DiffLine {
        &self.line
    }

    /// Where the line sits in the file's diff.
    pub fn position(&self) -> LinePosition {
        self.position
    }

    /// The version of the file the line was clicked in.
    pub fn side(&self) -> DiffSide {
        self.side
    }

    /// The line's number on [`Self::side`].
    pub fn number(&self) -> Option<u32> {
        match self.side {
            DiffSide::Base => self.line.old_line,
            DiffSide::Head => self.line.new_line,
        }
    }

    /// The text selected anywhere in the window, empty when nothing is. It can span
    /// several lines and reach past this one.
    pub fn selected_text(&self) -> &str {
        &self.selected_text
    }
}

/// One row of a file's diff, by position in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffRow {
    /// Path, counts and file actions.
    Header,
    /// The note shown instead of a binary file's contents.
    Binary,
    /// A hunk's `@@` line.
    Hunk(usize),
    /// A line of the unified layout.
    Line { hunk: usize, line: usize },
    /// A row of the split layout: base-side and head-side line indices.
    Split {
        hunk: usize,
        left: Option<usize>,
        right: Option<usize>,
    },
    /// The threads anchored to head-side line `line`.
    Threads(u32),
}

/// The rows `file` shows in `mode`, with a [`DiffRow::Threads`] slot after each line
/// for which `has_threads` is true. A collapsed file shows only its header.
pub fn diff_rows(
    file: &FileDiff,
    mode: DiffMode,
    collapsed: bool,
    has_threads: impl Fn(u32) -> bool,
) -> Vec<DiffRow> {
    let mut rows = vec![DiffRow::Header];
    if collapsed {
        return rows;
    }
    if file.is_binary {
        rows.push(DiffRow::Binary);
        return rows;
    }
    for (hunk_ix, hunk) in file.hunks.iter().enumerate() {
        rows.push(DiffRow::Hunk(hunk_ix));
        match mode {
            DiffMode::Unified => {
                for (line_ix, line) in hunk.lines.iter().enumerate() {
                    rows.push(DiffRow::Line {
                        hunk: hunk_ix,
                        line: line_ix,
                    });
                    if line.kind != LineKind::Removed
                        && let Some(number) = line.new_line
                        && has_threads(number)
                    {
                        rows.push(DiffRow::Threads(number));
                    }
                }
            }
            DiffMode::Split => {
                for split in split_rows(&hunk.lines) {
                    rows.push(DiffRow::Split {
                        hunk: hunk_ix,
                        left: split.left,
                        right: split.right,
                    });
                    if let Some(number) = split.right.and_then(|ix| hunk.lines[ix].new_line)
                        && has_threads(number)
                    {
                        rows.push(DiffRow::Threads(number));
                    }
                }
            }
        }
    }
    rows
}

/// One row of a file's diff. Rows of the same file stack into a card: the header rounds
/// the top corners and the row marked [`FileDiffRow::last`] rounds the bottom ones.
#[derive(IntoElement)]
pub struct FileDiffRow {
    file: Rc<FileDiff>,
    highlights: Rc<FileHighlights>,
    row: DiffRow,
    last: bool,
    collapsed: bool,
    viewed: bool,
    threads: Vec<AnyElement>,
    on_toggle_collapsed: Option<ClickHandler>,
    on_viewed: Option<ViewedHandler>,
    on_open_in_zed: Option<ClickHandler>,
    on_comment_line: Option<LineHandler>,
    menu: Option<MenuBuilder>,
    line_menu: Option<LineMenuBuilder>,
    on_select_line: Option<SelectHandler>,
    selected: Option<LineRange>,
    document_order: u64,
    find: Option<Find>,
}

/// Text to mark in a row's code lines, and the match find is on, if it is in this row.
#[derive(Clone)]
struct Find {
    query: SharedString,
    /// The hunk line index and byte range of the current match.
    current: Option<(usize, Range<usize>)>,
}

/// Where `query` occurs in `text`, ignoring case, as byte ranges that don't overlap.
pub fn find_ranges(text: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let query: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut ranges = Vec::new();
    let mut ix = 0;
    while ix + query.len() <= chars.len() {
        let hit = chars[ix..ix + query.len()]
            .iter()
            .zip(&query)
            .all(|((_, c), q)| c.to_lowercase().eq(std::iter::once(*q)));
        if hit {
            let end = chars
                .get(ix + query.len())
                .map_or(text.len(), |(byte, _)| *byte);
            ranges.push(chars[ix].0..end);
            ix += query.len();
        } else {
            ix += 1;
        }
    }
    ranges
}

impl FileDiffRow {
    /// Row `row` of `file`.
    pub fn new(file: Rc<FileDiff>, highlights: Rc<FileHighlights>, row: DiffRow) -> Self {
        Self {
            file,
            highlights,
            row,
            last: false,
            collapsed: false,
            viewed: false,
            threads: Vec::new(),
            on_toggle_collapsed: None,
            on_viewed: None,
            on_open_in_zed: None,
            on_comment_line: None,
            menu: None,
            line_menu: None,
            on_select_line: None,
            selected: None,
            document_order: 0,
            find: None,
        }
    }

    /// Marks `query` in the row's code; `current` is the hunk line and range of the
    /// match find is on, when it is in this row.
    pub fn find(
        mut self,
        query: impl Into<SharedString>,
        current: Option<(usize, Range<usize>)>,
    ) -> Self {
        self.find = Some(Find {
            query: query.into(),
            current,
        });
        self
    }

    /// Builds the file's menu, shown from the header's ⋯ button and on right-click.
    /// Without one the menu offers Copy path.
    pub fn menu(
        mut self,
        build: impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + 'static,
    ) -> Self {
        self.menu = Some(Rc::new(build));
        self
    }

    /// Adds actions to a code line's context menu, after Copy and Copy line.
    pub fn line_menu(
        mut self,
        build: impl Fn(PopupMenu, &LineMenuTarget, &mut Window, &mut App) -> PopupMenu + 'static,
    ) -> Self {
        self.line_menu = Some(Rc::new(build));
        self
    }

    /// Where the row's code falls in the window's reading order, so a selection across
    /// rows copies them top to bottom. Give each row a larger value than the row above.
    pub fn document_order(mut self, order: u64) -> Self {
        self.document_order = order;
        self
    }

    /// Closes the card below this row.
    pub fn last(mut self, last: bool) -> Self {
        self.last = last;
        self
    }

    /// Shows the header's chevron as collapsed.
    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    /// Checks the header's Viewed box.
    pub fn viewed(mut self, viewed: bool) -> Self {
        self.viewed = viewed;
        self
    }

    /// The elements a [`DiffRow::Threads`] row shows.
    pub fn threads(mut self, threads: impl IntoIterator<Item = AnyElement>) -> Self {
        self.threads.extend(threads);
        self
    }

    /// Handles the header's disclosure chevron.
    pub fn on_toggle_collapsed(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_toggle_collapsed = Some(Rc::new(handler));
        self
    }

    /// Handles the Viewed checkbox with the requested value.
    pub fn on_viewed(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_viewed = Some(Rc::new(handler));
        self
    }

    /// Handles Open in Zed.
    pub fn on_open_in_zed(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_open_in_zed = Some(Rc::new(handler));
        self
    }

    /// Handles a click on a head-side line number, to start a comment there.
    pub fn on_comment_line(
        mut self,
        handler: impl Fn(u32, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_comment_line = Some(Rc::new(handler));
        self
    }

    /// Handles the gestures that select lines: a press on a line's numbers, and the
    /// pointer crossing lines with the button held. The owner keeps the range and ends
    /// the drag when the button comes up, wherever that happens.
    pub fn on_select_line(
        mut self,
        handler: impl Fn(LinePosition, LineGesture, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_select_line = Some(Rc::new(handler));
        self
    }

    /// Paints the lines of `range` as selected.
    pub fn selected_lines(mut self, range: Option<LineRange>) -> Self {
        self.selected = range;
        self
    }

    fn render_header(&self, cx: &App) -> impl IntoElement {
        let file = &self.file;
        let toggle = self.on_toggle_collapsed.clone();
        let viewed = self.on_viewed.clone();
        let open = self.on_open_in_zed.clone();
        let path = file.path.clone();
        let menu: MenuBuilder = self.menu.clone().unwrap_or_else(|| {
            Rc::new(move |menu: PopupMenu, _: &mut Window, _: &mut App| {
                let path = path.clone();
                menu.item(
                    PopupMenuItem::new("Copy path")
                        .icon(Icon::new(IconName::Copy))
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(path.clone()))
                        }),
                )
            })
        });
        let context = menu.clone();
        h_flex()
            .id("file-header")
            .h_10()
            .pl_2()
            .pr_2()
            .gap_2()
            .bg(cx.theme().muted.opacity(0.5))
            .when(!self.last, |this| {
                this.border_b_1().border_color(cx.theme().border)
            })
            .child(
                Button::new("toggle")
                    .ghost()
                    .xsmall()
                    .icon(if self.collapsed {
                        IconName::ChevronRight
                    } else {
                        IconName::ChevronDown
                    })
                    .accessibility_label(if self.collapsed {
                        "Expand file"
                    } else {
                        "Collapse file"
                    })
                    .when_some(toggle, |this, toggle| {
                        this.on_click(move |event, window, cx| toggle(event, window, cx))
                    }),
            )
            .child(
                Icon::new(IconName::FileCode)
                    .small()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_code(cx)
                    .child(SharedString::from(file.path.clone())),
            )
            .child(
                div()
                    .text_code(cx)
                    .text_color(cx.theme().success)
                    .child(format!("+{}", file.additions())),
            )
            .child(
                div()
                    .text_code(cx)
                    .text_color(cx.theme().danger)
                    .child(format!("−{}", file.deletions())),
            )
            .child(div().flex_1())
            .child(
                Checkbox::new("viewed")
                    .small()
                    .label("Viewed")
                    .checked(self.viewed)
                    .when_some(viewed, |this, viewed| {
                        this.on_change(move |checked, window, cx| viewed(checked, window, cx))
                    }),
            )
            // Only where the owner can open files; a button that does nothing misleads.
            .when_some(open, |this, open| {
                this.child(
                    Button::new("open-in-zed")
                        .ghost()
                        .small()
                        .icon(IconName::ExternalLink)
                        .label("Open in Zed")
                        .on_click(move |event, window, cx| open(event, window, cx)),
                )
            })
            .child(
                Button::new("file-more")
                    .ghost()
                    .small()
                    .icon(IconName::Ellipsis)
                    .accessibility_label("More file actions")
                    .dropdown_menu_with_anchor(gpui_kit::Anchor::TopRight, {
                        let build = menu.clone();
                        move |menu: PopupMenu, window, cx| build(menu, window, cx)
                    }),
            )
            .context_menu(move |menu, window, cx| menu_for_context(&context, menu, window, cx))
    }

    fn line_actions(&self, side: DiffSide, hunk: usize, line: Option<usize>) -> LineActions {
        let position = LinePosition::new(hunk, line.unwrap_or_default());
        LineActions {
            side,
            position,
            selected: line.is_some() && self.selected.is_some_and(|range| range.contains(position)),
            on_select: self.on_select_line.clone(),
            on_comment_line: match side {
                DiffSide::Base => None,
                DiffSide::Head => self.on_comment_line.clone(),
            },
            menu: self.line_menu.clone(),
            document_order: DOCUMENT_ORDER_BASE
                + self.document_order * 2
                + u64::from(side == DiffSide::Head),
        }
    }

    /// The syntax colors of hunk line `line`, with find's matches marked over them.
    fn spans(&self, hunk: usize, line: usize, cx: &App) -> Vec<(Range<usize>, HighlightStyle)> {
        let syntax = self.highlights.line(hunk, line);
        let Some(find) = &self.find else {
            return syntax.to_vec();
        };
        let text = &self.file.hunks[hunk].lines[line].text;
        let warning = cx.theme().warning;
        let marks = find_ranges(text, &find.query).into_iter().map(|range| {
            let current = find.current.as_ref() == Some(&(line, range.clone()));
            let alpha = if current { 0.7 } else { 0.3 };
            let style = HighlightStyle {
                background_color: Some(warning.opacity(alpha)),
                ..HighlightStyle::default()
            };
            (range, style)
        });
        gpui_kit::combine_highlights(syntax.iter().cloned(), marks).collect()
    }

    /// Whether the current find match is on hunk line `line`.
    fn has_current(&self, line: usize) -> bool {
        self.find
            .as_ref()
            .and_then(|find| find.current.as_ref())
            .is_some_and(|(current, _)| *current == line)
    }

    fn row_id(&self, kind: &str) -> ElementId {
        let ix = match self.row {
            DiffRow::Hunk(hunk) => hunk << 20,
            DiffRow::Line { hunk, line } => (hunk << 20) | line,
            DiffRow::Split { hunk, left, right } => {
                (hunk << 20) | right.or(left).unwrap_or_default()
            }
            DiffRow::Threads(line) => line as usize,
            DiffRow::Header | DiffRow::Binary => 0,
        };
        ElementId::NamedInteger(
            SharedString::from(format!("{kind}-{}", self.file.path)),
            ix as u64,
        )
    }
}

impl RenderOnce for FileDiffRow {
    fn render(mut self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let file = self.file.clone();
        let body: AnyElement = match self.row {
            DiffRow::Header => self.render_header(cx).into_any_element(),
            DiffRow::Binary => div()
                .px_4()
                .py_3()
                .text_ui()
                .text_color(cx.theme().muted_foreground)
                .child("Binary file not shown.")
                .into_any_element(),
            DiffRow::Hunk(hunk) => hunk_header(&file.hunks[hunk].header(), cx).into_any_element(),
            DiffRow::Line { hunk, line } => unified_line(
                &file.hunks[hunk].lines[line],
                &self.spans(hunk, line, cx),
                self.line_actions(DiffSide::Head, hunk, Some(line)),
                cx,
            )
            .into_any_element(),
            DiffRow::Split { hunk, left, right } => {
                let lines = &file.hunks[hunk].lines;
                let base_actions = self.line_actions(DiffSide::Base, hunk, left);
                let head_actions = self.line_actions(DiffSide::Head, hunk, right);
                let left = left.map(|ix| (&lines[ix], self.spans(hunk, ix, cx)));
                let right = right.map(|ix| (&lines[ix], self.spans(hunk, ix, cx)));
                let left = left.as_ref().map(|(line, spans)| (*line, spans.as_slice()));
                let right = right
                    .as_ref()
                    .map(|(line, spans)| (*line, spans.as_slice()));
                h_flex()
                    .items_stretch()
                    .child(split_side(left, base_actions, cx))
                    .child(div().w_px().bg(cx.theme().border))
                    .child(split_side(right, head_actions, cx))
                    .into_any_element()
            }
            DiffRow::Threads(_) => {
                thread_slot(std::mem::take(&mut self.threads)).into_any_element()
            }
        };
        let current = match self.row {
            DiffRow::Line { line, .. } => self.has_current(line),
            DiffRow::Split { left, right, .. } => {
                left.is_some_and(|ix| self.has_current(ix))
                    || right.is_some_and(|ix| self.has_current(ix))
            }
            _ => false,
        };
        let body = if current {
            div()
                .id("find-match-current")
                .test_support()
                .child(body)
                .into_any_element()
        } else {
            body
        };
        let id = match self.row {
            DiffRow::Header => ElementId::Name(SharedString::from(format!("file-{}", file.path))),
            _ => self.row_id("row"),
        };
        div()
            .id(id)
            .when(self.row == DiffRow::Header, |this| {
                this.border_t_1()
                    .rounded_tl(cx.theme().radius)
                    .rounded_tr(cx.theme().radius)
                    .overflow_hidden()
            })
            .border_l_1()
            .border_r_1()
            .border_color(cx.theme().border)
            .when(self.last, |this| {
                this.border_b_1()
                    .rounded_bl(cx.theme().radius)
                    .rounded_br(cx.theme().radius)
                    .overflow_hidden()
            })
            .child(body)
    }
}

fn hunk_header(header: &str, cx: &App) -> impl IntoElement {
    h_flex()
        .min_h_6()
        .h(code_line_height(cx))
        .bg(cx.app_theme().diff_hunk)
        .text_code(cx)
        .text_color(cx.theme().muted_foreground)
        .child(div().w(gutter_width()).flex_shrink_0())
        .child(
            div()
                .pl_2()
                .truncate()
                .child(SharedString::from(header.to_string())),
        )
}

/// Two 40 px line-number columns and a 16 px sign column, as the design's gutter.
fn gutter_width() -> gpui_kit::Rems {
    rems(6.)
}

fn line_colors(kind: LineKind, cx: &App) -> (Hsla, Hsla, Hsla) {
    let theme = cx.app_theme();
    match kind {
        LineKind::Added => (theme.diff_add, theme.diff_add_strong, cx.theme().success),
        LineKind::Removed => (theme.diff_del, theme.diff_del_strong, cx.theme().danger),
        LineKind::Context => (
            cx.theme().transparent,
            cx.theme().transparent,
            cx.theme().muted_foreground,
        ),
    }
}

fn sign(kind: LineKind) -> &'static str {
    match kind {
        LineKind::Added => "+",
        LineKind::Removed => "−",
        LineKind::Context => "",
    }
}

fn number_cell(number: Option<u32>, cx: &App) -> impl IntoElement {
    div()
        .w(rems(2.5))
        .flex_shrink_0()
        .pr_2()
        .text_right()
        .text_gutter(cx)
        .text_color(cx.theme().muted_foreground)
        .children(number.map(|n| n.to_string()))
}

fn code_cell(text: &str, spans: &Spans, document_order: u64, cx: &App) -> impl IntoElement {
    let (text, spans) = expand_tabs(text, spans);
    div()
        .flex_1()
        .min_w_0()
        .pl_2()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_code(cx)
        .child(SelectableCode::new("code", text, spans).document_order(document_order))
}

/// What a code line offers besides its text: commenting and selecting from the gutter,
/// the context menu, and its place in the text selection order.
struct LineActions {
    side: DiffSide,
    position: LinePosition,
    selected: bool,
    on_select: Option<SelectHandler>,
    on_comment_line: Option<LineHandler>,
    menu: Option<LineMenuBuilder>,
    document_order: u64,
}

impl LineActions {
    /// Reports a press on the line's numbers, which anchors a selection or, with Shift,
    /// extends the one there is.
    fn press(&self) -> Option<impl Fn(&gpui_kit::MouseDownEvent, &mut Window, &mut App) + 'static> {
        let (select, position) = (self.on_select.clone()?, self.position);
        Some(
            move |event: &gpui_kit::MouseDownEvent, window: &mut Window, cx: &mut App| {
                let gesture = if event.modifiers.shift {
                    LineGesture::ShiftPress
                } else {
                    LineGesture::Press
                };
                select(position, gesture, window, cx)
            },
        )
    }

    /// Reports the pointer crossing the line with the button held.
    fn drag_over(
        &self,
    ) -> Option<impl Fn(&gpui_kit::MouseMoveEvent, &mut Window, &mut App) + 'static> {
        let (select, position) = (self.on_select.clone()?, self.position);
        Some(
            move |event: &gpui_kit::MouseMoveEvent, window: &mut Window, cx: &mut App| {
                if event.pressed_button == Some(MouseButton::Left) {
                    select(position, LineGesture::DragOver, window, cx)
                }
            },
        )
    }

    /// The line's context menu: Copy and Copy line, then the owner's actions.
    fn context_menu(
        &self,
        line: &DiffLine,
    ) -> impl Fn(PopupMenu, &mut Window, &mut gpui_kit::Context<PopupMenu>) -> PopupMenu + 'static
    {
        let (line, side, extra) = (line.clone(), self.side, self.menu.clone());
        let position = self.position;
        move |menu, window, cx| {
            // Read now: the press on a menu item clears the selection before its handler.
            let target = LineMenuTarget {
                line: line.clone(),
                position,
                side,
                selected_text: TextSelection::selected_text(window, cx),
            };
            let selection = target.selected_text.clone();
            let text = target.line.text.clone();
            let menu = menu
                .item(
                    PopupMenuItem::new("Copy")
                        .icon(Icon::new(IconName::Copy))
                        .disabled(selection.is_empty())
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(selection.clone()))
                        }),
                )
                .item(
                    PopupMenuItem::new("Copy line")
                        .icon(Icon::new(IconName::Code))
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
                        }),
                );
            match &extra {
                Some(build) => build(menu.separator(), &target, window, cx),
                None => menu,
            }
        }
    }
}

fn unified_line(
    line: &DiffLine,
    spans: &Spans,
    actions: LineActions,
    cx: &App,
) -> impl IntoElement {
    let (background, gutter, sign_color) = line_colors(line.kind, cx);
    let comment_line = line.new_line.filter(|_| line.kind != LineKind::Removed);
    let on_comment_line = actions.on_comment_line.clone();
    let commentable = comment_line.is_some() && on_comment_line.is_some();
    let actions = LineActions {
        side: if line.kind == LineKind::Removed {
            DiffSide::Base
        } else {
            DiffSide::Head
        },
        ..actions
    };
    let numbers = format!(
        "{}-{}",
        line.old_line.unwrap_or(0),
        line.new_line.unwrap_or(0)
    );
    let gutter = if actions.selected {
        cx.theme().selection
    } else {
        gutter
    };
    h_flex()
        .id("line")
        .group(LINE_GROUP)
        .relative()
        .h(code_line_height(cx))
        .bg(background)
        .when(actions.selected, |this| {
            this.child(selection_fill(format!("selected-{numbers}"), cx))
        })
        .when_some(actions.drag_over(), |this, select| {
            this.on_mouse_move(select)
        })
        .child(
            h_flex()
                .id(SharedString::from(format!("gutter-{numbers}")))
                .relative()
                .h_full()
                .w(gutter_width())
                .flex_shrink_0()
                .bg(gutter)
                .when_some(actions.press(), |this, select| {
                    this.on_mouse_down(MouseButton::Left, select)
                })
                .child(number_cell(line.old_line, cx))
                .child(number_cell(line.new_line, cx))
                .child(
                    div()
                        .w_4()
                        .text_gutter(cx)
                        .font_weight(FontWeight::BOLD)
                        .text_color(sign_color)
                        .child(sign(line.kind)),
                )
                .when(commentable, |this| this.child(comment_badge(cx)))
                .when_some(
                    comment_line.zip(on_comment_line),
                    |this, (number, handler)| {
                        this.cursor_pointer()
                            .hover(|style| style.bg(cx.app_theme().diff_hunk))
                            .tooltip(|window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(
                                    "Click to comment, drag to select lines",
                                )
                                .build(window, cx)
                            })
                            .on_click(comment_click(number, handler))
                    },
                )
                .test_support(),
        )
        .child(code_cell(&line.text, spans, actions.document_order, cx))
        .context_menu(actions.context_menu(line))
}

fn split_side(
    line: Option<(&DiffLine, &Spans)>,
    actions: LineActions,
    cx: &App,
) -> impl IntoElement {
    let old_side = actions.side == DiffSide::Base;
    let Some((line, spans)) = line else {
        return h_flex()
            .flex_1()
            .min_w_0()
            .h(code_line_height(cx))
            .bg(cx.theme().muted.opacity(0.4))
            .into_any_element();
    };
    let (background, gutter, sign_color) = line_colors(line.kind, cx);
    let number = if old_side {
        line.old_line
    } else {
        line.new_line
    };
    let comment_line = (!old_side).then_some(line.new_line).flatten();
    let on_comment_line = actions.on_comment_line.clone();
    let commentable = comment_line.is_some() && on_comment_line.is_some();
    let numbers = format!(
        "{}-{}",
        if old_side { "old" } else { "new" },
        number.unwrap_or(0)
    );
    let gutter = if actions.selected {
        cx.theme().selection
    } else {
        gutter
    };
    h_flex()
        .id(if old_side { "line-base" } else { "line-head" })
        .group(LINE_GROUP)
        .relative()
        .flex_1()
        .min_w_0()
        .h(code_line_height(cx))
        .bg(background)
        .when(actions.selected, |this| {
            this.child(selection_fill(format!("selected-{numbers}"), cx))
        })
        .when_some(actions.drag_over(), |this, select| {
            this.on_mouse_move(select)
        })
        .child(
            h_flex()
                .id(SharedString::from(format!("split-{numbers}")))
                .relative()
                .h_full()
                .w(rems(3.5))
                .flex_shrink_0()
                .bg(gutter)
                .when_some(actions.press(), |this, select| {
                    this.on_mouse_down(MouseButton::Left, select)
                })
                .child(number_cell(number, cx))
                .child(
                    div()
                        .w_4()
                        .text_gutter(cx)
                        .font_weight(FontWeight::BOLD)
                        .text_color(sign_color)
                        .child(sign(line.kind)),
                )
                .when(commentable, |this| this.child(comment_badge(cx)))
                .when_some(
                    comment_line.zip(on_comment_line),
                    |this, (number, handler)| {
                        this.cursor_pointer()
                            .on_click(comment_click(number, handler))
                    },
                )
                .test_support(),
        )
        .child(code_cell(&line.text, spans, actions.document_order, cx))
        .context_menu(actions.context_menu(line))
        .into_any_element()
}

/// A click on a line's numbers starts a comment there. A Shift-click extends the
/// selection instead, so it starts none.
fn comment_click(
    number: u32,
    handler: LineHandler,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |event, window, cx| {
        if !event.modifiers().shift {
            handler(number, window, cx)
        }
    }
}

/// The fill behind a selected line, under its numbers and code.
fn selection_fill(id: String, cx: &App) -> impl IntoElement {
    div()
        .id(SharedString::from(id))
        .absolute()
        .top_0()
        .bottom_0()
        .left_0()
        .right_0()
        .bg(cx.theme().selection)
        .test_support()
}

/// Columns between tab stops when a tab is expanded.
const TAB_WIDTH: usize = 4;

/// Expands tabs to the next tab stop, shifting the highlight spans by the bytes each
/// tab grew, so indented Go and Makefiles keep their colors.
fn expand_tabs(
    text: &str,
    spans: &[(std::ops::Range<usize>, gpui_kit::HighlightStyle)],
) -> (
    String,
    Vec<(std::ops::Range<usize>, gpui_kit::HighlightStyle)>,
) {
    if !text.contains('\t') {
        return (text.to_string(), spans.to_vec());
    }
    let mut expanded = String::with_capacity(text.len() + 16);
    // `offsets[i]` is where byte `i` of `text` lands in `expanded`, plus the end.
    let mut offsets = vec![0; text.len() + 1];
    let mut column = 0;
    for (ix, ch) in text.char_indices() {
        offsets[ix] = expanded.len();
        if ch == '\t' {
            let width = TAB_WIDTH - column % TAB_WIDTH;
            expanded.extend(std::iter::repeat_n(' ', width));
            column += width;
        } else {
            expanded.push(ch);
            column += 1;
        }
    }
    offsets[text.len()] = expanded.len();
    let spans = spans
        .iter()
        .filter(|(range, _)| range.end <= text.len())
        .map(|(range, style)| (offsets[range.start]..offsets[range.end], *style))
        .collect();
    (expanded, spans)
}

/// Builds a context menu with `build`, which needs the window context the menu runs in.
fn menu_for_context(
    build: &MenuBuilder,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut gpui_kit::Context<PopupMenu>,
) -> PopupMenu {
    build(menu, window, cx)
}

/// Hover group of a diff line, which reveals its comment badge.
const LINE_GROUP: &str = "diff-line";

/// The `+` that appears at the end of the gutter while the line is hovered, marking
/// the gutter as the place to click to comment.
fn comment_badge(cx: &App) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .right_0()
        .w_4()
        .flex()
        .items_center()
        .justify_center()
        .opacity(0.)
        .group_hover(LINE_GROUP, |style| style.opacity(1.))
        .child(
            div()
                .size_4()
                .flex()
                .items_center()
                .justify_center()
                .rounded(cx.theme().radius)
                .bg(cx.app_theme().accent)
                .text_color(cx.theme().background)
                .child(Icon::new(IconName::Plus).xsmall()),
        )
}

fn thread_slot(threads: Vec<AnyElement>) -> impl IntoElement {
    v_flex()
        .gap_2()
        .pt_2()
        .pb_3()
        .pr_3()
        .pl(rems(6.5))
        .children(threads)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_ranges_ignores_case() {
        assert_eq!(find_ranges("Foo foo", "FOO"), vec![0..3, 4..7]);
    }

    #[test]
    fn find_ranges_empty_query_is_empty() {
        assert!(find_ranges("foo", "").is_empty());
    }

    #[test]
    fn find_ranges_keep_multibyte_boundaries() {
        assert_eq!(find_ranges("é é", "É"), vec![0..2, 3..5]);
    }

    #[test]
    fn tabs_expand_to_stops_and_spans_follow() {
        let style = gpui_kit::HighlightStyle::default();
        let (text, spans) = expand_tabs("\treturn x\t// y", &[(1..7, style), (10..14, style)]);
        assert_eq!(text, "    return x    // y");
        assert_eq!(&text[spans[0].0.clone()], "return");
        assert_eq!(&text[spans[1].0.clone()], "// y");
    }
}

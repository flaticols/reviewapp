//! Find in a pane: a bar above the timeline that counts and steps through the places a
//! query occurs in the code lines and comment text the pane shows.

use std::ops::Range;
use std::rc::Rc;

use diff_view_ui::{DiffRow, find_ranges};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
};
use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, Styled as _, Subscription, TestSupportExt as _,
    Window, div, rems,
};
use review_model::{PullRequestDetail, TimelineItem};

use crate::ReviewThreadView;
use crate::rows::TimelineRow;

/// An open find bar.
pub(crate) struct FindState {
    input: Entity<InputState>,
    matches: Vec<FindMatch>,
    current: usize,
    /// The query, rows and data the matches were found in; a change to any of them
    /// searches again.
    searched: Option<(String, Rc<Vec<TimelineRow>>, Rc<PullRequestDetail>)>,
    _subscription: Subscription,
}

/// One place the query occurs, by timeline row.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FindMatch {
    pub row: usize,
    pub place: FindPlace,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FindPlace {
    /// In a code line: its index in the hunk and the bytes it covers.
    Code { line: usize, range: Range<usize> },
    /// In a description, comment or finding.
    Body,
}

impl ReviewThreadView {
    /// Opens the find bar, or puts the keyboard back in it with its query selected.
    pub(crate) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(find) = &self.find {
            let input = find.input.clone();
            input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
            return;
        }
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Find in pane")
                .context_menu(false)
        });
        let subscription =
            cx.subscribe(
                &input,
                |this: &mut Self, _, event: &InputEvent, cx| match event {
                    InputEvent::Change => cx.notify(),
                    InputEvent::PressEnter { shift, .. } => {
                        this.step_find(if *shift { -1 } else { 1 }, cx)
                    }
                    _ => {}
                },
            );
        input.update(cx, |input, cx| input.focus(window, cx));
        self.find = Some(FindState {
            input,
            matches: Vec::new(),
            current: 0,
            searched: None,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// Closes the find bar and hands the keyboard back to the pane.
    pub(crate) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find = None;
        window.focus(&self.focus_handle(cx), cx);
        cx.notify();
    }

    /// Whether the keyboard is in the find bar's query.
    pub(crate) fn find_has_focus(&self, window: &Window, cx: &gpui_kit::App) -> bool {
        self.find
            .as_ref()
            .is_some_and(|find| find.input.focus_handle(cx).is_focused(window))
    }

    /// The query to mark, while find is open and has one.
    pub(crate) fn find_query(&self, cx: &gpui_kit::App) -> Option<SharedString> {
        let find = self.find.as_ref()?;
        let query = find.input.read(cx).value();
        (!query.is_empty()).then(|| query.clone())
    }

    /// The match find is on.
    pub(crate) fn current_match(&self) -> Option<&FindMatch> {
        let find = self.find.as_ref()?;
        find.matches.get(find.current)
    }

    /// Searches again when the query, the rows or the data changed since the last
    /// search. A new query starts from its first match and scrolls to it.
    pub(crate) fn refresh_find(&mut self, cx: &mut Context<Self>) {
        let (Some(find), Some(detail)) = (&self.find, self.detail.clone()) else {
            return;
        };
        let query = find.input.read(cx).value().to_string();
        let same_query = match &find.searched {
            Some((searched, rows, data)) => {
                if searched == &query && Rc::ptr_eq(rows, &self.rows) && Rc::ptr_eq(data, &detail) {
                    return;
                }
                searched == &query
            }
            None => false,
        };
        let matches = self.find_matches(&query, &detail);
        let rows = self.rows.clone();
        let Some(find) = &mut self.find else {
            return;
        };
        find.current = if same_query {
            find.current.min(matches.len().saturating_sub(1))
        } else {
            0
        };
        find.matches = matches;
        find.searched = Some((query, rows, detail));
        if !same_query {
            self.reveal_current_match();
        }
    }

    /// Moves to the next match, or the previous one for a negative `delta`, wrapping
    /// around at either end.
    fn step_find(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(find) = &mut self.find else {
            return;
        };
        let count = find.matches.len() as isize;
        if count == 0 {
            return;
        }
        find.current = (find.current as isize + delta).rem_euclid(count) as usize;
        self.reveal_current_match();
        cx.notify();
    }

    fn reveal_current_match(&mut self) {
        if let Some(row) = self.current_match().map(|found| found.row) {
            self.list_state.scroll_to_reveal_item(row);
        }
    }

    /// Every place `query` occurs in the rows the pane shows, in row order.
    fn find_matches(&self, query: &str, detail: &PullRequestDetail) -> Vec<FindMatch> {
        let mut matches = Vec::new();
        if query.is_empty() {
            return matches;
        }
        let bodies = |row: usize, text: &str, matches: &mut Vec<FindMatch>| {
            matches.extend(find_ranges(text, query).into_iter().map(|_| FindMatch {
                row,
                place: FindPlace::Body,
            }));
        };
        for (row_ix, row) in self.rows.iter().enumerate() {
            match *row {
                TimelineRow::Item(item) => match &self.items[item] {
                    TimelineItem::Description => bodies(row_ix, &detail.body, &mut matches),
                    TimelineItem::Commented { comment } => {
                        bodies(row_ix, &comment.body, &mut matches)
                    }
                    TimelineItem::Finding { id } => {
                        if let Some(finding) = detail.finding(id) {
                            bodies(row_ix, &finding.body, &mut matches);
                        }
                    }
                    _ => {}
                },
                TimelineRow::File { item, row, .. } => {
                    let TimelineItem::File { path } = &self.items[item] else {
                        continue;
                    };
                    let lines: Vec<usize> = match row {
                        DiffRow::Line { line, .. } => vec![line],
                        DiffRow::Split { left, right, .. } => {
                            let mut lines: Vec<usize> = left.into_iter().chain(right).collect();
                            lines.dedup();
                            lines
                        }
                        DiffRow::Threads(number) => {
                            for thread in detail
                                .threads
                                .iter()
                                .filter(|thread| &thread.path == path && thread.line == number)
                            {
                                for comment in &thread.comments {
                                    bodies(row_ix, &comment.body, &mut matches);
                                }
                            }
                            continue;
                        }
                        DiffRow::Header | DiffRow::Binary | DiffRow::Hunk(_) => continue,
                    };
                    let (Some(file), DiffRow::Line { hunk, .. } | DiffRow::Split { hunk, .. }) =
                        (self.files.get(path), row)
                    else {
                        continue;
                    };
                    for line in lines {
                        let text = &file.hunks[hunk].lines[line].text;
                        matches.extend(find_ranges(text, query).into_iter().map(|range| {
                            FindMatch {
                                row: row_ix,
                                place: FindPlace::Code { line, range },
                            }
                        }));
                    }
                }
                _ => {}
            }
        }
        matches
    }

    /// The find bar, while find is open.
    pub(crate) fn render_find_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let find = self.find.as_ref()?;
        let count = match (find.input.read(cx).value().is_empty(), find.matches.len()) {
            (true, _) => None,
            (false, 0) => Some("No results".to_string()),
            (false, total) => Some(format!("{} of {total}", find.current + 1)),
        };
        Some(
            h_flex()
                .id("find-bar")
                .test_support()
                .flex_shrink_0()
                .px_6()
                .py_1p5()
                .gap_1()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    div().w(rems(18.)).child(
                        Input::new(&find.input)
                            .small()
                            .cleanable(true)
                            .prefix(Icon::new(IconName::Search).small()),
                    ),
                )
                .children(
                    count.map(|count| Button::new("find-count").ghost().xsmall().label(count)),
                )
                .child(
                    Button::new("find-previous")
                        .ghost()
                        .xsmall()
                        .icon(IconName::ChevronUp)
                        .tooltip("Previous match")
                        .on_click(cx.listener(|this, _, _, cx| this.step_find(-1, cx))),
                )
                .child(
                    Button::new("find-next")
                        .ghost()
                        .xsmall()
                        .icon(IconName::ChevronDown)
                        .tooltip("Next match")
                        .on_click(cx.listener(|this, _, _, cx| this.step_find(1, cx))),
                )
                .child(div().flex_1())
                .child(
                    Button::new("find-close")
                        .ghost()
                        .xsmall()
                        .icon(IconName::X)
                        .tooltip("Close")
                        .on_click(cx.listener(|this, _, window, cx| this.close_find(window, cx))),
                )
                .into_any_element(),
        )
    }
}

//! Asking an agent about lines of a diff: the reviewer selects lines by their numbers,
//! writes a question under them, and the agent's answer and suggested comment show in
//! the same place.

use app_state_ui::StoresExt as _;
use diff_view_ui::{LineGesture, LinePosition, LineRange};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, WindowExt as _, h_flex,
    input::{InputEvent, TextareaState},
    notification::Notification,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, Context, Entity, InteractiveElement as _, IntoElement as _,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, TestSupportExt as _, Window,
    div,
};
use theme_ui::TypographyExt as _;

use crate::quote::Placement;
use crate::{ReviewThreadView, editor, submits};

/// Lines the reviewer selected in one file's diff, and the question being written
/// about them.
pub(crate) struct LineSelection {
    path: String,
    /// Where the press landed; a drag or a Shift-click runs from here.
    origin: LinePosition,
    range: LineRange,
    /// The button is still down.
    dragging: bool,
    /// A Shift-click or the menu chose the lines, so a single line stays selected too.
    /// A plain press on one line is a click, which starts a comment instead.
    deliberate: bool,
    /// The question field and the head-side line it sits under, once the lines are
    /// chosen.
    ask: Option<(u32, Entity<TextareaState>)>,
}

impl ReviewThreadView {
    /// The selected lines of `path`, for its rows to paint.
    pub(crate) fn selected_lines(&self, path: &str) -> Option<LineRange> {
        self.line_selection
            .as_ref()
            .filter(|selection| selection.path == path)
            .map(|selection| selection.range)
    }

    /// Whether lines are selected, in any file.
    pub(crate) fn has_line_selection(&self) -> bool {
        self.line_selection.is_some()
    }

    /// Head-side lines of `path` that hold a question field or a question's card.
    pub(crate) fn ask_anchors(&self, path: &str, cx: &App) -> Vec<u32> {
        let draft = self
            .line_selection
            .as_ref()
            .filter(|selection| selection.path == path)
            .and_then(|selection| selection.ask.as_ref())
            .map(|(anchor, _)| *anchor);
        agent_review_ui::questions_for(&self.key, cx)
            .iter()
            .map(|question| question.read(cx))
            .filter(|question| question.path() == path)
            .map(|question| question.anchor())
            .chain(draft)
            .collect()
    }

    /// Follows a press or drag over the line numbers of `path`.
    pub(crate) fn select_line(
        &mut self,
        path: &str,
        position: LinePosition,
        gesture: LineGesture,
        cx: &mut Context<Self>,
    ) {
        let current = self
            .line_selection
            .as_ref()
            .filter(|selection| selection.path == path);
        let next = match (gesture, current) {
            (LineGesture::DragOver, Some(selection)) if selection.dragging => {
                let range = LineRange::new(selection.origin, position);
                if range == selection.range {
                    return;
                }
                LineSelection {
                    path: path.to_string(),
                    origin: selection.origin,
                    range,
                    dragging: true,
                    deliberate: selection.deliberate,
                    ask: None,
                }
            }
            (LineGesture::DragOver, _) => return,
            (LineGesture::ShiftPress, Some(selection)) => LineSelection {
                path: path.to_string(),
                origin: selection.origin,
                range: LineRange::new(selection.origin, position),
                dragging: true,
                deliberate: true,
                ask: None,
            },
            (gesture, _) => LineSelection {
                path: path.to_string(),
                origin: position,
                range: LineRange::new(position, position),
                dragging: true,
                deliberate: gesture == LineGesture::ShiftPress,
                ask: None,
            },
        };
        self.line_selection = Some(next);
        cx.notify();
    }

    /// Ends a drag over line numbers when the button comes up: several lines, or lines
    /// chosen with Shift, get the question field; a click on one line selects nothing.
    pub(crate) fn finish_line_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = self
            .line_selection
            .as_mut()
            .filter(|selection| selection.dragging)
        else {
            return;
        };
        selection.dragging = false;
        if selection.range.is_single() && !selection.deliberate {
            self.line_selection = None;
            cx.notify();
            return;
        }
        let (path, range) = (selection.path.clone(), selection.range);
        self.open_ask(&path, range, window, cx);
    }

    /// Opens the question field for the line a context menu was opened on: the whole
    /// selection when the line is part of it, else that line alone.
    pub(crate) fn ask_about_line(
        &mut self,
        path: &str,
        position: LinePosition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self
            .selected_lines(path)
            .filter(|range| range.contains(position))
            .unwrap_or_else(|| LineRange::new(position, position));
        self.open_ask(path, range, window, cx);
        let field = self
            .line_selection
            .as_ref()
            .and_then(|selection| selection.ask.as_ref())
            .map(|(_, input)| input.clone());
        // The closing menu takes focus back after this handler ran, so the field is
        // focused again once it has.
        if let Some(input) = field {
            window.defer(cx, move |window, cx| {
                input.update(cx, |input, cx| input.focus(window, cx))
            });
        }
    }

    /// Selects `range` of `path` and puts a focused question field under it.
    fn open_ask(
        &mut self,
        path: &str,
        range: LineRange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let anchor = self.files.get(path).and_then(|file| range.anchor(file));
        let Some(anchor) = anchor else {
            // A deleted file has no line in the new version to put the question under.
            self.line_selection = None;
            window.push_notification(
                Notification::warning("Lines of a deleted file can’t be asked about yet."),
                cx,
            );
            cx.notify();
            return;
        };
        let input = editor(
            "What do you want to know about these lines?".into(),
            None,
            window,
            cx,
        );
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| {
                if submits(event) {
                    this.submit_ask(window, cx);
                }
            },
        );
        self._subscriptions.push(subscription);
        input.update(cx, |input, cx| input.focus(window, cx));
        self.line_selection = Some(LineSelection {
            path: path.to_string(),
            origin: range.first(),
            range,
            dragging: false,
            deliberate: true,
            ask: Some((anchor, input)),
        });
        self.sync_rows(cx);
        cx.notify();
    }

    /// Drops the selection and the question being written about it.
    pub(crate) fn clear_line_selection(&mut self, cx: &mut Context<Self>) {
        self.line_selection = None;
        self.sync_rows(cx);
        cx.notify();
    }

    /// Sends the question to the default agent and replaces the field with its card.
    pub(crate) fn submit_ask(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = &self.line_selection else {
            return;
        };
        let Some((anchor, input)) = &selection.ask else {
            return;
        };
        let question = input.read(cx).value().trim().to_string();
        let Some(file) = self.files.get(&selection.path).cloned() else {
            return;
        };
        if question.is_empty() {
            return;
        }
        let Some(agent) = cx.stores().settings().read(cx).default_agent().cloned() else {
            window.push_notification(
                Notification::warning("Add an agent in Settings to ask about these lines."),
                cx,
            );
            return;
        };
        let lines = selection.range.lines(&file).into_iter().cloned().collect();
        let label = selection.range.label(&file);
        let options = agent_review_ui::AskOptions::new(file, lines, label, *anchor, question);
        if agent_review_ui::ask_about_lines(self.key.clone(), agent, options, window, cx).is_none()
        {
            return;
        }
        window.focus(&self.focus_handle, cx);
        self.clear_line_selection(cx);
    }

    /// The cards of the questions asked under head-side `line` of `path`. They come
    /// before the line's threads, so a suggested comment the reviewer adds appears below
    /// the answer it came from.
    pub(crate) fn render_questions(
        &mut self,
        path: &str,
        line: u32,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        agent_review_ui::questions_for(&self.key, cx)
            .into_iter()
            .filter(|question| {
                let question = question.read(cx);
                question.path() == path && question.anchor() == line
            })
            .map(|question| {
                let (add_view, add_path) = (cx.entity(), path.to_string());
                let (edit_view, edit_path) = (cx.entity(), path.to_string());
                let dismiss_view = cx.entity();
                agent_review_ui::AgentQuestionCard::new(question)
                    .on_add_comment(move |body, _, cx| {
                        add_view.update(cx, |this, cx| {
                            let (key, viewer) = (this.key.clone(), this.viewer(cx));
                            let body = body.to_string();
                            this.store(cx).update(cx, |store, cx| {
                                store.start_thread(&key, &add_path, line, &viewer, body, cx)
                            });
                            cx.notify();
                        })
                    })
                    .on_edit_comment(move |body, window, cx| {
                        edit_view.update(cx, |this, cx| {
                            let input = this.line_draft(&edit_path, line, window, cx);
                            this.insert_into(&input, body, Placement::Inline, window, cx);
                        })
                    })
                    .on_dismiss(move |_, cx| dismiss_view.update(cx, |_, cx| cx.notify()))
                    .into_any_element()
            })
            .collect()
    }

    /// The question being written under head-side `line` of `path`, if one is.
    pub(crate) fn render_ask_field(
        &mut self,
        path: &str,
        line: u32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let draft = self
            .line_selection
            .as_ref()
            .filter(|selection| selection.path == path)
            .and_then(|selection| {
                let (anchor, input) = selection.ask.as_ref()?;
                (*anchor == line).then(|| (selection.range, input.clone()))
            });
        let (range, input) = draft?;
        {
            let label = self
                .files
                .get(path)
                .map(|file| range.label(file).to_lowercase())
                .unwrap_or_default();
            let agent = cx
                .stores()
                .settings()
                .read(cx)
                .default_agent()
                .map(|agent| agent.name.clone());
            let heading = match agent {
                Some(agent) => format!("Ask {agent} about {label}"),
                None => format!("Add an agent in Settings to ask about {label}"),
            };
            Some(
                v_flex()
                    .gap_1p5()
                    .child(
                        h_flex()
                            .id("ask-agent-heading")
                            .aria_label(heading.clone())
                            .gap_1p5()
                            .text_ui()
                            .text_color(cx.theme().muted_foreground)
                            .child(Icon::new(IconName::Bot).small())
                            .child(div().child(heading))
                            .test_support(),
                    )
                    .child(self.render_editor(
                        "ask-agent",
                        &input,
                        "Ask",
                        |this, window, cx| this.submit_ask(window, cx),
                        |this, _, cx| this.clear_line_selection(cx),
                        cx,
                    ))
                    .into_any_element(),
            )
        }
    }
}

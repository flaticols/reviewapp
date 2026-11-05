//! The review screen split into panes: each a full copy of the review that scrolls on
//! its own, with one inspector for all of them.

use std::collections::HashMap;

use app_state_ui::StoresExt as _;
use app_state_ui::actions::{
    ClosePane, FocusNextPane, FocusPaneDown, FocusPaneLeft, FocusPaneRight, FocusPaneUp,
    SplitSideBySide, SplitStacked, ZoomPane,
};
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, CursorStyle, DragMoveEvent, Empty, Entity,
    EntityId, EventEmitter, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, Window, div, prelude::FluentBuilder as _, px, relative,
};
use pr_model::PullRequestKey;
use serde::{Deserialize, Serialize};

use super::{Direction, PaneState, PaneTree, SplitAxis};
use crate::{ReviewThreadEvent, ReviewThreadView};

pub(crate) const CONTEXT: &str = "ReviewPanes";
/// The name the layout is saved under, per pull request.
const VIEW_STATE: &str = "panes";

/// What a pull request's review looks like next time it opens.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct SavedPanes {
    tree: PaneTree<PaneState>,
    focused: usize,
    zoomed: bool,
}

/// A divider being dragged, named by the boundary of the split it resizes.
#[derive(Clone)]
struct SplitDrag {
    boundary: usize,
}

impl Render for SplitDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// One pull request's review, split into panes.
pub struct ReviewPanes {
    key: PullRequestKey,
    back_label: SharedString,
    tree: PaneTree<Entity<ReviewThreadView>>,
    focused: usize,
    zoomed: bool,
    /// Whether the saved layout was applied, or the reviewer changed the layout before
    /// it arrived. Until then nothing is saved, so a half-open review can't overwrite it.
    restored: bool,
    watches: HashMap<EntityId, [Subscription; 2]>,
    _load: Task<()>,
}

impl EventEmitter<ReviewThreadEvent> for ReviewPanes {}

impl Focusable for ReviewPanes {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.focused_view().focus_handle(cx)
    }
}

impl ReviewPanes {
    /// Opens the review of `key` as one pane, then as the layout saved for it, if any.
    pub fn new(
        key: PullRequestKey,
        back_label: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let back_label = back_label.into();
        let first = new_pane(&key, &back_label, PaneState::default(), true, window, cx);
        let load = cx
            .stores()
            .pull_requests()
            .read(cx)
            .load_view_state::<SavedPanes>(&key, VIEW_STATE, cx);
        let load = cx.spawn_in(window, async move |this, cx| {
            let saved = load.await;
            this.update_in(cx, |this, window, cx| this.restore(saved, window, cx))
                .ok();
        });
        let mut panes = Self {
            key,
            back_label,
            tree: PaneTree::Leaf(first.clone()),
            focused: 0,
            zoomed: false,
            restored: false,
            watches: HashMap::new(),
            _load: load,
        };
        panes.watch(&first, window, cx);
        window.focus(&first.focus_handle(cx), cx);
        panes
    }

    /// The pull request shown.
    pub fn key(&self) -> &PullRequestKey {
        &self.key
    }

    /// The pane that has focus, where commands for the review go.
    pub fn focused_view(&self) -> Entity<ReviewThreadView> {
        self.tree.leaves()[self.focused].clone()
    }

    /// How many panes there are.
    pub fn pane_count(&self) -> usize {
        self.tree.count()
    }

    /// The focused pane's position, first halves before second halves.
    pub fn focused_index(&self) -> usize {
        self.focused
    }

    /// Every pane, first halves before second halves.
    #[doc(hidden)]
    pub fn panes(&self) -> Vec<Entity<ReviewThreadView>> {
        self.tree.leaves().into_iter().cloned().collect()
    }

    /// Splits the focused pane along `axis`. The new pane copies it and takes focus.
    #[doc(hidden)]
    pub fn split(&mut self, axis: SplitAxis, window: &mut Window, cx: &mut Context<Self>) {
        self.restored = true;
        let state = self.focused_view().read(cx).pane_state();
        let pane = new_pane(&self.key, &self.back_label, state, false, window, cx);
        self.watch(&pane, window, cx);
        self.focused = self.tree.split(self.focused, axis, pane);
        self.zoomed = false;
        self.changed(window, cx);
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((closed, focus)) = self.tree.close(self.focused) else {
            return;
        };
        self.restored = true;
        self.watches.remove(&closed.entity_id());
        self.focused = focus;
        self.zoomed = false;
        if closed.read(cx).diffs_locally() {
            let heir = self.tree.leaves()[0].clone();
            heir.update(cx, |pane, cx| pane.set_diffs_locally(true, cx));
        }
        self.changed(window, cx);
    }

    fn focus_toward(&mut self, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(next) = self.tree.neighbor(self.focused, direction) {
            self.focus(next, window, cx);
        }
    }

    fn focus(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.focused = ix;
        self.zoomed = false;
        self.changed(window, cx);
    }

    fn toggle_zoom(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.zoomed = !self.zoomed && self.tree.count() > 1;
        self.changed(window, cx);
    }

    /// Puts keyboard focus in the focused pane, saves and redraws.
    fn changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focused_view().focus_handle(cx), cx);
        self.save(cx);
        cx.notify();
    }

    fn save(&self, cx: &mut Context<Self>) {
        if !self.restored {
            return;
        }
        let saved = SavedPanes {
            tree: self.tree.map(&mut |pane| pane.read(cx).pane_state()),
            focused: self.focused,
            zoomed: self.zoomed,
        };
        let key = self.key.clone();
        cx.stores()
            .pull_requests()
            .clone()
            .update(cx, |store, cx| {
                store.save_view_state(&key, VIEW_STATE, &saved, cx)
            });
    }

    /// Applies the layout saved for this pull request, unless the reviewer has already
    /// changed this one.
    fn restore(&mut self, saved: Option<SavedPanes>, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.restored, true) {
            return;
        }
        let Some(saved) = saved else {
            return;
        };
        // The pane already open becomes the first, so its loading and diffing carry on.
        let first = self.focused_view();
        let mut leaves = saved.tree.leaves().into_iter();
        if let Some(state) = leaves.next() {
            first.update(cx, |pane, cx| pane.apply_pane_state(state.clone(), cx));
        }
        let mut made = vec![first];
        for state in leaves {
            made.push(new_pane(&self.key, &self.back_label, state.clone(), false, window, cx));
        }
        for pane in &made[1..] {
            self.watch(pane, window, cx);
        }
        let mut panes = made.into_iter();
        self.tree = saved
            .tree
            .map(&mut |_| panes.next().expect("one pane per saved leaf"));
        self.focused = saved.focused.min(self.tree.count() - 1);
        self.zoomed = saved.zoomed && self.tree.count() > 1;
        window.focus(&self.focused_view().focus_handle(cx), cx);
        cx.notify();
    }

    fn watch(
        &mut self,
        pane: &Entity<ReviewThreadView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let events = cx.subscribe_in(
            pane,
            window,
            |this, _, event: &ReviewThreadEvent, _, cx| match event {
                ReviewThreadEvent::ViewChanged => this.save(cx),
                ReviewThreadEvent::Back | ReviewThreadEvent::Open(_) => cx.emit(event.clone()),
            },
        );
        // The inspector shows the focused pane, so a change there redraws this view.
        let redraws = cx.observe(pane, |_, _, cx| cx.notify());
        self.watches.insert(pane.entity_id(), [events, redraws]);
    }

    fn render_node(
        &self,
        node: &PaneTree<Entity<ReviewThreadView>>,
        offset: usize,
        framed: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match node {
            PaneTree::Leaf(pane) => self.render_leaf(pane, offset, framed, cx),
            PaneTree::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let boundary = offset + first.count();
                let first = self.render_node(first, offset, framed, cx);
                let second = self.render_node(second, boundary, framed, cx);
                let border = cx.theme().border;
                let side_by_side = *axis == SplitAxis::SideBySide;
                let container = match axis {
                    SplitAxis::SideBySide => h_flex(),
                    SplitAxis::Stacked => v_flex(),
                };
                container
                    .id(("pane-split", boundary))
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .on_drag_move(cx.listener(
                        move |this, event: &DragMoveEvent<SplitDrag>, _, cx| {
                            if event.drag(cx).boundary != boundary {
                                return;
                            }
                            let ratio = if side_by_side {
                                (event.event.position.x - event.bounds.origin.x)
                                    / event.bounds.size.width
                            } else {
                                (event.event.position.y - event.bounds.origin.y)
                                    / event.bounds.size.height
                            };
                            this.restored = true;
                            this.tree.set_ratio(boundary, ratio);
                            this.save(cx);
                            cx.notify();
                        },
                    ))
                    .child(
                        div()
                            .flex_none()
                            .min_w_0()
                            .min_h_0()
                            .map(|this| match axis {
                                SplitAxis::SideBySide => this.h_full().w(relative(*ratio)),
                                SplitAxis::Stacked => this.w_full().h(relative(*ratio)),
                            })
                            .child(first),
                    )
                    .child(
                        div()
                            .id(("pane-divider", boundary))
                            .flex_none()
                            .flex()
                            .justify_center()
                            .items_center()
                            .map(|this| match axis {
                                SplitAxis::SideBySide => this
                                    .w(px(5.))
                                    .h_full()
                                    .flex_row()
                                    .cursor(CursorStyle::ResizeLeftRight)
                                    .child(div().w(px(1.)).h_full().bg(border)),
                                SplitAxis::Stacked => this
                                    .h(px(5.))
                                    .w_full()
                                    .flex_col()
                                    .cursor(CursorStyle::ResizeUpDown)
                                    .child(div().h(px(1.)).w_full().bg(border)),
                            })
                            .on_drag(SplitDrag { boundary }, |drag, _, _, cx| {
                                cx.new(|_| drag.clone())
                            }),
                    )
                    .child(div().flex_1().min_w_0().min_h_0().child(second))
                    .into_any_element()
            }
        }
    }

    fn render_leaf(
        &self,
        pane: &Entity<ReviewThreadView>,
        ix: usize,
        framed: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let focused = ix == self.focused;
        let ring = cx.theme().ring;
        div()
            .id(("review-pane", pane.entity_id().as_u64()))
            .size_full()
            .min_w_0()
            .min_h_0()
            .when(framed, |this| {
                this.border_1()
                    .border_color(if focused { ring } else { gpui_kit::transparent_black() })
            })
            .capture_any_mouse_down(cx.listener(move |this, _, window, cx| {
                if this.focused != ix {
                    this.focus(ix, window, cx);
                }
            }))
            .child(pane.clone())
            .into_any_element()
    }
}

/// A pane for `key` that draws no inspector, as the panes share one.
fn new_pane(
    key: &PullRequestKey,
    back_label: &SharedString,
    state: PaneState,
    diffs_locally: bool,
    window: &mut Window,
    cx: &mut App,
) -> Entity<ReviewThreadView> {
    cx.new(|cx| {
        let mut pane = ReviewThreadView::new_pane(
            key.clone(),
            back_label.clone(),
            state,
            diffs_locally,
            window,
            cx,
        );
        pane.set_with_inspector(false);
        pane
    })
}

impl Render for ReviewPanes {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if self.zoomed {
            self.render_leaf(&self.focused_view(), self.focused, false, cx)
        } else {
            let tree = self.tree.clone();
            self.render_node(&tree, 0, tree.count() > 1, cx)
        };
        let inspector = self
            .focused_view()
            .update(cx, |pane, cx| pane.render_inspector_panel(cx));
        let content = match inspector {
            Some(inspector) => crate::with_inspector(body, inspector, window, cx),
            None => body,
        };
        div()
            .id("review-panes")
            .key_context(CONTEXT)
            .size_full()
            .on_action(cx.listener(|this, _: &SplitSideBySide, window, cx| {
                this.split(SplitAxis::SideBySide, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SplitStacked, window, cx| {
                this.split(SplitAxis::Stacked, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FocusPaneLeft, window, cx| {
                this.focus_toward(Direction::Left, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FocusPaneRight, window, cx| {
                this.focus_toward(Direction::Right, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FocusPaneUp, window, cx| {
                this.focus_toward(Direction::Up, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FocusPaneDown, window, cx| {
                this.focus_toward(Direction::Down, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FocusNextPane, window, cx| {
                let next = (this.focused + 1) % this.tree.count();
                this.focus(next, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ZoomPane, window, cx| this.toggle_zoom(window, cx)))
            .on_action(cx.listener(|this, _: &ClosePane, window, cx| this.close(window, cx)))
            .child(content)
    }
}

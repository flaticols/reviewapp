use std::rc::Rc;

use gpui_kit::component::{
    Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
};
use gpui_kit::{
    App, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _, Window,
    prelude::FluentBuilder as _,
};

use crate::Counter;

type SelectHandler = Rc<dyn Fn(&usize, &mut Window, &mut App)>;

/// A row of quiet tabs that switch what a region shows, such as `Open · Draft ·
/// Merged · Closed`. The selected tab keeps a muted fill; the others read as text.
#[derive(IntoElement)]
pub struct FilterTabs {
    id: SharedString,
    tabs: Vec<(SharedString, Option<usize>)>,
    selected: usize,
    on_select: Option<SelectHandler>,
}

impl FilterTabs {
    /// An empty tab row.
    pub fn new(id: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            tabs: Vec::new(),
            selected: 0,
            on_select: None,
        }
    }

    /// Adds a tab, with a count pill when `count` is set.
    pub fn tab(mut self, label: impl Into<SharedString>, count: Option<usize>) -> Self {
        self.tabs.push((label.into(), count));
        self
    }

    /// The index of the selected tab.
    pub fn selected(mut self, ix: usize) -> Self {
        self.selected = ix;
        self
    }

    /// Called with the index of the tab the user picked.
    pub fn on_select(mut self, handler: impl Fn(&usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for FilterTabs {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let id = self.id;
        h_flex().gap_1().children(
            self.tabs
                .into_iter()
                .enumerate()
                .map(|(ix, (label, count))| {
                    let handler = self.on_select.clone();
                    Button::new((id.clone(), ix))
                        .ghost()
                        .small()
                        .selected(ix == self.selected)
                        .label(label)
                        .when_some(count, |this, count| this.child(Counter::new(count)))
                        .when_some(handler, |this, handler| {
                            this.on_click(move |_, window, cx| handler(&ix, window, cx))
                        })
                }),
        )
    }
}

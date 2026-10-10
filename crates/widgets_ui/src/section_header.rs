use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex};
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _,
    TestSupportExt as _, Window, div, prelude::FluentBuilder as _,
};
use theme_ui::TypographyExt as _;

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A muted label that names a group of rows, with an optional trailing action such as
/// an add button, and optionally a disclosure chevron that folds the group.
#[derive(IntoElement)]
pub struct SectionHeader {
    label: SharedString,
    action: Option<AnyElement>,
    disclosure: Option<(ElementId, bool, ClickHandler)>,
}

impl SectionHeader {
    /// A header reading `label`.
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            action: None,
            disclosure: None,
        }
    }

    /// Adds an element at the trailing edge, usually a small ghost icon button.
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.action = Some(action.into_any_element());
        self
    }

    /// Makes the label a disclosure that folds its group: a chevron shows whether the
    /// group is `open`, and clicking the label calls `on_toggle`.
    pub fn disclosure(
        mut self,
        id: impl Into<ElementId>,
        open: bool,
        on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.disclosure = Some((id.into(), open, Rc::new(on_toggle)));
        self
    }
}

impl RenderOnce for SectionHeader {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let label = match self.disclosure {
            Some((id, open, on_toggle)) => h_flex()
                .id(id)
                .test_support()
                .flex_1()
                .min_w_0()
                .gap_1()
                .rounded(cx.theme().radius)
                .hover(|style| style.text_color(cx.theme().foreground))
                .on_click(move |event, window, cx| on_toggle(event, window, cx))
                .child(div().min_w_0().truncate().child(self.label))
                .child(
                    Icon::new(if open {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .xsmall()
                    .text_color(muted),
                )
                .into_any_element(),
            None => div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(self.label)
                .into_any_element(),
        };
        h_flex()
            .h_7()
            .px_2()
            .gap_2()
            .text_meta()
            .text_color(muted)
            .child(label)
            .when_some(self.action, |this, action| this.child(action))
    }
}

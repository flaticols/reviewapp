use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Collapsible, Sizable as _,
    button::{Button, ButtonVariants as _},
    sidebar::{SidebarItem, SidebarMenu},
    v_flex,
};
use gpui_kit::{
    App, ClickEvent, ElementId, IntoElement, ParentElement as _, SharedString, Styled as _, Window,
    prelude::FluentBuilder as _,
};
use widgets_ui::SectionHeader;

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(Clone)]
struct HeaderAction {
    id: SharedString,
    tooltip: SharedString,
    handler: ClickHandler,
}

/// A sidebar group whose header can carry an add button, which gpui-kit's
/// `SidebarGroup` has no slot for.
#[derive(Clone)]
pub struct NavSection {
    label: SharedString,
    action: Option<HeaderAction>,
    menu: SidebarMenu,
    collapsed: bool,
}

impl NavSection {
    /// A group labelled `label` listing `menu`'s items.
    pub fn new(label: impl Into<SharedString>, menu: SidebarMenu) -> Self {
        Self {
            label: label.into(),
            action: None,
            menu,
            collapsed: false,
        }
    }

    /// Adds a `+` button to the header.
    pub fn add_action(
        mut self,
        id: impl Into<SharedString>,
        tooltip: impl Into<SharedString>,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some(HeaderAction {
            id: id.into(),
            tooltip: tooltip.into(),
            handler: Rc::new(handler),
        });
        self
    }
}

impl Collapsible for NavSection {
    fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    fn is_collapsed(&self) -> bool {
        self.collapsed
    }
}

impl SidebarItem for NavSection {
    fn render(
        self,
        id: impl Into<ElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        let header = self.action.map_or_else(
            || SectionHeader::new(self.label.clone()),
            |action| {
                let handler = action.handler.clone();
                SectionHeader::new(self.label.clone()).action(
                    Button::new(action.id)
                        .ghost()
                        .xsmall()
                        .icon(IconName::Plus)
                        .tooltip(action.tooltip)
                        .on_click(move |event, window, cx| handler(event, window, cx)),
                )
            },
        );
        v_flex()
            .when(!self.collapsed, |this| this.child(header))
            .child(
                self.menu
                    .gap_0p5()
                    .collapsed(self.collapsed)
                    .render(id, window, cx),
            )
    }
}

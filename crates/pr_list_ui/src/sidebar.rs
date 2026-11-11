use std::rc::Rc;

use app_state_ui::StoresExt as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Collapsible, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenu},
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
    /// Set for sections the reviewer can fold: whether it is folded now.
    folded: Option<bool>,
    /// A `⋯` menu in the header for the section's options.
    menu_builder: Option<(SharedString, MenuBuilder)>,
}

type MenuBuilder = Rc<dyn Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu>;

impl NavSection {
    /// A group labelled `label` listing `menu`'s items.
    pub fn new(label: impl Into<SharedString>, menu: SidebarMenu) -> Self {
        Self {
            label: label.into(),
            action: None,
            menu,
            collapsed: false,
            folded: None,
            menu_builder: None,
        }
    }

    /// Adds a `⋯` menu to the header, built by `builder` each time it opens.
    pub fn options_menu(
        mut self,
        id: impl Into<SharedString>,
        builder: impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + 'static,
    ) -> Self {
        self.menu_builder = Some((id.into(), Rc::new(builder)));
        self
    }

    /// Lets the reviewer fold the section from its header; `folded` is its saved state.
    pub fn foldable(mut self, folded: bool) -> Self {
        self.folded = Some(folded);
        self
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
        let mut header = SectionHeader::new(self.label.clone());
        let mut actions = h_flex().gap_0p5();
        if let Some((id, builder)) = self.menu_builder {
            actions = actions.child(
                Button::new(id)
                    .ghost()
                    .xsmall()
                    .icon(IconName::Ellipsis)
                    .tooltip(format!("{} options", self.label))
                    .dropdown_menu(move |menu, window, cx| builder(menu, window, cx)),
            );
        }
        if let Some(action) = self.action {
            let handler = action.handler.clone();
            actions = actions.child(
                Button::new(action.id)
                    .ghost()
                    .xsmall()
                    .icon(IconName::Plus)
                    .tooltip(action.tooltip)
                    .on_click(move |event, window, cx| handler(event, window, cx)),
            );
        }
        header = header.action(actions);
        let folded = self.folded.unwrap_or(false);
        if self.folded.is_some() {
            let name = self.label.to_string();
            header = header.disclosure(
                SharedString::from(format!("fold-{name}")),
                !folded,
                move |_, _, cx| {
                    let name = name.clone();
                    cx.stores().settings().clone().update(cx, |settings, cx| {
                        settings.update_preferences(cx, |p| p.set_folded(&name, !folded))
                    });
                },
            );
        }
        v_flex()
            .when(!self.collapsed, |this| this.child(header))
            .when(!folded || self.collapsed, |this| {
                this.child(
                    self.menu
                        .gap_0p5()
                        .collapsed(self.collapsed)
                        .render(id, window, cx),
                )
            })
    }
}

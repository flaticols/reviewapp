use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::base::input::{Copy, Cut, InputBaseState, InputModeKind, Paste, SelectAll};
use gpui_kit::component::{
    Icon,
    menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem},
};
use gpui_kit::{
    Action, AnyElement, App, ElementId, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement as _, RenderOnce, StyleRefinement, Styled, TestSupportExt as _,
    Window, div,
};

type MenuBuilder = Rc<dyn Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu>;

/// A text field with the app's drawn edit menu: Cut, Copy, Paste and Select all, like
/// every other context menu in Switchfold.
///
/// gpui-kit's inputs open the system menu on macOS, which looks nothing like the rest.
/// Create the field's state with `.context_menu(false)` so only this menu opens, and
/// style this wrapper where the field itself would be sized.
#[derive(IntoElement)]
pub struct EditMenu {
    id: ElementId,
    menu: MenuBuilder,
    input: AnyElement,
    style: StyleRefinement,
}

impl EditMenu {
    /// `input`, the element that renders `state`, with the edit menu on right-click.
    pub fn new<M: InputModeKind>(
        id: impl Into<ElementId>,
        state: &Entity<InputBaseState<M>>,
        input: impl IntoElement,
    ) -> Self {
        let state = state.clone();
        Self {
            id: id.into(),
            menu: Rc::new(move |menu, _, cx| edit_menu(&state, menu, cx)),
            input: input.into_any_element(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for EditMenu {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for EditMenu {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let mut wrapper = div().id(self.id);
        *wrapper.style() = self.style;
        let menu = self.menu;
        wrapper
            .child(self.input)
            .test_support()
            .context_menu(move |popup, window, cx| menu(popup, window, cx))
    }
}

/// The edit commands for `state` as they stand when the menu opens, sent to the field
/// so its own key bindings and undo history handle them.
fn edit_menu<M: InputModeKind>(
    state: &Entity<InputBaseState<M>>,
    menu: PopupMenu,
    cx: &App,
) -> PopupMenu {
    let input = state.read(cx);
    let capabilities = input.context_menu_capabilities();
    let editable = !capabilities.is_disabled() && !capabilities.is_readonly();
    let all_selected = input.selected_range() == (0..input.value().len());
    let focus = input.focus_handle(cx);
    let item = |label: &'static str, icon: IconName, action: Box<dyn Action>, disabled: bool| {
        let focus = focus.clone();
        PopupMenuItem::new(label)
            .icon(Icon::new(icon))
            .action(action.boxed_clone())
            .disabled(disabled)
            .on_click(move |_, window, cx| {
                // The open menu holds focus until it has closed, so the field is
                // focused, and the command sent to it, only once it has.
                let (focus, action) = (focus.clone(), action.boxed_clone());
                window.defer(cx, move |window, cx| {
                    window.focus(&focus, cx);
                    window.dispatch_action(action, cx);
                });
            })
    };
    menu.item(item(
        "Cut",
        IconName::Scissors,
        Box::new(Cut),
        !(editable && capabilities.is_copyable()),
    ))
    .item(item(
        "Copy",
        IconName::Copy,
        Box::new(Copy),
        !capabilities.is_copyable(),
    ))
    .item(item(
        "Paste",
        IconName::ClipboardPaste,
        Box::new(Paste),
        !editable,
    ))
    .separator()
    .item(item(
        "Select all",
        IconName::SquareDashedText,
        Box::new(SelectAll),
        capabilities.is_disabled() || all_selected,
    ))
}

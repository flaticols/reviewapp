use app_state_ui::actions::{
    CloseWindow, FocusSearch, GoBack, OpenSettings, Quit, Refresh, ResetZoom, SubmitReview,
    ToggleFocusMode, ToggleInspector, ToggleSidebar, ZoomIn, ZoomOut,
};
use gpui_kit::{App, Menu, MenuItem};

/// Installs the macOS menu bar. Key bindings must be registered first so the menu shows
/// their shortcuts.
pub fn install(cx: &mut App) {
    cx.set_menus(vec![
        Menu::new("Switchfold").items([
            MenuItem::action("Settings…", OpenSettings),
            MenuItem::separator(),
            MenuItem::action("Quit Switchfold", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("Search…", FocusSearch),
            MenuItem::action("Go to File…", app_state_ui::actions::GoToFile),
            MenuItem::action("Refresh", Refresh),
            MenuItem::separator(),
            MenuItem::action("Close Window", CloseWindow),
        ]),
        Menu::new("View").items([
            MenuItem::action("Toggle Sidebar", ToggleSidebar),
            MenuItem::action("Toggle Right Sidebar", ToggleInspector),
            MenuItem::action("Focus Mode", ToggleFocusMode),
            MenuItem::separator(),
            MenuItem::action("Zoom In", ZoomIn),
            MenuItem::action("Zoom Out", ZoomOut),
            MenuItem::action("Actual Size", ResetZoom),
        ]),
        Menu::new("Review").items([
            MenuItem::action("Back", GoBack),
            MenuItem::action("Submit Review…", SubmitReview),
        ]),
    ]);
}

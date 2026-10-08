//! Settings › Keyboard: the shortcuts Switchfold binds, read from the live keymap.

use app_state_ui::actions::{
    CheckOut, FocusSearch, GoBack, OpenInZed, OpenSelected, OpenSettings, Refresh, ResetZoom,
    ReviewWithAgent, SelectNext, SelectPrevious, SubmitComment, SubmitReview, ToggleFocusMode,
    ToggleInspector, ToggleSidebar, ZoomIn, ZoomOut,
};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::{Action, App, IntoElement, ParentElement as _, RenderOnce, Window, div};
use widgets_ui::{SettingsPage, SettingsRow, SettingsSection};

/// The Keyboard page: one row per command with its current shortcut.
#[derive(IntoElement, Default)]
pub struct KeyboardPage;

impl KeyboardPage {
    /// Creates the page.
    pub fn new() -> Self {
        Self
    }
}

fn row(
    label: &'static str,
    action: &dyn Action,
    context: Option<&str>,
    window: &Window,
) -> impl IntoElement {
    let binding = match context {
        Some(context) => Kbd::binding_for_action(action, Some(context), window),
        None => Kbd::global_binding_for_action(action, window),
    };
    SettingsRow::new(label).trailing(match binding {
        Some(kbd) => kbd.into_any_element(),
        None => div().child("Not bound").into_any_element(),
    })
}

impl RenderOnce for KeyboardPage {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        SettingsPage::new(
            "keyboard-page",
            "Keyboard",
            "Shortcuts for the commands you use most.",
        )
        .child(
            SettingsSection::titled("Everywhere")
                .row(row(
                    "Search pull requests, files and commands",
                    &FocusSearch,
                    None,
                    window,
                ))
                .row(row("Refresh", &Refresh, None, window))
                .row(row("Settings", &OpenSettings, None, window))
                .row(row(
                    "Toggle sidebar",
                    &ToggleSidebar,
                    Some("Workspace"),
                    window,
                ))
                .row(row("Make the interface larger", &ZoomIn, None, window))
                .row(row("Make the interface smaller", &ZoomOut, None, window))
                .row(row("Restore the default size", &ResetZoom, None, window)),
        )
        .child(
            SettingsSection::titled("Pull request list")
                .row(row(
                    "Next pull request",
                    &SelectNext,
                    Some("PullRequestList"),
                    window,
                ))
                .row(row(
                    "Previous pull request",
                    &SelectPrevious,
                    Some("PullRequestList"),
                    window,
                ))
                .row(row(
                    "Open pull request",
                    &OpenSelected,
                    Some("PullRequestList"),
                    window,
                )),
        )
        .child(
            SettingsSection::titled("Review")
                .row(row(
                    "Back to pull requests",
                    &GoBack,
                    Some("ReviewThread"),
                    window,
                ))
                .row(row(
                    "Post comment",
                    &SubmitComment,
                    Some("ReviewThread"),
                    window,
                ))
                .row(row(
                    "Submit review",
                    &SubmitReview,
                    Some("ReviewThread"),
                    window,
                ))
                .row(row(
                    "Review with agent",
                    &ReviewWithAgent,
                    Some("ReviewThread"),
                    window,
                ))
                .row(row("Open in Zed", &OpenInZed, Some("ReviewThread"), window))
                .row(row("Check out", &CheckOut, Some("ReviewThread"), window))
                .row(row(
                    "Show or hide the file sidebar",
                    &ToggleSidebar,
                    Some("Workspace"),
                    window,
                ))
                .row(row(
                    "Show or hide the right sidebar",
                    &ToggleInspector,
                    Some("Workspace"),
                    window,
                ))
                .row(row(
                    "Focus mode",
                    &ToggleFocusMode,
                    Some("Workspace"),
                    window,
                )),
        )
    }
}

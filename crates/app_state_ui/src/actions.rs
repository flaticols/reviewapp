//! Commands that menus, buttons and key bindings dispatch. Each screen handles the ones
//! that apply to it on its own focused region.

gpui_kit::actions!(
    switchfold,
    [
        /// Quits the application.
        Quit,
        /// Closes the frontmost window.
        CloseWindow,
        /// Opens Settings.
        OpenSettings,
        /// Returns to the previous screen.
        GoBack,
        /// Syncs pull requests now.
        Refresh,
        /// Opens the command palette.
        FocusSearch,
        /// Focuses the pull request list's filter field in the title bar.
        FocusFilter,
        /// Shows or hides the sidebar.
        ToggleSidebar,
        /// Shows or hides a review's right sidebar: reviewers, checks and dependencies.
        ToggleInspector,
        /// Hides both of a review's sidebars so the timeline fills the window, or brings
        /// back what was open.
        ToggleFocusMode,
        /// Moves the selection down.
        SelectNext,
        /// Moves the selection up.
        SelectPrevious,
        /// Opens the selected item.
        OpenSelected,
        /// Opens the current pull request or file in Zed.
        OpenInZed,
        /// Checks out the current pull request.
        CheckOut,
        /// Posts the comment being written.
        SubmitComment,
        /// Opens the Submit review dialog.
        SubmitReview,
        /// Opens the Review with agent dialog.
        ReviewWithAgent,
        /// Marks the focused file as viewed.
        ToggleViewed,
        /// Makes the interface larger.
        ZoomIn,
        /// Makes the interface smaller.
        ZoomOut,
        /// Restores the default interface size.
        ResetZoom,
    ]
);

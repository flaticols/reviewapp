//! Window chrome: the title bar with breadcrumb, search trigger and account, and the
//! status bar with the current clone, branch, sync and agent state.

mod status_bar;
mod title_bar;

pub use status_bar::AppStatusBar;
pub use title_bar::{AppTitleBar, window_options};

//! The window's root: routes between sign-in, the pull request list, a review and
//! Settings, and owns launch-time setup, menus and window-level key bindings.

pub mod launch;
mod menus;
mod workspace;

pub use menus::install as install_menus;
pub use workspace::{Workspace, init};

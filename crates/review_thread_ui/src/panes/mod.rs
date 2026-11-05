//! Split panes for the review screen, tmux style.

mod state;
mod tree;
mod view;

pub use state::{PaneState, ScrollAnchor};
pub use tree::*;
pub use view::ReviewPanes;
pub(crate) use view::CONTEXT;

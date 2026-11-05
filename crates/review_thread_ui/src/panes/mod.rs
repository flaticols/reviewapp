//! Split panes for the review screen, tmux style.

mod state;
mod tree;

pub use state::{PaneState, ScrollAnchor};
#[allow(unused_imports)]
pub use tree::*;

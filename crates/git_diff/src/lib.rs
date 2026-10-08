//! Line diffs for pull requests: computed with gix from a local clone, or parsed from the
//! unified patch text GitHub returns for repositories that are not cloned.

mod clone;
mod patch;
mod text;

pub use clone::{CloneError, LocalClone};
pub use patch::parse_patch;
pub use text::{CONTEXT_LINES, diff_texts};

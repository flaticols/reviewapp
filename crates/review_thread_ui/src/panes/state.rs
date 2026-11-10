//! What one pane keeps when the layout is saved: its tab and where it is scrolled.

use serde::{Deserialize, Serialize};

use crate::TimelineFilter;

/// One pane's own view of the review.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaneState {
    pub filter: TimelineFilter,
    pub scroll: ScrollAnchor,
}

impl Default for PaneState {
    fn default() -> Self {
        Self {
            filter: TimelineFilter::Everything,
            scroll: ScrollAnchor::default(),
        }
    }
}

/// The row at the top of a pane. A file row is named by its file and how far below the
/// file's header it is, so it survives rows changing above it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScrollAnchor {
    pub file: Option<String>,
    /// Rows below the file's header, or the row's index when `file` is `None`.
    pub row: usize,
    /// How far into that row the top edge is, in pixels.
    pub offset: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_layout_saved_with_a_per_pane_tree_still_loads() {
        let saved = r#"{"filter":"Changes","outline":false,"scroll":{"file":"a.rs","row":2,"offset":4.0}}"#;
        let state: PaneState = serde_json::from_str(saved).expect("old layout loads");
        assert_eq!(state.scroll.file.as_deref(), Some("a.rs"));
    }
}

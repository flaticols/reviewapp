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

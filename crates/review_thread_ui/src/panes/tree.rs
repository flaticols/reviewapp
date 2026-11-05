//! The shape of a review's split panes: a binary tree whose leaves are panes.
//!
//! A leaf is named by its index in [`PaneTree::leaves`] order. A split is named by its
//! boundary, the index of the first leaf of its second half, which no other split shares.

use serde::{Deserialize, Serialize};

const MIN_RATIO: f32 = 0.1;
const MAX_RATIO: f32 = 0.9;
/// How close two edges must be to count as touching, in the unit square.
const EDGE: f32 = 1e-4;

/// How a split divides its space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitAxis {
    /// Left and right, as tmux's `%`.
    SideBySide,
    /// Top and bottom, as tmux's `"`.
    Stacked,
}

/// Which way focus moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// Panes and the splits between them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PaneTree<T> {
    Leaf(T),
    Split {
        axis: SplitAxis,
        /// The share of the space the first half takes.
        ratio: f32,
        first: Box<PaneTree<T>>,
        second: Box<PaneTree<T>>,
    },
}

/// Where a leaf sits, as a fraction of the whole area.
#[derive(Clone, Copy, Debug)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl<T: Clone> PaneTree<T> {
    /// How many panes there are.
    pub fn count(&self) -> usize {
        match self {
            PaneTree::Leaf(_) => 1,
            PaneTree::Split { first, second, .. } => first.count() + second.count(),
        }
    }

    /// The panes, first halves before second halves.
    pub fn leaves(&self) -> Vec<&T> {
        let mut leaves = Vec::new();
        self.collect(&mut leaves);
        leaves
    }

    fn collect<'a>(&'a self, leaves: &mut Vec<&'a T>) {
        match self {
            PaneTree::Leaf(value) => leaves.push(value),
            PaneTree::Split { first, second, .. } => {
                first.collect(leaves);
                second.collect(leaves);
            }
        }
    }

    /// Splits pane `ix` in half along `axis`, puts `new` in the second half and returns
    /// its index.
    pub fn split(&mut self, ix: usize, axis: SplitAxis, new: T) -> usize {
        let node = self.leaf_mut(ix);
        let old = std::mem::replace(node, PaneTree::Leaf(new.clone()));
        *node = PaneTree::Split {
            axis,
            ratio: 0.5,
            first: Box::new(old),
            second: Box::new(PaneTree::Leaf(new)),
        };
        ix + 1
    }

    fn leaf_mut(&mut self, ix: usize) -> &mut Self {
        match self {
            PaneTree::Leaf(_) => self,
            PaneTree::Split { first, second, .. } => {
                let in_first = first.count();
                if ix < in_first {
                    first.leaf_mut(ix)
                } else {
                    second.leaf_mut(ix - in_first)
                }
            }
        }
    }

    /// Removes pane `ix`, giving its space to its sibling. Returns the pane and the index
    /// of the pane to focus next, or `None` when `ix` is the only pane.
    pub fn close(&mut self, ix: usize) -> Option<(T, usize)> {
        let removed = self.remove(ix)?;
        Some((removed, ix.min(self.count() - 1)))
    }

    fn remove(&mut self, ix: usize) -> Option<T> {
        let PaneTree::Split { first, second, .. } = self else {
            return None;
        };
        let in_first = first.count();
        let (target, sibling, ix) = if ix < in_first {
            (first, second, ix)
        } else {
            (second, first, ix - in_first)
        };
        let PaneTree::Leaf(value) = target.as_ref() else {
            return target.remove(ix);
        };
        let value = value.clone();
        let kept = std::mem::replace(sibling.as_mut(), PaneTree::Leaf(value.clone()));
        *self = kept;
        Some(value)
    }

    /// Sets the split at `boundary` to give its first half `ratio` of the space.
    pub fn set_ratio(&mut self, boundary: usize, ratio: f32) {
        self.set_ratio_from(0, boundary, ratio.clamp(MIN_RATIO, MAX_RATIO));
    }

    fn set_ratio_from(&mut self, offset: usize, boundary: usize, value: f32) {
        let PaneTree::Split {
            ratio,
            first,
            second,
            ..
        } = self
        else {
            return;
        };
        let in_first = first.count();
        if offset + in_first == boundary {
            *ratio = value;
        } else if boundary < offset + in_first {
            first.set_ratio_from(offset, boundary, value);
        } else {
            second.set_ratio_from(offset + in_first, boundary, value);
        }
    }

    /// The pane next to `ix` in `direction`: the one sharing most of that edge, the upper
    /// or left one on a tie.
    pub fn neighbor(&self, ix: usize, direction: Direction) -> Option<usize> {
        let mut rects = Vec::new();
        self.layout(
            Rect {
                x: 0.,
                y: 0.,
                w: 1.,
                h: 1.,
            },
            &mut rects,
        );
        let from = *rects.get(ix)?;
        rects
            .iter()
            .enumerate()
            .filter(|(candidate, _)| *candidate != ix)
            .filter_map(|(candidate, rect)| {
                let (touches, overlap, along) = match direction {
                    Direction::Left => (
                        (rect.x + rect.w - from.x).abs() < EDGE,
                        span_overlap(from.y, from.h, rect.y, rect.h),
                        rect.y,
                    ),
                    Direction::Right => (
                        (from.x + from.w - rect.x).abs() < EDGE,
                        span_overlap(from.y, from.h, rect.y, rect.h),
                        rect.y,
                    ),
                    Direction::Up => (
                        (rect.y + rect.h - from.y).abs() < EDGE,
                        span_overlap(from.x, from.w, rect.x, rect.w),
                        rect.x,
                    ),
                    Direction::Down => (
                        (from.y + from.h - rect.y).abs() < EDGE,
                        span_overlap(from.x, from.w, rect.x, rect.w),
                        rect.x,
                    ),
                };
                (touches && overlap > EDGE).then_some((candidate, overlap, along))
            })
            .min_by(|a, b| b.1.total_cmp(&a.1).then(a.2.total_cmp(&b.2)))
            .map(|(candidate, _, _)| candidate)
    }

    fn layout(&self, area: Rect, rects: &mut Vec<Rect>) {
        match self {
            PaneTree::Leaf(_) => rects.push(area),
            PaneTree::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let (a, b) = match axis {
                    SplitAxis::SideBySide => {
                        let w = area.w * ratio;
                        (
                            Rect { w, ..area },
                            Rect {
                                x: area.x + w,
                                w: area.w - w,
                                ..area
                            },
                        )
                    }
                    SplitAxis::Stacked => {
                        let h = area.h * ratio;
                        (
                            Rect { h, ..area },
                            Rect {
                                y: area.y + h,
                                h: area.h - h,
                                ..area
                            },
                        )
                    }
                };
                first.layout(a, rects);
                second.layout(b, rects);
            }
        }
    }

    /// The same layout with each pane replaced by `f(pane)`.
    pub fn map<U>(&self, f: &mut impl FnMut(&T) -> U) -> PaneTree<U> {
        match self {
            PaneTree::Leaf(value) => PaneTree::Leaf(f(value)),
            PaneTree::Split {
                axis,
                ratio,
                first,
                second,
            } => PaneTree::Split {
                axis: *axis,
                ratio: *ratio,
                first: Box::new(first.map(f)),
                second: Box::new(second.map(f)),
            },
        }
    }
}

/// How much `[a, a + a_len)` and `[b, b + b_len)` overlap.
fn span_overlap(a: f32, a_len: f32, b: f32, b_len: f32) -> f32 {
    (a + a_len).min(b + b_len) - a.max(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitting_puts_the_new_pane_after_the_focused_one() {
        let mut tree = PaneTree::Leaf("a");
        assert_eq!(tree.split(0, SplitAxis::SideBySide, "b"), 1);
        assert_eq!(tree.leaves(), vec![&"a", &"b"]);
    }

    #[test]
    fn closing_a_pane_gives_its_space_to_the_sibling() {
        let mut tree = PaneTree::Leaf("a");
        tree.split(0, SplitAxis::SideBySide, "b");
        tree.split(1, SplitAxis::Stacked, "c");
        assert_eq!(tree.close(1), Some(("b", 1)));
        assert_eq!(tree.leaves(), vec![&"a", &"c"]);
    }

    #[test]
    fn closing_the_only_pane_is_refused() {
        let mut tree = PaneTree::Leaf("a");
        assert_eq!(tree.close(0), None);
        assert_eq!(tree.count(), 1);
    }

    #[test]
    fn arrows_find_the_pane_on_that_side() {
        let mut tree = PaneTree::Leaf("a");
        tree.split(0, SplitAxis::SideBySide, "b");
        tree.split(1, SplitAxis::Stacked, "c");
        assert_eq!(tree.neighbor(2, Direction::Left), Some(0));
        assert_eq!(tree.neighbor(0, Direction::Right), Some(1));
        assert_eq!(tree.neighbor(1, Direction::Down), Some(2));
    }

    #[test]
    fn arrows_at_the_edge_find_nothing() {
        let tree = PaneTree::Leaf("a");
        assert_eq!(tree.neighbor(0, Direction::Left), None);
    }

    #[test]
    fn ratios_are_clamped() {
        let mut tree = PaneTree::Leaf("a");
        tree.split(0, SplitAxis::SideBySide, "b");
        tree.set_ratio(1, 0.99);
        assert_eq!(
            tree,
            PaneTree::Split {
                axis: SplitAxis::SideBySide,
                ratio: 0.9,
                first: Box::new(PaneTree::Leaf("a")),
                second: Box::new(PaneTree::Leaf("b")),
            }
        );
    }

    #[test]
    fn a_layout_survives_json() {
        let mut tree = PaneTree::Leaf(1u8);
        tree.split(0, SplitAxis::Stacked, 2);
        let json = serde_json::to_string(&tree).unwrap();
        assert_eq!(serde_json::from_str::<PaneTree<u8>>(&json).unwrap(), tree);
    }
}

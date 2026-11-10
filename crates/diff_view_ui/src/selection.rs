use diff_model::{DiffLine, FileDiff, LineKind};

/// Where a line sits in a file's diff: its hunk, and its index among the hunk's lines.
/// Positions order top to bottom, as the unified layout shows the lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LinePosition {
    hunk: usize,
    line: usize,
}

impl LinePosition {
    /// Line `line` of hunk `hunk`.
    pub fn new(hunk: usize, line: usize) -> Self {
        Self { hunk, line }
    }

    /// The hunk's index in the file.
    pub fn hunk(self) -> usize {
        self.hunk
    }

    /// The line's index in the hunk.
    pub fn line(self) -> usize {
        self.line
    }
}

/// How the pointer reached a line while selecting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineGesture {
    /// The button went down on the line's numbers.
    Press,
    /// The button went down on the line's numbers with Shift held.
    ShiftPress,
    /// The pointer moved over the line with the button held.
    DragOver,
}

/// A run of consecutive lines in one file's diff, both ends included. In the unified
/// layout it can mix removed and added lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineRange {
    first: LinePosition,
    last: LinePosition,
}

impl LineRange {
    /// The lines from `a` to `b`, in either order.
    pub fn new(a: LinePosition, b: LinePosition) -> Self {
        Self {
            first: a.min(b),
            last: a.max(b),
        }
    }

    /// The topmost line.
    pub fn first(self) -> LinePosition {
        self.first
    }

    /// The bottommost line.
    pub fn last(self) -> LinePosition {
        self.last
    }

    /// Whether `position` is one of the lines.
    pub fn contains(self, position: LinePosition) -> bool {
        self.first <= position && position <= self.last
    }

    /// Whether the range is a single line.
    pub fn is_single(self) -> bool {
        self.first == self.last
    }

    /// The range's lines in `file`, top to bottom. Positions `file` doesn't have are
    /// skipped, as after the diff was recomputed.
    pub fn lines(self, file: &FileDiff) -> Vec<&DiffLine> {
        positions(file)
            .filter(|(position, _)| self.contains(*position))
            .map(|(_, line)| line)
            .collect()
    }

    /// The head-side line a comment on the range anchors to: the last line of the range
    /// in the new version, or for a range of removals only, the nearest line below it
    /// and then above it.
    pub fn anchor(self, file: &FileDiff) -> Option<u32> {
        let head = |line: &DiffLine| line.new_line.filter(|_| line.kind != LineKind::Removed);
        let within = positions(file)
            .filter(|(position, _)| self.contains(*position))
            .filter_map(|(_, line)| head(line))
            .last();
        let below = || {
            positions(file)
                .filter(|(position, _)| *position > self.last)
                .find_map(|(_, line)| head(line))
        };
        let above = || {
            positions(file)
                .filter(|(position, _)| *position < self.first)
                .filter_map(|(_, line)| head(line))
                .last()
        };
        within.or_else(below).or_else(above)
    }

    /// `Line 14` or `Lines 14–28`, by the numbers in the new version where the range
    /// has any, else by the old ones, said to be removed.
    pub fn label(self, file: &FileDiff) -> String {
        let lines = self.lines(file);
        let span =
            |numbers: Vec<u32>, one: &str, many: &str| match (numbers.first(), numbers.last()) {
                (Some(first), Some(last)) if first == last => Some(format!("{one} {first}")),
                (Some(first), Some(last)) => Some(format!("{many} {first}\u{2013}{last}")),
                _ => None,
            };
        let head = lines
            .iter()
            .filter(|line| line.kind != LineKind::Removed)
            .filter_map(|line| line.new_line)
            .collect();
        let base = lines.iter().filter_map(|line| line.old_line).collect();
        span(head, "Line", "Lines")
            .or_else(|| span(base, "Removed line", "Removed lines"))
            .unwrap_or_else(|| "Selected lines".into())
    }
}

/// Every line of `file` with its position, top to bottom.
fn positions(file: &FileDiff) -> impl Iterator<Item = (LinePosition, &DiffLine)> {
    file.hunks.iter().enumerate().flat_map(|(hunk_ix, hunk)| {
        hunk.lines
            .iter()
            .enumerate()
            .map(move |(line_ix, line)| (LinePosition::new(hunk_ix, line_ix), line))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use diff_model::{FileChange, Hunk};

    fn file() -> FileDiff {
        FileDiff::new(
            "src/a.rs",
            FileChange::Modified,
            vec![
                Hunk::new(
                    3,
                    3,
                    3,
                    3,
                    "",
                    vec![
                        DiffLine::new(LineKind::Context, Some(3), Some(3), "a"),
                        DiffLine::new(LineKind::Removed, Some(4), None, "b"),
                        DiffLine::new(LineKind::Removed, Some(5), None, "c"),
                        DiffLine::new(LineKind::Added, None, Some(4), "d"),
                        DiffLine::new(LineKind::Context, Some(6), Some(5), "e"),
                    ],
                ),
                Hunk::new(
                    20,
                    1,
                    19,
                    1,
                    "",
                    vec![DiffLine::new(LineKind::Context, Some(20), Some(19), "z")],
                ),
            ],
        )
    }

    fn range(a: (usize, usize), b: (usize, usize)) -> LineRange {
        LineRange::new(LinePosition::new(a.0, a.1), LinePosition::new(b.0, b.1))
    }

    #[test]
    fn a_range_reads_the_same_dragged_up_or_down() {
        assert_eq!(range((0, 3), (0, 1)), range((0, 1), (0, 3)));
        assert!(range((0, 3), (0, 1)).contains(LinePosition::new(0, 2)));
        assert!(!range((0, 3), (0, 1)).contains(LinePosition::new(0, 4)));
    }

    #[test]
    fn a_range_spans_hunks_and_lists_its_lines_in_order() {
        let file = file();
        let texts: Vec<&str> = range((1, 0), (0, 3))
            .lines(&file)
            .iter()
            .map(|line| line.text.as_str())
            .collect();
        assert_eq!(texts, ["d", "e", "z"]);
    }

    #[test]
    fn a_comment_anchors_to_the_last_head_line_or_the_nearest_one() {
        let file = file();
        assert_eq!(range((0, 0), (0, 3)).anchor(&file), Some(4));
        assert_eq!(
            range((0, 1), (0, 2)).anchor(&file),
            Some(4),
            "removals only: the line below"
        );
        assert_eq!(range((1, 0), (1, 0)).anchor(&file), Some(19));
    }

    #[test]
    fn labels_use_head_numbers_and_say_when_lines_are_removed() {
        let file = file();
        assert_eq!(range((0, 0), (0, 4)).label(&file), "Lines 3\u{2013}5");
        assert_eq!(range((0, 3), (0, 3)).label(&file), "Line 4");
        assert_eq!(
            range((0, 1), (0, 2)).label(&file),
            "Removed lines 4\u{2013}5"
        );
    }
}

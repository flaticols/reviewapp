use diff_model::{DiffLine, LineKind};

/// One row of the split layout: indices into the hunk's lines for the base side and the
/// head side. Context lines fill both; a run of removals pairs with the run of additions
/// that follows it, and the shorter run leaves blanks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitRow {
    pub left: Option<usize>,
    pub right: Option<usize>,
}

/// Pairs a hunk's lines into split rows.
pub fn split_rows(lines: &[DiffLine]) -> Vec<SplitRow> {
    let mut rows = Vec::new();
    let mut ix = 0;
    while ix < lines.len() {
        if lines[ix].kind == LineKind::Context {
            rows.push(SplitRow {
                left: Some(ix),
                right: Some(ix),
            });
            ix += 1;
            continue;
        }
        let removed_start = ix;
        while ix < lines.len() && lines[ix].kind == LineKind::Removed {
            ix += 1;
        }
        let added_start = ix;
        while ix < lines.len() && lines[ix].kind == LineKind::Added {
            ix += 1;
        }
        let removed = added_start - removed_start;
        let added = ix - added_start;
        for offset in 0..removed.max(added) {
            rows.push(SplitRow {
                left: (offset < removed).then_some(removed_start + offset),
                right: (offset < added).then_some(added_start + offset),
            });
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: LineKind) -> DiffLine {
        DiffLine::new(kind, None, None, "")
    }

    #[test]
    fn removals_pair_with_following_additions() {
        let lines = vec![
            line(LineKind::Context),
            line(LineKind::Removed),
            line(LineKind::Added),
            line(LineKind::Added),
            line(LineKind::Context),
        ];
        assert_eq!(
            split_rows(&lines),
            vec![
                SplitRow {
                    left: Some(0),
                    right: Some(0)
                },
                SplitRow {
                    left: Some(1),
                    right: Some(2)
                },
                SplitRow {
                    left: None,
                    right: Some(3)
                },
                SplitRow {
                    left: Some(4),
                    right: Some(4)
                },
            ]
        );
    }
}

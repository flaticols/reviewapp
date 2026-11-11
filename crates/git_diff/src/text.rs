use diff_model::{DiffLine, FileChange, FileDiff, Hunk, LineKind};
use gix::diff::blob::{Algorithm, Diff, InternedInput};

/// Unchanged lines kept around each change, as `git diff` does by default.
pub const CONTEXT_LINES: u32 = 3;

/// Diffs two versions of a file line by line with the histogram algorithm and groups
/// the changes into hunks with [`CONTEXT_LINES`] of context, merging hunks whose
/// context would overlap.
pub fn diff_texts(path: &str, old: &str, new: &str) -> FileDiff {
    let change = match (old.is_empty(), new.is_empty()) {
        (true, false) => FileChange::Added,
        (false, true) => FileChange::Deleted,
        _ => FileChange::Modified,
    };
    let input = InternedInput::new(old, new);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);

    let old_lines: Vec<&str> = old.split_inclusive('\n').collect();
    let new_lines: Vec<&str> = new.split_inclusive('\n').collect();
    let changes: Vec<(std::ops::Range<u32>, std::ops::Range<u32>)> =
        diff.hunks().map(|hunk| (hunk.before, hunk.after)).collect();

    let mut hunks = Vec::new();
    let mut group_start = 0;
    while group_start < changes.len() {
        let mut group_end = group_start + 1;
        while group_end < changes.len()
            && changes[group_end].0.start - changes[group_end - 1].0.end <= 2 * CONTEXT_LINES
        {
            group_end += 1;
        }
        hunks.push(build_hunk(
            &changes[group_start..group_end],
            &old_lines,
            &new_lines,
        ));
        group_start = group_end;
    }
    FileDiff::new(path, change, hunks)
}

fn build_hunk(
    changes: &[(std::ops::Range<u32>, std::ops::Range<u32>)],
    old_lines: &[&str],
    new_lines: &[&str],
) -> Hunk {
    let (first_before, first_after) = &changes[0];
    let (last_before, _) = &changes[changes.len() - 1];
    let lead = first_before.start.min(CONTEXT_LINES);
    let old_start = first_before.start - lead;
    let new_start = first_after.start - lead;
    let old_end = (last_before.end + CONTEXT_LINES).min(old_lines.len() as u32);

    let mut lines = Vec::new();
    let mut old_ix = old_start;
    let mut new_ix = new_start;
    for (before, after) in changes {
        while old_ix < before.start {
            lines.push(context(old_lines, old_ix, new_ix));
            old_ix += 1;
            new_ix += 1;
        }
        for ix in before.clone() {
            lines.push(DiffLine::new(
                LineKind::Removed,
                Some(ix + 1),
                None,
                strip_newline(old_lines[ix as usize]),
            ));
        }
        for ix in after.clone() {
            lines.push(DiffLine::new(
                LineKind::Added,
                None,
                Some(ix + 1),
                strip_newline(new_lines[ix as usize]),
            ));
        }
        old_ix = before.end;
        new_ix = after.end;
    }
    while old_ix < old_end {
        lines.push(context(old_lines, old_ix, new_ix));
        old_ix += 1;
        new_ix += 1;
    }

    let old_len = lines.iter().filter(|l| l.kind != LineKind::Added).count() as u32;
    let new_len = lines.iter().filter(|l| l.kind != LineKind::Removed).count() as u32;
    // Unified diffs number an empty side from the line before it, so a pure insertion
    // at the top of a file reads `-0,0`.
    let old_start = if old_len == 0 {
        old_start
    } else {
        old_start + 1
    };
    let new_start = if new_len == 0 {
        new_start
    } else {
        new_start + 1
    };
    let section = section_heading(old_lines, old_start.saturating_sub(1));
    Hunk::new(old_start, old_len, new_start, new_len, section, lines)
}

fn context(old_lines: &[&str], old_ix: u32, new_ix: u32) -> DiffLine {
    DiffLine::new(
        LineKind::Context,
        Some(old_ix + 1),
        Some(new_ix + 1),
        strip_newline(old_lines[old_ix as usize]),
    )
}

fn strip_newline(line: &str) -> &str {
    line.strip_suffix('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .unwrap_or(line)
}

/// The nearest line above the hunk that starts at column zero with an identifier,
/// approximating git's default funcname heuristic.
fn section_heading(old_lines: &[&str], before_ix: u32) -> String {
    old_lines[..(before_ix as usize).min(old_lines.len())]
        .iter()
        .rev()
        .map(|line| strip_newline(line))
        .find(|line| {
            line.chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        })
        .map(|line| line.chars().take(80).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(range: std::ops::RangeInclusive<u32>) -> String {
        range.map(|n| format!("line {n}\n")).collect()
    }

    #[test]
    fn single_change_has_three_lines_of_context() {
        let old = numbered(1..=20);
        let new = old.replace("line 10\n", "line ten\n");
        let diff = diff_texts("a.txt", &old, &new);
        assert_eq!(diff.hunks.len(), 1);
        let hunk = &diff.hunks[0];
        assert_eq!(hunk.header(), "@@ -7,7 +7,7 @@ line 6");
        assert_eq!(diff.additions(), 1);
        assert_eq!(diff.deletions(), 1);
        assert_eq!(hunk.lines[3].kind, LineKind::Removed);
        assert_eq!(hunk.lines[3].old_line, Some(10));
        assert_eq!(hunk.lines[4].new_line, Some(10));
    }

    #[test]
    fn nearby_changes_merge_and_distant_changes_split() {
        let old = numbered(1..=40);
        let near = old
            .replace("line 5\n", "five\n")
            .replace("line 10\n", "ten\n");
        assert_eq!(diff_texts("a", &old, &near).hunks.len(), 1);
        let far = old
            .replace("line 5\n", "five\n")
            .replace("line 30\n", "thirty\n");
        assert_eq!(diff_texts("a", &old, &far).hunks.len(), 2);
    }

    #[test]
    fn insertion_shifts_new_line_numbers() {
        let old = numbered(1..=6);
        let new = old.replace("line 3\n", "line 3\ninserted\n");
        let hunk = &diff_texts("a", &old, &new).hunks[0];
        assert_eq!(hunk.header(), "@@ -1,6 +1,7 @@");
        let after = hunk.lines.iter().find(|l| l.text == "line 4").unwrap();
        assert_eq!((after.old_line, after.new_line), (Some(4), Some(5)));
    }

    #[test]
    fn new_file_is_one_added_hunk() {
        let diff = diff_texts("new.rs", "", "fn main() {}\n");
        assert_eq!(diff.change, FileChange::Added);
        assert_eq!(diff.hunks[0].header(), "@@ -0,0 +1,1 @@");
    }
}

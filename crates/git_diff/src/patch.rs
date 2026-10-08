use diff_model::{DiffLine, FileChange, FileDiff, Hunk, LineKind};

/// Parses the hunks of a unified diff for one file, as GitHub's pull request files API
/// returns them in `patch`. File headers (`diff --git`, `---`, `+++`) are skipped, and
/// `\ No newline at end of file` markers are dropped. Lines before the first hunk header
/// are ignored.
pub fn parse_patch(path: &str, change: FileChange, patch: &str) -> FileDiff {
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut old_line = 0;
    let mut new_line = 0;
    for line in patch.lines() {
        if let Some(header) = line.strip_prefix("@@ ") {
            if let Some(hunk) = parse_header(header) {
                old_line = hunk.old_start;
                new_line = hunk.new_start;
                hunks.push(hunk);
            }
            continue;
        }
        let Some(hunk) = hunks.last_mut() else {
            continue;
        };
        let (kind, text) = match line.as_bytes().first() {
            Some(b'+') => (LineKind::Added, &line[1..]),
            Some(b'-') => (LineKind::Removed, &line[1..]),
            Some(b' ') => (LineKind::Context, &line[1..]),
            Some(b'\\') => continue,
            None => (LineKind::Context, ""),
            Some(_) => continue,
        };
        let (old, new) = match kind {
            LineKind::Added => (None, Some(new_line)),
            LineKind::Removed => (Some(old_line), None),
            LineKind::Context => (Some(old_line), Some(new_line)),
        };
        if old.is_some() {
            old_line += 1;
        }
        if new.is_some() {
            new_line += 1;
        }
        hunk.lines.push(DiffLine::new(kind, old, new, text));
    }
    FileDiff::new(path, change, hunks)
}

/// Parses `-a,b +c,d @@ section` (the part after the leading `@@ `).
fn parse_header(header: &str) -> Option<Hunk> {
    let (ranges, section) = header.split_once(" @@")?;
    let (old, new) = ranges.split_once(' ')?;
    let (old_start, old_len) = parse_range(old.strip_prefix('-')?)?;
    let (new_start, new_len) = parse_range(new.strip_prefix('+')?)?;
    Some(Hunk::new(
        old_start,
        old_len,
        new_start,
        new_len,
        section.trim(),
        Vec::new(),
    ))
}

fn parse_range(range: &str) -> Option<(u32, u32)> {
    match range.split_once(',') {
        Some((start, len)) => Some((start.parse().ok()?, len.parse().ok()?)),
        None => Some((range.parse().ok()?, 1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "\
@@ -136,6 +136,9 @@ impl ReviewThread {
     pub fn apply_push(&mut self) {
-        self.blocks = blocks.collect();
+        for block in blocks {
+            self.insert(block);
+        }
         self.reanchor_threads(cx);
\\ No newline at end of file";

    #[test]
    fn numbers_lines_from_the_header() {
        let diff = parse_patch("thread.rs", FileChange::Modified, PATCH);
        let hunk = &diff.hunks[0];
        assert_eq!(hunk.header(), "@@ -136,6 +136,9 @@ impl ReviewThread {");
        assert_eq!(hunk.lines.len(), 6);
        assert_eq!(hunk.lines[1].old_line, Some(137));
        assert_eq!(hunk.lines[2].new_line, Some(137));
        assert_eq!(hunk.lines[5].old_line, Some(138));
        assert_eq!(hunk.lines[5].new_line, Some(140));
        assert_eq!((diff.additions(), diff.deletions()), (3, 1));
    }

    #[test]
    fn single_line_ranges_omit_the_length() {
        let diff = parse_patch("a", FileChange::Modified, "@@ -1 +1 @@\n-a\n+b");
        assert_eq!(diff.hunks[0].old_len, 1);
        assert_eq!(diff.hunks[0].new_len, 1);
    }
}

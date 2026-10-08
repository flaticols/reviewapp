//! Line-level diffs of single files, independent of where they were computed.

/// How a file changed between the base and head of a pull request.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileChange {
    Added,
    Modified,
    Deleted,
    Renamed,
}

/// Which side of a diff a line belongs to.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

/// One line of a hunk. Line numbers are 1-based; a removed line has no new number and
/// an added line has no old number.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DiffLine {
    pub kind: LineKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub text: String,
}

impl DiffLine {
    /// Builds a line; numbers follow [`DiffLine`]'s convention.
    pub fn new(
        kind: LineKind,
        old_line: Option<u32>,
        new_line: Option<u32>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            old_line,
            new_line,
            text: text.into(),
        }
    }
}

/// A contiguous region of change with its surrounding context.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Hunk {
    pub old_start: u32,
    pub old_len: u32,
    pub new_start: u32,
    pub new_len: u32,
    /// Text git prints after the range, usually the enclosing function or impl.
    pub section: String,
    pub lines: Vec<DiffLine>,
}

impl Hunk {
    /// Builds a hunk from its ranges, section heading and lines.
    pub fn new(
        old_start: u32,
        old_len: u32,
        new_start: u32,
        new_len: u32,
        section: impl Into<String>,
        lines: Vec<DiffLine>,
    ) -> Self {
        Self {
            old_start,
            old_len,
            new_start,
            new_len,
            section: section.into(),
            lines,
        }
    }

    /// The `@@ -a,b +c,d @@ section` header.
    pub fn header(&self) -> String {
        let mut header = format!(
            "@@ -{},{} +{},{} @@",
            self.old_start, self.old_len, self.new_start, self.new_len
        );
        if !self.section.is_empty() {
            header.push(' ');
            header.push_str(&self.section);
        }
        header
    }
}

/// The change to one file.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    pub change: FileChange,
    pub hunks: Vec<Hunk>,
    /// Set when the content is binary or too large to show line by line.
    pub is_binary: bool,
}

impl FileDiff {
    /// Builds a file diff.
    pub fn new(path: impl Into<String>, change: FileChange, hunks: Vec<Hunk>) -> Self {
        Self {
            path: path.into(),
            old_path: None,
            change,
            hunks,
            is_binary: false,
        }
    }

    /// Marks the diff as a rename from `old_path`.
    pub fn with_old_path(mut self, old_path: impl Into<String>) -> Self {
        self.old_path = Some(old_path.into());
        self.change = FileChange::Renamed;
        self
    }

    /// Marks the content as binary.
    pub fn with_binary(mut self, is_binary: bool) -> Self {
        self.is_binary = is_binary;
        self
    }

    /// Number of added lines across all hunks.
    pub fn additions(&self) -> usize {
        self.count(LineKind::Added)
    }

    /// Number of removed lines across all hunks.
    pub fn deletions(&self) -> usize {
        self.count(LineKind::Removed)
    }

    /// The final path component.
    pub fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// The file extension without its dot, if any.
    pub fn extension(&self) -> Option<&str> {
        self.file_name()
            .rsplit_once('.')
            .map(|(_, extension)| extension)
    }

    fn count(&self, kind: LineKind) -> usize {
        self.hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .filter(|line| line.kind == kind)
            .count()
    }
}

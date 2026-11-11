use std::fmt;
use std::path::{Path, PathBuf};

use diff_model::{FileChange, FileDiff};
use pr_model::RepoSlug;

use crate::diff_texts;

/// Larger blobs are reported as binary instead of being diffed line by line.
const MAX_DIFF_BYTES: usize = 2 * 1024 * 1024;

/// A failure reading a local clone, with a message suitable for the interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneError(String);

impl CloneError {
    fn new(context: &str, error: impl fmt::Display) -> Self {
        Self(format!("{context}: {error}"))
    }
}

impl fmt::Display for CloneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CloneError {}

/// One changed file between two trees, by blob.
struct BlobChange {
    path: String,
    /// The path before a rename.
    old_path: Option<String>,
    old: Option<gix::ObjectId>,
    new: Option<gix::ObjectId>,
}

impl BlobChange {
    fn new(path: String, old: Option<gix::ObjectId>, new: Option<gix::ObjectId>) -> Self {
        Self {
            path,
            old_path: None,
            old,
            new,
        }
    }
}

/// A git clone on disk, opened read-only through gix.
pub struct LocalClone {
    path: PathBuf,
    repo: gix::Repository,
}

impl LocalClone {
    /// Opens the repository at `path` (a work tree or a bare repository).
    pub fn open(path: &Path) -> Result<Self, CloneError> {
        let repo =
            gix::open(path).map_err(|error| CloneError::new("Couldn’t open the clone", error))?;
        Ok(Self {
            path: path.to_path_buf(),
            repo,
        })
    }

    /// The folder the clone was opened from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The GitHub repository the default fetch remote points at, when it is on GitHub.
    pub fn github_slug(&self) -> Option<RepoSlug> {
        let remote = self
            .repo
            .find_default_remote(gix::remote::Direction::Fetch)?
            .ok()?;
        let url = remote.url(gix::remote::Direction::Fetch)?;
        let host = url.host()?;
        if !host.ends_with("github.com") {
            return None;
        }
        let path = url.path.to_string();
        RepoSlug::parse(path.trim_start_matches('/'))
    }

    /// Whether `spec` (a branch, remote branch or commit) resolves in this clone.
    pub fn has_revision(&self, spec: &str) -> bool {
        self.repo.rev_parse_single(spec).is_ok()
    }

    /// The changes commit `sha` makes against its first parent. Fails for a root commit,
    /// which has no parent to compare with.
    pub fn diff_commit(&self, sha: &str) -> Result<Vec<FileDiff>, CloneError> {
        self.diff_revisions(&format!("{sha}^"), sha)
    }

    /// The pull request diff: changes from the merge base of `base` and `head` to `head`,
    /// as GitHub shows them. Both are revision specs such as `origin/main`.
    pub fn diff_revisions(&self, base: &str, head: &str) -> Result<Vec<FileDiff>, CloneError> {
        let mut files = Vec::new();
        for change in self.changes(base, head)? {
            let file = self.file_diff(&change.path, change.old, change.new)?;
            files.push(match change.old_path {
                Some(old_path) => file.with_old_path(old_path),
                None => file,
            });
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }

    /// Writes both sides of the pull request diff of `base` and `head` as plain files:
    /// each changed file as it was at the merge base under `base_dir`, and as it is at
    /// `head` under `head_dir`. Added files exist only under `head_dir`, deleted ones
    /// only under `base_dir`. Returns how many files changed.
    pub fn export_sides(
        &self,
        base: &str,
        head: &str,
        base_dir: &Path,
        head_dir: &Path,
    ) -> Result<usize, CloneError> {
        let changes = self.changes(base, head)?;
        for change in &changes {
            let old_path = change.old_path.as_deref().unwrap_or(&change.path);
            self.export_blob(change.old, &base_dir.join(old_path))?;
            self.export_blob(change.new, &head_dir.join(&change.path))?;
        }
        Ok(changes.len())
    }

    fn export_blob(&self, id: Option<gix::ObjectId>, destination: &Path) -> Result<(), CloneError> {
        let Some(id) = id else {
            return Ok(());
        };
        let object = self
            .repo
            .find_object(id)
            .map_err(|error| CloneError::new("Couldn’t read blob", error))?;
        let written = destination
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(destination, &object.data));
        written.map_err(|error| CloneError::new("Couldn’t write the file", error))
    }

    /// The blob-level changes from the merge base of `base` and `head` to `head`.
    fn changes(&self, base: &str, head: &str) -> Result<Vec<BlobChange>, CloneError> {
        let base_id = self.resolve(base)?;
        let head_id = self.resolve(head)?;
        let merge_base = self
            .repo
            .merge_base(base_id, head_id)
            .map_err(|error| CloneError::new("No common ancestor", error))?
            .detach();
        let old_tree = self.tree_of(merge_base)?;
        let new_tree = self.tree_of(head_id)?;
        let mut options = gix::diff::Options::default();
        options.track_path();
        let changes = self
            .repo
            .diff_tree_to_tree(&old_tree, &new_tree, options)
            .map_err(|error| CloneError::new("Couldn’t compare trees", error))?;

        let mut blobs = Vec::new();
        for change in changes {
            use gix::object::tree::diff::ChangeDetached as Change;
            let blob = match change {
                Change::Addition {
                    location,
                    entry_mode,
                    id,
                    ..
                } if entry_mode.is_blob() => BlobChange::new(location.to_string(), None, Some(id)),
                Change::Deletion {
                    location,
                    entry_mode,
                    id,
                    ..
                } if entry_mode.is_blob() => BlobChange::new(location.to_string(), Some(id), None),
                Change::Modification {
                    location,
                    entry_mode,
                    previous_id,
                    id,
                    ..
                } if entry_mode.is_blob() => {
                    BlobChange::new(location.to_string(), Some(previous_id), Some(id))
                }
                Change::Rewrite {
                    source_location,
                    location,
                    source_id,
                    entry_mode,
                    id,
                    ..
                } if entry_mode.is_blob() => BlobChange {
                    old_path: Some(source_location.to_string()),
                    ..BlobChange::new(location.to_string(), Some(source_id), Some(id))
                },
                _ => continue,
            };
            blobs.push(blob);
        }
        Ok(blobs)
    }

    fn resolve(&self, spec: &str) -> Result<gix::ObjectId, CloneError> {
        self.repo
            .rev_parse_single(spec)
            .map(|id| id.detach())
            .map_err(|error| CloneError::new(&format!("Unknown revision {spec}"), error))
    }

    fn tree_of(&self, commit: gix::ObjectId) -> Result<gix::Tree<'_>, CloneError> {
        self.repo
            .find_commit(commit)
            .and_then(|commit| commit.tree())
            .map_err(|error| CloneError::new("Couldn’t read commit", error))
    }

    fn blob_text(&self, id: Option<gix::ObjectId>) -> Result<Option<String>, CloneError> {
        let Some(id) = id else {
            return Ok(Some(String::new()));
        };
        let object = self
            .repo
            .find_object(id)
            .map_err(|error| CloneError::new("Couldn’t read blob", error))?;
        if object.data.len() > MAX_DIFF_BYTES || object.data.contains(&0) {
            return Ok(None);
        }
        Ok(String::from_utf8(object.data.clone()).ok())
    }

    fn file_diff(
        &self,
        path: &str,
        old: Option<gix::ObjectId>,
        new: Option<gix::ObjectId>,
    ) -> Result<FileDiff, CloneError> {
        let change = match (old, new) {
            (None, _) => FileChange::Added,
            (_, None) => FileChange::Deleted,
            _ => FileChange::Modified,
        };
        match (self.blob_text(old)?, self.blob_text(new)?) {
            (Some(old), Some(new)) => {
                let mut diff = diff_texts(path, &old, &new);
                diff.change = change;
                Ok(diff)
            }
            _ => Ok(FileDiff::new(path, change, Vec::new()).with_binary(true)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?}");
    }

    #[test]
    fn diffs_a_branch_against_its_merge_base() {
        let dir = std::env::temp_dir().join(format!("git_diff_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        let body: String = (1..=20).map(|n| format!("line {n}\n")).collect();
        std::fs::write(dir.join("a.txt"), &body).unwrap();
        std::fs::write(dir.join("gone.txt"), "bye\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "base"]);
        git(&dir, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(dir.join("a.txt"), body.replace("line 10\n", "ten\n")).unwrap();
        std::fs::write(dir.join("new.rs"), "fn main() {}\n").unwrap();
        std::fs::remove_file(dir.join("gone.txt")).unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "feature"]);
        git(&dir, &["checkout", "-q", "main"]);
        std::fs::write(dir.join("other.txt"), "main moved on\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "main"]);
        git(
            &dir,
            &[
                "remote",
                "add",
                "origin",
                "git@github.com:flaticols/switchfold.git",
            ],
        );

        let clone = LocalClone::open(&dir).unwrap();
        assert_eq!(
            clone.github_slug().map(|slug| slug.to_string()),
            Some("flaticols/switchfold".into())
        );
        let files = clone.diff_revisions("main", "feature").unwrap();
        let summary: Vec<(String, FileChange, usize, usize)> = files
            .iter()
            .map(|f| (f.path.clone(), f.change, f.additions(), f.deletions()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("a.txt".into(), FileChange::Modified, 1, 1),
                ("gone.txt".into(), FileChange::Deleted, 0, 1),
                ("new.rs".into(), FileChange::Added, 1, 0),
            ]
        );

        let sides = dir.join("sides");
        let changed = clone
            .export_sides("main", "feature", &sides.join("base"), &sides.join("head"))
            .unwrap();
        assert_eq!(changed, 3);
        assert_eq!(
            std::fs::read_to_string(sides.join("base/a.txt")).unwrap(),
            body
        );
        assert!(
            std::fs::read_to_string(sides.join("head/a.txt"))
                .unwrap()
                .contains("ten\n")
        );
        assert!(sides.join("base/gone.txt").exists() && !sides.join("head/gone.txt").exists());
        assert!(!sides.join("base/new.rs").exists() && sides.join("head/new.rs").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// Diffs a real clone: `CLONE=… BASE=origin/main HEAD_REV=<sha> cargo test -p git_diff -- --ignored live`.
    #[test]
    #[ignore = "reads a clone named by environment variables"]
    fn live_clone_diff() {
        let clone = std::env::var("CLONE").expect("CLONE");
        let base = std::env::var("BASE").unwrap_or_else(|_| "origin/main".into());
        let head = std::env::var("HEAD_REV").expect("HEAD_REV");
        let started = std::time::Instant::now();
        let files = LocalClone::open(Path::new(&clone))
            .unwrap()
            .diff_revisions(&base, &head)
            .unwrap();
        let (adds, dels): (usize, usize) = files
            .iter()
            .fold((0, 0), |(a, d), f| (a + f.additions(), d + f.deletions()));
        eprintln!(
            "{} files +{adds} -{dels} in {:?}",
            files.len(),
            started.elapsed()
        );
        assert!(!files.is_empty());
    }
}

use std::fs;
use std::path::{Path, PathBuf};

/// Where a file sits in git terms, for finding the pull request it belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Located {
    /// The work tree the file is in: a clone, or a worktree of one.
    pub work_tree: PathBuf,
    /// The main clone, which settings map to a repository.
    pub clone: PathBuf,
    /// The file's path relative to `work_tree`, with `/` separators.
    pub relative: String,
    /// The checked-out branch, unless HEAD is detached.
    pub branch: Option<String>,
    /// The pull request number in a work tree Switchfold names `<clone>-pr-<n>`.
    pub number_hint: Option<u64>,
}

/// Finds the work tree, clone and branch for `file`, an absolute path.
pub fn locate(file: &Path) -> Option<Located> {
    let work_tree = file
        .ancestors()
        .skip(1)
        .find(|dir| dir.join(".git").exists())?
        .to_path_buf();
    let relative = file
        .strip_prefix(&work_tree)
        .ok()?
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    let dot_git = work_tree.join(".git");
    let (git_dir, clone) = if dot_git.is_dir() {
        (dot_git, work_tree.clone())
    } else {
        // A linked worktree's `.git` is a file: `gitdir: <clone>/.git/worktrees/<name>`.
        let text = fs::read_to_string(&dot_git).ok()?;
        let git_dir = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
        let git_dir = if git_dir.is_relative() {
            work_tree.join(git_dir)
        } else {
            git_dir
        };
        let clone = git_dir
            .ancestors()
            .find(|dir| dir.file_name().is_some_and(|name| name == ".git"))
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| work_tree.clone());
        (git_dir, clone)
    };
    let branch = fs::read_to_string(git_dir.join("HEAD"))
        .ok()
        .and_then(|head| {
            head.trim()
                .strip_prefix("ref: refs/heads/")
                .map(str::to_string)
        });
    let number_hint = work_tree
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.rsplit_once("-pr-"))
        .and_then(|(_, number)| number.parse().ok());
    Some(Located {
        work_tree,
        clone,
        relative,
        branch,
        number_hint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_clone_behind_a_review_worktree() {
        let root = std::env::temp_dir().join(format!("zed-bridge-locate-{}", std::process::id()));
        fs::remove_dir_all(&root).ok();
        let clone = root.join("switchfold");
        fs::create_dir_all(clone.join(".git/worktrees/switchfold-pr-482")).unwrap();
        fs::write(clone.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        fs::write(
            clone.join(".git/worktrees/switchfold-pr-482/HEAD"),
            "0123abcd\n",
        )
        .unwrap();
        let tree = root.join("review-worktrees/switchfold-pr-482");
        fs::create_dir_all(tree.join("src")).unwrap();
        fs::write(
            tree.join(".git"),
            format!(
                "gitdir: {}\n",
                clone.join(".git/worktrees/switchfold-pr-482").display()
            ),
        )
        .unwrap();

        let found = locate(&tree.join("src/lib.rs")).unwrap();
        assert_eq!(found.clone, clone);
        assert_eq!(found.relative, "src/lib.rs");
        assert_eq!(found.branch, None);
        assert_eq!(found.number_hint, Some(482));

        let in_clone = locate(&clone.join("README.md")).unwrap();
        assert_eq!(in_clone.clone, clone);
        assert_eq!(in_clone.branch.as_deref(), Some("main"));
        assert_eq!(in_clone.number_hint, None);
        fs::remove_dir_all(&root).ok();
    }
}

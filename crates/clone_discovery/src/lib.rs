//! Finding local clones of GitHub repositories in the folders developers usually keep
//! them in, so pull requests can be mapped to working trees without setup.

use std::path::{Path, PathBuf};

use git_diff::LocalClone;
use pr_model::RepoSlug;

/// Folders under the home directory searched for clones, two levels deep.
pub const SEARCH_ROOTS: [&str; 9] = [
    "src",
    "code",
    "dev",
    "Developer",
    "Projects",
    "projects",
    "work",
    "repos",
    "Documents/GitHub",
];

/// Every GitHub clone found under `home`'s usual source folders, with its repository.
/// Reads only git metadata; call it off the UI thread.
pub fn discover(home: &Path) -> Vec<(RepoSlug, PathBuf)> {
    let mut found = Vec::new();
    for root in SEARCH_ROOTS {
        visit(&home.join(root), 2, &mut found);
    }
    // When a repository is cloned twice, prefer the folder named after it.
    found.sort_by_key(|(slug, path)| {
        let named_after = path
            .file_name()
            .is_some_and(|name| name == slug.name().as_str());
        (slug.clone(), !named_after, path.clone())
    });
    found.dedup_by(|a, b| a.0 == b.0);
    found
}

fn visit(dir: &Path, depth: u32, found: &mut Vec<(RepoSlug, PathBuf)>) {
    // A worktree's `.git` is a file pointing at its main clone; only main clones count.
    if dir.join(".git").is_dir() {
        if let Some(slug) = LocalClone::open(dir)
            .ok()
            .and_then(|clone| clone.github_slug())
        {
            found.push((slug, dir.to_path_buf()));
        }
        return;
    }
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let hidden = path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with('.'));
        if !hidden && entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            visit(&path, depth - 1, found);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn finds_clones_two_levels_down() {
        let home = std::env::temp_dir().join(format!("clone_discovery_{}", std::process::id()));
        let repo = home.join("src/team/app");
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(&repo)
                    .status()
                    .unwrap()
                    .success()
            )
        };
        git(&["init", "-q"]);
        git(&["remote", "add", "origin", "https://github.com/acme/app.git"]);
        git(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "init",
        ]);
        let worktree = home.join("src/team/app-feature");
        git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            &worktree.to_string_lossy(),
        ]);
        let found = discover(&home);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1, repo);
        assert_eq!(found[0].0.to_string(), "acme/app");
        std::fs::remove_dir_all(&home).unwrap();
    }
}

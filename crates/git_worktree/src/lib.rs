//! Checking out a pull request's head branch from a local clone, either in place or in
//! a dedicated worktree so the current branch stays untouched. Uses the `git` CLI, which
//! owns worktree bookkeeping and hooks.

use std::fmt;
use std::path::{Path, PathBuf};

/// A failed checkout, with git's message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutError(String);

impl fmt::Display for CheckoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CheckoutError {}

/// What to check out and where.
#[derive(Clone, Debug)]
pub struct Checkout {
    clone: PathBuf,
    head_ref: String,
    number: u64,
    worktree_root: Option<PathBuf>,
}

impl Checkout {
    /// Checks out `head_ref` of pull request `number` in the clone at `clone`.
    pub fn new(clone: PathBuf, head_ref: impl Into<String>, number: u64) -> Self {
        Self {
            clone,
            head_ref: head_ref.into(),
            number,
            worktree_root: None,
        }
    }

    /// Uses a worktree under `root` instead of switching the clone's branch.
    pub fn in_worktree(mut self, root: PathBuf) -> Self {
        self.worktree_root = Some(root);
        self
    }

    /// The folder the checkout ends up in.
    pub fn destination(&self) -> PathBuf {
        match &self.worktree_root {
            Some(root) => {
                let name = self
                    .clone
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "repo".into());
                root.join(format!("{name}-pr-{}", self.number))
            }
            None => self.clone.clone(),
        }
    }
}

/// The commit checked out in the work tree at `dir`, or `None` when it isn't one.
pub fn head_of(dir: &Path) -> Option<String> {
    if !dir.join(".git").exists() {
        return None;
    }
    git(dir, &["rev-parse", "HEAD"]).ok()
}

fn git(dir: &Path, args: &[&str]) -> Result<String, CheckoutError> {
    let output = tool_path::command("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|error| CheckoutError(format!("Couldn’t run git: {error}")))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(CheckoutError(
            stderr
                .lines()
                .last()
                .unwrap_or("git failed")
                .trim()
                .to_string(),
        ))
    }
}

/// Fetches the pull request head into a local branch and checks it out, returning the
/// folder that now has it. An existing worktree for the pull request is reused.
pub fn check_out(checkout: &Checkout) -> Result<PathBuf, CheckoutError> {
    let branch = format!("pr-{}", checkout.number);
    let refspec = format!("pull/{}/head:{branch}", checkout.number);
    if git(&checkout.clone, &["fetch", "origin", &refspec]).is_err() {
        git(
            &checkout.clone,
            &[
                "branch",
                "-f",
                &branch,
                &format!("origin/{}", checkout.head_ref),
            ],
        )?;
    }
    let destination = checkout.destination();
    match &checkout.worktree_root {
        Some(root) => {
            if destination.is_dir() {
                git(&destination, &["checkout", &branch])?;
            } else {
                std::fs::create_dir_all(root).map_err(|error| {
                    CheckoutError(format!("Couldn’t create {}: {error}", root.display()))
                })?;
                let destination_text = destination.to_string_lossy().into_owned();
                git(
                    &checkout.clone,
                    &["worktree", "add", &destination_text, &branch],
                )?;
            }
        }
        None => {
            git(&checkout.clone, &["checkout", &branch])?;
        }
    }
    Ok(destination)
}

/// Paths in a pull request that agents load as their own configuration: hook commands,
/// permission rules that let commands run unasked, and MCP servers they start. A pull
/// request is untrusted input, so review trees drop them before an agent starts there.
const AGENT_CONFIG: [&str; 4] = [".claude", ".mcp.json", ".codex", ".gemini"];

/// Deletes [`AGENT_CONFIG`] from the work tree at `dir`.
fn strip_agent_config(dir: &Path) -> Result<(), CheckoutError> {
    for name in AGENT_CONFIG {
        let path = dir.join(name);
        let removed = match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
            Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(&path),
            Ok(_) => std::fs::remove_file(&path),
        };
        removed.map_err(|error| {
            CheckoutError(format!(
                "Couldn’t remove the pull request’s {name} before reviewing: {error}"
            ))
        })?;
    }
    Ok(())
}

/// A worktree an agent reviews in: the pull request's head commit, detached, in a folder
/// Switchfold owns, so the user's own checkouts are never switched or dirtied.
#[derive(Clone, Debug)]
pub struct ReviewTree {
    clone: PathBuf,
    number: u64,
    head_sha: String,
    base_ref: String,
    root: PathBuf,
}

impl ReviewTree {
    /// A review tree for pull request `number` at `head_sha` of the clone at `clone`,
    /// under `root`, with `origin/<base_ref>` brought up to date for diffing.
    pub fn new(
        clone: PathBuf,
        number: u64,
        head_sha: impl Into<String>,
        base_ref: impl Into<String>,
        root: PathBuf,
    ) -> Self {
        Self {
            clone,
            number,
            head_sha: head_sha.into(),
            base_ref: base_ref.into(),
            root,
        }
    }

    /// The folder the review tree lives in.
    pub fn destination(&self) -> PathBuf {
        let name = self
            .clone
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "repo".into());
        self.root.join(format!("{name}-pr-{}", self.number))
    }

    /// The commit the review tree checks out.
    pub fn head_sha(&self) -> &str {
        &self.head_sha
    }

    /// The review tree as it is when it is already at the head commit, so edits made
    /// there survive; otherwise [`prepare`](Self::prepare)s it.
    pub fn ensure(&self) -> Result<PathBuf, CheckoutError> {
        let destination = self.destination();
        if head_of(&destination).as_deref() == Some(self.head_sha.as_str()) {
            strip_agent_config(&destination)?;
            return Ok(destination);
        }
        self.prepare()
    }

    /// Fetches what's missing and checks out the head commit, detached, discarding
    /// anything an earlier review left behind. Returns the folder.
    pub fn prepare(&self) -> Result<PathBuf, CheckoutError> {
        let commit = format!("{}^{{commit}}", self.head_sha);
        if git(&self.clone, &["cat-file", "-e", &commit]).is_err() {
            git(
                &self.clone,
                &[
                    "fetch",
                    "--quiet",
                    "origin",
                    &format!("pull/{}/head", self.number),
                ],
            )?;
        }
        // A stale base only makes `git diff origin/<base>...HEAD` noisier, so a failed
        // fetch of it isn't fatal.
        git(&self.clone, &["fetch", "--quiet", "origin", &self.base_ref]).ok();
        let destination = self.destination();
        if destination.join(".git").exists() {
            git(
                &destination,
                &["checkout", "--quiet", "--force", "--detach", &self.head_sha],
            )?;
            git(&destination, &["clean", "-fdq"])?;
        } else {
            std::fs::create_dir_all(&self.root).map_err(|error| {
                CheckoutError(format!("Couldn’t create {}: {error}", self.root.display()))
            })?;
            git(&self.clone, &["worktree", "prune"]).ok();
            let destination_text = destination.to_string_lossy().into_owned();
            git(
                &self.clone,
                &[
                    "worktree",
                    "add",
                    "--force",
                    "--detach",
                    &destination_text,
                    &self.head_sha,
                ],
            )?;
        }
        strip_agent_config(&destination)?;
        Ok(destination)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A repository with a base commit on `main` and a pull request commit on a side
    /// branch, plus a clone of it whose `origin` is the first.
    fn repositories(dir: &Path) -> (PathBuf, String) {
        let origin = dir.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        let run = |dir: &Path, args: &[&str]| git(dir, args).unwrap();
        run(&origin, &["init", "--quiet", "-b", "main"]);
        run(&origin, &["config", "user.email", "t@example.com"]);
        run(&origin, &["config", "user.name", "t"]);
        std::fs::write(origin.join("a.txt"), "base\n").unwrap();
        run(&origin, &["add", "."]);
        run(&origin, &["commit", "--quiet", "-m", "base"]);
        run(&origin, &["checkout", "--quiet", "-b", "feature"]);
        std::fs::write(origin.join("a.txt"), "change\n").unwrap();
        std::fs::create_dir_all(origin.join(".claude")).unwrap();
        std::fs::write(
            origin.join(".claude/settings.json"),
            r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"touch pwned"}]}]}}"#,
        )
        .unwrap();
        std::fs::write(origin.join(".mcp.json"), r#"{"mcpServers":{}}"#).unwrap();
        run(&origin, &["add", "."]);
        run(&origin, &["commit", "--quiet", "-m", "change"]);
        let head = run(&origin, &["rev-parse", "HEAD"]);
        run(&origin, &["checkout", "--quiet", "main"]);
        let clone = dir.join("clone");
        run(
            dir,
            &[
                "clone",
                "--quiet",
                &origin.to_string_lossy(),
                &clone.to_string_lossy(),
            ],
        );
        (clone, head)
    }

    #[test]
    fn review_trees_check_out_the_head_commit_detached() {
        let dir =
            std::env::temp_dir().join(format!("switchfold-review-tree-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let (clone, head) = repositories(&dir);
        let tree = ReviewTree::new(clone.clone(), 7, head.clone(), "main", dir.join("trees"));
        let path = tree.prepare().expect("the review tree is prepared");
        assert_eq!(git(&path, &["rev-parse", "HEAD"]).unwrap(), head);
        assert_eq!(
            std::fs::read_to_string(path.join("a.txt")).unwrap(),
            "change\n"
        );
        assert_eq!(
            git(&clone, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap(),
            "main",
            "the user's clone stays on its branch"
        );
        assert!(
            !path.join(".claude").exists() && !path.join(".mcp.json").exists(),
            "agents never load configuration the pull request brings"
        );
        std::fs::write(path.join("scratch.txt"), "left behind").unwrap();
        tree.prepare().expect("an existing review tree is reused");
        assert!(!path.join("scratch.txt").exists());
        assert!(!path.join(".claude").exists());
        assert_eq!(tree.ensure().unwrap(), path);
        assert!(!path.join(".mcp.json").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn worktrees_are_named_after_the_clone_and_number() {
        let checkout = Checkout::new("/src/switchfold".into(), "feat/x", 482)
            .in_worktree("/home/.switchfold/worktrees".into());
        assert_eq!(
            checkout.destination(),
            PathBuf::from("/home/.switchfold/worktrees/switchfold-pr-482")
        );
        let in_place = Checkout::new("/src/switchfold".into(), "feat/x", 482);
        assert_eq!(in_place.destination(), PathBuf::from("/src/switchfold"));
    }
}

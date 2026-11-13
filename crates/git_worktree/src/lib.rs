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

/// Squashes a pull request's commits into one and force-pushes it to its branch.
#[derive(Clone, Debug)]
pub struct Squash {
    clone: PathBuf,
    number: u64,
    head_sha: String,
    head_ref: String,
    base_ref: String,
    message: String,
}

impl Squash {
    /// Squashes pull request `number` of the clone at `clone`, whose branch `head_ref`
    /// must still be at `head_sha`, onto its merge base with `base_ref`, as one commit
    /// with `message`.
    pub fn new(
        clone: PathBuf,
        number: u64,
        head_sha: impl Into<String>,
        head_ref: impl Into<String>,
        base_ref: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            clone,
            number,
            head_sha: head_sha.into(),
            head_ref: head_ref.into(),
            base_ref: base_ref.into(),
            message: message.into(),
        }
    }

    /// Does it in a temporary worktree, so no checkout of the user's changes, and
    /// pushes with a lease on `head_sha`: if anyone pushed since, nothing is
    /// overwritten. Returns the new head commit.
    pub fn run(&self) -> Result<String, CheckoutError> {
        if self.message.trim().is_empty() {
            return Err(CheckoutError("The commit message is empty.".into()));
        }
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
        git(&self.clone, &["fetch", "--quiet", "origin", &self.base_ref])?;
        let scratch = std::env::temp_dir().join(format!(
            "switchfold-squash-{}-{}",
            self.number,
            std::process::id()
        ));
        let scratch_text = scratch.to_string_lossy().into_owned();
        git(&self.clone, &["worktree", "prune"]).ok();
        git(
            &self.clone,
            &[
                "worktree",
                "add",
                "--force",
                "--detach",
                &scratch_text,
                &self.head_sha,
            ],
        )?;
        let result = (|| {
            let base = git(
                &scratch,
                &["merge-base", &format!("origin/{}", self.base_ref), "HEAD"],
            )?;
            git(&scratch, &["reset", "--soft", &base])?;
            git(
                &scratch,
                &["commit", "--quiet", "--no-verify", "-m", &self.message],
            )?;
            let squashed = git(&scratch, &["rev-parse", "HEAD"])?;
            git(
                &scratch,
                &[
                    "push",
                    "--quiet",
                    &format!(
                        "--force-with-lease=refs/heads/{}:{}",
                        self.head_ref, self.head_sha
                    ),
                    "origin",
                    &format!("HEAD:refs/heads/{}", self.head_ref),
                ],
            )?;
            Ok(squashed)
        })();
        git(
            &self.clone,
            &["worktree", "remove", "--force", &scratch_text],
        )
        .ok();
        result
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

/// A new worktree on a pull request's head branch, in a folder the user picks. The local
/// branch is named after the head branch, so pushing from the worktree updates the pull
/// request.
#[derive(Clone, Debug)]
pub struct NewWorktree {
    clone: PathBuf,
    head_ref: String,
    number: u64,
    path: PathBuf,
}

impl NewWorktree {
    /// A worktree of the clone at `clone` on `head_ref` of pull request `number`, at
    /// [`default_path`](Self::default_path) until [`at`](Self::at) picks another.
    pub fn new(clone: PathBuf, head_ref: impl Into<String>, number: u64) -> Self {
        let head_ref = head_ref.into();
        let path = Self::default_path(&clone, &head_ref);
        Self {
            clone,
            head_ref,
            number,
            path,
        }
    }

    /// Puts the worktree at `path` instead.
    pub fn at(mut self, path: PathBuf) -> Self {
        self.path = path;
        self
    }

    /// `<clone parent>/<clone name>.<branch>`, beside the clone, with each `/` of the
    /// branch turned into `-` so the folder doesn't nest.
    pub fn default_path(clone: &Path, head_ref: &str) -> PathBuf {
        let name = clone
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "repo".into());
        let branch = head_ref.replace(['/', '\\'], "-");
        clone
            .parent()
            .unwrap_or(clone)
            .join(format!("{name}.{branch}"))
    }

    /// The folder the worktree goes in.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The branch the worktree checks out.
    pub fn branch(&self) -> &str {
        &self.head_ref
    }

    /// Fetches the pull request head and adds the worktree, returning its folder.
    ///
    /// Refuses a path that already exists. A local branch of the same name is moved up to
    /// the pull request when it is behind, kept when it is ahead, and refused when the two
    /// have diverged: nothing local is overwritten.
    pub fn create(&self) -> Result<PathBuf, CheckoutError> {
        if !self.path.is_absolute() {
            return Err(CheckoutError("Choose an absolute folder path.".into()));
        }
        if self.path.exists() {
            return Err(CheckoutError(format!(
                "{} already exists. Choose another folder.",
                self.path.display()
            )));
        }
        let pull = format!("pull/{}/head", self.number);
        if git(&self.clone, &["fetch", "--quiet", "origin", &pull]).is_err() {
            git(&self.clone, &["fetch", "--quiet", "origin", &self.head_ref])?;
        }
        let tip = git(&self.clone, &["rev-parse", "FETCH_HEAD^{commit}"])?;
        let branch = &self.head_ref;
        let local = format!("refs/heads/{branch}");
        let path_text = self.path.to_string_lossy().into_owned();
        git(&self.clone, &["worktree", "prune"]).ok();
        if git(&self.clone, &["rev-parse", "--verify", "--quiet", &local]).is_ok() {
            let behind = git(&self.clone, &["merge-base", "--is-ancestor", &local, &tip]).is_ok();
            let ahead = git(&self.clone, &["merge-base", "--is-ancestor", &tip, &local]).is_ok();
            if !ahead && !behind {
                return Err(CheckoutError(format!(
                    "Your local {branch} has diverged from the pull request. Reconcile it first."
                )));
            }
            git(&self.clone, &["worktree", "add", &path_text, branch])?;
            if behind && !ahead {
                git(&self.path, &["merge", "--quiet", "--ff-only", &tip])?;
            }
        } else {
            git(
                &self.clone,
                &["worktree", "add", "-b", branch, &path_text, &tip],
            )?;
            // Only a branch in the same repository has an upstream to track; a fork's
            // doesn't, and the worktree is still useful without one.
            git(
                &self.clone,
                &[
                    "branch",
                    "--quiet",
                    &format!("--set-upstream-to=origin/{branch}"),
                    branch,
                ],
            )
            .ok();
        }
        Ok(self.path.clone())
    }
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
    fn squashing_pushes_one_commit_with_a_lease() {
        let dir =
            std::env::temp_dir().join(format!("switchfold-squash-test-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let (clone, _) = repositories(&dir);
        let origin = dir.join("origin");
        let run = |dir: &Path, args: &[&str]| git(dir, args).unwrap();
        // A second commit on the branch, so there is something to squash.
        run(&origin, &["checkout", "--quiet", "feature"]);
        std::fs::write(origin.join("b.txt"), "more\n").unwrap();
        run(&origin, &["add", "."]);
        run(&origin, &["commit", "--quiet", "-m", "more"]);
        let head = run(&origin, &["rev-parse", "HEAD"]);
        run(&origin, &["checkout", "--quiet", "main"]);
        run(&clone, &["fetch", "--quiet", "origin", "feature"]);
        run(&clone, &["config", "user.email", "t@example.com"]);
        run(&clone, &["config", "user.name", "t"]);
        let squash = Squash::new(
            clone.clone(),
            7,
            head.clone(),
            "feature",
            "main",
            "One commit",
        );
        let squashed = squash.run().expect("squash and push");
        let pushed = run(&origin, &["rev-parse", "feature"]);
        assert_eq!(pushed, squashed);
        assert_eq!(run(&origin, &["rev-list", "--count", "main..feature"]), "1");
        assert_eq!(
            run(&origin, &["log", "-1", "--format=%s", "feature"]),
            "One commit"
        );
        assert_eq!(
            run(&origin, &["show", "feature:b.txt"]),
            "more",
            "the squashed commit keeps every change"
        );
        // The branch moved on: a squash from the old head is refused, not forced.
        let stale = Squash::new(clone.clone(), 7, head, "feature", "main", "Again");
        assert!(stale.run().is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn new_worktrees_default_beside_the_clone() {
        assert_eq!(
            NewWorktree::default_path(Path::new("/src/switchfold"), "feat/thread-view"),
            PathBuf::from("/src/switchfold.feat-thread-view")
        );
        let worktree = NewWorktree::new("/src/switchfold".into(), "main", 1);
        assert_eq!(worktree.path(), Path::new("/src/switchfold.main"));
        let moved = worktree.at("/elsewhere/tree".into());
        assert_eq!(moved.path(), Path::new("/elsewhere/tree"));
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("switchfold-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn new_worktrees_check_out_the_head_branch() {
        let dir = scratch("new-worktree");
        let (clone, head) = repositories(&dir);
        let worktree = NewWorktree::new(clone.clone(), "feature", 7);
        let path = worktree.create().expect("the worktree is created");
        assert_eq!(path, dir.join("clone.feature"));
        assert_eq!(git(&path, &["rev-parse", "HEAD"]).unwrap(), head);
        assert_eq!(
            git(&path, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap(),
            "feature"
        );
        assert_eq!(
            git(&path, &["rev-parse", "--abbrev-ref", "feature@{upstream}"]).unwrap(),
            "origin/feature"
        );
        assert_eq!(
            git(&clone, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap(),
            "main",
            "the clone stays on its branch"
        );
        let again = worktree.create().unwrap_err();
        assert!(again.to_string().contains("already exists"), "{again}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn new_worktrees_move_a_stale_branch_up_but_never_a_diverged_one() {
        let dir = scratch("new-worktree-branch");
        let (clone, head) = repositories(&dir);
        let run = |dir: &Path, args: &[&str]| git(dir, args).unwrap();
        run(&clone, &["branch", "feature", "main"]);
        let path = NewWorktree::new(clone.clone(), "feature", 7)
            .at(dir.join("behind"))
            .create()
            .expect("a branch behind the pull request is fast-forwarded");
        assert_eq!(git(&path, &["rev-parse", "HEAD"]).unwrap(), head);
        run(&clone, &["worktree", "remove", &path.to_string_lossy()]);

        run(&clone, &["config", "user.email", "t@example.com"]);
        run(&clone, &["config", "user.name", "t"]);
        run(&clone, &["checkout", "--quiet", "-B", "feature", "main"]);
        std::fs::write(clone.join("local.txt"), "mine\n").unwrap();
        run(&clone, &["add", "."]);
        run(&clone, &["commit", "--quiet", "-m", "local"]);
        let local = run(&clone, &["rev-parse", "HEAD"]);
        run(&clone, &["checkout", "--quiet", "main"]);
        let error = NewWorktree::new(clone.clone(), "feature", 7)
            .at(dir.join("diverged"))
            .create()
            .unwrap_err();
        assert!(error.to_string().contains("diverged"), "{error}");
        assert_eq!(run(&clone, &["rev-parse", "feature"]), local);
        assert!(!dir.join("diverged").exists());
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

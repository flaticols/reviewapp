---
title: Check out a pull request in a new worktree
phase: done
session: 63ebd6b8-aeba-4dc6-84e1-44fe795dd193
ticket:
updated: 2026-11-13
---

## Goal
A pull request action, "Check out in new worktree…", that creates a git worktree on the pull
request's head branch at a path the user confirms. The path defaults to a sibling of the clone,
`<clone parent>/<clone name>.<branch>`.

## Approach
- New `git_worktree::NewWorktree`, separate from `Checkout`, which keeps its
  `~/.switchfold/worktrees/<repo>-pr-N` behaviour.
- `/` in the branch becomes `-` in the folder name (`feat/x` → `repo.feat-x`), as Worktrunk does,
  so the worktree doesn't nest a directory.
- The local branch is named after the head branch. An existing path is refused rather than reused,
  because the user can type any folder. An existing local branch is fast-forwarded when it is behind
  the pull request, used as-is when ahead, and refused when diverged; nothing is forced.
- The fetch tries `pull/N/head` first and falls back to `origin/<head_ref>`, like `check_out`.
- The dialog lives in `pr_actions_ui` next to Request review; the menu item sits after "Check out",
  so the list row menu and the title bar's ⋯ menu both get it.

## Tasks
### 1. Worktree creation in git_worktree
Files: crates/git_worktree/src/lib.rs
Accept: `cargo test -p git_worktree` passes, with tests for the default path, creation, an existing path and a diverged branch.
Verified: `cargo test -p git_worktree` → 6 passed (default path, create, existing path, stale and diverged branch).

### 2. Dialog and menu item
Files: crates/pr_actions_ui/src/new_worktree.rs, crates/pr_actions_ui/src/lib.rs, crates/pr_actions_ui/src/pull_request_menu.rs
Accept: `cargo test -p pr_actions_ui` passes; clippy clean on the changed crates.
Verified: `cargo test -p pr_actions_ui` → 2 passed; `cargo clippy --all-targets` on git_worktree, pr_actions_ui, workspace_ui, screenshots_ui → no warnings.

### 3. UI done gate
Files: crates/workspace_ui/tests/ui.rs, crates/screenshots_ui/src/main.rs
Accept: the UI test drives the row menu → dialog → Create, asserts painted state and the worktree on disk, fails when the menu item is removed; the `03f-new-worktree` scene renders; `cargo test -p workspace_ui` passes.
Verified: `the_row_menu_checks_a_pull_request_out_in_a_new_worktree` passes; breaking the default path fails with "the folder defaults to <clone>.<branch> beside the clone"; scene renders to target/screenshots/03f-new-worktree-dark.png; `cargo test -p workspace_ui` → 38 + 22 + 2 passed.

## Log
- 2026-11-13: started.
- 2026-11-13: the closing context menu took focus back from the path field; focus is deferred, as in ask.rs.
- 2026-11-13: notifications register no observable id, so the UI test checks the dialog closing and the worktree on disk, not the toast.

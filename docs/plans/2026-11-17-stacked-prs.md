---
title: Show GitHub stacks and the merge queue
phase: done
session: 230d57a3-c26a-4b25-a613-bde8fb3c7ee9
ticket:
updated: 2026-11-17
---

## Goal
A reviewer sees that a pull request is part of a GitHub stack: its position in the list, and the
whole stack in order on the review screen, with a click to any other member. A pull request that
waits in a merge queue says so. Both are read-only.

## Approach
- GitHub's GraphQL API has `PullRequest.stack`, `stackEntry` and `mergeQueueEntry`. They need no
  preview header on github.com. Checked against stack 580 of `github/gh-stack`: position 1 is the
  entry closest to the base branch, and merged entries stay in the stack.
- The list query asks only for scalars: `stack { number size }`, `stackEntry { position }` and
  `mergeQueueEntry { state }`. The stack's entries come with the detail query. Nesting them under
  fifty rows per inbox would multiply the query's cost for a marker that needs two numbers.
- Each stack entry carries its own title and state, because a member is often not in the loaded
  list (merged, or not in any inbox).
- The new fields are `Option`s with `#[serde(default)]`, so the cache format stays at 1.
- An older GitHub Enterprise Server has no `stack` field, and one unknown field fails the whole
  query. When GitHub answers that a field doesn't exist on `PullRequest`, the source drops the stack
  and merge queue fields, asks again and remembers it for the session.
- List: a "Stack 2/3" marker and an "In merge queue" marker in a row's meta line, not a new fixed lane. Most
  rows have neither.
- Review screen: a "Stack" section in the inspector, entries in merge order (1 first), the open
  pull request marked as current, the others open on click as "Depends on" rows do.
- The merge popover says when the pull request is in the merge queue. GitHub merges it from there, so
  the title bar's button reads "Queued" and the popover's merge button is off.
- Rejected: "Group by stack" in the list. A stack's members are rarely all in one inbox, so the
  group would show a partial stack; the inspector shows the whole one.
- Rejected: showing the queue position. No live entry was found to check whether it counts from 0
  or 1.
- Not done: merging a stack or adding to the merge queue from the app. GitHub's docs say the
  synchronous merge endpoint the app uses can't merge a stacked pull request; that needs the
  asynchronous endpoint and is a separate change.

## Tasks
### 1. Stack and merge queue from GitHub into the model
Files: crates/pr_model/src/lib.rs, crates/review_model/src/lib.rs, crates/github_api/src/gh_source.rs, crates/fixtures/src/lib.rs
Accept: `cargo test -p github_api -p pr_model -p review_model -p pr_cache -p fixtures` passes, with tests reading a stack, a null stack and a queue entry, and the retry without stack fields; the ignored live test reads a real stack.
Verified: `cargo test --workspace` → exit 0, 99 result lines ok, none failed; github_api 14 passed (a stack position and queue state are read, null and absent fields are `None`, members come in merge order, only a missing `stack` field drops the stack fields). `cargo test -p github_api -- --ignored live_stack live_unknown` → 2 passed: stack 580 of `github/gh-stack` is read through both queries, and asking github.com for a field it lacks still returns rows and stops the source asking. No real Enterprise Server was tried.

### 2. Stack and queue markers in the list
Files: crates/pr_list_ui/src/row.rs, crates/widgets_ui/src/assets.rs, crates/workspace_ui/tests/ui.rs
Accept: a UI test finds the painted markers by id on the stacked and queued rows and not on a plain row; it fails when the marker is dropped; `01-list` renders them.
Verified: `a_stack_shows_in_the_list_and_walks_from_the_inspector` and `a_queued_pull_request_says_so_in_the_list_and_the_merge_popover` pass; with the markers dropped they fail with "a stacked row paints its stack marker" and "a queued row paints its merge queue marker". Scene: target/screenshots/01-list-dark.png and -light.png.

### 3. Stack section on the review screen and the queue line in the merge popover
Files: crates/review_thread_ui/src/inspector.rs, crates/review_thread_ui/src/merge.rs, crates/workspace_ui/tests/ui.rs, crates/screenshots_ui/src/main.rs
Accept: a UI test asserts the painted entries and their order, clicks another entry and sees that pull request open; it fails when the section is dropped; a scene renders it; `cargo test -p workspace_ui` and `cargo clippy --workspace --all-targets` pass.
Verified: the same two tests click row #470, read the four entries and their order, click #482 and see it open with the mark moved, then click #468, a merged member the list doesn't hold, and see its review load; with the section dropped the first fails with "the inspector lists the stack's pull requests". The queue test fails with "the merge popover reports the queue" (left: "Ready to merge."), "the queue merges it, so Merge is off" (left: Merged) and "the title bar's Merge button says the pull request is queued" (left: "Merge") when each is broken. Scenes: target/screenshots/02-thread-dark.png, target/screenshots/03g-merge-queue-dark.png. `cargo test --workspace` → exit 0 with `tests/ui.rs` 44 passed; `cargo clippy --workspace --all-targets` → no code warnings; `git diff --check` clean.

## Log
- 2026-11-17: started. Branched from local `main`, not `origin/main`: the milestone commit this
  builds on (0a03736) is not pushed yet.
- 2026-11-17: the sample stack gives #477 and #482 their real bases, so #482's merge summary now
  reads "into feat/repo-mapping".
- 2026-11-17: `Layers` and `ListEnd` were not in the app's icon bundle and painted blank in the
  first screenshot; added to `DesignIcons` and its bundling test.
- 2026-11-17: `cargo fmt --all -- --check` still fails on the same seven files as on main; none is
  touched here.
- 2026-11-17: the sample stack got a merged first member, #468, that is in no list: merged entries
  stay in a real stack, and clicking one has to open it.

---
title: Group pull requests by GitHub milestone
phase: done
session: db965693-44d5-4706-9fce-57b0ca1263e7
ticket:
updated: 2026-11-17
---

## Goal
The pull request list can group its rows under the GitHub milestone each pull request is assigned
to, so a reviewer sees what is due for which release.

## Approach
- The milestone title comes from GitHub with the list (`milestone { title }` in the search query and
  the detail query) and lives on `PullRequest` as `milestone: Option<String>`. `#[serde(default)]`
  keeps cached lists readable, so the cache format stays at 1.
- Grouping is a persisted preference, off by default, toggled by a checked "Group by milestone" item
  in the list's Sort menu. It sits with the order because it changes how rows are arranged, not
  which rows show.
- `filters::entries` is the one source of display order: headers and rows in the order they are
  painted. The list's item count, row rendering, keyboard selection and scroll-to-item all read it,
  so the keyboard walks the rows in the order the screen shows them.
- Groups are ordered by title, case-insensitively, with "No milestone" last. The chosen sort applies
  inside each group. Ordering by due date was rejected for now: it needs a second field and a rule
  for milestones without a date.
- Milestones are per repository. In an inbox that spans repositories, two milestones with the same
  title share one group. That is the useful reading for a release name used across repositories.
- `uniform_list` gives every item the height of item 0, so a short header at index 0 would squash
  the rows. The list moves to `v_virtual_list` with explicit heights for headers and rows.
- No milestone filter, no folding of groups.

## Tasks
### 1. Milestone from GitHub into the model
Files: crates/pr_model/src/lib.rs, crates/github_api/src/gh_source.rs, crates/fixtures/src/lib.rs
Accept: `cargo test -p github_api -p pr_model -p pr_cache -p fixtures` passes, with the summary test reading a milestone.
Verified: `cargo test --workspace` → all passed; github_api 10 passed (a milestone title is read, a null milestone is `None`), pr_cache 5 passed.

### 2. Grouped order, preference and list rendering
Files: crates/pr_list_ui/src/filters.rs, crates/pr_list_ui/src/lib.rs, crates/pr_list_ui/src/toolbar.rs, crates/settings_model/src/lib.rs
Accept: `cargo test -p pr_list_ui -p settings_model` passes, with tests for group order, "No milestone" last and the sort inside a group; clippy clean on the changed crates.
Verified: `cargo test -p pr_list_ui` → 8 passed (4 new on `entries`); `cargo clippy --workspace --all-targets` → no warnings.

### 3. UI done gate
Files: crates/workspace_ui/tests/ui.rs, crates/screenshots_ui/src/main.rs
Accept: a UI test turns grouping on through the Sort menu, asserts the painted headers and their order against the rows, and opens the first grouped row from the keyboard; it fails when the headers are dropped; a scene renders the grouped list; `cargo test -p workspace_ui` passes.
Verified: `the_sort_menu_groups_the_list_by_milestone` passes; with the headers dropped from `entries` it fails with "a milestone header is painted above its pull requests"; scene renders to target/screenshots/01f-by-milestone-dark.png. The same test picks the item again and sees the headers go. `the_keyboard_keeps_its_row_on_screen_in_a_long_list` passes ungrouped and grouped, and fails with "the list scrolls to the selected row: … is not painted" when the scroll target is wrong. `cargo test -p workspace_ui` → 42 + 22 + 2 + 2 + 2 passed. `cargo test -p github_api -- --ignored live` → 1 passed against GitHub (first attempt hit a transient HTTP 499).

## Log
- 2026-11-17: started.
- 2026-11-17: the list moved from `uniform_list` to `v_virtual_list` in both modes, so there is one
  rendering path; `01-list` renders as before.
- 2026-11-17: a menu's separator takes an index, so "Group by milestone" is item 4 of the Sort menu.
- 2026-11-17: the rows container `pull-request-rows` registers for observation, so a test can check
  a row sits inside the list.
- 2026-11-17: `cargo fmt --all -- --check` already fails on main in seven files this change does not
  touch (app_state_ui, review_thread_ui panes, settings_keyboard_ui, theme_ui, workspace_ui panes
  test); left as they are.

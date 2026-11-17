---
title: Group pull requests by stack
phase: done
session: 230d57a3-c26a-4b25-a613-bde8fb3c7ee9
ticket:
updated: 2026-11-17
---

## Goal
The pull request list can group its rows under the GitHub stack each belongs to, so a reviewer
reads a stack top to bottom in merge order without opening a pull request.

## Approach
- A persisted preference, off by default, toggled by a checked "Group by stack" item under "Group by
  milestone" in the Sort menu. The two groupings exclude each other: turning one on turns the other
  off. Nesting them was rejected, a stack rarely spans milestones and two header levels cost rows.
- `filters::entries` stays the one source of display order and takes a `Grouping`.
- A stack is identified by repository and stack number. Stack groups come in the order their first
  row has in the chosen sort; inside a group rows are in merge order, position 1 first, whatever the
  sort, because the order is what a stack means. Pull requests outside a stack go last under "Not in
  a stack", in the chosen sort.
- The list holds only the members that are in an inbox and pass the filters, so a group can be a
  partial stack. Its header says so: the count is followed by "of 4" when members are missing. The
  inspector still shows the whole stack.
- The header reads "Stack #12 · switchfold": the list has no title for a stack, and its base branch
  comes only with the detail query.

## Tasks
### 1. Grouped order and preference
Files: crates/pr_list_ui/src/filters.rs, crates/settings_model/src/lib.rs
Accept: `cargo test -p pr_list_ui -p settings_model` passes with tests for group order, merge order inside a group, the same stack number in two repositories and "Not in a stack" last.
Verified: `cargo test -p pr_list_ui -p settings_model` → 10 + 4 passed, with `stacks_group_in_sort_order_with_members_in_merge_order` and `the_same_stack_number_in_two_repositories_is_two_groups`.

### 2. Menu item, headers and the UI gate
Files: crates/pr_list_ui/src/lib.rs, crates/pr_list_ui/src/toolbar.rs, crates/workspace_ui/tests/ui.rs, crates/screenshots_ui/src/main.rs
Accept: a UI test turns the grouping on through the Sort menu, asserts the painted headers and row order, opens the first grouped row from the keyboard and sees milestone grouping take over when picked; it fails when the headers are dropped; a scene renders it; `cargo test --workspace` and clippy pass.
Verified: `the_sort_menu_groups_the_list_by_stack` passes. Broken on purpose it fails with "a stack header is painted above its pull requests" (headers dropped), "flaticols/switchfold#470 is painted above flaticols/switchfold#477" (merge order dropped), "grouping by milestone replaces grouping by stack" (left: (true, true)) and "the header says the list holds three of the stack's four" (note dropped). Scene: target/screenshots/01g-by-stack-dark.png. `cargo test --workspace` → exit 0, `tests/ui.rs` 45 passed; `cargo clippy --workspace --all-targets` → no code warnings; `git diff --check` clean.

## Log
- 2026-11-17: started, on local `main` (two commits ahead of `origin/main`). This reverses the
  "Rejected: Group by stack" note in [the stacks plan](2026-11-17-stacked-prs.md), on request.
- 2026-11-17: picking the checked grouping again ungroups the list, as the milestone item already did.

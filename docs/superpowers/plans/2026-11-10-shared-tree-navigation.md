# Shared File Tree, Go to File, Find in Pane — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One file tree per review that drives the focused pane, a fuzzy ⌥⇧A file picker, and ⌘F find in the focused pane.

**Architecture:** `ReviewPanes` takes ownership of the tree's `TreeState` and its bookkeeping and draws the left sidebar once, the same way it already draws the focused pane's inspector. Panes keep their tabs, scroll and timeline. Go to file is a new `command_palette_ui::open_files` dialog with a hand-written fuzzy ranker. Find lives in each `ReviewThreadView`, as a bar above its timeline. It searches the pane's rows and draws code-line highlights through `FileDiffRow`.

**Tech Stack:** Rust, gpui-kit 0.7.1 (gpui-component `Command`, `TreeState`, `InputState`), `gpui_kit::test` UI tests.

**Spec:** `docs/superpowers/specs/2026-11-10-shared-tree-navigation-design.md`

## Global Constraints

- The UI done gate in `CLAUDE.md` applies to every task: drive through real input by element id, assert on painted elements, watch the new assertion fail, look at a PNG from `screenshots_ui`, and pass `cargo test -p workspace_ui`.
- Add no new crates. The fuzzy matcher is our own code.
- ⌘⇧F stays focus mode. ⌘F is find, ⌥⇧A is go to file, ⌘B is the tree.
- Saved pane layouts that contain `outline` must still load.
- Element ids for tests: `outline` (sidebar), `find-bar`, `find-input`, `find-count`, `find-match-current`, `go-to-file`.

## Review Focus

1. **Focus after a tree click.** ⌘F, ⌘↵ and Esc pressed right after clicking the tree must still reach the focused pane. Task 3 tests this.
2. **Splitting while the tree is folded.** A new pane must not reset the folded folders. Task 3 tests this.
3. **Typing "Å" in the composer.** ⌥⇧A must not open the picker while a text field has focus. Task 2 tests this.
4. **Find after switching tabs.** The count must re-run, not show stale "3 of 9". Task 4 tests this.
5. **Esc inside the find bar.** It must close find, not leave the review. Task 4 tests this.

---

### Task 1: Fuzzy file ranking

**Files:**
- Create: `crates/command_palette_ui/src/fuzzy.rs`
- Modify: `crates/command_palette_ui/src/lib.rs` (add `mod fuzzy; pub use fuzzy::rank;`)

**Interfaces:**
- Produces: `pub fn rank(query: &str, paths: &[impl AsRef<str>]) -> Vec<usize>`. It returns the indices of matching paths, best first. Ties keep input order. An empty query returns every index in order.

- [ ] **Step 1: Write failing unit tests in `fuzzy.rs`**

```rust
const PATHS: [&str; 4] = [
    "internal/services/ocn/service_test.go",
    "internal/services/ocn/service.go",
    "cmd/core/config/config.go",
    "docs/wiki/tenant-configuration.md",
];

#[test] fn subsequence_across_folders_matches() { assert!(rank("svcgo", &PATHS).contains(&1)); }
#[test] fn out_of_order_does_not_match() { assert!(!rank("gosvc", &PATHS).contains(&1)); }
#[test] fn case_is_ignored() { assert_eq!(rank("CONFIG.GO", &PATHS)[0], 2); }
#[test] fn file_name_hit_beats_folder_hit() { assert_eq!(rank("config", &PATHS)[0], 2); }
#[test] fn contiguous_beats_scattered() { assert_eq!(rank("service.go", &PATHS)[0], 1); }
#[test] fn empty_query_keeps_order() { assert_eq!(rank("", &PATHS), vec![0, 1, 2, 3]); }
```

- [ ] **Step 2: Run `cargo test -p command_palette_ui fuzzy`.** Expected: it fails to compile because `rank` is missing.

- [ ] **Step 3: Implement `rank`.** Do a greedy left-to-right subsequence match per path, on lowercased chars. Score each matched char:
  - +1 base;
  - +5 if it directly follows the previous matched char;
  - +8 if it starts a word: index 0, after `/ _ - .`, or a lower-to-upper change;
  - +10 if it lies in the file-name part, after the last `/`.

  Then try the match again, anchored to start at the file name. Keep the higher score. Sort by score descending with a stable sort.

- [ ] **Step 4: Run `cargo test -p command_palette_ui fuzzy`.** Expected: 6 passed.

- [ ] **Step 5: Commit** with `git commit -S -m "Rank files with a fuzzy matcher"`.

### Task 2: ⌥⇧A Go to file

**Files:**
- Modify: `crates/command_palette_ui/src/lib.rs`, which adds `open_files`.
- Modify: `crates/app_state_ui/src/actions.rs`, which adds the `GoToFile` action.
- Modify: `crates/review_thread_ui/src/panes/view.rs`, which handles `GoToFile`.
- Modify: `crates/review_thread_ui/src/lib.rs` `init`, which binds `alt-shift-a`.
- Modify: `crates/workspace_ui/src/menus.rs`, which adds a "Go to File…" menu item.
- Test: `crates/workspace_ui/tests/ui.rs`

**Interfaces:**
- Consumes: `command_palette_ui::rank` from Task 1.
- Produces: `pub fn open_files(paths: Vec<SharedString>, on_pick: impl Fn(SharedString, &mut Window, &mut App) + 'static, window: &mut Window, cx: &mut App)`.

- [ ] **Step 1: Write failing UI tests.**
  - `alt_shift_a_jumps_the_focused_pane_to_a_fuzzy_match`. Open the featured PR and split with `ctrl-b %`. Press `alt-shift-a` and `settle(window, "go-to-file", cx)`. Type `rndr` with `window.input`, then press `enter`. Then assert `window.try_find("go-to-file")` is none. Assert the focused pane's `pane_state().scroll.file == Some("crates/review/src/render.rs")`, and the other pane's scroll is unchanged.
  - `alt_shift_a_types_in_the_composer`. Click into the composer, press `alt-shift-a`, and assert that no `go-to-file` element was painted.

- [ ] **Step 2: Run `cargo test -p workspace_ui --test ui alt_shift_a`.** Expected: FAIL, because nothing opens.

- [ ] **Step 3: Implement `open_files`.**
  - Keep the ranked list in `Rc<RefCell<Vec<usize>>>`.
  - Build the `Command` with `.filterable(false)`, and give the dialog the id `go-to-file`.
  - In `on_query`, re-rank with `rank` and call `window.refresh()`.
  - In `on_confirm`, map the row to the path, close the dialog and call `on_pick`.
  - Placeholder text: "Go to file…".
- [ ] **Step 4: Bind and handle the action.**
  - Bind `KeyBinding::new("alt-shift-a", GoToFile, Some("ReviewPanes"))`.
  - `ReviewPanes` collects the focused pane's `detail().files` paths. The picked file runs `reveal_file` on the focused pane.
  - When `window.has_focused_input(cx)`, call `cx.propagate()` and return. If the second test then still fails because propagation doesn't insert the char, bind with context `"ReviewPanes && !Input"` instead. Check the Input context name in gpui-component's input element.
- [ ] **Step 5: Run the two tests.** Expected: PASS. **Watch them fail:** comment out the `.on_action` for `GoToFile` and run again. Expected: the first test fails, and its message names `go-to-file`. Restore the line with `git checkout -p` or an edit.
- [ ] **Step 6: Commit** with `git commit -S -m "Open a fuzzy file picker with ⌥⇧A"`.

⌘K keeps its substring matching. Ranking its Files group would mean turning filtering off for the whole grouped palette and re-filtering pull requests and commands by hand. The spec leaves this to the builder, and ⌥⇧A covers file search.

### Task 3: One shared file tree

**Files:**
- Modify: `crates/review_thread_ui/src/lib.rs`
  - Remove the `file_tree`, `file_tree_paths`, `folded_dirs` and `tree_revealed` fields and their two tree subscriptions.
  - Remove `outline`, `outline_visible` and `toggle_outline`.
  - `render_pane` stops drawing the outline panel.
- Modify: `crates/review_thread_ui/src/outline.rs`
  - `render_outline(&mut self, detail, tree: &Entity<TreeState>, cx)` takes the shared tree.
  - `sync_file_tree` and `on_file_tree_changed` move to `ReviewPanes`.
- Modify: `crates/review_thread_ui/src/panes/view.rs`
  - `ReviewPanes` owns the tree, syncs it on `PullRequestStoreEvent::DetailChanged` for its key, and reveals on selection.
  - It wraps the panes body in an `h_resizable` with the outline panel, visible per `ReviewLayout::get(cx).outline`, sized `rem * 16.25` with range `180..520`, as today.
  - It forwards the pane-only shortcuts.
- Modify: `crates/review_thread_ui/src/panes/state.rs`. Remove `PaneState::outline`. Serde ignores the unknown field in old saved layouts.
- Modify: `crates/settings_model/src/lib.rs`. Add `outline_collapsed: bool` to `Preferences`, mirroring `inspector_collapsed`, including its serde default.
- Modify: `crates/review_thread_ui/src/lib.rs` `ReviewLayout`.
  - `get` reads `outline: !outline_collapsed`.
  - `toggle_outline` saves `outline_collapsed`, as `toggle_inspector` saves `inspector_collapsed`.
- Modify: `crates/workspace_ui/src/workspace.rs`.
  - `ToggleSidebar` calls `ReviewLayout::toggle_outline`.
  - The title bar's `sidebar_open` reads `ReviewLayout::get(cx).outline`.
- Test: `crates/workspace_ui/tests/panes.rs` and `crates/workspace_ui/tests/ui.rs`.
  - Replace `hiding_one_panes_tree_*` and `hiding_a_panes_tree_redraws_the_title_bar`.
  - Update `outline_heights` and `every_pane_fills_its_share_of_the_window` to measure `review-pane` instead of the per-pane `outline`.

**Interfaces:**
- Produces:
  - `ReviewPanes` sets the tree's selection from `focused_view().read(cx).pane_state().scroll.file`. It does this after each pane `ViewChanged` and each focus change. It sets `tree_revealed` to that path first, so the selection observer does not reveal.
  - `ReviewThreadView::reveal_file` calls `show_all_commits` first when a commit is picked and that commit's files lack `path`.

- [ ] **Step 1: Write failing tests.**
  - In `panes.rs`:
    - `a_split_draws_one_file_tree`. Split with `ctrl-b %`. Assert exactly one painted element `outline` via `window.find("outline").visible()`. Assert `window.within(("review-pane", id)).try_find("outline")` is none for each pane.
    - `a_tree_click_scrolls_only_the_focused_pane`. Split, then `ctrl-b left`. Click `outline-crates/review/src/render.rs`. Assert pane 0's scroll file is `render.rs`. Assert pane 1's scroll file is unchanged.
    - `clicking_a_pane_then_the_tree_moves_that_pane`. Split, click into pane 0's `review-pane` element, then click the tree file. Assert pane 0 moved.
    - `after_a_tree_click_cmd_f_reaches_the_pane`. Click the tree file, press `cmd-f`, and assert `find-bar` is visible. This test passes only after Task 4. Mark it `#[ignore = "needs find bar, Task 4"]` until then.
    - `escape_in_the_tree_returns_focus_to_the_pane`. Click the tree file and press `escape`. Assert the review is still open and the focused pane's focus handle is focused.
    - `a_split_keeps_folded_folders`. Click the folder row `outline-crates/` to fold it, then split. Assert that `outline-crates/review/src/render.rs` is not painted.
    - `the_tree_follows_the_focused_panes_scroll`. Split, `reveal_file(HUNK)` on pane 1 through `ctrl-b right` and a tree click. Then `ctrl-b left`, where pane 0 sits at the top. Assert the selected row is not `hunk.rs`, through the tree row's `selected` state if exposed, or else `TreeState::selected_entry`.
  - In `ui.rs`, add `cmd_b_hides_the_shared_tree_and_remembers_it`. Press `cmd-b`. Assert that `on_screen(window, "outline")` is false and that preferences have `outline_collapsed == true`.

- [ ] **Step 2: Run `cargo test -p workspace_ui --test panes --test ui`.** Expected: the new tests FAIL. The old per-pane tests no longer compile, so delete them in this step.

- [ ] **Step 3: Implement the move per the Files list.** Keep `render_outline` a `ReviewThreadView` method, called on the focused pane like `render_inspector_panel`. Its thread-row clicks then reveal in that pane.

- [ ] **Step 4: Forward shortcuts while the tree has focus.**
  - Bind `cmd-f`, `cmd-enter`, `cmd-shift-enter`, `cmd-shift-o`, `cmd-shift-a` and `cmd-shift-c` also in the `ReviewPanes` context. A pane's deeper `ReviewThread` binding still wins when the pane has focus.
  - The `ReviewPanes` handler focuses the focused pane, then calls `window.dispatch_action(action.boxed_clone(), cx)`.
  - Bind `escape` to `GoBack` in `ReviewPanes`. That handler only focuses the pane.

- [ ] **Step 5: Run `cargo test -p workspace_ui --test panes --test ui`.** Expected: PASS, except the ignored test. **Watch them fail:** make `ReviewPanes` render the outline only when `tree.count() == 1`, and run `a_split_draws_one_file_tree`. Expected message: `outline` not found. Revert with `git checkout -- crates/review_thread_ui/src/panes/view.rs` after committing the real change, or revert by edit.

- [ ] **Step 6: Look at it.** Run `cargo run -p screenshots_ui -- target/screenshots 02m-split-panes` and read the dark PNG. Expect one tree on the left and two or three panes with tab rows and no trees.

- [ ] **Step 7: Commit** with `git commit -S -m "Share one file tree across split panes"`.

### Task 4: ⌘F Find in pane

**Files:**
- Create: `crates/review_thread_ui/src/find.rs`. It holds `FindState`, match collection, the bar render and next/previous.
- Modify: `crates/review_thread_ui/src/lib.rs`. It adds the `find: Option<FindState>` field, binds `cmd-f` to `FindInPane` in `CONTEXT`, and draws the bar between `render_toolbar` and `render_timeline`.
- Modify: `crates/app_state_ui/src/actions.rs`. It adds the `FindInPane`, `FindNext` and `FindPrevious` actions.
- Modify: `crates/diff_view_ui/src/lib.rs`.
  - Add `pub fn find_ranges(text: &str, query: &str) -> Vec<Range<usize>>`. It does a case-insensitive, non-overlapping search over byte ranges.
  - Add `FileDiffRow::find(self, query: Option<SharedString>, current: Option<(DiffSide, usize, Range<usize>)>)`.
  - `code_cell` merges a background `HighlightStyle` for each range into `spans` before `expand_tabs`. Use `cx.theme().warning` at 35% alpha, and 70% for the current match. The current match's element gets the id `find-match-current`.
- Modify: `crates/review_thread_ui/src/timeline.rs`. It passes the find query and the current match to `FileDiffRow`. Rows holding the current body match get a 2px `ring` border, with the id `find-match-current` on the card.
- Test: `crates/workspace_ui/tests/ui.rs`, plus `find.rs` unit tests for `find_ranges` in `diff_view_ui`.

**Interfaces:**
- Consumes: `TimelineRow` and `rows` from `rows.rs`.
- Produces:
  - `pub(crate) struct FindMatch { row: usize, place: FindPlace }`.
  - `enum FindPlace { Code { side: DiffSide, line: usize, range: Range<usize> }, Body }`.
  - `ReviewThreadView::open_find(&mut self, window, cx)`.

What gets searched, per `TimelineRow`:
- `File { row: Line | Split }`: the diff line text, on both sides.
- `File { row: Threads }`: the comment bodies of the threads anchored there.
- `Item(ix)`: the text depends on the item.
  - `Description` uses `detail.body`.
  - `Commented` uses `comment.body`.
  - `Finding` uses the finding's body.
  - `Reviewed` uses its body if it has one.
- Anything else: nothing. That includes headers, hunks, checks, runs and pushes. Collapsed files have no line rows, so they are skipped on their own.

Body matches are counted and navigated, and the current one's card gets the ring. Text inside Markdown bodies is **not** painted with per-match highlights. `markdown_ui::github_markdown` builds a stateless `TextView`, and range highlights need a `TextViewState` per body. That is a separate change, if wanted.

- [ ] **Step 1: Write failing tests.**
  - In `diff_view_ui`:
    - `find_ranges_ignores_case`: `find_ranges("Foo foo", "FOO") == [0..3, 4..7]`.
    - `find_ranges_empty_query_is_empty`.
  - In `ui.rs`:
    - `cmd_f_counts_and_steps_through_matches`. Press `cmd-f`, input `fn ` (a sample-diff token), then `enter`. Assert `window.find("find-count").label()` reads `2 of N`. Assert `find-match-current` is visible inside the viewport.
    - `shift_enter_wraps_to_the_last_match`. Search, then press `shift-enter` from match 1. Assert `N of N`.
    - `escape_closes_find_and_keeps_the_review`. Press `escape` in the find bar. Assert `find-bar` is not painted, and `reviewing()` is still the featured PR.
    - `switching_tabs_reruns_find`. Search on Everything, click the `Changes` tab, and assert that the count label changed to the Changes count. Compute the expected value with `find_ranges` over the sample files.
    - `find_matches_comment_text`. Search a word that only exists in a sample comment. Assert `1 of 1` and that `find-match-current` is painted.
  - Remove `#[ignore]` from `after_a_tree_click_cmd_f_reaches_the_pane`.

- [ ] **Step 2: Run `cargo test -p workspace_ui --test ui find` and `cargo test -p diff_view_ui find`.** Expected: FAIL.

- [ ] **Step 3: Implement `find.rs`.**
  - The bar is an `h_flex` with id `find-bar` and `.test_support()`.
  - It holds an `InputState` with id `find-input`, a count label with id `find-count` ("N of M" or "No results"), and up/down buttons.
  - Recompute matches on input change, on `sync_rows`, and on `show_filter`.
  - Next and previous wrap around, then `list_state.scroll_to_reveal_item(match.row)`.
  - Esc inside the bar closes find and focuses the pane. Handle `GoBack` in the bar's own key context so it does not reach the pane's Back handler. The composer already does this through `has_focused_input`, so check for the find input first.
- [ ] **Step 4: Thread highlights into `FileDiffRow` and the body cards** per the Files list.
- [ ] **Step 5: Run the tests from Step 2.** Expected: PASS. **Watch them fail:** remove the `.child(find_bar)` line and run `cmd_f_counts_and_steps_through_matches`. Expected: `find-count` not found. Restore it.
- [ ] **Step 6: Look at it.**
  - Add the scene `Scene::FindInPane`, named `02n-find-in-pane`, to `screenshots_ui`. It opens the review, dispatches `FindInPane`, inputs `fn ` and presses enter once.
  - Run `cargo run -p screenshots_ui -- target/screenshots 02n-find-in-pane` and read the dark PNG. Expect highlighted code matches, one stronger, and the "2 of N" bar.
- [ ] **Step 7: Commit** with `git commit -S -m "Find text in a pane with ⌘F"`.

### Task 5: Whole suite and app check

- [ ] **Step 1:** Run `cargo fmt --all`, then `cargo clippy --workspace --all-targets -- -D warnings`. Expected: no warnings.
- [ ] **Step 2:** Run `cargo test --workspace`. Expected: all pass. `cargo test -p workspace_ui` is the done gate's minimum.
- [ ] **Step 3:** Render `02-thread` and `08-palette` and read the dark PNGs, so the single-pane review and ⌘K look unchanged apart from the tree.
- [ ] **Step 4:** Run `make install`, open the app, split a review, and take a window screenshot with `screencapture -l <id>`. Confirm one tree.
- [ ] **Step 5: Commit** any fmt fixups with `git commit -S`.

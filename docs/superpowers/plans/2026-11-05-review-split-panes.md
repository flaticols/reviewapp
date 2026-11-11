# Review Split Panes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the review screen into tmux-style panes. Each pane is a full copy of the review that scrolls on its own, and every pull request remembers its own layout.

**Architecture:** A new `ReviewPanes` entity owns a binary tree of `ReviewThreadView` panes and renders them with draggable dividers. The single inspector sits to the right and shows the focused pane. A pure `PaneTree<T>` holds splitting, closing and directional focus, so it is unit-tested without gpui. The layout is saved per PR through two generic "view state" methods on `PullRequestStore`. They keep the layout in memory and write it next to the PR's cached detail.

**Tech Stack:** Rust, gpui-kit 0.7.1 (gpui-pre 0.3.8), serde/serde_json, existing `pr_cache`.

**Spec (agreed in chat, 2026-11-05):**
- Some PRs are big. The reviewer wants to see different files at the same time.
- A pane is a full copy of the review screen (toolbar, conversation, comments, diffs) with its own file tree. Each pane scrolls on its own. Clicking a file in a pane's tree jumps only that pane.
- There is one inspector, on the right edge. It shows PR-wide data and does not change with the focused pane.
- Keys are tmux defaults with prefix `ctrl-b`: `%` splits side by side and `"` splits top and bottom. Arrows move focus in that direction and `o` moves to the next pane. `z` zooms the focused pane and toggles back. `x` closes the focused pane.
- Splits can nest without limit. Dividers can be dragged to resize.
- Every PR remembers its layout, including across app restarts. That covers the splits, the divider positions, and each pane's tab, tree visibility and scroll position. A PR that was never split opens as one pane, as it does today.
- `cmd-b` shows or hides the focused pane's file tree only.
- Drafts typed in a pane's comment box stay in that pane.
- When a new push arrives, every pane refreshes and keeps its place.

## Global Constraints

- Work on branch `review-split-panes`, never on `main`. Every commit is signed (`git commit -S`) and has no co-author or AI trailer.
- No new crate dependencies beyond adding `serde.workspace = true` to `review_thread_ui`.
- Key bindings must be exactly: `ctrl-b %`, `ctrl-b "`, `ctrl-b left|right|up|down`, `ctrl-b o`, `ctrl-b z`, `ctrl-b x`. They are bound in context `ReviewPanes`.
- Divider ratios are clamped to `0.1..=0.9`. A new split starts at `0.5`.
- New actions go in `crates/app_state_ui/src/actions.rs`, following the existing doc-comment style.
- Tests follow the existing style: sentence-named `#[test]`/`#[gpui_kit::test]` functions, no `if`/`match` in test bodies, and success and failure cases in separate functions.
- Verify with `cargo build --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. The Makefile has no lint or test target.

## Review Focus

1. **Closing the last pane.** `ctrl-b x` with one pane must do nothing and must not panic. The test is in Task 1 (`closing_the_only_pane_is_refused`) and Task 4.
2. **A saved layout that no longer fits the PR.** A saved scroll anchor can name a file the new push removed. That pane must open at the top, not panic or stick to a stale row index. The test is in Task 3 (`a_missing_anchor_file_scrolls_to_the_top`).
3. **A saved layout that arrives after the reviewer already split.** It must not overwrite the reviewer's live layout. The test is in Task 4 (`a_late_saved_layout_does_not_replace_a_split_made_meanwhile`).
4. **Only one pane recomputes local diffs.** Opening N panes must run one gix diff, not N. When the pane holding that duty closes, another pane must take it over. The test is in Task 4 (`closing_the_diffing_pane_hands_the_duty_on`).
5. **Typing `%` or `"` in a comment box without the prefix.** The text must go into the box and no split may happen. The test is in Task 4 (`typing_percent_in_the_composer_does_not_split`).

---

### Task 0: Branch

- [ ] **Step 1:** `git switch -c review-split-panes`. Expected: `Switched to a new branch 'review-split-panes'`.

---

### Task 1: `PaneTree<T>`: the pure layout model

**Files:**
- Create: `crates/review_thread_ui/src/panes/tree.rs` (model and `#[cfg(test)] mod tests`)
- Create: `crates/review_thread_ui/src/panes/mod.rs` (`mod tree; pub use tree::*;` for now)
- Modify: `crates/review_thread_ui/src/lib.rs` (add `mod panes;`)
- Modify: `crates/review_thread_ui/Cargo.toml` (add `serde.workspace = true`)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
  pub enum SplitAxis { SideBySide, Stacked }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Direction { Left, Right, Up, Down }
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub enum PaneTree<T> {
      Leaf(T),
      Split { axis: SplitAxis, ratio: f32, first: Box<PaneTree<T>>, second: Box<PaneTree<T>> },
  }
  impl<T> PaneTree<T> {
      pub fn len(&self) -> usize;
      pub fn leaves(&self) -> Vec<&T>;                        // depth-first, first before second
      pub fn split(&mut self, ix: usize, axis: SplitAxis, new: T) -> usize; // returns ix + 1
      pub fn close(&mut self, ix: usize) -> Option<(T, usize)>;  // None when len() == 1; usize = leaf to focus
      pub fn set_ratio(&mut self, boundary: usize, ratio: f32);  // clamps 0.1..=0.9
      pub fn neighbor(&self, ix: usize, direction: Direction) -> Option<usize>;
      pub fn map<U>(&self, f: impl FnMut(&T) -> U) -> PaneTree<U>;
  }
  ```
  Leaves are named by their index in `leaves()` order. A split is named by its **boundary**, the leaf index where its `second` child starts. That index is unique per split.

- [ ] **Step 1: Write the failing tests** in `tree.rs`:

```rust
#[test] fn splitting_puts_the_new_pane_after_the_focused_one() {
    let mut tree = PaneTree::Leaf("a");
    assert_eq!(tree.split(0, SplitAxis::SideBySide, "b"), 1);
    assert_eq!(tree.leaves(), vec![&"a", &"b"]);
}
#[test] fn closing_a_pane_gives_its_space_to_the_sibling() {
    let mut tree = PaneTree::Leaf("a");
    tree.split(0, SplitAxis::SideBySide, "b");
    tree.split(1, SplitAxis::Stacked, "c");          // a | (b / c)
    assert_eq!(tree.close(1), Some(("b", 1)));        // focus lands on c, now leaf 1
    assert_eq!(tree.leaves(), vec![&"a", &"c"]);
}
#[test] fn closing_the_only_pane_is_refused() {
    let mut tree = PaneTree::Leaf("a");
    assert_eq!(tree.close(0), None);
    assert_eq!(tree.len(), 1);
}
#[test] fn arrows_find_the_pane_on_that_side() {
    let mut tree = PaneTree::Leaf("a");
    tree.split(0, SplitAxis::SideBySide, "b");
    tree.split(1, SplitAxis::Stacked, "c");           // a | (b / c)
    assert_eq!(tree.neighbor(2, Direction::Left), Some(0));
    assert_eq!(tree.neighbor(0, Direction::Right), Some(1)); // the larger overlap wins; tie → upper
    assert_eq!(tree.neighbor(1, Direction::Down), Some(2));
}
#[test] fn arrows_at_the_edge_find_nothing() {
    let tree = PaneTree::Leaf("a");
    assert_eq!(tree.neighbor(0, Direction::Left), None);
}
#[test] fn ratios_are_clamped() {
    let mut tree = PaneTree::Leaf("a");
    tree.split(0, SplitAxis::SideBySide, "b");
    tree.set_ratio(1, 0.99);
    assert_eq!(tree, PaneTree::Split { axis: SplitAxis::SideBySide, ratio: 0.9,
        first: Box::new(PaneTree::Leaf("a")), second: Box::new(PaneTree::Leaf("b")) });
}
#[test] fn a_layout_survives_json() {
    let mut tree = PaneTree::Leaf(1u8);
    tree.split(0, SplitAxis::Stacked, 2);
    let json = serde_json::to_string(&tree).unwrap();
    assert_eq!(serde_json::from_str::<PaneTree<u8>>(&json).unwrap(), tree);
}
```
(`serde_json` goes under `[dev-dependencies]`.)

- [ ] **Step 2:** `cargo test -p review_thread_ui panes::tree`. Expected: compile failure, `PaneTree` not found.
- [ ] **Step 3: Implement `PaneTree`.** For `neighbor`, lay each leaf out as a rectangle in the unit square, using the ratios. Candidates are leaves whose facing edge touches the pane's edge (within `1e-4`) and that overlap it on the other axis. Pick the largest overlap, and break ties by the smaller top/left coordinate. After `close`, focus goes to `min(ix, len - 1)` in the new order.
- [ ] **Step 4:** Run the tests again. Expected: 7 passed.
- [ ] **Step 5:** `git commit -S -m "Add the pane tree that review splits are built on"`.

---

### Task 2: Saved view state per pull request

**Files:**
- Modify: `crates/pr_cache/src/lib.rs` (two methods and a test)
- Modify: `crates/app_state_ui/src/pull_request_store.rs` (two methods, a memory map and a save-task map)
- Test: `crates/workspace_ui/tests/cache.rs` (one new test)

**Interfaces:**
- Produces in `pr_cache`:
  ```rust
  pub fn load_view<T: DeserializeOwned>(&self, key: &PullRequestKey, name: &str) -> Option<T>;
  pub fn save_view<T: Serialize>(&self, key: &PullRequestKey, name: &str, value: &T) -> io::Result<()>;
  ```
  The file is `pulls/<owner>/<name>/<number>.<sanitize(name)>.json`, with the same envelope and atomic write as the detail. `prune` and `clear` already cover it.
- Produces in `PullRequestStore`:
  ```rust
  pub fn load_view_state<T: DeserializeOwned + Send + 'static>(&self, key: &PullRequestKey, name: &'static str, cx: &App) -> Task<Option<T>>;
  pub fn save_view_state<T: Serialize>(&mut self, key: &PullRequestKey, name: &'static str, value: &T, cx: &mut Context<Self>);
  ```
  `save_view_state` stores a `serde_json::Value` in `view_states: HashMap<(PullRequestKey, &'static str), Value>` right away. It also schedules a disk write after `SAVE_DELAY`, in its own `view_saves` task map so it never cancels a detail save. `load_view_state` returns the memory copy when there is one and otherwise reads the disk on the background executor. Neither method emits `DetailChanged`: that event makes every pane drop its syntax highlights.

- [ ] **Step 1: Write the failing tests.**
  - `pr_cache`: `a_view_state_round_trips_next_to_the_detail`. Save `vec![1u8, 2]` under `"panes"`, then load it back and assert `Some(vec![1, 2])`. Also assert that the file `pulls/flaticols/switchfold/482.panes.json` exists.
  - `pr_cache`: `a_missing_view_state_reads_as_none`.
  - `workspace_ui/tests/cache.rs`: `a_saved_view_state_comes_back_in_the_next_run`. Store A calls `save_view_state(&key, "panes", &7u32)`, then advances the clock past `SAVE_DELAY` (`cx.executor().advance_clock`). Store B is built on the same cache root, and `load_view_state::<u32>` returns `Some(7)`.
- [ ] **Step 2:** `cargo test -p pr_cache -p workspace_ui --test cache`. Expected: compile failure.
- [ ] **Step 3: Implement the methods** with the signatures above.
- [ ] **Step 4:** Run the tests again. Expected: all pass.
- [ ] **Step 5:** `git commit -S -m "Save per-pull-request view state next to the cached detail"`.

---

### Task 3: A `ReviewThreadView` that can be one pane of many

**Files:**
- Modify: `crates/review_thread_ui/src/lib.rs`, `filters.rs`, `outline.rs`, `timeline.rs`, `rows.rs`
- Create: `crates/review_thread_ui/src/panes/state.rs`

**Interfaces:**
- Produces in `panes/state.rs`:
  ```rust
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct PaneState { pub filter: TimelineFilter, pub outline: bool, pub scroll: ScrollAnchor }
  #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
  pub struct ScrollAnchor { pub file: Option<String>, pub row: usize, pub offset: f32 }
  impl Default for PaneState  // Everything, outline true, top
  ```
  `ScrollAnchor` names the top row. If that row is a file row, `file` is its path and `row` counts rows below the file's header. Otherwise `file` is `None` and `row` is the absolute row index.
- `TimelineFilter` gains `Serialize, Deserialize`.
- Produces on `ReviewThreadView`:
  ```rust
  pub fn new(key, back_label, window, cx) -> Self   // unchanged; calls new_pane(.., PaneState::default(), true, ..)
  pub(crate) fn new_pane(key: PullRequestKey, back_label: SharedString, state: PaneState, diffs_locally: bool, window: &mut Window, cx: &mut Context<Self>) -> Self;
  pub(crate) fn pane_state(&self) -> PaneState;
  pub(crate) fn set_diffs_locally(&mut self, on: bool, cx: &mut Context<Self>); // on → runs diff_locally now if detail is loaded
  pub fn outline_visible(&self, cx: &App) -> bool;      // self.outline && ReviewLayout::get(cx).outline
  pub fn toggle_outline(&mut self, cx: &mut Context<Self>); // leaves focus mode first when it hid the tree
  pub(crate) fn render_pane(&mut self, window, cx) -> AnyElement;      // outline + toolbar + timeline, no inspector
  pub(crate) fn render_inspector_panel(&mut self, cx) -> Option<AnyElement>; // None while loading
  ```
- `ReviewThreadEvent` gains `ViewChanged`. A pane emits it on scroll (from `list_state.set_scroll_handler`, through a weak handle), on a tab change and on an outline toggle.
- The scroll from `new_pane`'s `state` is held in `pending_scroll: Option<ScrollAnchor>`. It is applied in `sync_detail` right after the first `sync_rows` with a loaded detail. If the anchor's file has no header row, it falls back to the top.
- `diff_locally` runs only when `diffs_locally` is true.
- `Render for ReviewThreadView` keeps its current output: `render_pane`, plus the inspector when shown. That keeps the view usable as a single pane and keeps the existing tests green.

- [ ] **Step 1: Write the failing tests** in a new `crates/workspace_ui/tests/panes.rs`. Open stores with sample data as `tests/ui.rs` does, then open a window whose root is a bare `ReviewThreadView` built with `new_pane`. Make `new_pane` and `pane_state` `pub` behind `#[doc(hidden)]` if the test needs them.
  - `a_pane_reports_where_it_is_scrolled`: `reveal_file("crates/review/src/thread.rs")`, render, then assert that `pane_state().scroll.file == Some("crates/review/src/thread.rs".into())`.
  - `a_pane_opens_where_its_state_says`: build with `ScrollAnchor { file: Some(path), row: 0, offset: 0. }` and assert that `pane_state().scroll.file == Some(path)` after `run_until_parked`.
  - `a_missing_anchor_file_scrolls_to_the_top`: build with `file: Some("gone.rs")` and assert `pane_state().scroll == ScrollAnchor::default()`.
  - `hiding_one_panes_tree_leaves_the_setting_for_new_panes_alone`: `toggle_outline` and assert `outline_visible == false`. Then assert `ReviewLayout::get(cx).outline == true`.
- [ ] **Step 2:** `cargo test -p workspace_ui --test panes`. Expected: compile failure.
- [ ] **Step 3: Implement** with the interfaces above. `render_body` splits into `render_pane` and `render_inspector_panel`. The existing `render_body` composes them, so the single-view output stays the same.
- [ ] **Step 4:** `cargo test -p workspace_ui`. Expected: the new tests and every existing `ui.rs`, `cache.rs`, `triage.rs` and `scroll_perf.rs` test pass.
- [ ] **Step 5:** `git commit -S -m "Let a review view be one pane: its own tree, saved scroll and state"`.

---

### Task 4: `ReviewPanes`: splitting, focus, zoom, dividers, saving

**Files:**
- Create: `crates/review_thread_ui/src/panes/view.rs`
- Modify: `crates/review_thread_ui/src/panes/mod.rs` (`pub use view::ReviewPanes;`), `lib.rs` (`pub use panes::ReviewPanes;`, bindings in `init`)
- Modify: `crates/app_state_ui/src/actions.rs`
- Test: `crates/workspace_ui/tests/panes.rs`

**Interfaces:**
- Actions added to `switchfold`, each with a one-line doc comment: `SplitSideBySide`, `SplitStacked`, `FocusPaneLeft`, `FocusPaneRight`, `FocusPaneUp`, `FocusPaneDown`, `FocusNextPane`, `ZoomPane`, `ClosePane`.
- Bindings in `review_thread_ui::init`, context `"ReviewPanes"`: `"ctrl-b %"`, `"ctrl-b \""`, `"ctrl-b left"`, `"ctrl-b right"`, `"ctrl-b up"`, `"ctrl-b down"`, `"ctrl-b o"`, `"ctrl-b z"`, `"ctrl-b x"`.
- Produces:
  ```rust
  pub struct ReviewPanes { key, back_label, tree: PaneTree<Entity<ReviewThreadView>>, focused: usize,
                           zoomed: bool, restored: bool, focus_handle, _subscriptions }
  impl EventEmitter<ReviewThreadEvent> for ReviewPanes {}   // re-emits Back and Open from any pane
  impl ReviewPanes {
      pub fn new(key: PullRequestKey, back_label: impl Into<SharedString>, window: &mut Window, cx: &mut Context<Self>) -> Self;
      pub fn key(&self) -> &PullRequestKey;
      pub fn focused_view(&self) -> Entity<ReviewThreadView>;
      pub fn pane_count(&self) -> usize;
      pub fn focused_index(&self) -> usize;
  }
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub(crate) struct SavedPanes { tree: PaneTree<PaneState>, focused: usize, zoomed: bool }
  const VIEW_STATE: &str = "panes";
  ```
- Behavior:
  - `new` builds one pane (`diffs_locally: true`) and calls `load_view_state::<SavedPanes>(key, VIEW_STATE)`. When it resolves and `restored` is still false, it rebuilds the tree from it. Only leaf 0 gets `diffs_locally: true`. Any structural change sets `restored = true` first.
  - A split copies the focused pane's `pane_state()`, inserts the new pane with `diffs_locally: false`, focuses it and turns off zoom. Close, focus moves and zoom follow `PaneTree`. A close that leaves leaf 0 changed calls `set_diffs_locally(true)` on the new leaf 0.
  - Render: each leaf is wrapped in `div().id(("review-pane", entity_id))`. It captures any mouse-down to make that leaf focused. When more than one pane is visible, it has a 1px border, `cx.theme().ring` when focused and transparent otherwise. Splits render as `h_flex`/`v_flex`, with `first` getting `flex_basis(relative(ratio))` and `second` getting `flex_1()`. Between them sits a 6px-hit, 1px-line divider. The divider is dragged with `on_drag(SplitDrag { boundary })`, and the split container's `on_drag_move::<SplitDrag>` sets the ratio from the pointer's position in `event.bounds`. It acts only when `drag.boundary` matches. Zoomed shows only the focused leaf. The focused pane's `render_inspector_panel` sits to the right in a `resizable_panel`, as today, visible per `ReviewLayout.inspector`.
  - `save(cx)` builds `SavedPanes` from `tree.map(|pane| pane.read(cx).pane_state())` and calls `save_view_state`. It runs after every split, close, focus change, zoom and divider move, and on each pane's `ViewChanged`.
  - The window focuses the focused pane's `focus_handle` after every structural change and focus move.

- [ ] **Step 1: Write the failing tests** in `tests/panes.rs`. The window root is `ReviewPanes::new(featured(), "Review requests", ..)`. Keys go through `window.press("ctrl-b", cx); window.press("%", cx);`.
  - `prefix_percent_splits_side_by_side`: assert `pane_count() == 2` and `focused_index() == 1`.
  - `prefix_x_closes_the_focused_pane`: split, then close, and assert `pane_count() == 1`.
  - `prefix_x_on_the_only_pane_does_nothing`: assert `pane_count() == 1`.
  - `prefix_arrow_moves_focus_across_a_split`: split, press `ctrl-b` then `left`, and assert `focused_index() == 0`.
  - `panes_scroll_independently`: split. Call `reveal_file(a)` on pane 0 and `reveal_file(b)` on pane 1. Assert that the two `pane_state().scroll.file` values differ.
  - `reopening_the_pull_request_restores_its_panes`: split, `ctrl-b "`, and drop the view. Build a new `ReviewPanes` for the same key, then assert `pane_count() == 3` after `run_until_parked`.
  - `a_late_saved_layout_does_not_replace_a_split_made_meanwhile`: seed `save_view_state` with a 3-pane layout. Build `ReviewPanes` and split before calling `run_until_parked`. Assert `pane_count() == 2`.
  - `closing_the_diffing_pane_hands_the_duty_on`: split, focus pane 0, close it, and assert that the remaining pane has `diffs_locally == true` (a `#[doc(hidden)] pub fn diffs_locally(&self) -> bool`).
  - `typing_percent_in_the_composer_does_not_split`: focus the composer, `window.type_text("%", cx)`, and assert `pane_count() == 1`.
- [ ] **Step 2:** `cargo test -p workspace_ui --test panes`. Expected: compile failure.
- [ ] **Step 3: Implement `ReviewPanes`** with the interfaces and behavior above.
- [ ] **Step 4:** `cargo test -p workspace_ui --test panes`. Expected: all pass.
- [ ] **Step 5:** `git commit -S -m "Add tmux-style split panes to the review screen"`.

---

### Task 5: Put `ReviewPanes` in the window

**Files:**
- Modify: `crates/workspace_ui/src/workspace.rs`, `crates/workspace_ui/src/workspace/zed.rs`, any other caller the compiler names
- Modify: `crates/settings_keyboard_ui/src/lib.rs`
- Test: `crates/workspace_ui/tests/ui.rs`

**Interfaces:**
- `Route::Review(Entity<ReviewPanes>)`.
- `pub fn review_view(&self, cx: &App) -> Option<Entity<ReviewThreadView>>` returns the focused pane. The new `cx` parameter makes the compiler list every caller to update.
- `reviewing`, `current_repository` and the status bar read `panes.read(cx).key()`.
- The title bar passes the focused view to `PullRequestTitle::new` and `ReviewActions::new`, with `sidebar_open(focused.read(cx).outline_visible(cx))`.
- `ToggleSidebar` on a review calls `focused.update(cx, |pane, cx| pane.toggle_outline(cx))` in place of `ReviewLayout::toggle_outline`.
- The palette's `file:` entry and `ReviewWithAgent` go to the focused pane.
- The Keyboard page gets a section `Panes`, with rows in context `"ReviewPanes"`: "Split side by side", "Split top and bottom", "Move to the pane on the left", "… right", "… above", "… below", "Next pane", "Zoom the pane", "Close the pane".

- [ ] **Step 1: Write the failing test** in `tests/ui.rs`: `ctrl_b_percent_splits_the_open_review`. Call `open_featured`, then `window.press("ctrl-b")` and `window.press("%")`, and assert that `workspace.read(cx)` holds a `ReviewPanes` with `pane_count() == 2`. Add a `#[doc(hidden)] pub fn review_panes(&self) -> Option<Entity<ReviewPanes>>` on `Workspace`.
- [ ] **Step 2:** `cargo test -p workspace_ui --test ui`. Expected: compile failure.
- [ ] **Step 3: Implement** the changes listed above.
- [ ] **Step 4: Verify the whole workspace:** `cargo build --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`. Expected: no warnings and every test passing.
- [ ] **Step 5: Run the app** with `make run`. Open PR `octocat/hello-world#42`. Press `ctrl-b %`, then `ctrl-b "`, and click different files in each pane's tree. Drag a divider. Quit, reopen the PR, and confirm the same three panes, divider and scroll positions come back.
- [ ] **Step 6:** `git commit -S -m "Open reviews in split panes and list the pane shortcuts"`.

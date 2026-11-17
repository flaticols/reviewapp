---
title: Long lines in the diff: scroll sideways or wrap
phase: done
session: 9b27c84c-b472-430a-9828-f02a5f132b67
ticket:
updated: 2026-11-17
---

## Goal
A diff line longer than the card can be read in full: by default a file's code scrolls sideways,
and a setting wraps long lines instead.

## Approach
- The timeline is a virtualized list with one row per diff line, so a file has no single scroll
  container. Every code cell of a file tracks one `ScrollHandle` the review view keeps per path,
  and every cell's content is as wide as the file's widest line, so all of them clamp the shared
  offset alike. The width is shaped from the data, once per file and font, because rows off screen
  have no layout. A per-line scroller was rejected: lines would move out of step with each other.
- Gutters stay put; only code moves. In the split layout both sides share the handle and scroll
  together. A hunk header stays put and truncated: it says where the code below is.
- Cells restrict the wheel to the horizontal axis, otherwise gpui turns a vertical wheel over a
  horizontal scroller into sideways motion and the timeline stops scrolling.
- `Preferences::wrap_diff_lines`, off by default, with a switch under Appearance › Code. Wrapped rows
  grow to the text; numbers stay on the first line; the selection paints one quad per visual line.
- Not built: a visible horizontal scrollbar, and the file header's path, which still truncates.

## Tasks
### 1. Sideways scroll
Files: crates/diff_view_ui/src/lib.rs, crates/review_thread_ui/src/{lib,timeline}.rs,
crates/theme_ui/src/typography.rs, crates/fixtures/src/lib.rs, crates/workspace_ui/tests/ui.rs
Accept: `cargo test -p workspace_ui --test ui long_diff_lines_scroll` passes, and fails with the
scroller removed.
Verified: `cargo test -p workspace_ui --test ui long_diff_lines` → passed; with `track_scroll` removed it failed with "and the file's other lines move with it" (389px vs 189px).

### 2. Wrap setting
Files: crates/settings_model/src/lib.rs, crates/settings_appearance_ui/src/lib.rs,
crates/diff_view_ui/src/{lib,selectable_code}.rs, crates/workspace_ui/tests/ui.rs
Accept: `cargo test -p workspace_ui --test ui the_wrap_switch_wraps_long_diff_lines` passes, and
fails with the wrap branch removed.
Verified: `cargo test -p workspace_ui --test ui the_wrap_switch` → passed; with the wrap branch set to nowrap it failed with "by taking more rows of text: … 698px × 20px, one line is 20px".

### 3. Screenshots and suite
Files: crates/screenshots_ui/src/main.rs
Accept: scenes for a scrolled and a wrapped long line render and read correctly;
`cargo test -p workspace_ui` and `cargo clippy --workspace --all-targets` pass.
Verified: read `target/screenshots/02t-long-line-split-light.png` (scrolled, hunk header fixed), `02s-long-line-wrapped-light.png` (wrapped, with a selection across the wrapped rows), `02u-long-line-split-wrapped-dark.png`, `07d-appearance-dark.png`; `cargo test -p workspace_ui` → 48 + 22 + 2 + 2 + 2 passed, 1 ignored; `cargo clippy --workspace --all-targets` → no warnings from the code.

## Log
- 2026-11-17: Find does not yet bring a match that lies off to the right into view.
- 2026-11-17: `an_open_review_rewraps_when_the_setting_changes` passes with the `rewrapped` remeasure removed: the list lays visible rows out again every frame. The remeasure stays for rows off screen, whose cached heights no test observes.
- 2026-11-17: The width cache compares the `Rc<FileDiff>` it was shaped from; clearing it by hand missed the commit picker.
- 2026-11-17: The sample patch's line 139 of thread.rs got a long trailing comment so the scenes and tests have a line to scroll.

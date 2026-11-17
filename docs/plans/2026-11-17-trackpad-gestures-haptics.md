---
title: Trackpad gestures and haptic feedback on macOS
phase: done
session: 3c371506-baf3-4272-875d-49e5ae0e42a9
ticket:
updated: 2026-11-17
---

## Goal
A reviewer on a MacBook trackpad can pinch to zoom the interface, force click a diff line to open
it in Zed, and swipe to go back, and feels a tick when a pinch changes the size.

## Approach
- gpui 0.3.8 already turns `magnifyWithEvent:`, `pressureChangeWithEvent:` and `swipeWithEvent:` into
  `PinchEvent`, `MousePressureEvent` and `MouseDown(Navigate)`. The gestures are built on those; the
  NSView class is not patched. Smart magnify (two-finger double tap) and rotate never reach gpui and
  are not built.
- **Haptics.** A new `haptics_ui` crate holds a global performer. The app installs the system one,
  `NSHapticFeedbackManager` through `objc2-app-kit` 0.3.2, whose calls are safe functions, so the
  workspace's `unsafe_code = "forbid"` stays. With no performer installed (tests, screenshots) a
  haptic is a no-op; tests install a recorder.
- **Pinch.** The interface has five sizes, so a pinch does not scale continuously: the workspace sums
  `delta` and steps one size per 0.25 of magnification. The sum resets when a gesture starts, not
  when it ends, because macOS reports a cancelled pinch without an end. One `LevelChange` haptic per
  pinch event that changed the size; none at the limits and none for the keyboard shortcuts.
- **Force click.** The app resolves no symbols, so there is no go-to-definition to bind. Force click
  on a line's code opens that file at that line in Zed, where go-to-definition lives. It is bound to
  the code, not the gutter: a press on the gutter starts a comment or a selection. Pressure events
  stream, so the review view opens once per crossing into the force stage. The app adds no haptic
  here: the trackpad clicks a second time by itself at the force stage. AppKit's `NSEvent.h` gives
  views `NSPressureBehaviorPrimaryDeepClick` by default, with actuations on 1->2 and 2->1, and gpui
  sets no pressure configuration of its own.
- **Swipe.** `Navigate(Back)` dispatches `GoBack`, the same action as Cmd-[ and Escape, so a swipe
  never drops a draft that Escape would keep. There is no forward history, so `Forward` does nothing.
  A two-finger swipe is not synthesised from scroll deltas: it would fight the sideways diff scroll.
- Settings › Keyboard gets a Trackpad section, otherwise nothing in the app says the gestures exist.
  The swipe row names the System Settings option it depends on, which is off by default.

## Tasks
### 1. Haptics crate
Files: crates/haptics_ui/, Cargo.toml, crates/app_ui/src/main.rs
Accept: `cargo test -p haptics_ui` passes and `cargo build -p app_ui` links.
Verified: `cargo test -p haptics_ui` → 1 passed; `cargo build -p app_ui` → Finished.

### 2. Pinch to zoom
Files: crates/workspace_ui/src/workspace.rs, crates/workspace_ui/tests/ui.rs
Accept: `cargo test -p workspace_ui --test ui a_pinch` passes, and fails with the pinch listener
removed.
Verified: `cargo test -p workspace_ui --test ui a_pinch` → passed; with `.on_pinch` removed it failed with "spreading the fingers draws the list larger: 56px after 56px".

### 3. Force click opens the line in Zed
Files: crates/diff_view_ui/src/lib.rs, crates/review_thread_ui/src/{lib,timeline}.rs,
crates/workspace_ui/tests/ui.rs
Accept: `cargo test -p workspace_ui --test ui a_force_click` passes, and fails with the pressure
listener removed.
Verified: `cargo test -p workspace_ui --test ui a_force_click` → passed; with the pressure listener removed it failed with "one press opens Zed once, however long it is held: []" (0 vs 1), with the once-per-press guard removed 3 vs 1, with the rearm removed "the next press opens again" (1 vs 2).

### 4. Swipe back
Files: crates/workspace_ui/src/workspace.rs, crates/workspace_ui/tests/ui.rs
Accept: `cargo test -p workspace_ui --test ui a_swipe` passes, and fails with the listener removed.
Verified: `cargo test -p workspace_ui --test ui a_swipe` → passed; with the `GoBack` dispatch removed it failed with "a swipe back takes the diff off screen".

### 5. Trackpad section, screenshots and suite
Files: crates/settings_keyboard_ui/src/lib.rs, crates/screenshots_ui/src/main.rs,
crates/workspace_ui/tests/ui.rs
Accept: the Keyboard scene shows the Trackpad section; `cargo test -p workspace_ui` and
`cargo clippy --workspace --all-targets` pass.
Verified: read `target/screenshots/07g-keyboard-trackpad-light.png` (Trackpad section, three rows, the swipe row with its condition) and `01h-list-pinched-light.png` (list two sizes larger after a pinch); `the_keyboard_page_lists_the_trackpad_gestures` failed with "trackpad-pinch is on the Keyboard page" with the row ids removed; `cargo test -p workspace_ui` → 52 + 22 + 2 + 2 + 2 passed, 1 ignored; `cargo clippy --workspace --all-targets` → no warnings from the code.

## Log
- 2026-11-17: Not verified on hardware, and not verifiable from tests: that a real trackpad delivers
  these events to the window, and how the haptic feels.
- 2026-11-17: macOS sends `swipeWithEvent:` for a three-finger swipe only when System Settings ›
  Trackpad › More Gestures › "Swipe between pages" includes three fingers.
- 2026-11-17: A removed line has no place in the head file; force click opens Zed at its base-side
  number, which lands near where it was.
- 2026-11-17: The force-click test runs a stand-in `zed` script, and `zed_bridge` logs every launch,
  so each run of the test appends two lines to `~/.switchfold/logs/zed.log`.
- 2026-11-17: `cargo fmt --all -- --check` reports diffs in `review_thread_ui/src/panes/` and
  `app_state_ui/src/pull_request_store.rs`, which this change does not touch; left as they are.

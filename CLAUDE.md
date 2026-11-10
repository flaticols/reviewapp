# CLAUDE.md

## UI changes: the done gate

Unit tests and model assertions do not prove the UI works. A test can pass because a
setting flipped while the panel never moved on screen. So any change that touches what
the app draws or how it reacts to input has to pass this gate.

**Never say a UI change is done, works or is fixed until all five steps pass.** If one is
missing, write "UI not verified" and name the missing step. Don't round it up to done.

1. **Drive it like a user.** Add or extend a test in `crates/workspace_ui/tests/ui.rs`
   (or the crate's own `tests/`). Act through real input by element id: `click`,
   `right_click`, `press`, `input`, `scroll`, `drag_to`. Don't call view methods to
   fake the interaction.
2. **Assert on what was painted.** Use `window.find(id)` / `window.try_find(id)` after
   `window.render_frame(cx)`, and check `visible()`, `bounds()`, `label()`, `value()`,
   `checked()` or `focused()`. Model, entity and settings assertions may sit next to
   these, never instead of them.
3. **Watch it fail.** Break the change on purpose, for example by reverting the render
   line, and run the test. The new assertion has to fail with its own message. A test
   that passes on broken code proves nothing. Undo the break with an edit or
   `git checkout -- <file>`, never by moving a backup copy back: the copy's older
   timestamp stops cargo from rebuilding, and later runs test the broken code.
4. **Look at it.** Render the affected scene offscreen and read the PNG:
   `cargo run -p screenshots_ui -- target/screenshots <scene-name>`. If no scene shows
   the new state, add one to `crates/screenshots_ui/src/main.rs`.
5. **Run the whole suite.** `cargo test -p workspace_ui` passes.

The completion report names the test, quotes the failure message from step 3, and gives
the path of the PNG from step 4.

## Writing UI tests

- Only elements with an id that register for observation can be found. gpui-kit
  components such as `Button` register themselves. A plain `div()` needs `.id(...)`
  followed by `.test_support()`.
- A hidden element can still be in the tree. `resizable_panel().visible(false)` keeps
  its child laid out at zero size. Check `visible()`, not presence. Use the `on_screen`
  helper in `tests/ui.rs`.
- Overlays animate on wall-clock time. Call `settle(window, id, cx)` before clicking
  inside one.
- If `find` panics, its message lists every registered element path. Read that first
  before guessing ids.

## Screenshots cost tokens

Each PNG is about 2k tokens when read. The scene argument matches by substring and
renders both light and dark. `thread` matches eight files, so pass the full name, for
example `02-thread`, and read one theme.

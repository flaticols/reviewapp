# One file tree for split panes, go to file, and find in pane

When you split a review, every pane gets its own copy of the file tree. Two panes show
two identical trees, which wastes space and does nothing for navigation. The review
should have one file tree on the left. Clicking a file should scroll whichever pane you
last worked in. Two keyboard shortcuts come with it: ⌥⇧A opens a fuzzy go-to-file list,
and ⌘F finds text in the focused pane.

Assumption: every pane in a review shows the same pull request. `ReviewPanes` already
guarantees this.

## What stays the same

Each pane keeps everything it has today except the tree:

- its own tab row (Everything / Unresolved / Changes / Checks / Reviews), commit picker
  and Unified/Split toggle;
- its own scroll position, saved per pane as it is now;
- one long scroll through every file. Panes do not become one-file-at-a-time tabs.

The right sidebar is already shared and follows the focused pane. It does not change.

## Shared file tree

- `ReviewPanes` draws the left sidebar once: the back link, "Files · N of M viewed", the
  file tree and the "Threads" list. Panes no longer draw it.
- Clicking a file, a folder's file or a thread scrolls the focused pane to it. "Focused"
  means the pane that last got a click or a pane-focus shortcut. This is the pane
  `ReviewPanes::focused` already tracks. Clicking the tree must not move focus to another
  pane.
- The jump is today's `ReviewThreadView::reveal_file`. If the pane's current tab hides
  the file, the pane switches to Changes first, as it does now.
- If the pane shows a single commit that doesn't touch the file, the pane switches back
  to all commits and then jumps, as `comment_on_line` already does. The same applies to
  Threads clicks and ⌥⇧A.
- Folder folding and the selection belong to the one tree. Switching panes does not
  reset them.
- The tree's selection follows the file at the top of the focused pane. It updates as
  that pane scrolls or as focus moves. Setting it this way must not call `reveal_file`,
  and must not scroll the pane or the tree. If that file is inside a folded folder,
  nothing is selected and the folder stays folded.
- A click in the tree gives the tree keyboard focus, so arrow keys move the selection,
  and each move reveals the file in the focused pane.
- While the tree has keyboard focus, Esc returns focus to the focused pane. The review
  shortcuts (⌘F, ⌥⇧A, ⌘↵, ⌘⇧↵, ⌘⇧O, ⌘⇧A, ⌘⇧C) still reach the focused pane.
  `ReviewPanes` forwards them.
- Right-clicking a file opens the same file menu as today.
- ⌘B shows and hides the shared tree. Focus mode (⌘⇧F) hides and restores it. The
  per-pane show/hide goes away, and so do `PaneState::outline` and
  `ReviewThreadView::toggle_outline`.
- Whether the tree is hidden is remembered across restarts in preferences, as the right
  sidebar's `inspector_collapsed` is.
- Saved layouts that still contain `outline` open without error. Serde already ignores
  the unknown field. Keep it that way, and do not delete saved state.
- Tests that cover per-pane tree hiding change to cover the shared tree:
  `hiding_one_panes_tree_*` in `crates/workspace_ui/tests/panes.rs` and
  `hiding_a_panes_tree_redraws_the_title_bar` in `crates/workspace_ui/tests/ui.rs`.

## ⌥⇧A — Go to file

- Opens a dialog listing only the pull request's changed files, in the same style as the
  ⌘K palette (`command_palette_ui`).
- Confirming a file scrolls the focused pane to it, exactly like a tree click.
- Only active on the review screen, and not while a text field has focus. On macOS
  ⌥⇧A types "Å", and the composer and reply fields must still accept it.
- Matching is fuzzy, in the GoLand style:
  - query characters must appear in order in the path, ignoring case;
  - `svcgo` finds `internal/services/ocn/service.go`;
  - a match inside the file name ranks above a match spread over folders;
  - consecutive characters and matches at word starts (after `/`, `_`, `-`, `.`, or a
    lowercase-to-uppercase change) rank higher;
  - results are sorted best first, and ties keep the pull request's file order.
- The gpui-component `Command` filter only does case-insensitive substring matching
  (`CommandItem::matches`). Turn its filtering off (`Command::filterable(false)`) and
  feed it the ranked list on every query change.
- Write the matcher as a small function. Do not add a crate for it.
- ⌘K keeps working. Make it use the same matcher for its Files group. That is the
  builder's call if it complicates the palette.

## ⌘F — Find in pane

- Opens a find bar at the top of the focused pane, under its tab row. If the bar is
  already open, ⌘F focuses it and selects its text.
- Searches these rows, wherever the pane's current tab shows them: diff lines on both
  sides, the pull request description, comment and thread bodies, review bodies, and
  finding bodies. Text inside collapsed files is not searched.
- It does not search the Checks tab, agent run output, commit and push entries, author
  names, or the paths in file headers.
- Plain text, case-insensitive, no regex.
- Every match is highlighted. The current match has a stronger highlight and is scrolled
  into view.
- Enter goes to the next match and ⇧Enter to the previous one. Both wrap around.
- The bar shows "N of M", or "No results".
- Esc closes the bar, clears the highlights and returns focus to the pane. Esc inside the
  bar must not trigger the pane's Back action.
- Each pane has its own find bar and query. Switching tabs or new data re-runs the
  search.
- ⌘F is free today. ⌘⇧F stays focus mode.

## Left to the builder

- Where the shared tree's state lives: moved into `ReviewPanes`, or a new small entity
  `ReviewPanes` owns. It must not stay per pane.
- How find highlights are drawn in diff rows and markdown bodies.
- The exact ranking weights, as long as the examples above hold.
- The name of the go-to-file action and its menu entry.

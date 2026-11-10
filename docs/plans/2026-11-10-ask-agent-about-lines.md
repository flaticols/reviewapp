---
title: Ask an agent about selected diff lines
phase: done
session: 0237e7b4-8414-45a8-8c38-876d50701245
ticket:
updated: 2026-11-10
---

## Goal
A reviewer drags across line numbers in a diff to select several lines, asks an agent a
question about them, and gets an elaborated answer plus a short comment they can post on
those lines.

## Approach
- **Selection** is a range of diff line positions (hunk, line) in one file, kept on
  `ReviewThreadView`. Line numbers alone can't name a unified range that mixes removed and
  added lines. Pressing a gutter anchors the range, dragging over other lines extends it,
  shift-click extends from the last anchor; a plain click still starts a line comment.
  Both layouts paint the range as a row fill.
- **Surface**: the ask card sits in the `DiffRow::Threads` slot under the range's last
  head-side line (the nearest head line when the range is removals only), the same slot a
  line comment uses. Question field, then the streamed answer, then the suggested comment
  with Add comment and Edit. Rejected: a sheet or the inspector, which take the answer away
  from the code it is about.
- **Context**: `acp_client::question_prompt` carries the pull request, the numbered
  selection, the file's patch and the discussion on that file when sharing is on; the agent
  works in the pull request's checkout, so it can read callers and tests.
- **Short suggestion** comes through a new `suggest_comment` tool on Switchfold's MCP
  server, as findings do. Rejected: parsing a delimited section out of the reply, which
  breaks whenever an agent words it differently.
- **Run**: `AgentQuestion` in `agent_review_ui`, separate from `AgentRun`, which marks the
  agent as a reviewer and writes a review summary. Agent sessions start through a
  replaceable `AgentLauncher` global so UI tests script the events.
- gpui-kit stays at 0.7.1: every component the card needs is in it. 0.9.1 is a separate
  upgrade.

## Tasks
### 1. Prompt, `suggest_comment` tool and event in `acp_client`
Files: crates/acp_client/src/{prompt,findings,session,lib}.rs, src/bin/acp-mock-agent.rs, tests/mock_agent.rs.
Accept: `cargo test -p acp_client` passes, with a test that asks the mock agent and receives the answer and `AgentEvent::Suggestion`.
Verified: `cargo test -p acp_client` → 11 unit + 4 mock-agent tests passed, incl. `a_question_streams_the_answer_and_a_suggested_comment`.

### 2. Line range selection in `diff_view_ui`
Files: crates/diff_view_ui/src/lib.rs.
Accept: `cargo test -p diff_view_ui` passes.
Verified: `cargo test -p diff_view_ui` → 9 passed.

### 3. `AgentQuestion`, launcher seam and the ask card
Files: crates/agent_review_ui/src/{question,question_card,launcher,lib,run,run_card}.rs, crates/review_thread_ui/src/{ask,lib,rows,timeline,quote}.rs.
Accept: `cargo clippy --workspace --all-targets -- -D warnings` is clean.
Verified: `cargo clippy --workspace --all-targets -- -D warnings` → clean.

### 4. UI done gate
Files: crates/workspace_ui/tests/ui.rs, crates/screenshots_ui/src/main.rs.
Accept: new test in `tests/ui.rs` drags gutters, asks, sees answer and suggestion painted, adds the comment; it fails with its own message when the render is broken; `cargo test -p workspace_ui` passes; PNG of the scene read.
Verified: 7 tests in `tests/ui.rs` (drag, split layout, click and Escape, release outside the diff, the line menu, stop/edit/dismiss, failure); with the selection paint removed 4 fail, e.g. "line 137-137 is painted as selected", and with the cards removed "the answer shows under the selected lines"; `cargo test -p workspace_ui` → 37 ui + 22 panes passed; read `target/screenshots/02p-ask-lines-{dark,light}.png`, `02q-ask-answer-dark.png` and `05-agent-running-dark.png`. UI not verified: Shift-click (the test driver sends no modifiers) and the permission prompt on a question's card (a `PermissionPrompt` can't be built outside `acp_client`); the same prompt is covered on a review run.

### 5. Full checks
Accept: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
Verified: clippy clean; `cargo test --workspace` → 99 test binaries ok, 0 failed; `cargo fmt --all --check` clean for every file this change touches, 7 files already unformatted on main left alone.

## Log
- 2026-11-10: plan written. No key binding for asking: the drag opens the field focused, and the line's context menu has Ask agent….
- 2026-11-10: lines of a deleted file can't be asked about: the card needs a head-side line to sit under. A notification says so.
- 2026-11-10: the suggested comment anchors to one line, the last of the range; the store has no multi-line threads.
- 2026-11-10: questions go to the default agent and are one turn; no agent picker, no follow-ups, no transcript file.
- 2026-11-10: no real agent has been asked yet, only the mock agent over ACP; whether Claude Code or Codex calls `suggest_comment` is unproven. Not run because it spends the reviewer's quota.
- 2026-11-10: a question started while another agent works in the checkout uses it as it is instead of resetting it.
- 2026-11-10: `cargo fmt --all` reformats 7 files that are unformatted on main; reverted, not part of this change.

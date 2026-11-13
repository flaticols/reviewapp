---
title: Fewer permission prompts and follow-ups in agent reviews
phase: done
session: 63ebd6b8-aeba-4dc6-84e1-44fe795dd193
ticket:
updated: 2026-11-13
---

## Goal
Agents stop asking before shell commands that only read, when the reviewer turns that on. After an
agent review finishes a turn, the reviewer can reply in the same agent session.

## Approach
- 64 permission prompts in 23 local run logs were almost all shell commands that only read
  (`sed -n`, `grep`, `cat | head`, `git diff`). None were ACP Read tool calls, so the toggle classifies
  commands, not tool kinds. The user picked this over a kind-based toggle.
- The classifier is an allowlist, conservative by construction: every pipeline segment must be an
  allowed program with allowed arguments. No command substitution, backticks, process substitution,
  heredocs, background `&`, env assignments or file redirects (`2>&1` and `>/dev/null` are fine).
  Build tools (`go`, `cargo`, `npm`) are out: they run code from the pull request. Network still asks.
- Off by default, next to "Ask before an agent runs a command" in Settings › Agents, decided in
  `permission::screen`, so review runs and line questions both get it.
- Follow-ups: opt-in on `ReviewRequest`; after a turn ends the session stays open and waits for the
  next prompt. A new non-terminal `TurnEnded` event; `Finished`/`Failed` stay terminal. Stop, dismiss,
  dropping the control and app quit end the process. Resuming after a restart is out of scope.

## Tasks
### 1. Read-only command classifier and setting
Files: crates/agent_review_ui/src/read_only.rs, crates/agent_review_ui/src/permission.rs, crates/settings_model/src/lib.rs, crates/settings_agents_ui/src/lib.rs, crates/acp_client/src/session.rs
Accept: `cargo test -p agent_review_ui` passes with classifier cases taken from the real logs, allowed and refused; UI gate for the toggle and an auto-allowed prompt.
Verified: `cargo test -p agent_review_ui` → 6 passed; `agents_run_read_only_commands_unasked_once_the_setting_allows_it` passes and fails with "a command that only reads runs without asking" when `permission::screen` drops the check; scene 07c-agents shows the toggle; `cargo test -p workspace_ui` → 38 + 22 + 2 passed. Against the 60 logged commands with a command line, 18 now run unasked.

### 2. Follow-up turns in the same session
Files: crates/acp_client/src/session.rs, crates/acp_client/src/bin/acp-mock-agent.rs, crates/acp_client/tests/mock_agent.rs, crates/agent_review_ui/src/run.rs, crates/agent_review_ui/src/run_card.rs
Accept: mock-agent test shows the second prompt on the same session id and the process ending on stop; UI gate for the reply box; scene added.
Verified: `a_session_with_follow_ups_answers_in_the_same_session_until_stopped` (mock agent over real ACP) gets "Turn 2 in mock-session: Why line 31?" and the session future ends after a stop; `the_reviewer_replies_to_an_agent_in_the_same_session` passes and fails with "a finished turn offers a reply to the agent" (Reviews tab) and "the thread keeps the run's card, with its reply, while the agent waits" (Everything) when the reply box is removed; scene target/screenshots/05c-agent-reply-dark.png.

### 3. Full checks
Accept: `cargo test --workspace`, clippy on changed crates, `git diff --check`.
Verified: `cargo test --workspace` → all passed; clippy on acp_client, agent_review_ui, review_thread_ui, screenshots_ui, workspace_ui, settings_* → no new warnings; `git diff --check` clean.

## Log
- 2026-11-13: started; scope confirmed with the user (read-only commands, reply after a turn).
- 2026-11-13: added `sed s///` (without `w`/`e` flags) and awk `>=` after the log scan showed them in read-only pipelines.
- 2026-11-13: the timeline is a virtual list, so the UI test scrolls `timeline` (now observable) to reach the run card.
- 2026-11-13: the agent process ending on stop is shown by the run future finishing; the process kill itself is the existing drop path, not separately asserted.

# Switchfold Review — implementation plan

Desktop GitHub pull-request reviewer built on gpui-kit 0.7.1, from the Sketch
document "Switchfold Review" (`fa9bc26c-9781-4e18-95dd-1a2d8748746c`).

## Screens in the design (each in Dark and Light)

| Frame | Screen |
| --- | --- |
| 01 / 01b | PR list: sidebar (inboxes, saved filters, repositories), filter bar, rows; skeleton + empty states |
| 02 / 02b | PR thread: file/thread outline, single-scroll timeline with inline diffs, comments, agent findings, composer, review bar, details inspector |
| 03 | Submit review dialog (Comment / Approve / Request changes + summary) |
| 04 | Check log sheet (steps, expandable log, re-run) |
| 05a / 05 | Review with agent dialog; agent run in progress with command permission prompt |
| 06 | Sign in |
| 07 / 07b / 07c | Settings: Accounts, Repositories, Agents & editor |

Not designed: Settings · Appearance and Settings · Keyboard (listed in the settings nav).

## Workspace layout (granular crates)

Rule: any crate whose code touches gpui is suffixed `_ui`. Everything else is gpui-free.

```
crates/
  # model — plain data, no gpui, no I/O
  pr_model/                 RepoSlug, PullRequest, Inbox, saved filters, time formatting
  diff_model/               FileDiff, Hunk, DiffLine
  review_model/             threads, comments, findings, checks, timeline, PullRequestDetail
  settings_model/           accounts, repository mappings, agent configs, preferences
  fixtures/                 sample data reproducing the Sketch content

  # services — no gpui
  git_diff/                 gix: open clone, merge-base, tree diff → diff_model; unified patch parser
  github_api/               GitHub client trait, fixture impl, `gh` CLI session detection
  acp_client/               ACP connection, review sessions, permission requests, findings tool
  agent_registry/           detect installed agents and versions
  editor_launch/            Open in Zed
  keychain/                 token storage in the macOS Keychain

  # ui — gpui
  theme_ui/                 app tokens (diff, syntax, merged, accent), typography, theme switching
  widgets_ui/               status tags, avatars, counters, section headers, status icons
  syntax_ui/                tree-sitter highlighting via gpui-kit's highlighter
  app_state_ui/             shared entities: PR store, accounts, repositories, preferences, services
  chrome_ui/                title bar, status bar, command palette
  pr_list_ui/               PR list screen
  diff_view_ui/             file diff block: header, hunks, gutter, inline thread slots
  review_thread_ui/         PR thread screen: outline, timeline, composer, review bar, inspector
  submit_review_ui/         Submit review dialog
  agent_review_ui/          Review with agent dialog, run card, permission prompt
  check_log_ui/             check log sheet
  settings_ui/              settings shell + navigation
  settings_accounts_ui/
  settings_repositories_ui/
  settings_agents_ui/
  settings_appearance_ui/
  settings_keyboard_ui/
  sign_in_ui/               sign in screen
  app_ui/                   binary `switchfold`: window, Root, menus, key bindings, routing
```

## Status (2026-10-07)

Done: every designed screen in light and dark; Appearance and Keyboard pages; command
palette (⌘K); live GitHub data through `gh`; gix diffs for local clones; tree-sitter
highlighting; ACP client with permission prompts and the findings MCP tool; clone
discovery; Keychain tokens; offscreen screenshot tool; UI integration and ACP end-to-end
tests.

Open:
- Saved filters are fixed (Failing CI); creating one from the current filters is not built.
- Single-commit diffs in the commit range menu need per-commit diffs (local clone).
- Expanding hunk context and per-push diffs.
- GitHub notifications API: the bell lists review requests and mentions from the inboxes.
- The Claude Code adapter and `gh auth login --web` are wired but untested end to end.

## Phase 1 (approved)

- Every designed screen and state in both themes, with app tokens taken from the Foundations page.
- A fixture data set reproducing the design content, so the app can be compared frame by frame with Sketch.
- Real where cheap: `gh auth token` for auth, gix for diffs from a local clone, tree-sitter syntax highlighting,
  `zed` for Open in Zed.
- GitHub sync and ACP behind traits, with fixture implementations.

## Later phases

- GitHub REST/GraphQL sync for PR list, timeline, reviews and checks; review submission.
- ACP client (`agent-client-protocol` 3.0) driving real agents with permission prompts.
- Keychain-backed accounts and PAT validation; worktree management.

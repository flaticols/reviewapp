# Switchfold

A macOS desktop app for reviewing GitHub pull requests from local clones, with agents
reviewing over the Agent Client Protocol (ACP). Built with [gpui-kit](https://gpui-kit.com)
from the Sketch design "Switchfold Review".

## Run

```
cargo run -p app_ui --release                     # your GitHub pull requests, through `gh`
cargo run -p app_ui -- --sample                   # the design's sample workspace
cargo run -p app_ui -- flaticols/switchfold#482   # open a pull request at launch
cargo run -p app_ui -- --appearance dark          # override light/dark for this run
```

Build the app bundle (release build, the Icon Composer icon compiled with `actool`,
ad-hoc signed) with Nushell; `--install` also puts it in /Applications:

```
nu scripts/bundle.nu --install   # → /Applications/Switchfold Review.app (dev.switchfold.review)
```

The icon is `assets/AppIcon.icon`; open it in Icon Composer (inside Xcode) to edit it.

Sign-in uses the GitHub CLI: the first run adopts the account `gh` is signed in with.
Settings live in `~/Library/Application Support/dev.switchfold.review/`.

## What is real

| Area | Source |
| --- | --- |
| Pull request inboxes, details, review threads, timeline, checks, job logs | GitHub GraphQL + REST through `gh api` |
| Submit review, replies, line comments, re-run failed jobs | GitHub REST through `gh api` (only when you act) |
| Diffs | GitHub `patch` per file; `git_diff` also diffs local clones with gix |
| Syntax highlighting | tree-sitter through gpui-kit's highlighter |
| Agent reviews | ACP client (`agent-client-protocol` 3.0): launches any ACP agent, streams tool calls, routes permission prompts, collects findings through the `report_finding` MCP tool |
| Clone mapping | discovers clones under `~/src`, `~/Developer`, … and maps them to repositories |
| Open in Zed, Check out | `zed` launcher; `git worktree` per pull request |
| Accounts | `gh auth status`, `gh auth login --web`; personal access tokens in the Keychain |

Claude Code runs through `npx -y @agentclientprotocol/claude-agent-acp@latest`; any
other ACP agent can be added in Settings › Agents & editor.

## Layout

Crates are split by capability; every crate that touches gpui ends in `_ui`.

```
model      pr_model diff_model review_model settings_model fixtures
services   git_diff git_worktree clone_discovery github_api acp_client agent_registry editor_launch keychain
ui         theme_ui widgets_ui syntax_ui app_state_ui chrome_ui command_palette_ui
           pr_list_ui diff_view_ui review_thread_ui submit_review_ui agent_review_ui check_log_ui
           sign_in_ui github_auth_ui settings_ui settings_{accounts,repositories,agents,appearance,keyboard}_ui
           workspace_ui app_ui (binary `switchfold`) screenshots_ui (dev tool)
```

## Checks

```
cargo test --workspace                                   # unit, UI integration and ACP end-to-end tests
cargo test -p github_api -- --ignored live --nocapture   # reads your real GitHub data; PR=owner/name#N times one detail
cargo test -p workspace_ui --release --test scroll_perf -- --ignored --nocapture   # frame cost of scrolling 14,400 diff lines
cargo clippy --workspace --all-targets
cargo run -p screenshots_ui -- target/screenshots        # every screen, light and dark, offscreen
cargo run -p screenshots_ui -- --live target/screenshots # same, with your GitHub data
```

---
title: Prepare the repository for public sharing
phase: active
updated: 2026-11-11
---

## Goal

Replace company-specific examples with generic fixtures, credit the contributor,
and publish the current snapshot as one root commit on main.

## Approach

Keep behavior unchanged. Use octocat/hello-world for example links and a synthetic
commit hash. Preserve contributor credit in the [README](../../README.md) because
the new root commit replaces the authorship history on main. Push only main with
an explicit force-with-lease; other branches and retained pull request refs are
outside this rewrite.

## Tasks

### 1. Clean examples and credit the contributor

Accept: Inspect the diff, scan tracked files for company identifiers, and run Rust checks.
Verified: Tracked-content scan and diff whitespace check pass. All three menu tests
pass; ACP unit and mock-agent integration tests pass. Workspace build and Clippy
pass. Changed Rust files pass rustfmt. Workspace formatting has unrelated existing
failures. Workspace tests stop at clone_discovery because Git signing cannot resolve
the local user (uid 502). UI screenshots and live-service tests were not run because
this change only updates fixtures and documentation.

### 2. Publish a single root commit

Accept: Main has one commit and the remote main matches it after force-with-lease.
Verified: Publication blocked: GitHub API access fails and SSH cannot resolve the
local user (uid 502), including with the system SSH client. Prepare an unsigned root
commit with the configured personal identity; the same environment prevents signing.
Other branches and retained pull request refs still contain the original history.

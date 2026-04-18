<!--
PR titles are what end up in the release changelog (squash-merge + PR-title
parsing is the default strategy for Versionx itself). Follow Conventional
Commits, e.g.:
  feat(cli): add --output json-lines
  fix(adapter-node): handle missing packageManager field
  chore: bump rmcp to 1.5.3
Breaking changes: append `!` and include a BREAKING CHANGE footer in the PR body.
-->

## What changed

<!-- 1–3 sentences. Be concrete. -->

## Why

<!-- User-facing rationale. Link the issue / spec section. -->

## Test plan

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo nextest run --workspace`
- [ ] New unit tests for the added behavior
- [ ] `cargo doc --workspace --no-deps` (if docs changed)

## Screenshots / logs

<!-- Only if relevant. TUI or CLI output pastes go here. -->

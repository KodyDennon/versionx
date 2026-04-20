---
title: Full API (docs.rs)
description: Pointer to the full rustdoc-generated API reference hosted on docs.rs.
sidebar_position: 5
---

# Full API on docs.rs

The complete API reference — every public type, function, trait, and example — is hosted on docs.rs. It's auto-generated from the source's doc comments on every release.

- **Public SDK:** [docs.rs/versionx-sdk](https://docs.rs/versionx-sdk)
- **Adapter trait:** [docs.rs/versionx-adapter-trait](https://docs.rs/versionx-adapter-trait)
- **Runtime trait:** [docs.rs/versionx-runtime-trait](https://docs.rs/versionx-runtime-trait)
- **Config crate:** [docs.rs/versionx-config](https://docs.rs/versionx-config)
- **Events crate:** [docs.rs/versionx-events](https://docs.rs/versionx-events)

Every `pub` item carries a doc comment; `clippy::pedantic` enforces that in CI so nothing publishes without one.

## Browsing tips

- **Start at `Core`.** The top-level handle in the SDK. Most of the public surface hangs off it.
- **Follow the command options.** Each command has a `*Options` builder documented alongside its return type.
- **Use the search.** docs.rs search is fast and works across every crate listed above.

## See also

- [SDK overview](./overview) — the embedding story.
- [Plan / apply cookbook](./plan-apply-cookbook) — concrete recipes.

---
title: Migrating from asdf
description: Translate an asdf setup to Versionx. `.tool-versions` compatibility, plugin awareness, cutover steps.
sidebar_position: 5
---

# Migrating from asdf

You'll learn:

- How `.tool-versions` maps into `versionx.toml`.
- How to run asdf and Versionx side by side during cutover.
- Which asdf plugins have Versionx-native equivalents and which don't yet.

**Prerequisites:** a working asdf setup, Versionx [installed](/get-started/install).

## The fast path

```bash
versionx import --from asdf
```

Versionx reads `.tool-versions` in the current directory and writes a
`versionx.toml` that expresses the same pins.

## Config mapping

asdf config is sparser than mise:

| asdf | Versionx |
|---|---|
| `.tool-versions` | `versionx.toml` → `[runtimes]` (or `tool-versions = true`) |
| `$ASDF_DATA_DIR` | `$XDG_DATA_HOME/versionx` |
| Plugin list (`asdf plugin list`) | Runtime registry — see below |

## Plugin parity

Versionx has native runtime installers for the languages most people use asdf for:

| Language | asdf plugin | Versionx runtime |
|---|---|---|
| Node.js | `asdf-nodejs` | `versionx-runtime-node` |
| Python | `asdf-python` | `versionx-runtime-python` |
| Rust | `asdf-rust` | `versionx-runtime-rust` |

For languages Versionx doesn't support yet, stay on asdf for those specific tools. Versionx's shim dispatch respects PATH, so asdf-managed tools continue to work as long as asdf stays activated.

## Shim coexistence

Both tools manage a shim directory on PATH. During cutover:

1. Keep asdf activated.
2. Install Versionx and run `versionx install-shell-hook`.
3. Versionx's shims go in front of asdf's on PATH. Tools Versionx manages resolve through Versionx; tools it doesn't fall through to asdf.
4. Once every runtime you care about is covered by Versionx, remove the asdf activation line from your shell rc.

## Performance differences

asdf's shims shell out to Bash on every invocation — this is why running `node` under asdf can take ~50ms of startup overhead per call. Versionx's shims are native binaries with mmap'd PATH caches. Cold dispatch is sub-millisecond.

This matters if you have editors that run `node --version` on every save.

## What changes

- **`versionx status` replaces most of what you used `asdf current` for.**
- **`versionx install` replaces `asdf install` (and you can drop the language name for "install all pins in this repo").**
- **No `asdf plugin add` step.** Runtimes Versionx supports are built-in.

## What you gain

- Native-speed shims.
- Unified config (toolchain, dependencies, policy, releases) in one `versionx.toml`.
- Cross-repo state, fleet orchestration, and the plan/apply contract.

## Troubleshooting

- **`command not found` after cutover.** Open a new shell. PATH changes only apply to future shells.
- **`asdf: No such plugin`.** You've removed asdf but a shim for an asdf-managed tool is still resolving. Delete `~/.asdf` or run `versionx doctor` for a full PATH audit.
- **A language you use is not supported yet.** Keep asdf active for that tool and let Versionx handle the runtimes it already supports well.

## See also

- [Managing toolchains](/guides/managing-toolchains) — Versionx runtime workflow in depth.
- [Adding a runtime installer](/contributing/adding-a-runtime-installer) — if you want to port an asdf plugin to Versionx.

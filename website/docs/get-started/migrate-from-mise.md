---
title: Migrating from mise
description: Translate a mise setup to Versionx. Config mapping, shim coexistence, cutover steps.
sidebar_position: 4
---

# Migrating from mise

You'll learn:

- How a `.mise.toml` or `.tool-versions` maps into `versionx.toml`.
- How to run both tools side by side during cutover.
- What to do differently once you're fully on Versionx.

**Prerequisites:** a working mise setup, Versionx [installed](/get-started/install).

## The fast path

```bash
versionx migrate mise
```

Versionx reads `.mise.toml`, `mise.toml`, or `.tool-versions` in the current directory and writes a `versionx.toml` that expresses the same pins. Inspect the output; it's meant to be readable.

## Config mapping

| mise | Versionx |
|---|---|
| `.tool-versions` | `versionx.toml` → `[runtimes]` |
| `.mise.toml` / `mise.toml` | `versionx.toml` → `[runtimes]` and `[env]` |
| `[tools]` | `[runtimes]` |
| `[env]` | `[env]` |
| `[tasks]` | `[tasks]` (phased — full parity in 1.1) |

A minimal example:

```toml
# .mise.toml                            # versionx.toml
[tools]                                 [runtimes]
node = "22"                             node = "22"
python = "3.13"                         python = "3.13"
rust = "1.95"                           rust = "1.95"
```

## Shim coexistence during cutover

Both tools manage a shim directory on PATH. To avoid conflicts while migrating:

1. Keep mise activated as today.
2. Install Versionx and run `versionx install-shell-hook`.
3. Open a new shell. Versionx's shim directory goes to the front of PATH; its shims look up both Versionx-managed runtimes and any already-installed mise runtimes.
4. Verify: `which node` should print the Versionx shim path.
5. Run `versionx sync` to install anything that mise had but Versionx doesn't yet.

Once everything resolves through Versionx, remove the mise activation line from your shell rc and `brew uninstall mise` (or your install method's equivalent).

## What changes

- **Shell hook is Versionx's, not mise's.** `versionx install-shell-hook` replaces `mise activate`.
- **`versionx current` replaces `mise current`.** Same information, slightly richer output.
- **`versionx install` replaces `mise install`.** Package manager versions (pnpm, yarn, uv) are pinned directly instead of through corepack.
- **No `.tool-versions` anymore.** Everything lives in `versionx.toml`. If you have a strong preference for `.tool-versions`, Versionx continues to read it — you don't have to migrate the file, just configure Versionx to use it.

## What you gain

- Package managers (pnpm, yarn, uv) are first-class pinnable runtimes, not hacks via corepack.
- `versionx.toml` holds toolchain pins, dependency rules, policy rules, and release config — one file, one surface.
- Cross-repo awareness via the state DB. Mise is per-repo only.
- Plan/apply for toolchain installs. `versionx install --plan` shows exactly what would change.

## What doesn't carry over (yet)

- **Custom plugins.** mise has a plugin ecosystem; Versionx is in the process of growing one. For runtimes not yet in [Tier 1 or 2](/contributing/adding-a-runtime-installer), you'll need to stay on mise for those specific tools or file an issue.
- **Experimental mise features.** Anything behind `experimental = true` in mise doesn't have a mapping.

## Troubleshooting

- **`command not found` after cutover.** Open a new shell. PATH changes from `install-shell-hook` only apply to future shells.
- **`versionx: no runtimes installed yet`.** Run `versionx sync`. Installs anything in `versionx.toml` that isn't already present.
- **Mixed PATH entries.** `versionx doctor` inspects your shell and prints any conflicting entries with suggestions.

## See also

- [Managing toolchains](/guides/managing-toolchains) — the full Versionx runtime workflow.
- [`versionx.toml` reference](/reference/versionx-toml) — every key Versionx understands.

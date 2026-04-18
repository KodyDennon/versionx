# Security Policy

## Reporting a vulnerability

If you believe you've found a security vulnerability in Versionx, **please do not open a public issue**.

Instead, report it via one of:

1. **GitHub private security advisory** — preferred. Open the "Security" tab → "Report a vulnerability".
2. **Email** — `kodydennon@gmail.com` with subject line starting with `[SECURITY]`.

Include:
- A description of the issue and the impact.
- Steps to reproduce (minimal repro preferred).
- Affected versions / commits.
- Any mitigations you've identified.

## What to expect

- **Acknowledgement** within 3 business days.
- **Initial triage** within 7 business days.
- **Fix + advisory** coordinated with you; credit in the advisory unless you request otherwise.

## Scope

Versionx's security-sensitive surface includes:

- **Shim binary** — runs on every invocation of a shimmed tool; any escape into arbitrary code execution matters.
- **Policy engine (Luau sandbox)** — fleet-inherited policies are untrusted; any sandbox escape is critical.
- **MCP server** — tool-output injection into calling LLM context.
- **OIDC publishing flows** — any flow that exchanges a GitHub OIDC token for a registry credential.
- **State DB** — Zstd decompression of event blobs, SQL injection surfaces (we use parameterized queries everywhere, but reports welcome).
- **Git operations** — especially submodule/subtree flows and anything that shells out to `git`.

Out of scope:
- DoS via local resource exhaustion (report, but low priority).
- Issues in upstream tools we drive (npm/pip/cargo) — report those upstream.

## Supply chain

- Binary releases are **signed** (macOS: `rcodesign` notarization; Windows: Authenticode via Azure Key Vault).
- Artifacts ship with **sigstore attestations** on GitHub Releases.
- `cargo-deny` enforces advisory + license + banned-dep policies in CI.
- Dependencies are pinned to exact minor versions in `Cargo.toml`; full resolution tracked in `Cargo.lock`.

## Supported versions

During 0.x development, only the latest 0.x release receives security fixes. After 1.0, we will document a formal support window.

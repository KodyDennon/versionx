# Versionx

> One tool for runtimes, dependencies, and releases — cross-platform, cross-language, cross-package-manager.
>
> Written in Rust. Binary: `versionx`.

**Versionx** unifies the jobs that today require at least five separate tools — toolchain pinning (mise / asdf), dependency management (npm / pip / cargo), release orchestration (changesets / release-please), multi-repo coordination (submodules / subtrees / virtual monorepos), and policy enforcement — behind a single progressive-disclosure CLI that stays simple for one repo and scales to fleet management.

**The wedge:** cross-repo atomic release orchestration with plan/apply safety, polyglot version handling, and AI-as-client architecture. No existing tool sits at this intersection.

---

## Install

```bash
# macOS / Linux
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/KodyDennon/versionx/releases/latest/download/versionx-cli-installer.sh | sh

# Windows (PowerShell)
powershell -ExecutionPolicy ByPass -c "irm https://github.com/KodyDennon/versionx/releases/latest/download/versionx-cli-installer.ps1 | iex"

# Cargo
cargo install versionx-cli
```

Homebrew, Scoop, npm, and PyPI shim packages — see [the full install guide](https://kodydennon.github.io/versionx/get-started/install).

Wire the shell hook once:

```bash
versionx install-shell-hook     # writes to ~/.zshrc / ~/.bashrc / fish config
exec $SHELL                     # reload
versionx                        # bare run — auto-detects workspace + suggests next steps
```

---

## 60-second demo

```console
$ versionx
Versionx 0.7.0

Workspace  ./my-app    (node 22.11.0, python 3.13.1, rust 1.95)
Outdated   3 packages in apps/web  (axios ^1.6 → ^1.7)
Policy     clean
Ready      release plan   (last release 12d ago)

What next?
  versionx status                show ecosystem + release health
  versionx update --plan         preview dependency bumps
  versionx release plan          propose the next release
```

Every mutating command produces a plan you can inspect:

```console
$ versionx update --plan > plan.json
$ versionx apply plan.json
```

The same plan/apply contract covers dependency updates, release bumps, toolchain installs, and policy changes — for humans and AI agents alike.

---

## What's in the box

- **Runtime & toolchain management** — mise/asdf replacement with native-speed shims.
- **Polyglot dependency handling** — unified status / update / audit across npm, pip, cargo, bundler, maven, gradle.
- **Release orchestration** — SemVer, CalVer, PR-title parsing, changesets. Multi-ecosystem. Multi-repo. Plan / approve / apply / rollback.
- **Policy engine** — declarative TOML + sandboxed Luau. Waivers with mandatory expiry.
- **AI as a client, not a component** — first-class MCP server. BYO API key. No bundled LLM.

---

## Status

**0.7 feature-complete.** 30 crates, 280+ tests. Workspace discovery, content-hash bump planner, release engine with rollback, policy engine, MCP server, versiond JSON-RPC daemon, TUI dashboard, cross-repo saga protocol.

Road to 1.0: hardening, Windows parity, ecosystem breadth. See the [roadmap](https://kodydennon.github.io/versionx/roadmap).

---

## Docs

<table>
<tr>
<td width="33%" valign="top">

### Run it on your repo

Zero config. Bare `versionx` detects your ecosystems and suggests next steps.

[Quickstart →](https://kodydennon.github.io/versionx/get-started/quickstart)

</td>
<td width="33%" valign="top">

### Drive it from an agent

MCP, JSON-RPC, HTTP. Every mutation is plan/apply with Blake3-hashed prerequisites.

[Integrations →](https://kodydennon.github.io/versionx/integrations/mcp/overview)

</td>
<td width="33%" valign="top">

### Contribute

30-crate Rust workspace. `cargo xtask ci` runs what CI runs.

[Contributing →](https://kodydennon.github.io/versionx/contributing/dev-environment)

</td>
</tr>
</table>

Full documentation: **[kodydennon.github.io/versionx](https://kodydennon.github.io/versionx)**

---

## Community

- [Discussions](https://github.com/KodyDennon/versionx/discussions) — design questions, ideas, feedback.
- [Issues](https://github.com/KodyDennon/versionx/issues) — bugs and feature requests.

---

## License

Licensed under the [Apache License, Version 2.0](./LICENSE-APACHE).

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Versionx shall be licensed as above, without any additional terms or conditions.

## Security

Report vulnerabilities per [SECURITY.md](./SECURITY.md). Please do not open public issues for suspected vulnerabilities.

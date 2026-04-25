# Versionx

> One tool for runtimes, dependencies, and releases — cross-platform, cross-language, cross-package-manager.
>
> Written in Rust. Binary: `versionx`.

**Versionx** unifies the jobs that today require at least five separate tools — toolchain pinning (mise / asdf), dependency management (npm / pip / cargo), release orchestration (changesets / release-please), multi-repo coordination (submodules / subtrees / virtual monorepos), and policy enforcement — behind a single progressive-disclosure CLI that stays simple for one repo and scales to fleet management.

**The wedge:** cross-repo atomic release orchestration with plan/apply safety, polyglot version handling, and AI-as-client architecture. No existing tool sits at this intersection.

---

## Install

```bash
# Alpha testers: download the newest prerelease for your platform:
#   https://github.com/KodyDennon/versionx/releases

# Or build from a cloned checkout
git clone https://github.com/KodyDennon/versionx
cd versionx
cargo install --path crates/versionx-cli
```

GitHub Releases are the real public install surface today. Homebrew, Scoop, npm,
and PyPI channels are planned after the alpha hardening pass.

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
versionx 0.1.0 · ./my-app
  git✓ · config✗ · lock✗ · daemon✗ · 3 components discovered

  → run `versionx init` to synthesize a versionx.toml for this workspace.
  → run `versionx daemon start` (or `versionx install-shell-hook`) for warm caching.
```

Current alpha workflow:

```console
$ versionx init
$ versionx sync
$ versionx release plan
$ versionx release approve <plan-id>
$ versionx release apply <plan-id>
```

The release planner, workspace discovery, runtime install path, and MCP server are
real today. Dependency-update planning and broader automation surfaces are still
landing.

---

## What's in the box

- **Workspace discovery** — Node, Python, Rust, and mixed workspaces are detected with no config.
- **Runtime management** — install, pin, and list core runtimes with native shims + shell hook support.
- **Release planning** — propose, approve, apply, rollback, snapshot, and prerelease flows are available in the CLI alpha.
- **Policy engine** — starter policies, checks, lockfile verification, and expiring waivers are implemented.
- **AI as a client** — MCP server with a compact tool surface and no bundled model.

---

## Status

**0.1 alpha, publicly testable.** The foundation is real: workspace discovery,
runtime install path, release planning, policy checks, daemon, and MCP server.
The roadmap from here is hardening the alpha surface and filling the missing
automation/features before a broader 1.0 push.

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

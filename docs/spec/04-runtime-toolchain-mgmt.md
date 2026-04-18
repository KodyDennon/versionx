# 04 — Runtime & Toolchain Management

## Scope
The mise/asdf replacement piece: downloading, installing, pinning, and shimming language runtimes per-repo. Also covers tool-level version pinning (pnpm, uv, etc.) which we treat as "runtimes" for consistency — critical because Node 25+ drops corepack.

## Contract
After reading this file you should be able to: implement a `RuntimeInstaller` for a new language, understand how shims resolve the right version, and know how Versionx's per-repo pinning interacts with global user installs.

---

## 1. Why own this layer

- **Consistency**: one `versionx.toml` declares everything — no separate `.tool-versions` + `package.json engines` + `pyproject.toml requires-python` + `rust-toolchain.toml` (though all of those are **read** for compat).
- **Speed**: Rust-native shim that resolves the version in <5ms cold (<1ms warm), vs asdf's bash overhead.
- **Integration**: runtime state lives in the same state DB as repos; `versionx fleet query "node < 20"` just works.
- **Distribution**: a single static binary that installs language toolchains is a strong UX vs installing mise, then installing mise plugins, then pinning.
- **Corepack is being removed.** Node TSC voted Node 25+ stops shipping corepack. Vx must own pnpm/yarn installation directly.

We do **not** reimplement distro package managers. We download official (or vetted) builds and extract them into the XDG-compliant runtimes dir.

---

## 2. The `RuntimeInstaller` trait

```rust
#[async_trait]
pub trait RuntimeInstaller: Send + Sync {
    /// "node", "python", "rust", "go", "ruby", "jvm", "pnpm", "uv", ...
    fn id(&self) -> &'static str;

    /// Human-friendly: "Node.js", "CPython", "Rust toolchain", ...
    fn display_name(&self) -> &'static str;

    /// Resolve a version spec ("20", "^20", "lts", "stable") to a concrete version.
    async fn resolve_version(&self, spec: &VersionSpec, ctx: &InstallerContext) -> Result<ResolvedVersion, InstallerError>;

    /// List versions available for install from configured providers.
    async fn available_versions(&self, ctx: &InstallerContext) -> Result<Vec<ResolvedVersion>, InstallerError>;

    /// Determine whether this version is already installed.
    async fn is_installed(&self, version: &ResolvedVersion, ctx: &InstallerContext) -> bool;

    /// Install the given version. Idempotent.
    async fn install(&self, version: &ResolvedVersion, ctx: &InstallerContext) -> Result<Installation, InstallerError>;

    /// Uninstall. Never touches other installations.
    async fn uninstall(&self, version: &ResolvedVersion, ctx: &InstallerContext) -> Result<(), InstallerError>;

    /// Return the paths that should be shimmed (relative to the install root).
    fn shim_binaries(&self, installation: &Installation) -> Vec<ShimEntry>;

    /// Optional post-install patching (e.g., python-build-standalone sysconfig fix).
    async fn post_install(&self, installation: &Installation, ctx: &InstallerContext) -> Result<(), InstallerError> {
        Ok(())
    }

    /// Optional: health check to verify the install actually runs.
    async fn verify(&self, installation: &Installation) -> Result<(), InstallerError> {
        Ok(())
    }
}
```

### Supporting types

```rust
pub struct InstallerContext {
    pub runtimes_dir: Utf8PathBuf,      // $XDG_DATA_HOME/versionx/runtimes/<id>/
    pub cache_dir: Utf8PathBuf,
    pub http: reqwest::Client,
    pub events: EventSender,
    pub platform: Platform,             // os, arch, libc
}

pub struct ResolvedVersion {
    pub version: String,                // semantic: "20.11.1"
    pub channel: Option<String>,        // "lts", "stable", "nightly-2024-04-18"
    pub source: String,                 // "nodejs.org", "python-build-standalone", "rv"
    pub sha256: Option<String>,         // expected checksum (SHA-256 for interop)
}

pub struct Installation {
    pub version: ResolvedVersion,
    pub install_path: Utf8PathBuf,
    pub installed_at: DateTime<Utc>,
}

pub struct ShimEntry {
    pub name: String,                   // "node"
    pub target: Utf8PathBuf,            // path inside install dir
    pub kind: ShimKind,                 // Executable | Script
}
```

---

## 3. Shims — how version resolution works

### 3.1 The shim binary

- **Linux/macOS**: Symlinks pointing to a single `versionx-shim` Rust binary.
- **Windows**: A small (~200KB) fast **wrapper .exe** copied (or hardlinked where same-volume) per shimmed command (`node.exe`, `npm.exe`, `pnpm.exe`, etc.). This follows the **Volta trampoline pattern** (proven in production by Volta since 2019).
  - **Why .exe copies?**: Windows symlinks require Developer Mode or Admin. Hardlinks break across drives. A tiny wrapper ensures 100% compatibility at sub-1ms overhead and works without any elevated privileges.
  - **Dispatch**: The wrapper reads its own filename (`argv[0]`), looks up the resolved binary in an mmap'd PATH cache, and `CreateProcess`es the real binary with inherited handles.

**Performance target**: <5ms cold, <1ms warm. Achieved by:
- Pre-resolving PATH per directory and caching to a small binary file (mmap'd). Invalidation via mtime check on `versionx.toml` / `.tool-versions` / `package.json`.
- No TOML parsing on the hot path — only a binary-format cache read.
- Written in Rust, LTO'd, stripped, `panic=abort`.
- Target shim binary size: ~200KB.

### 3.2 PATH integration

The shim dir is at the XDG location:
- Linux: `$XDG_DATA_HOME/versionx/shims` (default `~/.local/share/versionx/shims`)
- macOS: `~/Library/Application Support/versionx/shims`
- Windows: `%LOCALAPPDATA%\versionx\shims`

Shell-hook activation prepends this dir to PATH once per login:
- bash / zsh: `eval "$(versionx activate bash)"` in rc file. Hook sets PATH, starts the daemon, and adds a chdir hook to pre-warm the resolution cache.
- fish: `versionx activate fish | source` in `config.fish`.
- PowerShell: `Invoke-Expression (& versionx activate pwsh)` in profile.
- Windows cmd: supported but minimal (daemon does not auto-start).
- Containers: `ENV PATH="/root/.local/share/versionx/shims:$PATH"` in Dockerfile or via `versionx docker-image` helper.

**Shim vs activate**: Activate mode is primary in v1.0 — zero per-exec cost, daemon pre-warms caches on `cd`. Shims exist for non-interactive contexts (IDE integrations, cron, systemd services) where shell hooks can't fire.

### 3.3 Fallback when no `versionx.toml` exists

Shim walks up for a config. If none found:
- Uses the **user-level default** (`versionx global set node 20.11.1` writes to `$XDG_CONFIG_HOME/versionx/global.toml`).
- If no global default either: fail loudly with a clear error, OR fall back to system `node` per `[advanced] shim_fallback = "system" | "error"` (default: `error`).

### 3.4 Escape hatches

- `versionx exec --runtime node@18.20.0 -- node server.js` — run with a specific version without modifying the repo config.
- `VERSIONX_<TOOL>_VERSION` env var — per-process override.
- `versionx which node` — show what the shim would resolve to and why.

---

## 4. Per-installer implementation notes

### 4.1 Node (`versionx-runtime-node`)

**Source**: official nodejs.org releases. Mirror via `NODEJS_MIRROR` env var or `[runtimes.providers] node = "https://..."`.

**Distribution format**: tarballs (Linux/macOS), zip (Windows).

**Install layout**: `$XDG_DATA_HOME/versionx/runtimes/node/<version>/` with unmodified contents of the extracted tarball (`bin/`, `include/`, `lib/`, `share/`).

**Shims**: `node`, `npm`, `npx`. **No corepack.** Pnpm/yarn are separate runtimes installed by Versionx directly (see §4.7).

**LTS resolution**: `"lts"` → latest LTS major; `"lts/iron"` → specific codename. Resolved via `https://nodejs.org/dist/index.json` with caching (never block prompt on network — cached metadata with explicit TTL).

### 4.2 Python (`versionx-runtime-python`)

**Source**: [python-build-standalone](https://github.com/astral-sh/python-build-standalone) (astral-sh builds, formerly indygreg). Static, portable, no system dependencies. Used by uv.

**Known gotchas we handle** (documented in python-build-standalone's `docs/quirks.rst`):
1. **sysconfig patching**: `_sysconfigdata_*.py` and Makefiles contain absolute build-time paths. Versionx runs `sysconfigpatcher` post-extract to relocate them — required for native-extension builds.
2. **Windows `pip.exe`**: does not exist in PBS builds. Versionx generates `pip.exe` / `pip3.exe` shims as thin wrappers calling `python -m pip`.
3. **macOS SSL**: references `/private/etc/ssl`. `versionx doctor` checks `SSL_CERT_FILE`/`SSL_CERT_DIR` and warns if missing for users with custom CAs.
4. **Terminfo**: REPL backspace/arrows break if `TERMINFO_DIRS` is unset; `versionx activate` exports a sane default.
5. **SQLite drift**: PBS statically inlines sqlite, potentially newer/older than official CPython. Pin-aware.
6. **No Tix on Linux/macOS**. Documented.

**Install layout**: `$XDG_DATA_HOME/versionx/runtimes/python/<version>/`.

**Shims**: `python`, `python3`, `pip`, `pip3` (generated on Windows).

**Virtualenv management**: separate from runtime install. Versionx **delegates venv creation to uv/poetry** (see `03-ecosystem-adapters.md §5.2`). Only for pip-only projects does Versionx create its own venv.

**PyPy / alternatives**: `[runtimes] python = "pypy@7.3"` supported via a subtype; resolved to pypy.org builds.

### 4.3 Rust (`versionx-runtime-rust`)

**Source**: `rustup`'s distribution channel. We wrap rustup, we don't reimplement it.

**Strategy**:
- Call `rustup` if present; auto-install `rustup-init` if not.
- Set `RUSTUP_TOOLCHAIN` env per invocation to force the right toolchain.
- **Never** set `RUSTC` (rustup 1.25+ regression — issue #3031 — breaks `+toolchain` overrides).
- Shared `RUSTUP_HOME` across repos (toolchain sharing is desirable); per-project `RUSTUP_TOOLCHAIN` for selection.
- Pin rustup version in lockfile; don't let `rustup self-update` run unattended.

**Install layout**: `$XDG_DATA_HOME/versionx/runtimes/rust/toolchains/<version>/` (standard rustup layout under our `RUSTUP_HOME`).

**Shims**: `cargo`, `rustc`, `rustup`, `rustfmt`, `clippy-driver`. The shim invokes rustup's own proxies with the correct toolchain env var.

**`rust-toolchain.toml`** compatibility: if a repo has `rust-toolchain.toml`, Versionx treats it as a source of truth alongside `versionx.toml`, preferring `versionx.toml` on conflict with a warning.

### 4.4 Go (`versionx-runtime-go`) — v1.1

**Source**: official go.dev downloads.

**Install layout**: `$XDG_DATA_HOME/versionx/runtimes/go/<version>/` with `bin/go`, `pkg/`, etc.

**Shims**: `go`, `gofmt`.

**`GOTOOLCHAIN` support**: Go 1.21+ can auto-switch toolchains. We set `GOTOOLCHAIN=local` in shimmed invocations and handle switching ourselves via Versionx config.

### 4.5 Ruby (`versionx-runtime-ruby`) — v1.1

**Primary source**: **[rv](https://github.com/spinel-coop/rv-ruby) prebuilt binaries** (spinel-coop, 2025). First real alternative to compile-from-source — fast (<1s) installs for common versions on macOS (Apple Silicon + Intel), Linux. Gives Ruby python-build-standalone parity.

**Fallback**: `ruby-build` where `rv` doesn't have a version. Slower (compile from source), requires build deps.

**Windows**: `RubyInstaller` for Windows. Documented limitations for version coverage.

**Install layout**: `$XDG_DATA_HOME/versionx/runtimes/ruby/<version>/`.

**Shims**: `ruby`, `irb`, `gem`, `bundle` (if bundler installed).

### 4.6 JVM (`versionx-runtime-jvm`) — v1.2

**Source**: **[foojay Disco API](https://api.foojay.io)** — canonical metadata source used by Gradle's auto-toolchain plugin. Supports filtering by distribution, arch, JDK/JRE, version.

**Default distribution**: **Eclipse Temurin (Adoptium)**. Alternates: Azul Zulu, Amazon Corretto, GraalVM (for native-image).

**Install layout**: `$XDG_DATA_HOME/versionx/runtimes/jvm/<vendor>-<version>/`.

**Shims**: `java`, `javac`, `jar`, `jshell`.

**Selection**: `[runtimes] jvm = "21"` → latest LTS 21 Temurin. `jvm = { version = "21", distribution = "corretto" }` → vendor-specific.

**Maven/Gradle**: separate runtimes:
- `maven = "3.9.6"` → download from Apache archives.
- `gradle = "8.6"` → download from gradle.org.

### 4.7 Package managers as runtimes (the corepack replacement)

Critical post-Node-25: Versionx manages **pnpm**, **yarn**, **npm-version-overrides**, **uv**, **poetry**, **bundler** as first-class runtimes. Each exposes the `RuntimeInstaller` trait and gets shimmed.

| Tool | Source | Notes |
|---|---|---|
| `pnpm` | GitHub Releases (standalone binary) | pnpm v10+ supports `manage-package-manager-versions` — respected. Versionx reads `packageManager` field and installs the exact version. |
| `yarn` (classic) | npm (bootstrap via downloaded Node) | v1.x is essentially frozen. |
| `yarn` (berry) | GitHub Releases | Yarn 4+ supports self-managing via `.yarn/releases/`. Versionx respects it. |
| `uv` | GitHub Releases (astral-sh/uv single binary) | Fast, simple. |
| `poetry` | `install.python-poetry.org` installer | Runs against the pinned Python. |
| `bundler` | Installed via `gem install bundler -v <version>` against pinned Ruby | Explicit pin supported. |

When `packageManager` in `package.json` says `pnpm@8.15.0`:
1. Versionx installs pnpm 8.15.0 into its runtime store.
2. Shims `pnpm` to resolve to that version when inside the repo.
3. `corepack` is never invoked, never required.
4. If the user has corepack enabled system-wide, the Versionx shim dir prepends PATH so our pnpm wins.

---

## 5. Platform & architecture matrix

| Platform | Arch | Notes |
|---|---|---|
| Linux | x86_64 (glibc + musl) | glibc primary; musl builds for Alpine |
| Linux | aarch64 | Raspberry Pi 4/5, AWS Graviton, etc. |
| macOS | x86_64 | Intel Macs, still common |
| macOS | aarch64 | Apple Silicon — primary dev platform for many |
| Windows | x86_64 | Full support |
| Windows | aarch64 | Best-effort (some ecosystems lack builds) |

The installer trait exposes `ctx.platform`; each installer selects the right artifact URL.

**Build infrastructure**:
- **cargo-zigbuild** on Linux runners for Linux (glibc + musl) + macOS targets.
- **Dedicated Windows runner** for MSVC builds + Authenticode signing (cargo-zigbuild does not support `+crt-static`).
- **macOS notarization via `rcodesign`** (Gregory Szorc's pure-Rust impl) from any host.
- **Windows EV signing via Azure Key Vault + AzureSignTool** (physical USB EV keys are deprecated under CA/B Forum 2024 rules).

---

## 6. Security

### 6.1 Checksum verification
Every install:
1. Downloads the archive + the official checksum file (SHA-256 typically).
2. Verifies checksum before extraction.
3. Records the verified checksum in the lockfile.

Mismatch = install aborts, archive quarantined, user notified.

### 6.2 Signature verification (where offered)
- Node.js: GPG-signed SHASUMS. Verify against the known Node.js release keys.
- Python builds: astral-sh signs releases via sigstore; verify.
- rv: sigstore-signed releases; verify.
- Others: best-effort; document which installers verify signatures.

### 6.3 TOFU model for custom providers
If a user configures a custom mirror, first-fetch records the checksum; subsequent fetches must match. `versionx runtime pin-checksum` command to re-trust after intentional changes.

### 6.4 Sandboxing installs
Install is a straight download + extract + sysconfig-patch (for Python). **No arbitrary post-install scripts run.** This is a deliberate difference from asdf plugins that can execute arbitrary bash. We accept that some edge cases won't be supported in exchange for security.

---

## 7. Global vs per-repo vs per-shell state

| Scope | Set via | Stored in | Resolved by |
|---|---|---|---|
| Per-repo | `versionx.toml [runtimes]` | Repo, committed | Shim, by walking up |
| User default | `versionx global set <tool> <version>` | `$XDG_CONFIG_HOME/versionx/global.toml` | Shim, fallback |
| Per-shell | `versionx shell use <tool>@<version>` | Env var for current shell | Shim, env-var priority |
| Per-invocation | `versionx exec --runtime` or `VERSIONX_<TOOL>_VERSION=` | env var | Shim, highest priority |

Resolution priority (highest first): per-invocation env > per-shell env > repo config > user default > shim_fallback.

---

## 8. Reclaiming space

Installed runtimes pile up. Versionx ships:

- `versionx runtime list` — show all installs, last-used timestamps, sizes.
- `versionx runtime prune` — remove runtimes with no repo pinning them, last-used > 90 days.
- `versionx runtime prune --keep 3 --per-major` — keep latest 3 per major version.

Pinning a runtime to a specific repo counts as "in use" for prune purposes.

---

## 9. Testing

### 9.1 Installer tests
- Unit: version parsing, URL construction per platform.
- Integration: actually install a small runtime (pnpm, uv are fast and small) in CI on all supported platforms.

### 9.2 Shim tests
- **Benchmark**: `versionx shim node --version` must complete in <5ms cold, <1ms warm. Regression is a release blocker.
- Correctness: fixture repos with different pin configs; shim must select the right version.

### 9.3 Signal handling
- Shim relays SIGINT/SIGTERM correctly (test with slow-running child).
- Exit code propagation verified on all platforms.

---

## 10. Migration from mise/asdf

`versionx import` reads `.tool-versions`, `.mise.toml`, `.nvmrc`, `.python-version`, `rust-toolchain.toml`, and `package.json#engines`/`packageManager` fields:

```bash
$ versionx import
Detected .mise.toml with: node 20.11.1, python 3.12.2, pnpm 8.15.0
Created versionx.toml with runtimes + ecosystems.node.package_manager sections.
```

Reverse: `versionx export --format mise|asdf` writes compatibility files for the migration period.

**Read-only compatibility**: if a repo has `.tool-versions` or `.mise.toml` and no `versionx.toml`, versionx reads them at runtime without requiring `versionx init`. Respects user intent not to commit to Versionx yet.

---

## 11. Non-goals

- Not a cross-language build system (that's the task runner, see §10-mvp-and-roadmap §2.2).
- Not a replacement for system package managers (apt/brew/pacman).
- Not a container image builder — we shim binaries, we don't containerize them.
- Not a "pip install this Python package globally" tool; use per-repo or explicit `uv tool install` for global CLIs.
- Not reimplementing rustup, corepack, or foojay — we wrap or replace selectively.

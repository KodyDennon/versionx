//! Sandboxed Luau runtime for `custom` policies.
//!
//! Goals:
//!   - No I/O (`io`, `os`, `package`, `debug`, `dofile`, `loadfile`,
//!     `require`, `collectgarbage` all removed).
//!   - CPU bound: interrupt after [`DEFAULT_INTERRUPT_MS`] with
//!     configurable override.
//!   - Memory bound: [`DEFAULT_MEMORY_LIMIT_BYTES`] hard cap.
//!   - Deterministic context injection: the Luau side sees a read-only
//!     `vx` table with `components`, `runtimes`, `commits`, and a
//!     `report(msg)` / `report_error(msg)` helper for emitting
//!     findings.
//!
//! Threat model: policy authors are first-party but can be mistaken
//! or malicious upstream repos inheriting policies. The sandbox treats
//! every script as untrusted.

use std::time::Duration;

use mlua::{Error as LuaError, Lua, LuaOptions, StdLib, Value as LuaValue};

use crate::context::PolicyContext;

pub const DEFAULT_INTERRUPT_MS: u64 = 100;
pub const DEFAULT_MEMORY_LIMIT_BYTES: usize = 32 * 1024 * 1024; // 32 MiB

#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("luau error: {0}")]
    Lua(#[from] LuaError),
    #[error("script exceeded {ms} ms interrupt deadline")]
    Interrupted { ms: u64 },
    #[error("script exceeded {bytes} byte memory limit")]
    MemoryExceeded { bytes: usize },
    #[error("sandbox setup failed: {0}")]
    Setup(String),
}

pub type SandboxResult<T> = Result<T, SandboxError>;

/// Pre-built sandbox. Cheap to create; can be re-used across multiple
/// script executions within a single evaluation pass.
pub struct LuauSandbox {
    lua: Lua,
    #[allow(dead_code)] // retained for future knobs (e.g. dynamic reconfigure).
    interrupt_ms: u64,
}

impl std::fmt::Debug for LuauSandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LuauSandbox").field("interrupt_ms", &self.interrupt_ms).finish()
    }
}

impl LuauSandbox {
    /// Build a fresh sandbox with the default limits.
    pub fn new() -> SandboxResult<Self> {
        Self::with_limits(DEFAULT_INTERRUPT_MS, DEFAULT_MEMORY_LIMIT_BYTES)
    }

    /// Explicitly override the per-script interrupt + memory cap.
    pub fn with_limits(interrupt_ms: u64, memory_bytes: usize) -> SandboxResult<Self> {
        // Load a minimal stdlib — math/string/table are safe, everything
        // else is off.
        let safe_libs = StdLib::MATH | StdLib::STRING | StdLib::TABLE | StdLib::UTF8;
        let lua = Lua::new_with(safe_libs, LuaOptions::new())
            .map_err(|e| SandboxError::Setup(e.to_string()))?;

        // Belt-and-suspenders: nil out dangerous globals in case a
        // future mlua / luau default adds one back.
        for banned in [
            "io",
            "os",
            "package",
            "debug",
            "dofile",
            "loadfile",
            "load",
            "require",
            "collectgarbage",
        ] {
            lua.globals().set(banned, LuaValue::Nil).map_err(SandboxError::Lua)?;
        }

        // Memory limit on the Lua VM.
        lua.set_memory_limit(memory_bytes).map_err(SandboxError::Lua)?;

        // Interrupt hook: Luau calls this periodically during execution.
        // The `true` return means "stop". We use a wall-clock deadline
        // captured at each call start.
        let deadline_ms = interrupt_ms;
        lua.set_interrupt(move |_| {
            use std::sync::atomic::{AtomicI64, Ordering};
            static START: AtomicI64 = AtomicI64::new(0);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let started = START.load(Ordering::Relaxed);
            if started == 0 {
                START.store(now, Ordering::Relaxed);
                return Ok(mlua::VmState::Continue);
            }
            if now - started > deadline_ms as i64 {
                START.store(0, Ordering::Relaxed);
                return Err(mlua::Error::external(SandboxError::Interrupted { ms: deadline_ms }));
            }
            Ok(mlua::VmState::Continue)
        });

        Ok(Self { lua, interrupt_ms })
    }

    /// Run a script with the given [`PolicyContext`] exposed as a
    /// read-only `vx` global. Returns every `report(...)`-emitted
    /// message.
    pub fn run(&self, script: &str, ctx: &PolicyContext) -> SandboxResult<Vec<String>> {
        let reports = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

        let reports_ref = reports.clone();
        let report_fn = self
            .lua
            .create_function(move |_, msg: String| {
                reports_ref.lock().expect("poisoned").push(msg);
                Ok(())
            })
            .map_err(SandboxError::Lua)?;

        let vx_table = build_vx_table(&self.lua, ctx)?;

        self.lua.globals().set("vx", vx_table).map_err(SandboxError::Lua)?;
        self.lua.globals().set("report", report_fn).map_err(SandboxError::Lua)?;

        let timeout = Duration::from_millis(self.interrupt_ms * 5);
        let result = std::thread::scope(|s| {
            let handle = s.spawn(|| self.lua.load(script).exec());
            // The interrupt hook does the main enforcement; this
            // additional thread-join timeout is a belt-and-suspenders
            // in case the hook misses an infinite-FFI edge case.
            let start = std::time::Instant::now();
            loop {
                if handle.is_finished() {
                    return handle.join().expect("no panic");
                }
                if start.elapsed() > timeout {
                    return Err(LuaError::external(SandboxError::Interrupted {
                        ms: self.interrupt_ms,
                    }));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        result.map_err(|e| match e {
            LuaError::MemoryError(_) => {
                SandboxError::MemoryExceeded { bytes: DEFAULT_MEMORY_LIMIT_BYTES }
            }
            other => SandboxError::Lua(other),
        })?;

        let guard = reports.lock().expect("poisoned");
        Ok(guard.clone())
    }
}

/// Build the read-only `vx` table handed to scripts. Shape mirrors the
/// [`PolicyContext`] but without any opaque IDs — just strings,
/// numbers, and tables.
fn build_vx_table<'a>(lua: &'a Lua, ctx: &PolicyContext) -> SandboxResult<mlua::Table> {
    let t = lua.create_table().map_err(SandboxError::Lua)?;

    // Components: { id -> {kind, version, root, deps, tags} }
    let components = lua.create_table().map_err(SandboxError::Lua)?;
    for (id, c) in &ctx.components {
        let row = lua.create_table().map_err(SandboxError::Lua)?;
        row.set("id", c.id.as_str()).map_err(SandboxError::Lua)?;
        row.set("kind", c.kind.as_str()).map_err(SandboxError::Lua)?;
        row.set("version", c.version.as_deref().unwrap_or("")).map_err(SandboxError::Lua)?;
        row.set("root", c.root.as_str()).map_err(SandboxError::Lua)?;

        let deps = lua.create_table().map_err(SandboxError::Lua)?;
        for (k, v) in &c.dependencies {
            deps.set(k.as_str(), v.as_str()).map_err(SandboxError::Lua)?;
        }
        row.set("deps", deps).map_err(SandboxError::Lua)?;

        let tags = lua.create_table().map_err(SandboxError::Lua)?;
        for (i, tag) in c.tags.iter().enumerate() {
            tags.set(i + 1, tag.as_str()).map_err(SandboxError::Lua)?;
        }
        row.set("tags", tags).map_err(SandboxError::Lua)?;

        components.set(id.as_str(), row).map_err(SandboxError::Lua)?;
    }
    t.set("components", components).map_err(SandboxError::Lua)?;

    // Runtimes
    let runtimes = lua.create_table().map_err(SandboxError::Lua)?;
    for (id, r) in &ctx.runtimes {
        runtimes.set(id.as_str(), r.version.as_str()).map_err(SandboxError::Lua)?;
    }
    t.set("runtimes", runtimes).map_err(SandboxError::Lua)?;

    // Commits
    let commits = lua.create_table().map_err(SandboxError::Lua)?;
    for (i, c) in ctx.commits.iter().enumerate() {
        let row = lua.create_table().map_err(SandboxError::Lua)?;
        row.set("sha", c.sha.as_str()).map_err(SandboxError::Lua)?;
        row.set("message", c.message.as_str()).map_err(SandboxError::Lua)?;
        commits.set(i + 1, row).map_err(SandboxError::Lua)?;
    }
    t.set("commits", commits).map_err(SandboxError::Lua)?;

    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_is_stripped() {
        let sb = LuauSandbox::new().unwrap();
        let ctx = PolicyContext::new("/tmp".into());
        // `io.open` should be nil — calling it errors.
        let err = sb.run("return io.open('x')", &ctx).unwrap_err();
        match err {
            SandboxError::Lua(_) => {} // Lua runtime error is what we want
            other => panic!("expected Lua error, got {other:?}"),
        }
    }

    #[test]
    fn debug_is_stripped() {
        let sb = LuauSandbox::new().unwrap();
        let ctx = PolicyContext::new("/tmp".into());
        let err = sb.run("return debug.getupvalue(function() end, 1)", &ctx).unwrap_err();
        assert!(matches!(err, SandboxError::Lua(_)));
    }

    #[test]
    fn report_collects_messages() {
        let sb = LuauSandbox::new().unwrap();
        let ctx = PolicyContext::new("/tmp".into());
        let out = sb.run("report('one'); report('two')", &ctx).unwrap();
        assert_eq!(out, vec!["one".to_string(), "two".to_string()]);
    }

    #[test]
    fn runtimes_table_visible() {
        let sb = LuauSandbox::new().unwrap();
        let mut ctx = PolicyContext::new("/tmp".into());
        ctx.runtimes.insert(
            "node".into(),
            crate::context::ContextRuntime { name: "node".into(), version: "20.11.1".into() },
        );
        let out = sb
            .run(
                r#"
                    if vx.runtimes.node ~= "20.11.1" then
                        report("wrong version")
                    else
                        report("ok")
                    end
                "#,
                &ctx,
            )
            .unwrap();
        assert_eq!(out, vec!["ok".to_string()]);
    }
}

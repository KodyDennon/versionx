//! Versionx shim — a minimal trampoline dispatched by filename.
//!
//! On Unix, this binary is symlinked as `node`, `npm`, `python`, etc.
//! On Windows, copied (or hardlinked) per tool.
//!
//! The shim reads `argv[0]`, looks up the resolved binary in an mmap'd
//! PATH cache, and `exec`s (Unix) / `CreateProcess`es (Windows) with
//! inherited stdio. Hot-path target: <1ms warm, <5ms cold.
//!
//! Status: scaffold. The PATH cache + dispatch logic is TODO for 0.1.0.

#![deny(unsafe_code)]

fn main() {
    eprintln!("versionx-shim v{} — scaffolded, not yet implemented.", env!("CARGO_PKG_VERSION"));
    eprintln!("See docs/spec/04-runtime-toolchain-mgmt.md for the intended behavior.");
    std::process::exit(0);
}

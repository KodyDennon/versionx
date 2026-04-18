//! Public Rust SDK re-exporting versionx-core's stable surface.
//!
//! Part of the Versionx workspace. See the workspace README and `docs/spec/` for architecture.
//!
//! Status: scaffold (crate is stubbed for 0.1.0 implementation).

#![deny(unsafe_code)]

/// Crate version as declared in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_cargo() {
        assert!(!VERSION.is_empty());
    }
}

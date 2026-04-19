//! Host platform detection. Used to pick the right installer artifact URL.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[allow(clippy::enum_variant_names)] // `MacOs` is well-known shorthand, keeping it.
pub enum Os {
    Linux,
    MacOs,
    Windows,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arch {
    X86_64,
    Aarch64,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Libc {
    /// Standard Linux builds linked against glibc.
    Glibc,
    /// Alpine / static builds linked against musl.
    Musl,
    /// Apple libc (macOS).
    Apple,
    /// Microsoft CRT (Windows).
    Msvc,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Platform {
    pub os: Os,
    pub arch: Arch,
    pub libc: Libc,
}

impl Platform {
    #[must_use]
    pub const fn new(os: Os, arch: Arch, libc: Libc) -> Self {
        Self { os, arch, libc }
    }

    /// Detect the current host.
    ///
    /// We can't reliably detect musl vs glibc without reading `/proc/self/exe`
    /// and parsing the ELF interpreter; for 0.1.0 we assume **glibc** on Linux
    /// and document how to override. Alpine users can set
    /// `VERSIONX_LIBC=musl` before running any command; core wires that in.
    #[must_use]
    pub fn detect() -> Self {
        let os = if cfg!(target_os = "linux") {
            Os::Linux
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            // Fall back to Linux so unsupported hosts at least get coherent
            // artifact URLs; installers will hard-error on actual download.
            Os::Linux
        };

        let arch = if cfg!(target_arch = "x86_64") {
            Arch::X86_64
        } else if cfg!(target_arch = "aarch64") {
            Arch::Aarch64
        } else {
            Arch::X86_64
        };

        let libc = match (os, arch) {
            (Os::Linux, _) => match std::env::var("VERSIONX_LIBC").as_deref() {
                Ok("musl") => Libc::Musl,
                _ => Libc::Glibc,
            },
            (Os::MacOs, _) => Libc::Apple,
            (Os::Windows, _) => Libc::Msvc,
        };

        Self { os, arch, libc }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let os = match self.os {
            Os::Linux => "linux",
            Os::MacOs => "macos",
            Os::Windows => "windows",
        };
        let arch = match self.arch {
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
        };
        let libc = match self.libc {
            Libc::Glibc => "glibc",
            Libc::Musl => "musl",
            Libc::Apple => "apple",
            Libc::Msvc => "msvc",
        };
        write!(f, "{os}-{arch}-{libc}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_does_not_panic() {
        let _ = Platform::detect();
    }

    #[test]
    fn display_is_stable() {
        let p = Platform::new(Os::Linux, Arch::X86_64, Libc::Glibc);
        assert_eq!(p.to_string(), "linux-x86_64-glibc");
    }
}

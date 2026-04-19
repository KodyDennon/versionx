//! Install shims into the user's shim dir.
//!
//! Unix uses symlinks (cheap, atomic, refresh by re-link). Windows
//! copies `versionx-shim.exe` per tool — Volta-style — because file
//! symlinks on Windows require Developer Mode or Admin and break
//! when the target lives on another drive. The shim binary inspects
//! `argv[0]` to figure out which tool it should resolve.

use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use versionx_runtime_trait::ShimEntry;

use crate::error::{CoreError, CoreResult};

/// Locate the `versionx-shim` binary. Looks for it next to the calling
/// executable (the typical layout when both are distributed together) and
/// falls back to the first hit on PATH.
#[must_use]
pub fn shim_binary_path() -> Option<Utf8PathBuf> {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join(platform_basename());
        if candidate.exists() {
            return Utf8PathBuf::from_path_buf(candidate).ok();
        }
    }
    which::which("versionx-shim").ok().and_then(|p| Utf8PathBuf::from_path_buf(p).ok())
}

#[cfg(unix)]
const fn platform_basename() -> &'static str {
    "versionx-shim"
}

#[cfg(windows)]
const fn platform_basename() -> &'static str {
    "versionx-shim.exe"
}

/// Create one shim per [`ShimEntry`] pointing at the shim binary.
///
/// Returns the basenames of every shim that now exists (either pre-existing
/// + left alone, or freshly created).
///
/// # Errors
/// Returns [`CoreError::Io`] if the shim dir can't be created or a symlink
/// can't be written.
pub fn install_shims(
    shims_dir: &Utf8Path,
    entries: &[ShimEntry],
    shim_binary: Option<&Utf8Path>,
) -> CoreResult<Vec<String>> {
    fs::create_dir_all(shims_dir)
        .map_err(|source| CoreError::Io { path: shims_dir.to_string(), source })?;

    // Without a shim binary we still note the expected names so the CLI can
    // surface them; creating the shim requires the binary.
    let Some(shim_binary) = shim_binary else {
        return Ok(entries.iter().map(|e| e.name.clone()).collect());
    };

    let mut created = Vec::with_capacity(entries.len());
    for entry in entries {
        let path = shims_dir.join(&entry.name);
        create_or_refresh_symlink(shim_binary, &path)?;
        created.push(entry.name.clone());
    }
    Ok(created)
}

#[cfg(unix)]
fn create_or_refresh_symlink(target: &Utf8Path, link: &Utf8Path) -> CoreResult<()> {
    use std::os::unix::fs::symlink;

    // Best-effort cleanup — if `link` already exists as a regular file or a
    // stale symlink, remove it before re-creating.
    if link.symlink_metadata().is_ok() {
        let _ = fs::remove_file(link);
    }
    symlink(target.as_std_path(), link.as_std_path())
        .map_err(|source| CoreError::Io { path: link.to_string(), source })?;
    Ok(())
}

#[cfg(windows)]
fn create_or_refresh_symlink(target: &Utf8Path, link: &Utf8Path) -> CoreResult<()> {
    // Windows: copy versionx-shim.exe under the target tool name +
    // `.exe`. The shim binary detects the requested tool from
    // `std::env::current_exe().file_stem()` so a copy works as a
    // drop-in proxy.
    //
    // Re-copy unconditionally — `std::fs::copy` is atomic-ish on
    // NTFS (uses CopyFileW which writes via temp + rename on most
    // configurations) and the shim binary is small (<200 KB).
    let link_with_ext = if link.extension().map(|e| e.eq_ignore_ascii_case("exe")).unwrap_or(false)
    {
        link.to_path_buf()
    } else {
        Utf8PathBuf::from(format!("{link}.exe"))
    };

    if link_with_ext.symlink_metadata().is_ok() {
        let _ = fs::remove_file(&link_with_ext);
    }
    fs::copy(target.as_std_path(), link_with_ext.as_std_path())
        .map_err(|source| CoreError::Io { path: link_with_ext.to_string(), source })?;
    Ok(())
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn install_shims_copies_exe_per_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let shim_bin = base.join("versionx-shim.exe");
        std::fs::write(&shim_bin, b"MZ\x00\x00stub-pe").unwrap();
        let shims_dir = base.join("shims");

        let entries = vec![
            ShimEntry { name: "node".into(), target: base.join("node-real") },
            ShimEntry { name: "npm".into(), target: base.join("npm-real") },
        ];

        install_shims(&shims_dir, &entries, Some(&shim_bin)).unwrap();
        for name in &["node", "npm"] {
            let copied = shims_dir.join(format!("{name}.exe"));
            assert!(copied.is_file(), "expected {copied} to be a file");
            let bytes = std::fs::read(copied.as_std_path()).unwrap();
            assert_eq!(bytes, b"MZ\x00\x00stub-pe");
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn install_shims_creates_symlinks_to_target() {
        let tmp = tempfile::tempdir().unwrap();
        let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let shim_bin = base.join("versionx-shim");
        std::fs::write(&shim_bin, b"#!/bin/sh\necho shim\n").unwrap();
        let shims_dir = base.join("shims");

        let entries = vec![
            ShimEntry { name: "node".into(), target: base.join("node-real") },
            ShimEntry { name: "npm".into(), target: base.join("npm-real") },
        ];

        let created = install_shims(&shims_dir, &entries, Some(&shim_bin)).unwrap();
        assert_eq!(created, vec!["node".to_string(), "npm".to_string()]);

        for name in &["node", "npm"] {
            let path = shims_dir.join(name);
            let link = std::fs::read_link(path.as_std_path()).unwrap();
            assert_eq!(link, shim_bin.as_std_path());
        }
    }

    #[test]
    fn install_shims_replaces_existing_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let old_shim = base.join("old-shim");
        let new_shim = base.join("new-shim");
        std::fs::write(&old_shim, b"old").unwrap();
        std::fs::write(&new_shim, b"new").unwrap();
        let shims_dir = base.join("shims");

        let entries = vec![ShimEntry { name: "node".into(), target: base.join("target") }];

        install_shims(&shims_dir, &entries, Some(&old_shim)).unwrap();
        install_shims(&shims_dir, &entries, Some(&new_shim)).unwrap();

        let link = std::fs::read_link(shims_dir.join("node").as_std_path()).unwrap();
        assert_eq!(link, new_shim.as_std_path());
    }
}

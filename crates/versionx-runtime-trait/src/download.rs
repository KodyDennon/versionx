//! Shared helpers for installers that download tarballs / zips.
//!
//! Each helper hits the network, verifies checksums, and emits progress
//! events. Installers shouldn't need to write this code directly.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path as StdPath;

use camino::{Utf8Path, Utf8PathBuf};
use reqwest::Client;
use sha2::Digest;
use versionx_events::{EventSender, Level};

use crate::error::{InstallerError, InstallerResult};

/// Download `url` into `dest`, streaming + hashing as we go.
///
/// Returns the bare (no-prefix) lowercase-hex SHA-256 of what was written.
///
/// `dest` is written to atomically via a sibling `.partial` file + rename,
/// so a SIGINT never leaves half-written archives in the cache.
pub async fn download_to_file(
    http: &Client,
    url: &str,
    dest: &Utf8Path,
    events: &EventSender,
) -> InstallerResult<String> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|source| InstallerError::Io { path: parent.to_path_buf(), source })?;
    }

    events.emit(versionx_events::Event::new(
        "runtime.download.start",
        Level::Info,
        format!("downloading {url}"),
    ));

    let resp = http
        .get(url)
        .send()
        .await
        .map_err(|source| InstallerError::Network { url: url.to_string(), source })?;
    if !resp.status().is_success() {
        return Err(InstallerError::Http { url: url.to_string(), status: resp.status().as_u16() });
    }

    let tmp = dest.with_extension("partial");
    let mut file =
        File::create(&tmp).map_err(|source| InstallerError::Io { path: tmp.clone(), source })?;

    let mut hasher = sha2::Sha256::new();
    let mut stream = resp.bytes_stream();
    use futures::StreamExt as _;
    while let Some(chunk) = stream.next().await {
        let bytes =
            chunk.map_err(|source| InstallerError::Network { url: url.to_string(), source })?;
        hasher.update(&bytes);
        file.write_all(&bytes)
            .map_err(|source| InstallerError::Io { path: tmp.clone(), source })?;
    }
    file.flush().map_err(|source| InstallerError::Io { path: tmp.clone(), source })?;
    drop(file);

    std::fs::rename(&tmp, dest)
        .map_err(|source| InstallerError::Io { path: dest.to_path_buf(), source })?;

    let sha = hex::encode(hasher.finalize());
    events.emit(versionx_events::Event::new(
        "runtime.download.complete",
        Level::Info,
        format!("wrote {dest} (sha256:{sha})"),
    ));
    Ok(sha)
}

/// Download into memory and return the bytes + SHA-256. For small files like
/// index / manifest JSON.
pub async fn download_to_memory(http: &Client, url: &str) -> InstallerResult<(Vec<u8>, String)> {
    let resp = http
        .get(url)
        .send()
        .await
        .map_err(|source| InstallerError::Network { url: url.to_string(), source })?;
    if !resp.status().is_success() {
        return Err(InstallerError::Http { url: url.to_string(), status: resp.status().as_u16() });
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|source| InstallerError::Network { url: url.to_string(), source })?;
    let sha = {
        let mut h = sha2::Sha256::new();
        h.update(&bytes);
        hex::encode(h.finalize())
    };
    Ok((bytes.to_vec(), sha))
}

/// Check a previously-downloaded file's SHA-256 against an expected value.
pub fn verify_sha256(path: &Utf8Path, expected: &str) -> InstallerResult<String> {
    let mut file = File::open(path)
        .map_err(|source| InstallerError::Io { path: path.to_path_buf(), source })?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|source| InstallerError::Io { path: path.to_path_buf(), source })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = hex::encode(hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(InstallerError::ChecksumMismatch {
            url: path.as_str().to_string(),
            expected: expected.to_string(),
            actual,
        });
    }
    Ok(actual)
}

/// Extract a `.tar.xz` / `.tar.gz` / `.tar.zst` into `dest`. Caller ensures
/// `dest` exists + is empty; extraction creates it if needed.
///
/// Honors a `strip_components` count to drop N leading path segments from
/// every archive entry (Node archives have a `node-v22.12.0-linux-x64/` root).
pub fn extract_tar(
    archive: &Utf8Path,
    dest: &Utf8Path,
    strip_components: usize,
) -> InstallerResult<()> {
    use std::fs::File;

    std::fs::create_dir_all(dest)
        .map_err(|source| InstallerError::Io { path: dest.to_path_buf(), source })?;

    let file = File::open(archive)
        .map_err(|source| InstallerError::Io { path: archive.to_path_buf(), source })?;

    let decompressed: Box<dyn Read> = match archive.as_str() {
        p if p.ends_with(".tar.xz") || p.ends_with(".txz") => {
            Box::new(xz2::read::XzDecoder::new(file))
        }
        p if p.ends_with(".tar.gz") || p.ends_with(".tgz") => {
            Box::new(flate2::read::GzDecoder::new(file))
        }
        p if p.ends_with(".tar.zst") || p.ends_with(".tar.zstd") => Box::new(
            zstd::stream::read::Decoder::new(file)
                .map_err(|source| InstallerError::Io { path: archive.to_path_buf(), source })?,
        ),
        p if p.ends_with(".tar") => Box::new(file),
        other => {
            return Err(InstallerError::Extract {
                path: archive.to_path_buf(),
                message: format!("unrecognised tar compression in `{other}`"),
            });
        }
    };

    let mut tar = tar::Archive::new(decompressed);
    tar.set_preserve_permissions(true);

    for entry in tar.entries().map_err(|e| InstallerError::Extract {
        path: archive.to_path_buf(),
        message: e.to_string(),
    })? {
        let mut entry = entry.map_err(|e| InstallerError::Extract {
            path: archive.to_path_buf(),
            message: e.to_string(),
        })?;
        let entry_path = entry.path().map_err(|e| InstallerError::Extract {
            path: archive.to_path_buf(),
            message: e.to_string(),
        })?;
        let stripped = entry_path.iter().skip(strip_components).collect::<std::path::PathBuf>();
        if stripped.as_os_str().is_empty() {
            continue;
        }
        let target = StdPath::new(dest.as_str()).join(&stripped);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|source| InstallerError::Io {
                path: Utf8PathBuf::from_path_buf(parent.to_path_buf())
                    .unwrap_or_else(|p| Utf8PathBuf::from(p.display().to_string())),
                source,
            })?;
        }
        entry.unpack(&target).map_err(|e| InstallerError::Extract {
            path: archive.to_path_buf(),
            message: format!("unpack {:?}: {e}", target.display()),
        })?;
    }

    Ok(())
}

/// Extract a `.zip` into `dest` with `strip_components`. Used for Windows
/// Node archives.
pub fn extract_zip(
    archive: &Utf8Path,
    dest: &Utf8Path,
    strip_components: usize,
) -> InstallerResult<()> {
    std::fs::create_dir_all(dest)
        .map_err(|source| InstallerError::Io { path: dest.to_path_buf(), source })?;
    let file = File::open(archive)
        .map_err(|source| InstallerError::Io { path: archive.to_path_buf(), source })?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| InstallerError::Extract {
        path: archive.to_path_buf(),
        message: e.to_string(),
    })?;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| InstallerError::Extract {
            path: archive.to_path_buf(),
            message: e.to_string(),
        })?;
        let Some(entry_name) = entry.enclosed_name() else {
            continue;
        };
        let stripped = entry_name.iter().skip(strip_components).collect::<std::path::PathBuf>();
        if stripped.as_os_str().is_empty() {
            continue;
        }
        let target = StdPath::new(dest.as_str()).join(&stripped);
        if entry.is_dir() {
            std::fs::create_dir_all(&target).map_err(|source| InstallerError::Io {
                path: Utf8PathBuf::from_path_buf(target.clone())
                    .unwrap_or_else(|p| Utf8PathBuf::from(p.display().to_string())),
                source,
            })?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|source| InstallerError::Io {
                path: Utf8PathBuf::from_path_buf(parent.to_path_buf())
                    .unwrap_or_else(|p| Utf8PathBuf::from(p.display().to_string())),
                source,
            })?;
        }
        let mut out = File::create(&target).map_err(|source| InstallerError::Io {
            path: Utf8PathBuf::from_path_buf(target.clone())
                .unwrap_or_else(|p| Utf8PathBuf::from(p.display().to_string())),
            source,
        })?;
        io::copy(&mut entry, &mut out).map_err(|source| InstallerError::Io {
            path: Utf8PathBuf::from_path_buf(target.clone())
                .unwrap_or_else(|p| Utf8PathBuf::from(p.display().to_string())),
            source,
        })?;
    }
    Ok(())
}

//! Per-call audit log.
//!
//! Every MCP tool call appends one JSON line to `audit.ndjson`. Lines
//! are append-only, flushed on write, and survive crashes. The log
//! captures:
//!   - `client_info` as reported by the peer (spoofable; logged, not
//!     trusted).
//!   - Tool name + argument blob (may be redacted later — see 0.7).
//!   - Timestamp, result status, elapsed ms.
//!
//! Log files can be tailed with `versionx mcp logs` (deferred to 0.7).

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::Mutex;
use std::time::Instant;

use camino::Utf8Path;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct AuditLog {
    writer: Mutex<BufWriter<File>>,
}

impl AuditLog {
    pub fn open(path: &Utf8Path) -> std::io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path.as_std_path())?;
        Ok(Self { writer: Mutex::new(BufWriter::new(file)) })
    }

    /// Record one call. Infallible — errors here must not take down the
    /// server.
    pub fn record(&self, entry: &AuditEntry) {
        let Ok(line) = serde_json::to_string(entry) else { return };
        if let Ok(mut w) = self.writer.lock() {
            let _ = writeln!(w, "{line}");
            let _ = w.flush();
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditEntry {
    pub at: DateTime<Utc>,
    pub tool: String,
    pub client_info: Option<String>,
    pub success: bool,
    pub elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Convenience builder: start a stopwatch + return a closure that writes
/// the completed entry once the tool finishes.
pub fn start_call(tool: impl Into<String>, client_info: Option<String>) -> CallTimer {
    CallTimer { tool: tool.into(), client_info, started: Instant::now() }
}

#[derive(Debug)]
pub struct CallTimer {
    tool: String,
    client_info: Option<String>,
    started: Instant,
}

impl CallTimer {
    pub fn finish(self, success: bool, error: Option<String>) -> AuditEntry {
        AuditEntry {
            at: Utc::now(),
            tool: self.tool,
            client_info: self.client_info,
            success,
            elapsed_ms: self.started.elapsed().as_millis() as u64,
            error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_entry_to_ndjson() {
        let tmp = tempfile::tempdir().unwrap();
        let p = camino::Utf8PathBuf::from_path_buf(tmp.path().join("audit.ndjson")).unwrap();
        let log = AuditLog::open(&p).unwrap();
        let timer = start_call("workspace_list", Some("claude-code/1.0".into()));
        log.record(&timer.finish(true, None));
        drop(log);
        let body = std::fs::read_to_string(p.as_std_path()).unwrap();
        assert!(body.contains("workspace_list"));
        assert!(body.contains("claude-code/1.0"));
    }
}

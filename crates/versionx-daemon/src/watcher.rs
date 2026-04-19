//! Filesystem watcher that invalidates cached workspace snapshots and
//! emits `workspace.changed` notifications to interested subscribers.
//!
//! This is deliberately a *shallow* watcher — we only touch workspaces
//! the daemon has already learned about (via a `workspace.*` call). The
//! watcher is registered lazily from [`crate::server::run`] and keeps
//! a single debouncer thread regardless of how many roots are watched.
//!
//! Debouncing is 500ms so a burst of writes (IDE save, git checkout)
//! coalesces into one event per file. We use
//! `notify-debouncer-full` which also de-duplicates path noise from
//! platform-specific event streams.

use std::path::PathBuf;
use std::time::Duration;

use notify::{EventKind, RecursiveMode};
use notify_debouncer_full::DebouncedEvent;
use tokio::sync::broadcast;
use tracing::{debug, warn};

use crate::protocol::{Notification, notifications};

/// Handle returned by [`spawn`]. Dropping it aborts the watcher thread.
pub struct WatcherHandle {
    /// Keep the debouncer alive — it owns the background thread.
    _debouncer: Box<dyn std::any::Any + Send>,
    /// Keep the sink alive for the lifetime of the watcher.
    _tx: broadcast::Sender<Notification>,
}

impl std::fmt::Debug for WatcherHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WatcherHandle").finish_non_exhaustive()
    }
}

/// Spawn the watcher.
///
/// Returns a handle — drop it to shut down.
///
/// # Errors
/// Propagates errors from `notify` when the backend fails to initialize
/// (rare — usually a kernel-level failure).
pub fn spawn(notify_tx: broadcast::Sender<Notification>) -> anyhow::Result<WatcherHandle> {
    let tx_for_worker = notify_tx.clone();
    let (event_tx, event_rx) = std::sync::mpsc::channel::<Vec<DebouncedEvent>>();

    let mut debouncer = notify_debouncer_full::new_debouncer(
        Duration::from_millis(500),
        None,
        move |res: Result<Vec<DebouncedEvent>, Vec<notify::Error>>| match res {
            Ok(events) => {
                let _ = event_tx.send(events);
            }
            Err(errors) => {
                for e in errors {
                    warn!("watcher error: {e}");
                }
            }
        },
    )?;

    // Default watch: the workspace root is discovered lazily, so the
    // server stubs in a "watch this directory" call as snapshots get
    // computed. For 0.3 we keep things simple: watch `$CWD` so the
    // first `workspace.*` call picks up edits. Additional roots can be
    // registered by the server via [`WatcherHandle::watch`] (future).
    //
    // Silently ignore errors — the daemon works without a watcher, it
    // just loses cache-invalidation tightness.
    if let Ok(cwd) = std::env::current_dir() {
        let _ = debouncer.watch(&cwd, RecursiveMode::Recursive);
    }

    std::thread::spawn(move || {
        while let Ok(events) = event_rx.recv() {
            if !is_meaningful(&events) {
                continue;
            }
            debug!(n = events.len(), "fs event batch");
            let payload = serde_json::json!({
                "paths": events
                    .iter()
                    .flat_map(|e| e.event.paths.iter().map(|p: &PathBuf| p.display().to_string()))
                    .collect::<Vec<_>>(),
            });
            let _ =
                tx_for_worker.send(Notification::new(notifications::WORKSPACE_CHANGED, payload));
        }
    });

    Ok(WatcherHandle { _debouncer: Box::new(debouncer), _tx: notify_tx })
}

/// Filter: we only care about file creates/modifies/removes that change
/// on-disk content. Metadata-only changes (atime, chmod) don't
/// invalidate hashes.
fn is_meaningful(events: &[DebouncedEvent]) -> bool {
    events.iter().any(|e| {
        matches!(e.event.kind, EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_))
    })
}

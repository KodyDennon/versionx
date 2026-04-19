//! Filesystem watcher that invalidates cached workspace snapshots and
//! emits `workspace.changed` notifications to interested subscribers.
//!
//! ### Lifecycle
//!
//! The watcher is started once at daemon boot and exposes
//! [`WatcherHandle::watch`] so the server can register new workspace
//! roots as they're discovered (lazy — only roots a client has actually
//! asked about are watched). Watched roots persist for the lifetime of
//! the daemon; we don't try to reclaim watch descriptors when caches
//! expire because the cost is negligible (a watch descriptor is a few
//! kilobytes per inode subtree).
//!
//! ### Cache invalidation
//!
//! On every meaningful event batch the watcher calls into a
//! [`CacheInvalidator`] supplied by the server. The invalidator is
//! responsible for dropping any cached snapshots whose roots are
//! ancestors of the changed paths. We err on the side of dropping more
//! than necessary — a wrong-cache hit is much worse than a recompute.
//!
//! Debouncing is 500ms so a burst of writes (IDE save, git checkout)
//! coalesces into one event per file. We use
//! `notify-debouncer-full` which also de-duplicates path noise from
//! platform-specific event streams.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::{EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, DebouncedEvent, Debouncer, RecommendedCache};
use parking_lot::Mutex;
use tokio::sync::broadcast;
use tracing::{debug, warn};

use crate::protocol::{Notification, notifications};

/// Object the watcher calls when it sees changes. The server
/// implements this on its snapshot cache.
pub trait CacheInvalidator: Send + Sync + 'static {
    /// Drop any cached entries whose root is an ancestor of any changed
    /// path. Implementations should be cheap and lock-friendly.
    fn invalidate(&self, changed_paths: &[PathBuf]);
}

/// Handle returned by [`spawn`]. Dropping it aborts the watcher thread.
pub struct WatcherHandle {
    /// The debouncer owns the platform watcher + the worker thread that
    /// drains its event channel. We keep it behind a Mutex so
    /// [`Self::watch`] can register additional roots after spawn.
    debouncer: Arc<Mutex<Debouncer<RecommendedWatcher, RecommendedCache>>>,
    /// Keep the sink alive for the lifetime of the watcher.
    _tx: broadcast::Sender<Notification>,
}

impl std::fmt::Debug for WatcherHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WatcherHandle").finish_non_exhaustive()
    }
}

impl WatcherHandle {
    /// Add a new workspace root to the watch set. Idempotent — adding
    /// the same root twice is a cheap no-op at the platform level.
    pub fn watch(&self, root: &Path) -> anyhow::Result<()> {
        self.debouncer.lock().watch(root, RecursiveMode::Recursive)?;
        debug!(?root, "watching workspace root");
        Ok(())
    }
}

/// Spawn the watcher.
///
/// Returns a handle — drop it to shut down. The caller passes a
/// [`CacheInvalidator`] which will be called on every meaningful event
/// batch.
///
/// # Errors
/// Propagates errors from `notify` when the backend fails to initialize
/// (rare — usually a kernel-level failure).
pub fn spawn(
    notify_tx: broadcast::Sender<Notification>,
    invalidator: Arc<dyn CacheInvalidator>,
) -> anyhow::Result<WatcherHandle> {
    let tx_for_worker = notify_tx.clone();
    let invalidator_for_worker = invalidator.clone();
    let (event_tx, event_rx) = std::sync::mpsc::channel::<Vec<DebouncedEvent>>();

    let debouncer = notify_debouncer_full::new_debouncer(
        Duration::from_millis(500),
        None,
        move |res: DebounceEventResult| match res {
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

    std::thread::spawn(move || {
        while let Ok(events) = event_rx.recv() {
            if !is_meaningful(&events) {
                continue;
            }
            let changed: Vec<PathBuf> =
                events.iter().flat_map(|e| e.event.paths.iter().cloned()).collect();
            debug!(n = events.len(), paths = ?changed.len(), "fs event batch");

            // 1. Drop stale cache entries before clients can read them.
            invalidator_for_worker.invalidate(&changed);

            // 2. Notify any subscribed clients so they can refresh
            //    (e.g. the TUI dashboard, MCP `workspace.changed`).
            let payload = serde_json::json!({
                "paths": changed.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            });
            let _ =
                tx_for_worker.send(Notification::new(notifications::WORKSPACE_CHANGED, payload));
        }
    });

    Ok(WatcherHandle { debouncer: Arc::new(Mutex::new(debouncer)), _tx: notify_tx })
}

/// Filter: we only care about file creates/modifies/removes that change
/// on-disk content. Metadata-only changes (atime, chmod) don't
/// invalidate hashes.
fn is_meaningful(events: &[DebouncedEvent]) -> bool {
    events.iter().any(|e| {
        matches!(e.event.kind, EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_))
    })
}

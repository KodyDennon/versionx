//! A pre-built context that bundles everything a command needs.

use std::sync::Arc;

use camino::Utf8PathBuf;
use reqwest::Client;
use versionx_events::{EventBus, EventSender};
use versionx_runtime_trait::{InstallerContext, Platform};

use crate::error::CoreResult;
use crate::paths::VersionxHome;
use crate::runtime_registry::{RuntimeRegistry, registry};

/// Bundled dependencies passed into command handlers.
#[derive(Clone)]
pub struct CoreContext {
    pub home: VersionxHome,
    pub events: EventSender,
    pub http: Client,
    pub platform: Platform,
    pub registry: Arc<RuntimeRegistry>,
}

impl CoreContext {
    /// Build a context using real filesystem paths, a fresh http client,
    /// and the default runtime registry.
    ///
    /// # Errors
    /// Propagates failures from `VersionxHome::detect()`.
    pub fn detect(events: EventSender) -> CoreResult<Self> {
        let home = VersionxHome::detect()?;
        let http = Client::builder()
            .user_agent(concat!(
                "versionx/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/KodyDennon/versionx)"
            ))
            .build()
            .map_err(|source| crate::CoreError::Io {
                path: "reqwest-client".into(),
                source: std::io::Error::other(source),
            })?;
        Ok(Self {
            home,
            events,
            http,
            platform: Platform::detect(),
            registry: Arc::new(registry()),
        })
    }

    /// Convenience: a context backed by a throwaway in-process `EventBus`.
    pub fn detect_with_own_bus() -> CoreResult<(Self, EventBus)> {
        let bus = EventBus::new();
        let ctx = Self::detect(bus.sender())?;
        Ok((ctx, bus))
    }

    /// Build an [`InstallerContext`] scoped to a single runtime tool.
    #[must_use]
    pub fn installer_ctx(&self) -> InstallerContext {
        InstallerContext {
            runtimes_dir: self.home.runtimes_dir(),
            cache_dir: Utf8PathBuf::from(self.home.cache_dir()),
            http: self.http.clone(),
            events: self.events.clone(),
            platform: self.platform,
        }
    }
}

impl std::fmt::Debug for CoreContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoreContext")
            .field("home", &self.home)
            .field("platform", &self.platform)
            .finish_non_exhaustive()
    }
}

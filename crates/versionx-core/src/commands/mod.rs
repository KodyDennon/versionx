//! User-facing command implementations.

pub mod activate;
pub mod context;
pub mod init;
pub mod install;
pub mod shim_install;
pub mod sync;
pub mod which;

pub use activate::{ActivateOptions, Shell, activate};
pub use context::CoreContext;
pub use init::{InitOptions, InitOutcome, init};
pub use install::{InstallOptions, InstallOutcome, install};
pub use sync::{SyncOptions, SyncOutcome, sync};
pub use which::{WhichOptions, WhichOutcome, which};

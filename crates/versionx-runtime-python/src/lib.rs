//! Python ecosystem runtime installers.
//!
//! Ships three installers:
//!   - [`PythonInstaller`] — `CPython` via astral-sh/python-build-standalone.
//!   - [`UvInstaller`] — astral's `uv` package + project manager.
//!   - [`PoetryInstaller`] — Poetry, installed into an isolated venv on
//!     top of a Versionx-managed Python.

#![deny(unsafe_code)]

pub mod cpython;
pub mod poetry;
pub mod uv;

pub use cpython::PythonInstaller;
pub use poetry::PoetryInstaller;
pub use uv::UvInstaller;

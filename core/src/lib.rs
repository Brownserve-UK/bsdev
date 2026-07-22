//! Core orchestration logic for the `bsdev` launcher.
//!
//! The CLI crate is a thin shell over this: everything that talks to `docker`
//! and `ssh`, and all of the configuration, lives here so it can be tested and
//! reused. This crate never installs tooling or provisions the container - that
//! is the image's and chezmoi's job respectively.

pub mod adbtunnel;
pub mod codebridge;
pub mod config;
pub mod docker;
pub mod error;
pub mod forward;
pub mod process;
pub mod settings;
pub mod ssh;
mod tunnel;

pub use error::{BsdevError, Result};
pub use settings::Settings;

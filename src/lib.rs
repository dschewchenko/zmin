#![deny(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

//! Zmin Core library
//!
//! This crate re-exports the Git-compatible core crates shipped in this repository.

pub use zmin_git_core as git_core;
pub use zmin_primitives::config;
pub use zmin_primitives::domain;
pub use zmin_primitives::error;
pub use zmin_primitives::git_runtime;
pub use zmin_primitives::i18n;
pub use zmin_primitives::id;
pub use zmin_primitives::transport;

pub mod prelude;

pub use error::{Error, Result};

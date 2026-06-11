#![deny(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

//! Skron Core library
//!
//! This crate re-exports the Git-compatible core crates shipped in this repository.

pub use skron_git_core as git_core;
pub use skron_primitives::config;
pub use skron_primitives::domain;
pub use skron_primitives::error;
pub use skron_primitives::git_runtime;
pub use skron_primitives::i18n;
pub use skron_primitives::id;
pub use skron_primitives::transport;

pub mod prelude;

pub use error::{Error, Result};

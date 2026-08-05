//! SQLite persistence for the module cache (schema v9).
//!
//! Stores a full `Option<FileAnalysis>` per module, serialized via bincode
//! and compressed with zstd. Validates entries against mtime + file size to
//! detect stale data. Invalidates the entire cache when `@INC` changes.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use dashmap::DashMap;
use rusqlite::{params, Connection};

use crate::model::file_analysis::FileAnalysis;
use crate::index::module_index::CachedModule;

mod blob;
pub use blob::*;
mod conn;
pub use conn::*;
mod rows;
pub use rows::*;
mod schema;
pub use schema::*;
mod stubs;
pub use stubs::*;
mod warm;
pub use warm::*;

#[cfg(test)]
#[path = "module_cache_tests.rs"]
mod tests;

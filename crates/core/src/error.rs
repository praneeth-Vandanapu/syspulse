// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/error.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Error type for syspulse-core. Serialises to a plain message so it
//           can be returned straight from Tauri commands.
// =============================================================================

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("process {0} does not exist")]
    NoSuchProcess(u32),
    #[error("permission denied")]
    PermissionDenied,
    #[error("authentication was cancelled")]
    Cancelled,
    #[error("{0}")]
    Refused(String),
    #[error("{0} is not supported on this platform")]
    Unsupported(&'static str),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

impl Serialize for CoreError {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, CoreError>;

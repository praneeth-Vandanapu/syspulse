// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/lib.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Platform logic for SysPulse, independent of any UI framework so it
//           can also power a CLI or a headless agent.
// =============================================================================

pub mod alerts;
pub mod cleaner;
pub mod error;
pub mod forge;
pub mod fsutil;
pub mod gpu;
pub mod hardware;
pub mod largefiles;
pub mod launcher;
pub mod models;
pub mod monitor;
pub mod netconfig;
pub mod netconn;
pub mod netlens;
pub mod netperf;
pub mod nettools;
pub mod privileged;
pub mod process;
pub mod recorder;
pub mod search;
pub mod services;
pub mod software;
pub mod startup;
pub mod util;

pub use error::{CoreError, Result};

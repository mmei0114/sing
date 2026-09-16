//! Common system-proxy status with a macOS-only production backend.
use serde::{Deserialize, Serialize};

pub mod helper;
#[cfg(target_os = "macos")]
pub mod macos;

// Keep the portable transaction tests on every platform without compiling the
// macOS-only backend into non-macOS production binaries.
#[cfg(any(target_os = "macos", test))]
mod controller;
#[cfg(target_os = "macos")]
pub use controller::*;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProxyStatus {
    pub supported: bool,
    pub helper_ready: bool,
    pub configured: bool,
    pub effective: bool,
    pub pending_restore: bool,
    pub safe_to_stop: bool,
    pub services: Vec<String>,
    pub detail: String,
}

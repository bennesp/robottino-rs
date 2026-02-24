//! # xplorer-rs
//!
//! Local control library for X-Plorer Serie 75 S / Serie 95 S robot vacuum cleaners.
//!
//! Built on top of [`tuya_rs`] for Tuya v3.3 protocol communication. Provides
//! high-level commands (room cleaning, zone cleaning, forbidden zones, virtual walls)
//! and map file decoding (layout + route).
//!
//! ## Features
//!
//! - **Default**: local TCP control, map decoding (LZ4-compressed layout + route)
//! - **`cloud`**: cloud API access (login, device discovery, map download via AWS STS)
//! - **`render`**: PNG rendering of layout maps and cleaning routes

pub mod device;
pub mod map;
pub mod protocol;
pub mod types;

// Re-export for convenience
#[cfg(feature = "cloud")]
pub use device::xplorer_oem_credentials;
pub use device::{Device, XPlorer};
pub use tuya_rs;

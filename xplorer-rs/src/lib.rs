#![warn(missing_docs)]
#![deny(unsafe_code)]

//! # xplorer-rs
//!
//! Control library for X-Plorer Serie 75 S / Serie 95 S robot vacuum cleaners.
//!
//! Built on top of [`tuya_rs`] for Tuya v3.3 protocol communication. Provides
//! high-level commands (room cleaning, zone cleaning, forbidden zones, virtual walls)
//! and map file decoding (layout + route).
//!
//! Two implementations of the [`Device`] trait are available:
//!
//! - [`LocalXPlorer`] — local TCP control via the Tuya v3.3 protocol (default)
//! - [`CloudXPlorer`] — cloud control via the Tuya OEM Mobile API (`cloud` feature)
//!
//! ## Quick start (local)
//!
//! ```no_run
//! # async fn example() {
//! use xplorer_rs::{LocalXPlorer, Device, DeviceConfig};
//! use xplorer_rs::protocol::{RoomCleanCommand, Zone, ZoneCleanCommand};
//!
//! let config = DeviceConfig {
//!     dev_id: "device_id_here".into(),
//!     address: "192.168.1.100".into(),
//!     local_key: "0123456789abcdef".into(),
//!     ..Default::default()
//! };
//! let mut robot = LocalXPlorer::connect(&config).unwrap();
//!
//! // Check status
//! let state = robot.status().await.unwrap();
//! println!("battery: {}%, mode: {}", state.battery, state.mode);
//!
//! // Clean specific rooms (1 pass, rooms 0 and 2)
//! robot.clean_rooms(&RoomCleanCommand {
//!     clean_times: 1,
//!     room_ids: vec![0, 2],
//! }).await.unwrap();
//!
//! // Or clean a rectangular zone
//! robot.clean_zone(&ZoneCleanCommand {
//!     clean_times: 1,
//!     zones: vec![Zone::rect(82, -13, 453, 203)],
//! }).await.unwrap();
//! # }
//! ```
//!
//! ## Quick start (cloud)
//!
//! ```no_run
//! # #[cfg(feature = "cloud")]
//! # async fn example() {
//! use xplorer_rs::{CloudXPlorer, Device, xplorer_oem_credentials};
//!
//! let oem_creds = xplorer_oem_credentials("your_44char_hex_app_device_id_here");
//! let mut robot = CloudXPlorer::login(oem_creds, "you@email.com", "password", "your_device_id")
//!     .await.unwrap();
//! let state = robot.status().await.unwrap();
//! println!("battery: {}%, mode: {}", state.battery, state.mode);
//! # }
//! ```
//!
//! ## Parsing device events
//!
//! ```
//! use xplorer_rs::device::parse_dps_response;
//! use xplorer_rs::types::DpsEvent;
//!
//! let json = r#"{"dps":{"1":true,"4":"smart","8":72}}"#;
//! let events = parse_dps_response(json).unwrap();
//! for event in &events {
//!     match event {
//!         DpsEvent::Battery(pct) => println!("battery: {pct}%"),
//!         DpsEvent::Mode(mode) => println!("mode: {mode}"),
//!         _ => {}
//!     }
//! }
//! ```
//!
//! ## Decoding sweeper messages
//!
//! The robot sends binary commands on DP 15 as base64. Decode them with
//! [`protocol::SweeperMessage`]:
//!
//! ```
//! use xplorer_rs::protocol::SweeperMessage;
//!
//! let msg = SweeperMessage::decode_base64("qgAEFQEBBBs=").unwrap();
//! assert_eq!(msg.cmd, 0x15); // room clean status
//! assert!(msg.checksum_ok);
//! ```
//!
//! ## Features
//!
//! - **Default**: local TCP control, map decoding (LZ4-compressed layout + route)
//! - **`cloud`**: cloud API access (login, device discovery, map download via AWS STS),
//!   [`CloudXPlorer`] for remote device control
//! - **`render`**: PNG rendering of layout maps and cleaning routes

/// Cloud-based vacuum control via Tuya OEM API.
#[cfg(feature = "cloud")]
pub mod cloud_device;
/// Vacuum cleaner device control: [`Device`] trait, [`LocalXPlorer`] (local TCP).
pub mod device;
/// Map file decoder (layout + route) with optional PNG rendering.
pub mod map;
/// DP 15 binary sweeper protocol: room/zone clean, forbidden zones, virtual walls.
pub mod protocol;
/// Device state model: DPS event parsing and enum types.
pub mod types;

// Re-export for convenience
pub use device::{Device, LocalXPlorer};
pub use tuya_rs;
pub use tuya_rs::connection::{DeviceConfig, DeviceError, DpValue, DpsUpdate, Transport};
pub use tuya_rs::discovery;

#[cfg(feature = "cloud")]
pub use cloud_device::{CloudXPlorer, cloud_discover};
#[cfg(feature = "cloud")]
pub use device::xplorer_oem_credentials;
#[cfg(feature = "cloud")]
pub use tuya_rs::api::{DeviceInfo, Home, StorageCredentials, generate_presigned_url};

#![warn(missing_docs)]
#![deny(unsafe_code)]

//! # xplorer-rs
//!
//! Local control library for X-Plorer Serie 75 S / Serie 95 S robot vacuum cleaners.
//!
//! Built on top of [`tuya_rs`] for Tuya v3.3 protocol communication. Provides
//! high-level commands (room cleaning, zone cleaning, forbidden zones, virtual walls)
//! and map file decoding (layout + route).
//!
//! ## Quick start
//!
//! ```no_run
//! use xplorer_rs::{XPlorer, Device};
//! use xplorer_rs::protocol::{RoomCleanCommand, Zone, ZoneCleanCommand};
//! use tuya_rs::connection::DeviceConfig;
//!
//! let config = DeviceConfig {
//!     dev_id: "device_id_here".into(),
//!     address: "192.168.1.100".into(),
//!     local_key: "0123456789abcdef".into(),
//!     ..Default::default()
//! };
//! let mut robot = XPlorer::connect(&config).unwrap();
//!
//! // Check status
//! let state = robot.status().unwrap();
//! println!("battery: {}%, mode: {}", state.battery, state.mode);
//!
//! // Clean specific rooms (1 pass, rooms 0 and 2)
//! robot.clean_rooms(&RoomCleanCommand {
//!     clean_times: 1,
//!     room_ids: vec![0, 2],
//! }).unwrap();
//!
//! // Or clean a rectangular zone
//! robot.clean_zone(&ZoneCleanCommand {
//!     clean_times: 1,
//!     zones: vec![Zone::rect(82, -13, 453, 203)],
//! }).unwrap();
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
//! - **`cloud`**: cloud API access (login, device discovery, map download via AWS STS)
//! - **`render`**: PNG rendering of layout maps and cleaning routes

/// Vacuum cleaner device control via Tuya TCP.
pub mod device;
/// Map file decoder (layout + route) with optional PNG rendering.
pub mod map;
/// DP 15 binary sweeper protocol: room/zone clean, forbidden zones, virtual walls.
pub mod protocol;
/// Device state model: DPS event parsing and enum types.
pub mod types;

// Re-export for convenience
#[cfg(feature = "cloud")]
pub use device::xplorer_oem_credentials;
pub use device::{Device, XPlorer};
pub use tuya_rs;
pub use tuya_rs::connection::Transport;

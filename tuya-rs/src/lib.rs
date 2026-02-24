#![warn(missing_docs)]

//! # tuya-rs
//!
//! Tuya v3.3 local protocol implementation in Rust.
//!
//! Provides TCP connection, packet encoding/decoding, and AES-ECB encryption
//! for communicating with Tuya-based IoT devices on the local network.
//!
//! ## Quick start
//!
//! ```no_run
//! use tuya_rs::connection::{DeviceConfig, TuyaConnection, TuyaCommand, build_dps_json, now};
//! use serde_json::json;
//!
//! // Connect to a device
//! let config = DeviceConfig {
//!     dev_id: "device_id_here".into(),
//!     address: "192.168.1.100".into(),
//!     local_key: "0123456789abcdef".into(),
//!     ..Default::default()
//! };
//! let mut conn = TuyaConnection::connect(&config).unwrap();
//!
//! // Query device state — triggers STATUS pushes
//! conn.send(TuyaCommand::DpQuery, b"{}".to_vec()).unwrap();
//!
//! // Set a DPS value (e.g. turn on = DP 1)
//! let payload = build_dps_json(conn.dev_id(), now(), &[("1", json!(true))]);
//! conn.send(TuyaCommand::Control, payload.into_bytes()).unwrap();
//! ```
//!
//! ## Discovery
//!
//! Find devices on the local network via UDP broadcast:
//!
//! ```no_run
//! use tuya_rs::discovery::discover;
//! use std::time::Duration;
//!
//! let devices = discover(Duration::from_secs(5)).unwrap();
//! for dev in &devices {
//!     println!("{} @ {} (v{})", dev.device_id, dev.ip, dev.version);
//! }
//! ```
//!
//! Note: discovery finds `device_id` and `ip` but **not** the `local_key` —
//! that requires the Tuya cloud API or tools like
//! [tinytuya](https://github.com/jasonacox/tinytuya).
//!
//! ## Features
//!
//! - **Default (local only)**: UDP device discovery, TCP packet codec, AES-128-ECB encryption, CRC32 validation
//! - **`cloud`**: OEM Mobile API client (login, device discovery, AWS STS map storage)

/// Tuya OEM Mobile API client (cloud feature).
#[cfg(feature = "cloud")]
pub mod api;
/// TCP connection and packet codec for Tuya v3.3 protocol.
pub mod connection;
/// AES-128-ECB encryption and RSA utilities.
pub mod crypto;
/// UDP device discovery on the local network.
pub mod discovery;
/// HMAC-SHA256 request signing for Tuya API (cloud feature).
#[cfg(feature = "cloud")]
pub mod signing;

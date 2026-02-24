//! # tuya-rs
//!
//! Tuya v3.3 local protocol implementation in Rust.
//!
//! Provides TCP connection, packet encoding/decoding, and AES-ECB encryption
//! for communicating with Tuya-based IoT devices on the local network.
//!
//! ## Features
//!
//! - **Default (local only)**: UDP device discovery, TCP packet codec, AES-128-ECB encryption, CRC32 validation
//! - **`cloud`**: OEM Mobile API client (login, device discovery, AWS STS map storage)

#[cfg(feature = "cloud")]
pub mod api;
pub mod connection;
pub mod crypto;
pub mod discovery;
#[cfg(feature = "cloud")]
pub mod signing;

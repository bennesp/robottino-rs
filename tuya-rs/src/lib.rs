#![warn(missing_docs)]

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

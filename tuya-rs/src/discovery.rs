//! Tuya local device discovery via UDP broadcast.
//!
//! Tuya devices broadcast their presence every ~5 seconds on the local network:
//! - Port 6666: plaintext JSON
//! - Port 6667: AES-ECB encrypted JSON
//!
//! The encryption key is a well-known constant baked into the Tuya firmware.
//! Some devices (even v3.3) use `md5(key)` instead of the raw key.
//!
//! Discovery finds `device_id` and `ip` but **not** the `local_key` —
//! that must be obtained from the Tuya cloud API or tools like
//! [tinytuya](https://github.com/jasonacox/tinytuya).

use std::collections::HashMap;
use std::net::UdpSocket;
use std::time::{Duration, Instant};

use md5::{Digest, Md5};
use serde::Deserialize;
use thiserror::Error;

use crate::crypto;

/// Well-known UDP broadcast key, common to all Tuya devices.
const UDP_KEY: &[u8; 16] = b"yGAdlopoPVldABfn";

/// Tuya packet magic prefix.
const MAGIC_PREFIX: [u8; 4] = [0x00, 0x00, 0x55, 0xAA];

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("socket error: {0}")]
    Socket(#[from] std::io::Error),
    #[error("no devices found within timeout")]
    Timeout,
}

/// A device discovered on the local network.
#[derive(Debug, Clone, Deserialize)]
pub struct DiscoveredDevice {
    /// Device ID (also known as `gwId`).
    #[serde(alias = "gwId")]
    pub device_id: String,
    /// Local IP address.
    pub ip: String,
    /// Protocol version reported by the device.
    #[serde(default)]
    pub version: String,
    /// Tuya product key.
    #[serde(alias = "productKey", default)]
    pub product_key: String,
    /// Whether the device uses encrypted communication.
    #[serde(default)]
    pub encrypt: bool,
}

/// Discover Tuya devices on the local network.
///
/// Listens on UDP ports 6666 and 6667 for broadcast packets.
/// Returns all unique devices found within `timeout`.
///
/// # Example
///
/// ```no_run
/// use tuya_rs::discovery::discover;
/// use std::time::Duration;
///
/// let devices = discover(Duration::from_secs(10)).unwrap();
/// for dev in &devices {
///     println!("{} @ {}", dev.device_id, dev.ip);
/// }
/// ```
pub fn discover(timeout: Duration) -> Result<Vec<DiscoveredDevice>, DiscoveryError> {
    let mut devices: HashMap<String, DiscoveredDevice> = HashMap::new();

    let sock_plain = bind_udp(6666)?;
    let sock_encrypted = bind_udp(6667)?;

    // Pre-compute both decryption keys
    let key_raw = *UDP_KEY;
    let key_md5 = md5_key(&key_raw);

    let deadline = Instant::now() + timeout;
    let mut buf = [0u8; 4096];

    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }

        // Try plaintext socket
        if let Ok(n) = sock_plain.recv(&mut buf)
            && let Some(dev) = parse_plaintext(&buf[..n])
        {
            devices.entry(dev.device_id.clone()).or_insert(dev);
        }

        // Try encrypted socket
        if let Ok(n) = sock_encrypted.recv(&mut buf)
            && let Some(dev) = parse_encrypted(&buf[..n], &key_md5, &key_raw)
        {
            devices.entry(dev.device_id.clone()).or_insert(dev);
        }
    }

    Ok(devices.into_values().collect())
}

/// Discover a single device, returning as soon as one is found.
///
/// Useful when you know there's exactly one Tuya device on the network.
pub fn discover_one(timeout: Duration) -> Result<DiscoveredDevice, DiscoveryError> {
    let sock_plain = bind_udp(6666)?;
    let sock_encrypted = bind_udp(6667)?;

    let key_raw = *UDP_KEY;
    let key_md5 = md5_key(&key_raw);

    let deadline = Instant::now() + timeout;
    let mut buf = [0u8; 4096];

    while Instant::now() < deadline {
        if let Ok(n) = sock_plain.recv(&mut buf)
            && let Some(dev) = parse_plaintext(&buf[..n])
        {
            return Ok(dev);
        }

        if let Ok(n) = sock_encrypted.recv(&mut buf)
            && let Some(dev) = parse_encrypted(&buf[..n], &key_md5, &key_raw)
        {
            return Ok(dev);
        }
    }

    Err(DiscoveryError::Timeout)
}

fn bind_udp(port: u16) -> Result<UdpSocket, std::io::Error> {
    let sock = UdpSocket::bind(("0.0.0.0", port))?;
    sock.set_nonblocking(true)?;
    Ok(sock)
}

fn md5_key(key: &[u8; 16]) -> [u8; 16] {
    let mut hasher = Md5::new();
    hasher.update(key);
    hasher.finalize().into()
}

/// Parse a plaintext broadcast (port 6666).
fn parse_plaintext(data: &[u8]) -> Option<DiscoveredDevice> {
    // Find JSON object within the packet (may have Tuya prefix/suffix)
    let start = data.iter().position(|&b| b == b'{')?;
    let end = data.iter().rposition(|&b| b == b'}')? + 1;
    serde_json::from_slice(&data[start..end]).ok()
}

/// Parse an encrypted broadcast (port 6667).
///
/// Packet format:
///   prefix(4) + seq(4) + cmd(4) + len(4) + return_code(4) + encrypted + crc(4) + suffix(4)
fn parse_encrypted(
    data: &[u8],
    key_md5: &[u8; 16],
    key_raw: &[u8; 16],
) -> Option<DiscoveredDevice> {
    if data.len() < 28 || data[..4] != MAGIC_PREFIX {
        return None;
    }

    // Encrypted payload sits between return_code and crc+suffix
    let payload = &data[20..data.len() - 8];
    if payload.is_empty() || !payload.len().is_multiple_of(16) {
        return None;
    }

    // Try md5(key) first (most common), then raw key
    for key in [key_md5, key_raw] {
        if let Ok(plain) = crypto::aes_ecb_decrypt(key, payload)
            && let Ok(dev) = serde_json::from_slice::<DiscoveredDevice>(&plain)
        {
            return Some(dev);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plaintext_json() {
        let json = br#"{"ip":"192.168.1.100","gwId":"abc123","version":"3.3","encrypt":true}"#;
        let dev = parse_plaintext(json).unwrap();
        assert_eq!(dev.device_id, "abc123");
        assert_eq!(dev.ip, "192.168.1.100");
        assert_eq!(dev.version, "3.3");
        assert!(dev.encrypt);
    }

    #[test]
    fn parse_plaintext_with_prefix_suffix() {
        // Simulate Tuya packet wrapping around JSON
        let mut data = vec![0x00, 0x00, 0x55, 0xAA];
        data.extend_from_slice(
            br#"{"ip":"10.0.0.1","gwId":"dev456","version":"3.4","productKey":"pk789"}"#,
        );
        data.extend_from_slice(&[0x00, 0x00, 0xAA, 0x55]);
        let dev = parse_plaintext(&data).unwrap();
        assert_eq!(dev.device_id, "dev456");
        assert_eq!(dev.ip, "10.0.0.1");
        assert_eq!(dev.product_key, "pk789");
    }

    #[test]
    fn parse_encrypted_roundtrip() {
        // Build a fake encrypted packet
        let json = br#"{"ip":"192.168.1.50","gwId":"enc789","version":"3.3","encrypt":true}"#;
        let key_md5 = md5_key(UDP_KEY);
        let encrypted = crypto::aes_ecb_encrypt(&key_md5, json);

        let mut packet = Vec::new();
        packet.extend_from_slice(&MAGIC_PREFIX); // prefix
        packet.extend_from_slice(&[0u8; 4]); // seq
        packet.extend_from_slice(&[0, 0, 0, 0x13]); // cmd = 19
        let data_len = (4 + encrypted.len() + 4) as u32; // ret + payload + crc
        packet.extend_from_slice(&data_len.to_be_bytes()); // len
        packet.extend_from_slice(&[0u8; 4]); // return_code
        packet.extend_from_slice(&encrypted); // encrypted payload
        packet.extend_from_slice(&[0u8; 4]); // crc (not validated in discovery)
        packet.extend_from_slice(&[0x00, 0x00, 0xAA, 0x55]); // suffix

        let key_raw = *UDP_KEY;
        let dev = parse_encrypted(&packet, &key_md5, &key_raw).unwrap();
        assert_eq!(dev.device_id, "enc789");
        assert_eq!(dev.ip, "192.168.1.50");
    }

    #[test]
    fn parse_encrypted_with_raw_key() {
        // Some v3.3 devices use the raw key instead of md5
        let json = br#"{"ip":"10.0.0.5","gwId":"raw123","version":"3.3"}"#;
        let encrypted = crypto::aes_ecb_encrypt(UDP_KEY, json);

        let mut packet = Vec::new();
        packet.extend_from_slice(&MAGIC_PREFIX);
        packet.extend_from_slice(&[0u8; 4]); // seq
        packet.extend_from_slice(&[0, 0, 0, 0x13]); // cmd
        let data_len = (4 + encrypted.len() + 4) as u32;
        packet.extend_from_slice(&data_len.to_be_bytes());
        packet.extend_from_slice(&[0u8; 4]); // return_code
        packet.extend_from_slice(&encrypted);
        packet.extend_from_slice(&[0u8; 4]); // crc
        packet.extend_from_slice(&[0x00, 0x00, 0xAA, 0x55]);

        let key_md5 = md5_key(UDP_KEY);
        let key_raw = *UDP_KEY;
        let dev = parse_encrypted(&packet, &key_md5, &key_raw).unwrap();
        assert_eq!(dev.device_id, "raw123");
    }

    #[test]
    fn parse_encrypted_garbage() {
        let key_md5 = md5_key(UDP_KEY);
        let key_raw = *UDP_KEY;

        assert!(parse_encrypted(&[], &key_md5, &key_raw).is_none());
        assert!(parse_encrypted(&[0u8; 28], &key_md5, &key_raw).is_none());
    }

    #[test]
    fn md5_key_is_deterministic() {
        let k1 = md5_key(UDP_KEY);
        let k2 = md5_key(UDP_KEY);
        assert_eq!(k1, k2);
        assert_ne!(&k1, UDP_KEY, "md5 key should differ from raw key");
    }
}

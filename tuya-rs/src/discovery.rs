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

/// Errors that can occur during device discovery.
#[derive(Debug, Error)]
pub enum DiscoveryError {
    /// UDP socket bind or receive failure.
    #[error("socket error: {0}")]
    Socket(#[from] std::io::Error),
    /// No devices responded within the given timeout.
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

/// Abstraction over a non-blocking UDP receiver.
///
/// Implement this trait to provide a custom UDP backend (e.g. for testing).
pub trait UdpReceiver {
    /// Try to receive data. Returns `Err(WouldBlock)` if no data is available.
    fn recv(&self, buf: &mut [u8]) -> std::io::Result<usize>;
}

impl UdpReceiver for UdpSocket {
    fn recv(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        UdpSocket::recv(self, buf)
    }
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
    let sock_plain = bind_udp(6666)?;
    let sock_encrypted = bind_udp(6667)?;
    discover_with(&sock_plain, &sock_encrypted, timeout)
}

/// Discover a single device, returning as soon as one is found.
///
/// Useful when you know there's exactly one Tuya device on the network.
pub fn discover_one(timeout: Duration) -> Result<DiscoveredDevice, DiscoveryError> {
    let sock_plain = bind_udp(6666)?;
    let sock_encrypted = bind_udp(6667)?;
    discover_one_with(&sock_plain, &sock_encrypted, timeout)
}

/// Discovery loop over two generic receivers (plaintext + encrypted).
fn discover_with<R: UdpReceiver>(
    plain: &R,
    encrypted: &R,
    timeout: Duration,
) -> Result<Vec<DiscoveredDevice>, DiscoveryError> {
    let mut devices: HashMap<String, DiscoveredDevice> = HashMap::new();

    let key_raw = *UDP_KEY;
    let key_md5 = md5_key(&key_raw);

    let deadline = Instant::now() + timeout;
    let mut buf = [0u8; 4096];

    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }

        if let Ok(n) = plain.recv(&mut buf)
            && let Some(dev) = parse_plaintext(&buf[..n])
        {
            devices.entry(dev.device_id.clone()).or_insert(dev);
        }

        if let Ok(n) = encrypted.recv(&mut buf)
            && let Some(dev) = parse_encrypted(&buf[..n], &key_md5, &key_raw)
        {
            devices.entry(dev.device_id.clone()).or_insert(dev);
        }
    }

    Ok(devices.into_values().collect())
}

/// Single-device discovery loop over two generic receivers.
fn discover_one_with<R: UdpReceiver>(
    plain: &R,
    encrypted: &R,
    timeout: Duration,
) -> Result<DiscoveredDevice, DiscoveryError> {
    let key_raw = *UDP_KEY;
    let key_md5 = md5_key(&key_raw);

    let deadline = Instant::now() + timeout;
    let mut buf = [0u8; 4096];

    while Instant::now() < deadline {
        if let Ok(n) = plain.recv(&mut buf)
            && let Some(dev) = parse_plaintext(&buf[..n])
        {
            return Ok(dev);
        }

        if let Ok(n) = encrypted.recv(&mut buf)
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
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::io;

    // ── MockUdpReceiver ───────────────────────────────────

    struct MockReceiver {
        packets: RefCell<VecDeque<Vec<u8>>>,
    }

    impl MockReceiver {
        fn new(packets: Vec<Vec<u8>>) -> Self {
            Self {
                packets: RefCell::new(packets.into()),
            }
        }

        fn empty() -> Self {
            Self::new(vec![])
        }
    }

    impl UdpReceiver for MockReceiver {
        fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
            match self.packets.borrow_mut().pop_front() {
                Some(data) => {
                    let n = data.len().min(buf.len());
                    buf[..n].copy_from_slice(&data[..n]);
                    Ok(n)
                }
                None => Err(io::Error::new(io::ErrorKind::WouldBlock, "no data")),
            }
        }
    }

    /// Build a plaintext discovery packet.
    fn plaintext_packet(device_id: &str, ip: &str) -> Vec<u8> {
        format!(r#"{{"ip":"{ip}","gwId":"{device_id}","version":"3.3"}}"#).into_bytes()
    }

    /// Build an encrypted discovery packet.
    fn encrypted_packet(device_id: &str, ip: &str) -> Vec<u8> {
        let json =
            format!(r#"{{"ip":"{ip}","gwId":"{device_id}","version":"3.3","encrypt":true}}"#);
        let key_md5 = md5_key(UDP_KEY);
        let encrypted = crypto::aes_ecb_encrypt(&key_md5, json.as_bytes());

        let mut packet = Vec::new();
        packet.extend_from_slice(&MAGIC_PREFIX);
        packet.extend_from_slice(&[0u8; 4]); // seq
        packet.extend_from_slice(&[0u8; 4]); // cmd
        packet.extend_from_slice(&[0u8; 4]); // len
        packet.extend_from_slice(&[0u8; 4]); // return_code
        packet.extend_from_slice(&encrypted);
        packet.extend_from_slice(&[0u8; 4]); // crc
        packet.extend_from_slice(&[0u8; 4]); // suffix
        packet
    }

    // ── parse_* tests (existing) ──────────────────────────

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
        let json = br#"{"ip":"192.168.1.50","gwId":"enc789","version":"3.3","encrypt":true}"#;
        let key_md5 = md5_key(UDP_KEY);
        let encrypted = crypto::aes_ecb_encrypt(&key_md5, json);

        let mut packet = Vec::new();
        packet.extend_from_slice(&MAGIC_PREFIX);
        packet.extend_from_slice(&[0u8; 4]);
        packet.extend_from_slice(&[0, 0, 0, 0x13]);
        let data_len = (4 + encrypted.len() + 4) as u32;
        packet.extend_from_slice(&data_len.to_be_bytes());
        packet.extend_from_slice(&[0u8; 4]);
        packet.extend_from_slice(&encrypted);
        packet.extend_from_slice(&[0u8; 4]);
        packet.extend_from_slice(&[0x00, 0x00, 0xAA, 0x55]);

        let key_raw = *UDP_KEY;
        let dev = parse_encrypted(&packet, &key_md5, &key_raw).unwrap();
        assert_eq!(dev.device_id, "enc789");
        assert_eq!(dev.ip, "192.168.1.50");
    }

    #[test]
    fn parse_encrypted_with_raw_key() {
        let json = br#"{"ip":"10.0.0.5","gwId":"raw123","version":"3.3"}"#;
        let encrypted = crypto::aes_ecb_encrypt(UDP_KEY, json);

        let mut packet = Vec::new();
        packet.extend_from_slice(&MAGIC_PREFIX);
        packet.extend_from_slice(&[0u8; 4]);
        packet.extend_from_slice(&[0, 0, 0, 0x13]);
        let data_len = (4 + encrypted.len() + 4) as u32;
        packet.extend_from_slice(&data_len.to_be_bytes());
        packet.extend_from_slice(&[0u8; 4]);
        packet.extend_from_slice(&encrypted);
        packet.extend_from_slice(&[0u8; 4]);
        packet.extend_from_slice(&[0x00, 0x00, 0xAA, 0x55]);

        let key_md5 = md5_key(UDP_KEY);
        let key_raw = *UDP_KEY;
        let dev = parse_encrypted(&packet, &key_md5, &key_raw).unwrap();
        assert_eq!(dev.device_id, "raw123");
    }

    #[test]
    fn parse_plaintext_no_braces() {
        assert!(parse_plaintext(b"no json here").is_none());
        assert!(parse_plaintext(b"").is_none());
    }

    #[test]
    fn parse_plaintext_invalid_json() {
        assert!(parse_plaintext(b"{not valid json}").is_none());
    }

    #[test]
    fn parse_encrypted_bad_prefix() {
        let key_md5 = md5_key(UDP_KEY);
        let key_raw = *UDP_KEY;
        let mut data = vec![0xFF; 32];
        data[..4].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
        assert!(parse_encrypted(&data, &key_md5, &key_raw).is_none());
    }

    #[test]
    fn parse_encrypted_empty_payload() {
        let key_md5 = md5_key(UDP_KEY);
        let key_raw = *UDP_KEY;
        let mut data = vec![0u8; 28];
        data[..4].copy_from_slice(&MAGIC_PREFIX);
        assert!(parse_encrypted(&data, &key_md5, &key_raw).is_none());
    }

    #[test]
    fn parse_encrypted_non_aligned_payload() {
        let key_md5 = md5_key(UDP_KEY);
        let key_raw = *UDP_KEY;
        let mut data = vec![0u8; 37];
        data[..4].copy_from_slice(&MAGIC_PREFIX);
        assert!(parse_encrypted(&data, &key_md5, &key_raw).is_none());
    }

    #[test]
    fn parse_encrypted_valid_aes_but_invalid_json() {
        let key_md5 = md5_key(UDP_KEY);
        let key_raw = *UDP_KEY;
        let encrypted = crypto::aes_ecb_encrypt(&key_md5, b"not json at all!");

        let mut packet = Vec::new();
        packet.extend_from_slice(&MAGIC_PREFIX);
        packet.extend_from_slice(&[0u8; 4]);
        packet.extend_from_slice(&[0u8; 4]);
        packet.extend_from_slice(&[0u8; 4]);
        packet.extend_from_slice(&[0u8; 4]);
        packet.extend_from_slice(&encrypted);
        packet.extend_from_slice(&[0u8; 4]);
        packet.extend_from_slice(&[0u8; 4]);
        assert!(parse_encrypted(&packet, &key_md5, &key_raw).is_none());
    }

    #[test]
    fn discovered_device_defaults() {
        let json = br#"{"ip":"10.0.0.1","gwId":"d1"}"#;
        let dev = parse_plaintext(json).unwrap();
        assert_eq!(dev.version, "");
        assert_eq!(dev.product_key, "");
        assert!(!dev.encrypt);
    }

    // ── discover_with tests ───────────────────────────────

    #[test]
    fn discover_with_plaintext_device() {
        let plain = MockReceiver::new(vec![plaintext_packet("dev1", "10.0.0.1")]);
        let encrypted = MockReceiver::empty();
        let devices = discover_with(&plain, &encrypted, Duration::from_millis(10)).unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].device_id, "dev1");
        assert_eq!(devices[0].ip, "10.0.0.1");
    }

    #[test]
    fn discover_with_encrypted_device() {
        let plain = MockReceiver::empty();
        let encrypted = MockReceiver::new(vec![encrypted_packet("dev2", "10.0.0.2")]);
        let devices = discover_with(&plain, &encrypted, Duration::from_millis(10)).unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].device_id, "dev2");
        assert!(devices[0].encrypt);
    }

    #[test]
    fn discover_with_deduplicates() {
        let plain = MockReceiver::new(vec![
            plaintext_packet("dev1", "10.0.0.1"),
            plaintext_packet("dev1", "10.0.0.1"), // duplicate
        ]);
        let encrypted = MockReceiver::empty();
        let devices = discover_with(&plain, &encrypted, Duration::from_millis(10)).unwrap();
        assert_eq!(devices.len(), 1);
    }

    #[test]
    fn discover_with_multiple_devices() {
        let plain = MockReceiver::new(vec![
            plaintext_packet("dev1", "10.0.0.1"),
            plaintext_packet("dev2", "10.0.0.2"),
        ]);
        let encrypted = MockReceiver::new(vec![encrypted_packet("dev3", "10.0.0.3")]);
        let devices = discover_with(&plain, &encrypted, Duration::from_millis(10)).unwrap();
        assert_eq!(devices.len(), 3);
    }

    #[test]
    fn discover_with_no_devices() {
        let plain = MockReceiver::empty();
        let encrypted = MockReceiver::empty();
        let devices = discover_with(&plain, &encrypted, Duration::from_millis(10)).unwrap();
        assert!(devices.is_empty());
    }

    #[test]
    fn discover_with_ignores_garbage() {
        let plain = MockReceiver::new(vec![
            b"not json".to_vec(),
            plaintext_packet("dev1", "10.0.0.1"),
        ]);
        let encrypted = MockReceiver::new(vec![vec![0xFF; 32]]);
        let devices = discover_with(&plain, &encrypted, Duration::from_millis(10)).unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].device_id, "dev1");
    }

    // ── discover_one_with tests ───────────────────────────

    #[test]
    fn discover_one_with_plaintext() {
        let plain = MockReceiver::new(vec![plaintext_packet("dev1", "10.0.0.1")]);
        let encrypted = MockReceiver::empty();
        let dev = discover_one_with(&plain, &encrypted, Duration::from_millis(100)).unwrap();
        assert_eq!(dev.device_id, "dev1");
    }

    #[test]
    fn discover_one_with_encrypted() {
        let plain = MockReceiver::empty();
        let encrypted = MockReceiver::new(vec![encrypted_packet("dev2", "10.0.0.2")]);
        let dev = discover_one_with(&plain, &encrypted, Duration::from_millis(100)).unwrap();
        assert_eq!(dev.device_id, "dev2");
    }

    #[test]
    fn discover_one_with_timeout() {
        let plain = MockReceiver::empty();
        let encrypted = MockReceiver::empty();
        let err = discover_one_with(&plain, &encrypted, Duration::from_millis(10)).unwrap_err();
        assert!(matches!(err, DiscoveryError::Timeout));
    }

    #[test]
    fn discover_one_with_returns_first() {
        let plain = MockReceiver::new(vec![
            plaintext_packet("first", "10.0.0.1"),
            plaintext_packet("second", "10.0.0.2"),
        ]);
        let encrypted = MockReceiver::empty();
        let dev = discover_one_with(&plain, &encrypted, Duration::from_millis(100)).unwrap();
        assert_eq!(dev.device_id, "first");
    }

    #[test]
    fn discover_one_with_skips_garbage() {
        let plain = MockReceiver::new(vec![
            b"garbage".to_vec(),
            plaintext_packet("dev1", "10.0.0.1"),
        ]);
        let encrypted = MockReceiver::empty();
        let dev = discover_one_with(&plain, &encrypted, Duration::from_millis(100)).unwrap();
        assert_eq!(dev.device_id, "dev1");
    }
}

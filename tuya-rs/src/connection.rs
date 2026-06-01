use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use thiserror::Error;

use crate::crypto;

/// Connection parameters for a Tuya device over local network.
///
/// # Examples
///
/// ```
/// use tuya_rs::connection::DeviceConfig;
///
/// let config = DeviceConfig {
///     dev_id: "my_device_id".into(),
///     address: "192.168.1.100".into(),
///     local_key: "0123456789abcdef".into(),
///     ..Default::default()
/// };
/// assert_eq!(config.version, 3.3);
/// assert_eq!(config.port, 6668);
/// ```
#[derive(Debug, Clone)]
pub struct DeviceConfig {
    /// Tuya device ID (`devId`).
    pub dev_id: String,
    /// Device IP address on local network.
    pub address: String,
    /// AES-128 local key (16 ASCII characters).
    pub local_key: String,
    /// Protocol version (default 3.3).
    pub version: f32,
    /// TCP port (default 6668).
    pub port: u16,
}

impl DeviceConfig {
    /// Build config from environment variables: DEVICE_IP, DEVICE_ID, LOCAL_KEY.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tuya_rs::connection::DeviceConfig;
    ///
    /// // Requires DEVICE_ID, DEVICE_IP, LOCAL_KEY env vars
    /// let config = DeviceConfig::from_env().expect("env vars not set");
    /// ```
    pub fn from_env() -> Result<Self, String> {
        Ok(Self {
            dev_id: std::env::var("DEVICE_ID").map_err(|_| "DEVICE_ID not set")?,
            address: std::env::var("DEVICE_IP").map_err(|_| "DEVICE_IP not set")?,
            local_key: std::env::var("LOCAL_KEY").map_err(|_| "LOCAL_KEY not set")?,
            ..Default::default()
        })
    }
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            dev_id: String::new(),
            address: String::new(),
            local_key: String::new(),
            version: 3.3,
            port: 6668,
        }
    }
}

/// Error type for device operations.
#[derive(Debug, Error)]
pub enum DeviceError {
    /// TCP connection could not be established.
    #[error("TCP connection failed: {0}")]
    ConnectionFailed(String),
    /// AES decryption produced invalid data.
    #[error("AES decryption failed")]
    DecryptionFailed,
    /// Socket read timed out.
    #[error("socket timeout")]
    Timeout,
    /// Response packet is malformed.
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    /// TCP connection was dropped.
    #[error("connection dropped")]
    Disconnected,
}

// ── Tuya v3.3 packet format ────────────────────────────────

const MAGIC_PREFIX: u32 = 0x000055AA;
const MAGIC_SUFFIX: u32 = 0x0000AA55;
/// Maximum allowed packet payload size (64 KB). Protects against
/// malformed packets that would otherwise cause unbounded allocation.
const MAX_PACKET_SIZE: usize = 65_536;

/// Tuya command codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum TuyaCommand {
    /// Set device DPS values (cmd 7).
    Control = 7,
    /// Status push from device (cmd 8).
    Status = 8,
    /// Keep-alive ping (cmd 9).
    Heartbeat = 9,
    /// Query device DPS state (cmd 10).
    DpQuery = 10,
    /// Request refresh of specific DPS (cmd 18).
    UpdateDps = 18,
}

/// A raw Tuya v3.3 packet.
#[derive(Debug, Clone, PartialEq)]
pub struct TuyaPacket {
    /// Sequence number.
    pub seq_num: u32,
    /// Command code.
    pub command: u32,
    /// Decrypted payload bytes (typically JSON).
    pub payload: Vec<u8>,
}

impl TuyaPacket {
    /// Encode a packet to bytes with AES-ECB encryption and CRC32.
    ///
    /// Format: `prefix(4) + seq(4) + cmd(4) + len(4) + [v3.3_header(15)] + encrypted + crc32(4) + suffix(4)`
    ///
    /// The v3.3 protocol header is only included for certain commands (Control, Status).
    /// DpQuery, UpdateDps, and Heartbeat send encrypted data directly without the header,
    /// matching tinytuya's `NO_PROTOCOL_HEADER_CMDS` behavior.
    pub fn to_bytes(&self, key: &[u8; 16]) -> Vec<u8> {
        let encrypted = crypto::aes_ecb_encrypt(key, &self.payload);

        // Commands that skip the v3.3 protocol header (same as tinytuya)
        let needs_header = !matches!(
            self.command,
            9 | 10 | 16 | 18 // Heartbeat, DpQuery, DpQueryNew, UpdateDps
        );

        let header_bytes: &[u8] = if needs_header {
            b"3.3\0\0\0\0\0\0\0\0\0\0\0\0"
        } else {
            b""
        };
        let data_len = header_bytes.len() + encrypted.len() + 8; // +8 for CRC + suffix

        let mut buf = Vec::with_capacity(16 + data_len);
        buf.extend_from_slice(&MAGIC_PREFIX.to_be_bytes());
        buf.extend_from_slice(&self.seq_num.to_be_bytes());
        buf.extend_from_slice(&self.command.to_be_bytes());
        buf.extend_from_slice(&(data_len as u32).to_be_bytes());
        buf.extend_from_slice(header_bytes);
        buf.extend_from_slice(&encrypted);

        // CRC32 over everything before this point
        let crc = crc32fast::hash(&buf);
        buf.extend_from_slice(&crc.to_be_bytes());
        buf.extend_from_slice(&MAGIC_SUFFIX.to_be_bytes());

        buf
    }

    /// Decode a packet from bytes, decrypting the payload with AES-ECB.
    ///
    /// Tuya v3.3 response formats vary — the data region may include:
    ///   - v3.3 prefix ("3.3\0..." 15 bytes)
    ///   - return code (4 bytes: 0x00000000 = OK, 0x00000001 = error)
    ///   - AES-encrypted payload (multiple of 16 bytes)
    ///
    /// Not all responses include all parts. Status pushes (cmd 8) typically
    /// have only the v3.3 prefix + encrypted data, with no return code.
    /// ACKs may have prefix + return code only. This method tries multiple
    /// interpretations and returns the first that succeeds.
    pub fn from_bytes(data: &[u8], key: &[u8; 16]) -> Result<Self, DeviceError> {
        if data.len() < 24 {
            return Err(DeviceError::InvalidResponse("packet too short".into()));
        }

        // Verify prefix
        let prefix = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        if prefix != MAGIC_PREFIX {
            return Err(DeviceError::InvalidResponse(format!(
                "bad prefix: 0x{prefix:08X}"
            )));
        }

        let seq_num = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let command = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        let total_len = u32::from_be_bytes([data[12], data[13], data[14], data[15]]) as usize;

        // Verify suffix
        let suffix_start = 16 + total_len - 4;
        if data.len() < suffix_start + 4 {
            return Err(DeviceError::InvalidResponse("packet truncated".into()));
        }
        let suffix = u32::from_be_bytes([
            data[suffix_start],
            data[suffix_start + 1],
            data[suffix_start + 2],
            data[suffix_start + 3],
        ]);
        if suffix != MAGIC_SUFFIX {
            return Err(DeviceError::InvalidResponse(format!(
                "bad suffix: 0x{suffix:08X}"
            )));
        }

        // CRC32: last 8 bytes are CRC + suffix
        let crc_start = suffix_start - 4;
        let received_crc = u32::from_be_bytes([
            data[crc_start],
            data[crc_start + 1],
            data[crc_start + 2],
            data[crc_start + 3],
        ]);
        let computed_crc = crc32fast::hash(&data[..crc_start]);
        if received_crc != computed_crc {
            return Err(DeviceError::InvalidResponse(format!(
                "CRC mismatch: received 0x{received_crc:08X}, computed 0x{computed_crc:08X}"
            )));
        }

        let offset = 16usize;
        let data_end = crc_start;

        if data_end <= offset {
            return Ok(TuyaPacket {
                seq_num,
                command,
                payload: Vec::new(),
            });
        }

        let raw = &data[offset..data_end];

        // Try all known v3.3 response formats and return the first that works.
        if let Some(payload) = Self::try_decode(raw, key) {
            return Ok(TuyaPacket {
                seq_num,
                command,
                payload,
            });
        }

        // Nothing worked — include hex dump for debugging
        let hex: String = raw
            .iter()
            .take(64)
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
        Err(DeviceError::InvalidResponse(format!(
            "cannot decode seq={seq_num} cmd={command} data={}b: {hex}{}",
            raw.len(),
            if raw.len() > 64 { "..." } else { "" }
        )))
    }

    /// Try to decode the data region of a v3.3 packet.
    ///
    /// The device can send data in several formats:
    ///   A. `prefix(15) + encrypted`              — command responses (prefix first)
    ///   B. `prefix(15) + retcode(4) + encrypted`  — command ACKs with data
    ///   C. `retcode(4) + prefix(15) + encrypted`  — status pushes (retcode first!)
    ///   D. `retcode(4) + encrypted`               — responses without prefix
    ///   E. `retcode(4) + plaintext`               — error messages (retcode=1)
    ///   F. `retcode(4)` only                      — ACK with no data
    ///   G. `encrypted` only                       — bare encrypted payload
    fn try_decode(raw: &[u8], key: &[u8; 16]) -> Option<Vec<u8>> {
        if raw.is_empty() {
            return Some(Vec::new());
        }

        let has_prefix_at = |off: usize| -> bool {
            raw.len() > off + 2 && raw[off] == b'3' && raw[off + 1] == b'.' && raw[off + 2] == b'3'
        };

        let try_aes = |slice: &[u8]| -> Option<Vec<u8>> {
            if !slice.is_empty() && slice.len().is_multiple_of(16) {
                let decrypted = crypto::aes_ecb_decrypt(key, slice).ok()?;
                if decrypted.is_empty() {
                    return Some(decrypted);
                }
                // Tuya encrypted payloads are always JSON objects.
                // Reject false-positive PKCS7 padding matches.
                if decrypted.first() == Some(&b'{') {
                    Some(decrypted)
                } else {
                    None
                }
            } else {
                None
            }
        };

        // Format A: prefix(15) + encrypted
        if has_prefix_at(0) && raw.len() > 15 {
            let after_prefix = &raw[15..];
            if let Some(payload) = try_aes(after_prefix) {
                return Some(payload);
            }

            // Format B: prefix(15) + retcode(4) + encrypted/plaintext
            if after_prefix.len() >= 4 {
                let retcode = u32::from_be_bytes([
                    after_prefix[0],
                    after_prefix[1],
                    after_prefix[2],
                    after_prefix[3],
                ]);
                let after_rc = &after_prefix[4..];
                if retcode == 1 {
                    return Some(after_rc.to_vec()); // plaintext error
                }
                if retcode == 0 {
                    if after_rc.is_empty() {
                        return Some(Vec::new());
                    }
                    if let Some(payload) = try_aes(after_rc) {
                        return Some(payload);
                    }
                }
            }
        }

        // Check for retcode at position 0
        if raw.len() >= 4 {
            let retcode = u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]);

            if retcode <= 1 {
                let after_rc = &raw[4..];

                // Format F: retcode only
                if after_rc.is_empty() {
                    return Some(Vec::new());
                }

                // Format E: retcode(1) + plaintext error
                if retcode == 1 {
                    return Some(after_rc.to_vec());
                }

                // Format C: retcode(0) + prefix(15) + encrypted
                if has_prefix_at(4) && after_rc.len() > 15 {
                    let after_both = &after_rc[15..];
                    if let Some(payload) = try_aes(after_both) {
                        return Some(payload);
                    }
                }

                // Format D: retcode(0) + encrypted
                if let Some(payload) = try_aes(after_rc) {
                    return Some(payload);
                }
            }
        }

        // Format G: bare encrypted (no prefix, no retcode)
        if let Some(payload) = try_aes(raw) {
            return Some(payload);
        }

        // Last resort: plaintext
        if raw.iter().all(|&b| b.is_ascii_graphic() || b == b' ') {
            return Some(raw.to_vec());
        }

        None
    }
}

// ── Transport trait ──────────────────────────────────────────

/// Abstraction over the Tuya device communication channel.
///
/// Implemented by [`TuyaConnection`] for real TCP connections.
/// Can be mocked for testing without a physical device.
pub trait Transport {
    /// Return the device ID.
    fn dev_id(&self) -> &str;
    /// Send a command and read one response.
    fn send(&mut self, command: TuyaCommand, payload: Vec<u8>) -> Result<TuyaPacket, DeviceError>;
    /// Read one packet from the device.
    fn recv(&mut self) -> Result<TuyaPacket, DeviceError>;
}

// ── TCP connection ───────────────────────────────────────────

/// Live TCP connection to a Tuya v3.3 device.
pub struct TuyaConnection {
    dev_id: String,
    key: [u8; 16],
    stream: TcpStream,
    seq: u32,
}

impl TuyaConnection {
    /// Connect to a device over TCP/6668 (5 s timeout).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tuya_rs::connection::{DeviceConfig, TuyaConnection, TuyaCommand, Transport};
    ///
    /// let config = DeviceConfig {
    ///     dev_id: "my_device_id".into(),
    ///     address: "192.168.1.100".into(),
    ///     local_key: "0123456789abcdef".into(),
    ///     ..Default::default()
    /// };
    /// let mut conn = TuyaConnection::connect(&config).unwrap();
    /// let response = conn.send(TuyaCommand::DpQuery, b"{}".to_vec()).unwrap();
    /// println!("payload: {:?}", String::from_utf8_lossy(&response.payload));
    /// ```
    pub fn connect(config: &DeviceConfig) -> Result<Self, DeviceError> {
        let addr = format!("{}:{}", config.address, config.port);
        let sock_addr: std::net::SocketAddr = addr
            .parse()
            .map_err(|e: std::net::AddrParseError| DeviceError::ConnectionFailed(e.to_string()))?;

        let stream = TcpStream::connect_timeout(&sock_addr, Duration::from_secs(5))
            .map_err(|e| DeviceError::ConnectionFailed(e.to_string()))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| DeviceError::ConnectionFailed(e.to_string()))?;

        let key_bytes = config.local_key.as_bytes();
        if key_bytes.len() != 16 {
            return Err(DeviceError::ConnectionFailed(format!(
                "local_key must be exactly 16 bytes, got {}",
                key_bytes.len()
            )));
        }
        let mut key = [0u8; 16];
        key.copy_from_slice(key_bytes);

        Ok(Self {
            dev_id: config.dev_id.clone(),
            key,
            stream,
            seq: 0,
        })
    }

    /// Read one packet from the device.
    fn recv_packet(&mut self) -> Result<TuyaPacket, DeviceError> {
        let mut header = [0u8; 16];
        self.read_exact(&mut header)?;

        let prefix = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        if prefix != MAGIC_PREFIX {
            return Err(DeviceError::InvalidResponse(format!(
                "bad prefix: 0x{prefix:08X}"
            )));
        }

        let data_len =
            u32::from_be_bytes([header[12], header[13], header[14], header[15]]) as usize;

        if data_len > MAX_PACKET_SIZE {
            return Err(DeviceError::InvalidResponse(format!(
                "packet too large: {data_len} bytes (max {MAX_PACKET_SIZE})"
            )));
        }

        let mut rest = vec![0u8; data_len];
        self.read_exact(&mut rest)?;

        let mut full = Vec::with_capacity(16 + data_len);
        full.extend_from_slice(&header);
        full.extend_from_slice(&rest);

        TuyaPacket::from_bytes(&full, &self.key)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), DeviceError> {
        self.stream.read_exact(buf).map_err(|e| match e.kind() {
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => DeviceError::Timeout,
            _ => DeviceError::Disconnected,
        })
    }
}

impl Transport for TuyaConnection {
    fn dev_id(&self) -> &str {
        &self.dev_id
    }

    fn send(&mut self, command: TuyaCommand, payload: Vec<u8>) -> Result<TuyaPacket, DeviceError> {
        self.seq += 1;
        let packet = TuyaPacket {
            seq_num: self.seq,
            command: command as u32,
            payload,
        };
        let bytes = packet.to_bytes(&self.key);

        self.stream
            .write_all(&bytes)
            .map_err(|_| DeviceError::Disconnected)?;

        self.recv_packet()
    }

    fn recv(&mut self) -> Result<TuyaPacket, DeviceError> {
        self.recv_packet()
    }
}

// ── Helpers ─────────────────────────────────────────────────

/// Build a JSON payload for setting DPS values.
///
/// # Examples
///
/// ```
/// use tuya_rs::connection::build_dps_json;
/// use serde_json::json;
///
/// let payload = build_dps_json("device123", 1700000000, &[("1", json!(true))]);
/// let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
/// assert_eq!(parsed["devId"], "device123");
/// assert_eq!(parsed["dps"]["1"], true);
/// ```
pub fn build_dps_json(dev_id: &str, timestamp: u64, dps: &[(&str, serde_json::Value)]) -> String {
    let mut dps_map = serde_json::Map::new();
    for (k, v) in dps {
        dps_map.insert(k.to_string(), v.clone());
    }

    serde_json::json!({
        "devId": dev_id,
        "uid": "",
        "t": timestamp,
        "dps": dps_map,
    })
    .to_string()
}

/// Return current UNIX timestamp in seconds.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is before UNIX epoch")
        .as_secs()
}

/// Possible DP value types for raw access.
#[derive(Debug, Clone, PartialEq)]
pub enum DpValue {
    /// Boolean value.
    Boolean(bool),
    /// Integer value.
    Integer(i64),
    /// String value.
    String(String),
    /// Raw bytes (sent as base64).
    Raw(Vec<u8>),
}

/// Raw DPS update from the device.
#[derive(Debug, Clone, PartialEq)]
pub struct DpsUpdate {
    /// List of (DP number, value string) pairs.
    pub dps: Vec<(u8, String)>,
    /// Optional update timestamp.
    pub timestamp: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn build_dps_json_format() {
        let json = build_dps_json("devId123", 1770808371, &[("1", json!(true))]);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["devId"], "devId123");
        assert_eq!(parsed["uid"], "");
        assert_eq!(parsed["t"], 1770808371u64);
        assert_eq!(parsed["dps"]["1"], true);
    }

    // ── DeviceConfig ────────────────────────────────────────

    #[test]
    fn device_config_default() {
        let cfg = DeviceConfig::default();
        assert!(cfg.dev_id.is_empty());
        assert!(cfg.address.is_empty());
        assert!(cfg.local_key.is_empty());
        assert_eq!(cfg.version, 3.3);
        assert_eq!(cfg.port, 6668);
    }

    #[test]
    fn device_config_from_env_missing_vars() {
        // All env vars are almost certainly unset in test
        assert!(DeviceConfig::from_env().is_err());
    }

    // ── Packet encode (no-header commands) ──────────────────

    #[test]
    fn packet_encode_heartbeat_no_header() {
        let key = b"0123456789abcdef";
        let pkt = TuyaPacket {
            seq_num: 1,
            command: TuyaCommand::Heartbeat as u32,
            payload: b"{}".to_vec(),
        };
        let bytes = pkt.to_bytes(key);
        // Heartbeat should NOT have the "3.3\0..." header after the 16-byte fixed header
        // The encrypted data starts right at offset 16
        assert_ne!(&bytes[16..19], b"3.3");
    }

    #[test]
    fn packet_encode_control_has_header() {
        let key = b"0123456789abcdef";
        let pkt = TuyaPacket {
            seq_num: 1,
            command: TuyaCommand::Control as u32,
            payload: b"{}".to_vec(),
        };
        let bytes = pkt.to_bytes(key);
        assert_eq!(&bytes[16..19], b"3.3");
    }

    // ── from_bytes error paths ──────────────────────────────

    /// Build a raw packet with valid envelope (prefix, CRC, suffix) around arbitrary data.
    fn build_raw_packet(seq: u32, cmd: u32, data: &[u8]) -> Vec<u8> {
        let data_len = data.len() + 8; // +8 for CRC + suffix
        let mut buf = Vec::with_capacity(16 + data_len);
        buf.extend_from_slice(&MAGIC_PREFIX.to_be_bytes());
        buf.extend_from_slice(&seq.to_be_bytes());
        buf.extend_from_slice(&cmd.to_be_bytes());
        buf.extend_from_slice(&(data_len as u32).to_be_bytes());
        buf.extend_from_slice(data);
        let crc = crc32fast::hash(&buf);
        buf.extend_from_slice(&crc.to_be_bytes());
        buf.extend_from_slice(&MAGIC_SUFFIX.to_be_bytes());
        buf
    }

    #[test]
    fn from_bytes_too_short() {
        let key = b"0123456789abcdef";
        let err = TuyaPacket::from_bytes(&[0; 20], key).unwrap_err();
        assert!(matches!(err, DeviceError::InvalidResponse(_)));
    }

    #[test]
    fn from_bytes_bad_prefix() {
        let key = b"0123456789abcdef";
        let mut pkt = build_raw_packet(1, 7, b"");
        pkt[0] = 0xFF; // corrupt prefix
        let err = TuyaPacket::from_bytes(&pkt, key).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("bad prefix"));
    }

    #[test]
    fn from_bytes_bad_suffix() {
        let key = b"0123456789abcdef";
        let mut pkt = build_raw_packet(1, 7, b"some data here!!");
        let last = pkt.len();
        pkt[last - 1] = 0xFF; // corrupt suffix
        let err = TuyaPacket::from_bytes(&pkt, key).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("bad suffix"));
    }

    #[test]
    fn from_bytes_truncated() {
        let key = b"0123456789abcdef";
        // Valid prefix but claimed length exceeds actual data
        let mut buf = Vec::new();
        buf.extend_from_slice(&MAGIC_PREFIX.to_be_bytes());
        buf.extend_from_slice(&0u32.to_be_bytes()); // seq
        buf.extend_from_slice(&7u32.to_be_bytes()); // cmd
        buf.extend_from_slice(&255u32.to_be_bytes()); // len = 255 (way too big)
        buf.extend_from_slice(&[0u8; 8]); // not enough data
        let err = TuyaPacket::from_bytes(&buf, key).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("truncated"));
    }

    #[test]
    fn from_bytes_crc_mismatch() {
        let key = b"0123456789abcdef";
        let mut pkt = build_raw_packet(1, 7, b"some data here!!");
        // Corrupt a data byte (between header and CRC)
        pkt[16] ^= 0xFF;
        let err = TuyaPacket::from_bytes(&pkt, key).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("CRC mismatch"));
    }

    #[test]
    fn from_bytes_empty_payload() {
        let key = b"0123456789abcdef";
        let pkt = build_raw_packet(42, 9, &[]);
        let decoded = TuyaPacket::from_bytes(&pkt, key).unwrap();
        assert_eq!(decoded.seq_num, 42);
        assert_eq!(decoded.command, 9);
        assert!(decoded.payload.is_empty());
    }

    #[test]
    fn from_bytes_undecryptable_data() {
        let key = b"0123456789abcdef";
        // 16 bytes of random non-decodable data (not valid AES, not plaintext, not prefix)
        let garbage = [0x80u8; 16];
        let pkt = build_raw_packet(1, 7, &garbage);
        let err = TuyaPacket::from_bytes(&pkt, key).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("cannot decode"));
    }

    // ── try_decode format coverage (via from_bytes) ─────────

    #[test]
    fn decode_format_a_prefix_plus_encrypted() {
        // Already covered by packet_encode_decode_roundtrip (Control command)
        // but let's verify explicitly for a Status command
        let key = b"0123456789abcdef";
        let pkt = TuyaPacket {
            seq_num: 1,
            command: TuyaCommand::Status as u32,
            payload: b"{\"dps\":{\"1\":true}}".to_vec(),
        };
        let bytes = pkt.to_bytes(key);
        let decoded = TuyaPacket::from_bytes(&bytes, key).unwrap();
        assert_eq!(decoded.payload, pkt.payload);
    }

    #[test]
    fn decode_format_b_prefix_retcode0_encrypted() {
        let key = b"0123456789abcdef";
        let plaintext = b"{\"result\":\"ok\"}";
        let encrypted = crypto::aes_ecb_encrypt(key, plaintext);

        let mut data = Vec::new();
        data.extend_from_slice(b"3.3\0\0\0\0\0\0\0\0\0\0\0\0"); // prefix
        data.extend_from_slice(&0u32.to_be_bytes()); // retcode = 0
        data.extend_from_slice(&encrypted);

        let pkt = build_raw_packet(1, 8, &data);
        let decoded = TuyaPacket::from_bytes(&pkt, key).unwrap();
        assert_eq!(decoded.payload, plaintext);
    }

    #[test]
    fn decode_format_b_prefix_retcode0_empty() {
        let key = b"0123456789abcdef";
        let mut data = Vec::new();
        data.extend_from_slice(b"3.3\0\0\0\0\0\0\0\0\0\0\0\0"); // prefix
        data.extend_from_slice(&0u32.to_be_bytes()); // retcode = 0
        // no encrypted data → empty

        let pkt = build_raw_packet(1, 8, &data);
        let decoded = TuyaPacket::from_bytes(&pkt, key).unwrap();
        assert!(decoded.payload.is_empty());
    }

    #[test]
    fn decode_format_b_prefix_retcode1_plaintext_error() {
        let key = b"0123456789abcdef";
        let mut data = Vec::new();
        data.extend_from_slice(b"3.3\0\0\0\0\0\0\0\0\0\0\0\0"); // prefix
        data.extend_from_slice(&1u32.to_be_bytes()); // retcode = 1
        data.extend_from_slice(b"parse data error"); // plaintext error

        let pkt = build_raw_packet(1, 8, &data);
        let decoded = TuyaPacket::from_bytes(&pkt, key).unwrap();
        assert_eq!(decoded.payload, b"parse data error");
    }

    #[test]
    fn decode_format_c_retcode0_prefix_encrypted() {
        let key = b"0123456789abcdef";
        let plaintext = b"{\"status\":\"ok\"}";
        let encrypted = crypto::aes_ecb_encrypt(key, plaintext);

        let mut data = Vec::new();
        data.extend_from_slice(&0u32.to_be_bytes()); // retcode = 0
        data.extend_from_slice(b"3.3\0\0\0\0\0\0\0\0\0\0\0\0"); // prefix
        data.extend_from_slice(&encrypted);

        let pkt = build_raw_packet(1, 8, &data);
        let decoded = TuyaPacket::from_bytes(&pkt, key).unwrap();
        assert_eq!(decoded.payload, plaintext);
    }

    #[test]
    fn decode_format_d_retcode0_encrypted() {
        let key = b"0123456789abcdef";
        let plaintext = b"{\"dps\":{\"8\":72}}";
        let encrypted = crypto::aes_ecb_encrypt(key, plaintext);

        let mut data = Vec::new();
        data.extend_from_slice(&0u32.to_be_bytes()); // retcode = 0
        data.extend_from_slice(&encrypted);

        let pkt = build_raw_packet(1, 10, &data);
        let decoded = TuyaPacket::from_bytes(&pkt, key).unwrap();
        assert_eq!(decoded.payload, plaintext);
    }

    #[test]
    fn decode_format_e_retcode1_plaintext_error() {
        let key = b"0123456789abcdef";
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_be_bytes()); // retcode = 1
        data.extend_from_slice(b"json parse error");

        let pkt = build_raw_packet(1, 10, &data);
        let decoded = TuyaPacket::from_bytes(&pkt, key).unwrap();
        assert_eq!(decoded.payload, b"json parse error");
    }

    #[test]
    fn decode_format_f_retcode_only() {
        let key = b"0123456789abcdef";
        let data = 0u32.to_be_bytes(); // retcode = 0, nothing else

        let pkt = build_raw_packet(1, 7, &data);
        let decoded = TuyaPacket::from_bytes(&pkt, key).unwrap();
        assert!(decoded.payload.is_empty());
    }

    #[test]
    fn decode_format_g_bare_encrypted() {
        let key = b"0123456789abcdef";
        let plaintext = b"{\"bare\":true}";
        let encrypted = crypto::aes_ecb_encrypt(key, plaintext);

        // No prefix, no retcode — just encrypted
        let pkt = build_raw_packet(1, 10, &encrypted);
        let decoded = TuyaPacket::from_bytes(&pkt, key).unwrap();
        assert_eq!(decoded.payload, plaintext);
    }

    #[test]
    fn decode_plaintext_fallback() {
        let key = b"0123456789abcdef";
        // ASCII-only data that doesn't look like any known format
        // (not starting with "3.3", not a valid retcode pattern, not valid AES)
        let data = b"PLAIN TEXT ERROR MSG";

        let pkt = build_raw_packet(1, 7, data);
        let decoded = TuyaPacket::from_bytes(&pkt, key).unwrap();
        assert_eq!(decoded.payload, b"PLAIN TEXT ERROR MSG");
    }

    #[test]
    fn decode_hex_dump_truncated_at_64() {
        let key = b"0123456789abcdef";
        // 80 bytes of non-decodable binary (> 64 bytes for hex dump truncation)
        let garbage: Vec<u8> = (0..80).map(|i| 0x80 | (i & 0x0F)).collect();
        let pkt = build_raw_packet(1, 7, &garbage);
        let err = TuyaPacket::from_bytes(&pkt, key).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("..."));
    }
}

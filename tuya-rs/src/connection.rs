use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use thiserror::Error;

use crate::crypto;

/// Connection parameters for a Tuya device over local network.
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
                let retcode = u32::from_be_bytes(after_prefix[..4].try_into().unwrap());
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
            let retcode = u32::from_be_bytes(raw[..4].try_into().unwrap());

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
        .unwrap()
        .as_secs()
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
    /// use tuya_rs::connection::{DeviceConfig, TuyaConnection, TuyaCommand};
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

    /// Return the device ID.
    pub fn dev_id(&self) -> &str {
        &self.dev_id
    }

    /// Send a command and read one response.
    pub fn send(
        &mut self,
        command: TuyaCommand,
        payload: Vec<u8>,
    ) -> Result<TuyaPacket, DeviceError> {
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

        self.recv()
    }

    /// Read one packet from the device.
    pub fn recv(&mut self) -> Result<TuyaPacket, DeviceError> {
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
    fn magic_prefix_and_suffix() {
        assert_eq!(MAGIC_PREFIX, 0x000055AA);
        assert_eq!(MAGIC_SUFFIX, 0x0000AA55);
    }

    #[test]
    fn packet_encode_has_correct_markers() {
        let key = b"0123456789abcdef";
        let pkt = TuyaPacket {
            seq_num: 1,
            command: TuyaCommand::DpQuery as u32,
            payload: b"{}".to_vec(),
        };
        let bytes = pkt.to_bytes(key);

        // Check prefix
        assert_eq!(&bytes[0..4], &MAGIC_PREFIX.to_be_bytes());
        // Check suffix
        assert_eq!(&bytes[bytes.len() - 4..], &MAGIC_SUFFIX.to_be_bytes());
        // Check seq_num
        assert_eq!(
            u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            1
        );
        // Check command
        assert_eq!(
            u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            TuyaCommand::DpQuery as u32
        );
    }

    #[test]
    fn packet_crc32_valid() {
        let key = b"0123456789abcdef";
        let pkt = TuyaPacket {
            seq_num: 42,
            command: TuyaCommand::Control as u32,
            payload: b"{\"dps\":{\"1\":true}}".to_vec(),
        };
        let bytes = pkt.to_bytes(key);

        // CRC is 4 bytes before suffix
        let crc_start = bytes.len() - 8;
        let received_crc = u32::from_be_bytes([
            bytes[crc_start],
            bytes[crc_start + 1],
            bytes[crc_start + 2],
            bytes[crc_start + 3],
        ]);
        let computed_crc = crc32fast::hash(&bytes[..crc_start]);
        assert_eq!(received_crc, computed_crc);
    }

    #[test]
    fn packet_encode_decode_roundtrip() {
        let key = b"0123456789abcdef";
        let original = TuyaPacket {
            seq_num: 100,
            command: TuyaCommand::Status as u32,
            payload: b"{\"devId\":\"test\",\"dps\":{\"1\":true}}".to_vec(),
        };
        let bytes = original.to_bytes(key);
        let decoded = TuyaPacket::from_bytes(&bytes, key).unwrap();

        assert_eq!(decoded.seq_num, original.seq_num);
        assert_eq!(decoded.command, original.command);
        assert_eq!(decoded.payload, original.payload);
    }

    #[test]
    fn packet_decode_wrong_key_fails() {
        let key1 = b"0123456789abcdef";
        let key2 = b"fedcba9876543210";
        let pkt = TuyaPacket {
            seq_num: 1,
            command: TuyaCommand::Control as u32,
            payload: b"{\"test\":true}".to_vec(),
        };
        let bytes = pkt.to_bytes(key1);

        // CRC is computed over plaintext+encrypted combo, so it will still
        // be valid even with wrong key. The decryption will give garbage.
        // In practice this may or may not produce a padding error.
        // We just verify it doesn't panic.
        let result = TuyaPacket::from_bytes(&bytes, key2);
        match result {
            Ok(decoded) => assert_ne!(decoded.payload, pkt.payload),
            Err(_) => {} // Decryption/padding error is fine
        }
    }

    #[test]
    fn build_dps_json_format() {
        let json = build_dps_json("devId123", 1770808371, &[("1", json!(true))]);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["devId"], "devId123");
        assert_eq!(parsed["uid"], "");
        assert_eq!(parsed["t"], 1770808371u64);
        assert_eq!(parsed["dps"]["1"], true);
    }

    #[test]
    fn build_dps_json_multiple() {
        let json = build_dps_json(
            "dev",
            100,
            &[("1", json!(true)), ("4", json!("smart")), ("26", json!(50))],
        );
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["dps"]["1"], true);
        assert_eq!(parsed["dps"]["4"], "smart");
        assert_eq!(parsed["dps"]["26"], 50);
    }

    #[test]
    fn tuya_command_values() {
        assert_eq!(TuyaCommand::Control as u32, 7);
        assert_eq!(TuyaCommand::Status as u32, 8);
        assert_eq!(TuyaCommand::Heartbeat as u32, 9);
        assert_eq!(TuyaCommand::DpQuery as u32, 10);
        assert_eq!(TuyaCommand::UpdateDps as u32, 18);
    }
}

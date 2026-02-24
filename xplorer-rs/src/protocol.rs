use base64::Engine;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("message too short: need at least 4 bytes, got {0}")]
    TooShort(usize),
    #[error("invalid start byte: expected 0xAA, got 0x{0:02X}")]
    InvalidStartByte(u8),
    #[error("invalid base64: {0}")]
    InvalidBase64(#[from] base64::DecodeError),
    #[error("length mismatch: header says {expected} bytes, got {actual}")]
    LengthMismatch { expected: usize, actual: usize },
    #[error("unexpected command 0x{0:02X} for RoomCleanStatusResponse (expected 0x15)")]
    UnexpectedCommand(u8),
    #[error("payload too short for RoomCleanStatusResponse")]
    PayloadTooShort,
}

/// Command types for DP 15 binary protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CommandType {
    SetVirtualWall = 0x12,
    VirtualWallStatus = 0x13,
    SetRoomClean = 0x14,
    RoomCleanStatus = 0x15,
    RequestAreaClean = 0x17,
    SetVirtualArea = 0x1A,
    VirtualAreaStatus = 0x1B,
    SetZoneClean = 0x28,
    ZoneCleanStatus = 0x29,
    CustomizeData = 0x31,
}

/// Restriction mode for forbidden zones (cmd 0x1a).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ForbiddenMode {
    /// Full ban — robot will not enter this area ("zona vietata" in app).
    FullBan = 0x00,
    /// No sweep — robot enters but won't vacuum ("zona non lavabile" in app).
    NoSweep = 0x01,
    /// No mop — robot enters but won't mop.
    NoMop = 0x02,
}

/// A forbidden zone with its restriction mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForbiddenZone {
    pub mode: ForbiddenMode,
    pub zone: Zone,
}

/// A virtual wall defined by two endpoints (a line segment).
///
/// The robot will not cross this line during cleaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wall {
    pub start: (i16, i16),
    pub end: (i16, i16),
}

/// A room cleaning command to send via DP 15.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomCleanCommand {
    pub clean_times: u8,
    pub room_ids: Vec<u8>,
}

impl RoomCleanCommand {
    /// Encode to raw bytes: `aa <len_2byte_BE> 0x14 <clean_times> <num_rooms> <room_ids...> <checksum>`
    pub fn encode(&self) -> Vec<u8> {
        let cmd: u8 = 0x14;
        let num_rooms = self.room_ids.len() as u8;
        // payload = cmd + clean_times + num_rooms + room_ids
        let payload_len = 1 + 1 + 1 + self.room_ids.len();

        let mut buf = Vec::with_capacity(3 + payload_len + 1);
        buf.push(0xAA);
        buf.push((payload_len >> 8) as u8);
        buf.push(payload_len as u8);
        buf.push(cmd);
        buf.push(self.clean_times);
        buf.push(num_rooms);
        buf.extend_from_slice(&self.room_ids);

        // Checksum = sum of (cmd + data bytes) & 0xFF
        let checksum: u8 = buf[3..].iter().copied().fold(0u16, |acc, b| acc + b as u16) as u8;
        buf.push(checksum);
        buf
    }

    /// Encode and return as base64 string.
    pub fn encode_base64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(self.encode())
    }
}

/// A rectangular zone for zone cleaning, defined by 4 vertices (polygon).
///
/// Coordinates are in raw map units (i16, same as route points × 10).
/// Vertices are ordered: top-left, top-right, bottom-right, bottom-left.
/// For a simple axis-aligned rectangle, construct from two corners with [`Zone::rect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zone {
    pub vertices: [(i16, i16); 4],
}

impl Zone {
    /// Create an axis-aligned rectangular zone from two opposite corners.
    ///
    /// The corners are (x1, y1) = bottom-left and (x2, y2) = top-right
    /// (y-axis points up, matching the robot's coordinate system).
    pub fn rect(x1: i16, y1: i16, x2: i16, y2: i16) -> Self {
        Zone {
            vertices: [
                (x1, y2), // top-left
                (x2, y2), // top-right
                (x2, y1), // bottom-right
                (x1, y1), // bottom-left
            ],
        }
    }

    /// Create a rotated rectangular zone.
    ///
    /// Builds an axis-aligned rectangle from two corners, then rotates all
    /// vertices around the rectangle's center by `angle_deg` degrees
    /// (counter-clockwise positive).
    ///
    /// Use the map's `theta` value (typically `route_header.theta as f64 / 100.0`)
    /// to compensate for the coordinate system rotation relative to room walls.
    pub fn rotated_rect(x1: i16, y1: i16, x2: i16, y2: i16, angle_deg: f64) -> Self {
        let cx = (x1 as f64 + x2 as f64) / 2.0;
        let cy = (y1 as f64 + y2 as f64) / 2.0;
        let cos = angle_deg.to_radians().cos();
        let sin = angle_deg.to_radians().sin();

        let rotate = |x: f64, y: f64| -> (i16, i16) {
            let dx = x - cx;
            let dy = y - cy;
            let rx = cx + dx * cos - dy * sin;
            let ry = cy + dx * sin + dy * cos;
            (rx.round() as i16, ry.round() as i16)
        };

        let corners: [(f64, f64); 4] = [
            (x1 as f64, y2 as f64), // top-left
            (x2 as f64, y2 as f64), // top-right
            (x2 as f64, y1 as f64), // bottom-right
            (x1 as f64, y1 as f64), // bottom-left
        ];

        Zone {
            vertices: [
                rotate(corners[0].0, corners[0].1),
                rotate(corners[1].0, corners[1].1),
                rotate(corners[2].0, corners[2].1),
                rotate(corners[3].0, corners[3].1),
            ],
        }
    }
}

/// Encode a zone-based sweeper frame (used by [`ZoneCleanCommand`], cmd 0x28).
///
/// Format: `aa <len_2byte_BE> <cmd> <first_byte> <num_zones> [<num_vertices> <vertices...>]* <checksum>`
fn encode_zone_frame(cmd: u8, first_byte: u8, zones: &[Zone]) -> Vec<u8> {
    // payload = cmd + first_byte + num_zones + per-zone(num_vertices + vertices)
    let mut payload_len = 1 + 1 + 1;
    for zone in zones {
        payload_len += 1 + zone.vertices.len() * 4;
    }

    let mut buf = Vec::with_capacity(3 + payload_len + 1);
    buf.push(0xAA);
    buf.push((payload_len >> 8) as u8);
    buf.push(payload_len as u8);
    buf.push(cmd);
    buf.push(first_byte);
    buf.push(zones.len() as u8);
    for zone in zones {
        buf.push(zone.vertices.len() as u8);
        for &(x, y) in &zone.vertices {
            buf.extend_from_slice(&x.to_be_bytes());
            buf.extend_from_slice(&y.to_be_bytes());
        }
    }

    // Checksum = sum of (cmd + data bytes) & 0xFF
    let checksum: u8 = buf[3..].iter().copied().fold(0u16, |acc, b| acc + b as u16) as u8;
    buf.push(checksum);
    buf
}

/// A zone cleaning command to send via DP 15 (cmd 0x28).
///
/// Discovered by brute-force testing: cmd 0x28 is the setter, the robot
/// reports status back in cmd 0x29. Each zone is a polygon defined by
/// `num_vertices` points (typically 4 for a rectangle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneCleanCommand {
    pub clean_times: u8,
    pub zones: Vec<Zone>,
}

impl ZoneCleanCommand {
    /// Encode to raw bytes:
    /// `aa <len_2byte_BE> 0x28 <clean_times> <num_zones> <num_vertices> <vertices...> <checksum>`
    ///
    /// Each vertex is 2 × i16 BE = 4 bytes (x, y).
    /// For a single zone with 4 vertices: payload = 1 + 1 + 1 + 1 + 4*4 = 20 bytes.
    pub fn encode(&self) -> Vec<u8> {
        encode_zone_frame(0x28, self.clean_times, &self.zones)
    }

    /// Encode and return as base64 string.
    pub fn encode_base64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(self.encode())
    }
}

/// A forbidden zone command to send via DP 15 (cmd 0x1a).
///
/// Sets no-go / no-sweep / no-mop zones. The robot reports status back in cmd 0x1B.
/// Each zone has its own [`ForbiddenMode`].
///
/// Format: `aa <len> 0x1a <num_zones> [<mode> <num_pts=4> <coords...>]* <checksum>`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForbiddenZoneCommand {
    pub zones: Vec<ForbiddenZone>,
}

impl ForbiddenZoneCommand {
    /// Encode to raw bytes (cmd 0x1a).
    pub fn encode(&self) -> Vec<u8> {
        // payload = cmd(1) + num_zones(1) + per-zone(mode(1) + num_pts(1) + 4*4 coords)
        let mut payload_len = 1 + 1;
        for fz in &self.zones {
            payload_len += 1 + 1 + fz.zone.vertices.len() * 4;
        }

        let mut buf = Vec::with_capacity(3 + payload_len + 1);
        buf.push(0xAA);
        buf.push((payload_len >> 8) as u8);
        buf.push(payload_len as u8);
        buf.push(0x1A);
        buf.push(self.zones.len() as u8);
        for fz in &self.zones {
            buf.push(fz.mode as u8);
            buf.push(fz.zone.vertices.len() as u8);
            for &(x, y) in &fz.zone.vertices {
                buf.extend_from_slice(&x.to_be_bytes());
                buf.extend_from_slice(&y.to_be_bytes());
            }
        }

        let checksum: u8 = buf[3..].iter().copied().fold(0u16, |acc, b| acc + b as u16) as u8;
        buf.push(checksum);
        buf
    }

    /// Encode and return as base64 string.
    pub fn encode_base64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(self.encode())
    }

    /// Create a command that clears all forbidden zones.
    pub fn clear() -> Self {
        Self { zones: vec![] }
    }
}

/// A virtual wall command to send via DP 15 (cmd 0x12).
///
/// Sets line barriers that the robot will not cross. The robot reports status
/// back in cmd 0x13.
///
/// Format: `aa <len> 0x12 <num_walls> [<x1> <y1> <x2> <y2>]* <checksum>`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualWallCommand {
    pub walls: Vec<Wall>,
}

impl VirtualWallCommand {
    /// Encode to raw bytes (cmd 0x12).
    pub fn encode(&self) -> Vec<u8> {
        // payload = cmd(1) + num_walls(1) + per-wall(2 points * 2 coords * 2 bytes)
        let payload_len = 1 + 1 + self.walls.len() * 8;

        let mut buf = Vec::with_capacity(3 + payload_len + 1);
        buf.push(0xAA);
        buf.push((payload_len >> 8) as u8);
        buf.push(payload_len as u8);
        buf.push(0x12);
        buf.push(self.walls.len() as u8);
        for wall in &self.walls {
            buf.extend_from_slice(&wall.start.0.to_be_bytes());
            buf.extend_from_slice(&wall.start.1.to_be_bytes());
            buf.extend_from_slice(&wall.end.0.to_be_bytes());
            buf.extend_from_slice(&wall.end.1.to_be_bytes());
        }

        let checksum: u8 = buf[3..].iter().copied().fold(0u16, |acc, b| acc + b as u16) as u8;
        buf.push(checksum);
        buf
    }

    /// Encode and return as base64 string.
    pub fn encode_base64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(self.encode())
    }

    /// Create a command that clears all virtual walls.
    pub fn clear() -> Self {
        Self { walls: vec![] }
    }
}

/// A decoded DP 15 message from the vacuum cleaner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweeperMessage {
    pub cmd: u8,
    pub data: Vec<u8>,
    pub checksum_ok: bool,
}

impl SweeperMessage {
    /// Decode from raw bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() < 4 {
            return Err(ProtocolError::TooShort(bytes.len()));
        }
        if bytes[0] != 0xAA {
            return Err(ProtocolError::InvalidStartByte(bytes[0]));
        }

        let payload_len = ((bytes[1] as usize) << 8) | (bytes[2] as usize);
        // Total = 3 (header) + payload_len + 1 (checksum)
        let expected_total = 3 + payload_len + 1;
        if bytes.len() < expected_total {
            return Err(ProtocolError::LengthMismatch {
                expected: expected_total,
                actual: bytes.len(),
            });
        }

        let cmd = bytes[3];
        let data = bytes[4..3 + payload_len].to_vec();
        let received_checksum = bytes[3 + payload_len];

        let computed: u8 = bytes[3..3 + payload_len]
            .iter()
            .copied()
            .fold(0u16, |acc, b| acc + b as u16) as u8;

        Ok(SweeperMessage {
            cmd,
            data,
            checksum_ok: computed == received_checksum,
        })
    }

    /// Decode from base64 string.
    pub fn decode_base64(s: &str) -> Result<Self, ProtocolError> {
        let bytes = base64::engine::general_purpose::STANDARD.decode(s)?;
        Self::decode(&bytes)
    }
}

/// Parsed room clean status from the vacuum cleaner (cmd 0x15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomCleanStatusResponse {
    pub clean_times: u8,
    pub num_rooms: u8,
    pub room_ids: Vec<u8>,
}

impl TryFrom<&SweeperMessage> for RoomCleanStatusResponse {
    type Error = ProtocolError;

    fn try_from(msg: &SweeperMessage) -> Result<Self, Self::Error> {
        if msg.cmd != 0x15 {
            return Err(ProtocolError::UnexpectedCommand(msg.cmd));
        }
        if msg.data.len() < 2 {
            return Err(ProtocolError::PayloadTooShort);
        }
        let clean_times = msg.data[0];
        let num_rooms = msg.data[1];
        let room_ids = msg.data[2..].to_vec();
        Ok(RoomCleanStatusResponse {
            clean_times,
            num_rooms,
            room_ids,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_single_room() {
        let cmd = RoomCleanCommand {
            clean_times: 1,
            room_ids: vec![4],
        };
        assert_eq!(
            cmd.encode(),
            vec![0xAA, 0x00, 0x04, 0x14, 0x01, 0x01, 0x04, 0x1A]
        );
    }

    #[test]
    fn encode_base64_single_room() {
        let cmd = RoomCleanCommand {
            clean_times: 1,
            room_ids: vec![4],
        };
        assert_eq!(cmd.encode_base64(), "qgAEFAEBBBo=");
    }

    #[test]
    fn encode_multi_room() {
        let cmd = RoomCleanCommand {
            clean_times: 2,
            room_ids: vec![0, 2, 3],
        };
        let encoded = cmd.encode();
        // header: AA 00 06, cmd: 14, clean_times: 02, num_rooms: 03, rooms: 00 02 03
        assert_eq!(encoded[0], 0xAA);
        assert_eq!(encoded[1], 0x00);
        assert_eq!(encoded[2], 0x06); // payload = 1+1+1+3 = 6
        assert_eq!(encoded[3], 0x14);
        assert_eq!(encoded[4], 0x02);
        assert_eq!(encoded[5], 0x03);
        assert_eq!(encoded[6], 0x00);
        assert_eq!(encoded[7], 0x02);
        assert_eq!(encoded[8], 0x03);
        // checksum: (0x14+0x02+0x03+0x00+0x02+0x03) = 0x1E
        assert_eq!(encoded[9], 0x1E);
    }

    #[test]
    fn decode_room_clean_status() {
        let bytes = [0xAA, 0x00, 0x04, 0x15, 0x01, 0x01, 0x04, 0x1B];
        let msg = SweeperMessage::decode(&bytes).unwrap();
        assert_eq!(msg.cmd, 0x15);
        assert_eq!(msg.data, vec![0x01, 0x01, 0x04]);
        assert!(msg.checksum_ok);
    }

    #[test]
    fn decode_base64_room_clean_status() {
        let msg = SweeperMessage::decode_base64("qgAEFQEBBBs=").unwrap();
        assert_eq!(msg.cmd, 0x15);
        assert_eq!(msg.data, vec![0x01, 0x01, 0x04]);
        assert!(msg.checksum_ok);
    }

    #[test]
    fn decode_bad_checksum() {
        let bytes = [0xAA, 0x00, 0x04, 0x15, 0x01, 0x01, 0x04, 0xFF];
        let msg = SweeperMessage::decode(&bytes).unwrap();
        assert!(!msg.checksum_ok);
    }

    #[test]
    fn decode_too_short() {
        assert!(SweeperMessage::decode(&[0xAA, 0x00]).is_err());
    }

    #[test]
    fn decode_bad_start_byte() {
        assert!(SweeperMessage::decode(&[0xBB, 0x00, 0x01, 0x14, 0x14]).is_err());
    }

    #[test]
    fn room_clean_status_try_from() {
        let msg = SweeperMessage {
            cmd: 0x15,
            data: vec![0x01, 0x01, 0x04],
            checksum_ok: true,
        };
        let resp = RoomCleanStatusResponse::try_from(&msg).unwrap();
        assert_eq!(resp.clean_times, 1);
        assert_eq!(resp.num_rooms, 1);
        assert_eq!(resp.room_ids, vec![4]);
    }

    #[test]
    fn room_clean_status_wrong_cmd() {
        let msg = SweeperMessage {
            cmd: 0x14,
            data: vec![1, 1, 4],
            checksum_ok: true,
        };
        assert!(RoomCleanStatusResponse::try_from(&msg).is_err());
    }

    #[test]
    fn multi_room_encode_decode_roundtrip() {
        let cmd = RoomCleanCommand {
            clean_times: 2,
            room_ids: vec![0, 2, 3],
        };
        let encoded = cmd.encode();
        let b64 = cmd.encode_base64();

        let msg = SweeperMessage::decode(&encoded).unwrap();
        assert!(msg.checksum_ok);
        assert_eq!(msg.cmd, 0x14);

        let msg2 = SweeperMessage::decode_base64(&b64).unwrap();
        assert_eq!(msg, msg2);
    }

    #[test]
    fn checksum_is_sum_mod_256() {
        let cmd = RoomCleanCommand {
            clean_times: 1,
            room_ids: vec![4],
        };
        let encoded = cmd.encode();
        let sum: u16 = encoded[3..encoded.len() - 1]
            .iter()
            .map(|&b| b as u16)
            .sum();
        assert_eq!(encoded.last().copied().unwrap(), (sum & 0xFF) as u8);
    }

    // ── Zone clean ──────────────────────────────────────────────

    #[test]
    fn zone_encode_sala_zone() {
        // Zone matching a real sniffed command (coordinates from device traffic):
        // vertices: (82,203), (453,203), (453,-13), (82,-13)
        // cmd 0x28 is the setter; robot reports back via 0x29 status.
        let cmd = ZoneCleanCommand {
            clean_times: 1,
            zones: vec![Zone {
                vertices: [(82, 203), (453, 203), (453, -13), (82, -13)],
            }],
        };
        let encoded = cmd.encode();
        let expected: Vec<u8> = vec![
            0xAA, 0x00, 0x14, // header: payload_len = 20
            0x28, // cmd (setter)
            0x01, // clean_times = 1
            0x01, // num_zones = 1
            0x04, // num_vertices = 4
            0x00, 0x52, // x=82
            0x00, 0xCB, // y=203
            0x01, 0xC5, // x=453
            0x00, 0xCB, // y=203
            0x01, 0xC5, // x=453
            0xFF, 0xF3, // y=-13
            0x00, 0x52, // x=82
            0xFF, 0xF3, // y=-13
            0xD8, // checksum (0xD9 - 1 because cmd 0x28 vs 0x29)
        ];
        assert_eq!(encoded, expected);
    }

    #[test]
    fn zone_rect_helper() {
        // Zone::rect(x1=82, y1=-13, x2=453, y2=203) should produce the same
        // 4 vertices as the app capture (TL, TR, BR, BL)
        let zone = Zone::rect(82, -13, 453, 203);
        assert_eq!(
            zone.vertices,
            [
                (82, 203),  // top-left
                (453, 203), // top-right
                (453, -13), // bottom-right
                (82, -13),  // bottom-left
            ]
        );
    }

    #[test]
    fn zone_rotated_rect_zero_angle() {
        // 0° rotation should equal Zone::rect
        let plain = Zone::rect(100, 200, 300, 400);
        let rotated = Zone::rotated_rect(100, 200, 300, 400, 0.0);
        assert_eq!(plain.vertices, rotated.vertices);
    }

    #[test]
    fn zone_rotated_rect_90_degrees() {
        // 90° CCW around center (200, 300)
        let zone = Zone::rotated_rect(100, 200, 300, 400, 90.0);
        // TL (100,400) → rotated: center + (100, -100) → (300, 200)
        // TR (300,400) → rotated: center + (100, 100) → (100, 200) ... wait
        // Let me just verify it's still a valid rectangle (all sides equal)
        let [v0, v1, v2, v3] = zone.vertices;
        let dist = |a: (i16, i16), b: (i16, i16)| {
            let dx = (b.0 - a.0) as f64;
            let dy = (b.1 - a.1) as f64;
            (dx * dx + dy * dy).sqrt()
        };
        let side_a = dist(v0, v1);
        let side_b = dist(v1, v2);
        // Opposite sides should be equal
        assert!((dist(v0, v1) - dist(v2, v3)).abs() < 1.0);
        assert!((dist(v1, v2) - dist(v3, v0)).abs() < 1.0);
        // Diagonals should be equal (rectangle property)
        assert!((dist(v0, v2) - dist(v1, v3)).abs() < 1.0);
        // Side ratio should match original (200 wide, 200 tall = square)
        assert!((side_a - side_b).abs() < 1.0);
    }

    #[test]
    fn zone_rotated_rect_small_angle() {
        // 2.68° — typical map theta
        let zone = Zone::rotated_rect(82, -13, 453, 203, 2.68);
        let [v0, v1, v2, v3] = zone.vertices;
        // Should still be a valid rectangle
        let dist = |a: (i16, i16), b: (i16, i16)| {
            let dx = (b.0 - a.0) as f64;
            let dy = (b.1 - a.1) as f64;
            (dx * dx + dy * dy).sqrt()
        };
        assert!((dist(v0, v1) - dist(v2, v3)).abs() < 2.0);
        assert!((dist(v1, v2) - dist(v3, v0)).abs() < 2.0);
        // Vertices should differ from axis-aligned
        let plain = Zone::rect(82, -13, 453, 203);
        assert_ne!(zone.vertices, plain.vertices);
        // Center should be the same
        let center_r = (
            zone.vertices.iter().map(|v| v.0 as i32).sum::<i32>(),
            zone.vertices.iter().map(|v| v.1 as i32).sum::<i32>(),
        );
        let center_p = (
            plain.vertices.iter().map(|v| v.0 as i32).sum::<i32>(),
            plain.vertices.iter().map(|v| v.1 as i32).sum::<i32>(),
        );
        assert!((center_r.0 - center_p.0).abs() <= 2);
        assert!((center_r.1 - center_p.1).abs() <= 2);
    }

    #[test]
    fn zone_encode_single() {
        let cmd = ZoneCleanCommand {
            clean_times: 1,
            zones: vec![Zone::rect(82, -13, 453, 203)],
        };
        let encoded = cmd.encode();
        assert_eq!(encoded[0], 0xAA);
        // payload_len = 1 + 1 + 1 + 1 + 4*4 = 20 = 0x0014
        assert_eq!(encoded[1], 0x00);
        assert_eq!(encoded[2], 0x14);
        assert_eq!(encoded[3], 0x28); // cmd (setter)
        assert_eq!(encoded[4], 0x01); // clean_times
        assert_eq!(encoded[5], 0x01); // num_zones
        assert_eq!(encoded[6], 0x04); // num_vertices
        // total = 3 + 20 + 1 = 24 bytes
        assert_eq!(encoded.len(), 24);
        // verify checksum
        let sum: u16 = encoded[3..encoded.len() - 1]
            .iter()
            .map(|&b| b as u16)
            .sum();
        assert_eq!(encoded.last().copied().unwrap(), (sum & 0xFF) as u8);
    }

    #[test]
    fn zone_encode_multi() {
        let cmd = ZoneCleanCommand {
            clean_times: 2,
            zones: vec![
                Zone::rect(100, 200, 300, 400),
                Zone::rect(500, 600, 700, 800),
            ],
        };
        let encoded = cmd.encode();
        // payload_len = 1 + 1 + 1 + 2*(1 + 4*4) = 3 + 2*17 = 37 = 0x0025
        assert_eq!(encoded[1], 0x00);
        assert_eq!(encoded[2], 0x25);
        assert_eq!(encoded[3], 0x28); // cmd (setter)
        assert_eq!(encoded[4], 0x02); // clean_times
        assert_eq!(encoded[5], 0x02); // num_zones
        assert_eq!(encoded[6], 0x04); // num_vertices zone 1
        // total = 3 + 37 + 1 = 41 bytes
        assert_eq!(encoded.len(), 41);
        // verify checksum
        let sum: u16 = encoded[3..encoded.len() - 1]
            .iter()
            .map(|&b| b as u16)
            .sum();
        assert_eq!(encoded.last().copied().unwrap(), (sum & 0xFF) as u8);
    }

    #[test]
    fn zone_encode_negative_coords() {
        let cmd = ZoneCleanCommand {
            clean_times: 1,
            zones: vec![Zone::rect(-100, -200, 100, 200)],
        };
        let encoded = cmd.encode();
        // First vertex is top-left: (-100, 200) → 0xFF9C, 0x00C8
        assert_eq!(encoded[7], 0xFF);
        assert_eq!(encoded[8], 0x9C);
        assert_eq!(encoded[9], 0x00);
        assert_eq!(encoded[10], 0xC8);
    }

    #[test]
    fn zone_encode_decode_roundtrip() {
        let cmd = ZoneCleanCommand {
            clean_times: 1,
            zones: vec![Zone::rect(82, -13, 453, 203)],
        };
        let encoded = cmd.encode();
        let msg = SweeperMessage::decode(&encoded).unwrap();
        assert!(msg.checksum_ok);
        assert_eq!(msg.cmd, 0x28);
        // data: clean_times + num_zones + num_vertices + 4*4 bytes = 19
        assert_eq!(msg.data.len(), 19);
        assert_eq!(msg.data[0], 1); // clean_times
        assert_eq!(msg.data[1], 1); // num_zones
        assert_eq!(msg.data[2], 4); // num_vertices
    }

    #[test]
    fn zone_encode_base64_roundtrip() {
        let cmd = ZoneCleanCommand {
            clean_times: 1,
            zones: vec![Zone::rect(82, -13, 453, 203)],
        };
        let b64 = cmd.encode_base64();
        let msg = SweeperMessage::decode_base64(&b64).unwrap();
        assert!(msg.checksum_ok);
        assert_eq!(msg.cmd, 0x28);
    }

    // ── Forbidden zone (cmd 0x1a) ─────────────────────────────

    #[test]
    fn forbidden_zone_encode_full_ban() {
        // Verified against real robot: mode=0x00 shows as "zona vietata" in app
        let cmd = ForbiddenZoneCommand {
            zones: vec![ForbiddenZone {
                mode: ForbiddenMode::FullBan,
                zone: Zone::rect(82, -13, 453, 203),
            }],
        };
        let encoded = cmd.encode();
        assert_eq!(encoded[0], 0xAA);
        // payload_len = 1 + 1 + (1 + 1 + 4*4) = 20 = 0x0014
        assert_eq!(encoded[1], 0x00);
        assert_eq!(encoded[2], 0x14);
        assert_eq!(encoded[3], 0x1A); // cmd
        assert_eq!(encoded[4], 0x01); // num_zones
        assert_eq!(encoded[5], 0x00); // mode = FullBan
        assert_eq!(encoded[6], 0x04); // num_points
        assert_eq!(encoded.len(), 24);
        let sum: u16 = encoded[3..encoded.len() - 1]
            .iter()
            .map(|&b| b as u16)
            .sum();
        assert_eq!(encoded.last().copied().unwrap(), (sum & 0xFF) as u8);
    }

    #[test]
    fn forbidden_zone_encode_no_sweep() {
        // Verified against real robot: mode=0x01 shows as "zona non lavabile" in app
        let cmd = ForbiddenZoneCommand {
            zones: vec![ForbiddenZone {
                mode: ForbiddenMode::NoSweep,
                zone: Zone::rect(82, -13, 453, 203),
            }],
        };
        let encoded = cmd.encode();
        assert_eq!(encoded[3], 0x1A);
        assert_eq!(encoded[5], 0x01); // mode = NoSweep
        // Matches verified frame: aa 00 14 1a 01 01 04 00 52 00 cb ...
        assert_eq!(encoded[7], 0x00);
        assert_eq!(encoded[8], 0x52); // x=82
    }

    #[test]
    fn forbidden_zone_matches_verified_frame() {
        // Exact frame verified on real robot (mode=0x00, full ban, straight rect)
        // Sent: aa 00 14 1a 01 00 04 00 52 00 cb 01 c5 00 cb 01 c5 ff f3 00 52 ff f3 c9
        let cmd = ForbiddenZoneCommand {
            zones: vec![ForbiddenZone {
                mode: ForbiddenMode::FullBan,
                zone: Zone::rect(82, -13, 453, 203),
            }],
        };
        let encoded = cmd.encode();
        let expected: Vec<u8> = vec![
            0xAA, 0x00, 0x14, 0x1A, 0x01, 0x00, 0x04, 0x00, 0x52, 0x00, 0xCB, 0x01, 0xC5, 0x00,
            0xCB, 0x01, 0xC5, 0xFF, 0xF3, 0x00, 0x52, 0xFF, 0xF3, 0xC9,
        ];
        assert_eq!(encoded, expected);
    }

    #[test]
    fn forbidden_zone_clear() {
        // Clear = aa 00 02 1a 00 1a (verified on real robot)
        let cmd = ForbiddenZoneCommand::clear();
        let encoded = cmd.encode();
        assert_eq!(encoded, vec![0xAA, 0x00, 0x02, 0x1A, 0x00, 0x1A]);
    }

    #[test]
    fn forbidden_zone_encode_decode_roundtrip() {
        let cmd = ForbiddenZoneCommand {
            zones: vec![ForbiddenZone {
                mode: ForbiddenMode::NoSweep,
                zone: Zone::rect(100, 200, 300, 400),
            }],
        };
        let encoded = cmd.encode();
        let msg = SweeperMessage::decode(&encoded).unwrap();
        assert!(msg.checksum_ok);
        assert_eq!(msg.cmd, 0x1A);
        assert_eq!(msg.data[0], 0x01); // num_zones
        assert_eq!(msg.data[1], 0x01); // mode = NoSweep
        assert_eq!(msg.data[2], 0x04); // num_points
    }

    #[test]
    fn forbidden_zone_multi_mode() {
        // Two zones with different modes
        let cmd = ForbiddenZoneCommand {
            zones: vec![
                ForbiddenZone {
                    mode: ForbiddenMode::FullBan,
                    zone: Zone::rect(0, 0, 100, 100),
                },
                ForbiddenZone {
                    mode: ForbiddenMode::NoSweep,
                    zone: Zone::rect(200, 200, 300, 300),
                },
            ],
        };
        let encoded = cmd.encode();
        assert_eq!(encoded[3], 0x1A);
        assert_eq!(encoded[4], 0x02); // num_zones = 2
        assert_eq!(encoded[5], 0x00); // zone 1 mode = FullBan
        assert_eq!(encoded[6], 0x04); // zone 1 num_pts
        // zone 2 starts at 5 + 1 + 1 + 16 = 23
        assert_eq!(encoded[23], 0x01); // zone 2 mode = NoSweep
        assert_eq!(encoded[24], 0x04); // zone 2 num_pts
    }

    // ── Virtual wall (cmd 0x12) ─────────────────────────────

    #[test]
    fn virtual_wall_encode_horizontal() {
        // Verified on real robot: horizontal wall (100,100) -> (400,100)
        let cmd = VirtualWallCommand {
            walls: vec![Wall {
                start: (100, 100),
                end: (400, 100),
            }],
        };
        let encoded = cmd.encode();
        let expected: Vec<u8> = vec![
            0xAA, 0x00, 0x0A, // header: payload = 10
            0x12, // cmd
            0x01, // num_walls
            0x00, 0x64, 0x00, 0x64, // start: (100, 100)
            0x01, 0x90, 0x00, 0x64, // end: (400, 100)
            0xD0, // checksum
        ];
        assert_eq!(encoded, expected);
    }

    #[test]
    fn virtual_wall_encode_diagonal() {
        // Verified on real robot: diagonal wall (100,-100) -> (400,200)
        let cmd = VirtualWallCommand {
            walls: vec![Wall {
                start: (100, -100),
                end: (400, 200),
            }],
        };
        let encoded = cmd.encode();
        assert_eq!(encoded[3], 0x12);
        assert_eq!(encoded[4], 0x01); // num_walls
        // start: (100, -100) = 0x0064, 0xFF9C
        assert_eq!(encoded[5], 0x00);
        assert_eq!(encoded[6], 0x64);
        assert_eq!(encoded[7], 0xFF);
        assert_eq!(encoded[8], 0x9C);
        // end: (400, 200) = 0x0190, 0x00C8
        assert_eq!(encoded[9], 0x01);
        assert_eq!(encoded[10], 0x90);
        assert_eq!(encoded[11], 0x00);
        assert_eq!(encoded[12], 0xC8);
        let sum: u16 = encoded[3..encoded.len() - 1]
            .iter()
            .map(|&b| b as u16)
            .sum();
        assert_eq!(encoded.last().copied().unwrap(), (sum & 0xFF) as u8);
    }

    #[test]
    fn virtual_wall_clear() {
        // Clear = aa 00 02 12 00 12 (verified on real robot)
        let cmd = VirtualWallCommand::clear();
        let encoded = cmd.encode();
        assert_eq!(encoded, vec![0xAA, 0x00, 0x02, 0x12, 0x00, 0x12]);
    }

    #[test]
    fn virtual_wall_encode_decode_roundtrip() {
        let cmd = VirtualWallCommand {
            walls: vec![Wall {
                start: (-50, 100),
                end: (300, -200),
            }],
        };
        let encoded = cmd.encode();
        let msg = SweeperMessage::decode(&encoded).unwrap();
        assert!(msg.checksum_ok);
        assert_eq!(msg.cmd, 0x12);
        assert_eq!(msg.data[0], 0x01); // num_walls
        assert_eq!(msg.data.len(), 9); // 1 + 8 bytes
    }

    #[test]
    fn virtual_wall_multi() {
        let cmd = VirtualWallCommand {
            walls: vec![
                Wall {
                    start: (0, 0),
                    end: (100, 0),
                },
                Wall {
                    start: (0, 0),
                    end: (0, 100),
                },
            ],
        };
        let encoded = cmd.encode();
        // payload = 1 + 1 + 2*8 = 18 = 0x12
        assert_eq!(encoded[2], 0x12);
        assert_eq!(encoded[4], 0x02); // num_walls
        assert_eq!(encoded.len(), 3 + 18 + 1); // 22 bytes
        let sum: u16 = encoded[3..encoded.len() - 1]
            .iter()
            .map(|&b| b as u16)
            .sum();
        assert_eq!(encoded.last().copied().unwrap(), (sum & 0xFF) as u8);
    }
}

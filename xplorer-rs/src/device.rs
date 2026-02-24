use std::collections::HashMap;

use base64::Engine;
use serde_json::json;

use tuya_rs::connection::*;

use crate::protocol::{
    ForbiddenZone, ForbiddenZoneCommand, RoomCleanCommand, RoomCleanStatusResponse, SweeperMessage,
    VirtualWallCommand, Wall, ZoneCleanCommand,
};
use crate::types::*;

const MAX_DRAIN_ATTEMPTS: usize = 5;

/// OEM credentials for the Rowenta X-Plorer Serie 75 S / Serie 95 S app.
///
/// These are static values extracted from the Android APK (`com.groupeseb.ext.xplorer`).
/// `app_device_id` must be provided — any 44-character lowercase hex string works
/// (e.g. generated with `openssl rand -hex 22`).
#[cfg(feature = "cloud")]
pub fn xplorer_oem_credentials(app_device_id: impl Into<String>) -> tuya_rs::api::OemCredentials {
    tuya_rs::api::OemCredentials {
        client_id: "staxmyjjd8thqxypvr5v".into(),
        app_secret: "q39ksm4c5yps9atn9repakn4gxpja3vh".into(),
        bmp_key: "4rkkvamwnhedxecyexd9t5cxkchxtqff".into(),
        cert_hash: "1B:D3:2E:D5:5E:D7:47:E3:81:A1:AF:EC:66:FA:AC:7B:E4:C8:A6:B2:DD:1F:1A:17:48:5E:1E:D1:1E:37:DB:92".into(),
        package_name: "com.groupeseb.ext.xplorer".into(),
        app_device_id: app_device_id.into(),
    }
}

/// Vacuum cleaner control via Tuya v3.3 TCP protocol.
pub trait Device {
    /// Query the full device state by triggering DPS updates.
    fn status(&mut self) -> Result<DeviceState, DeviceError>;
    /// Turn the vacuum on (DP 1 = true).
    fn power_on(&mut self) -> Result<(), DeviceError>;
    /// Turn the vacuum off (DP 1 = false).
    fn power_off(&mut self) -> Result<(), DeviceError>;
    /// Pause the current operation (DP 2 = false).
    fn pause(&mut self) -> Result<(), DeviceError>;
    /// Resume the current operation (DP 2 = true).
    fn resume(&mut self) -> Result<(), DeviceError>;
    /// Send the vacuum back to the charging dock.
    fn charge_go(&mut self) -> Result<(), DeviceError>;
    /// Make the vacuum emit a sound to help locate it.
    fn locate(&mut self) -> Result<(), DeviceError>;
    /// Set the cleaning mode (smart, wall_follow, spiral, etc.).
    fn set_mode(&mut self, mode: Mode) -> Result<(), DeviceError>;
    /// Start room-based cleaning for the specified rooms.
    fn clean_rooms(
        &mut self,
        cmd: &RoomCleanCommand,
    ) -> Result<Option<RoomCleanStatusResponse>, DeviceError>;
    /// Start zone-based cleaning for the specified rectangular zones.
    fn clean_zone(&mut self, cmd: &ZoneCleanCommand) -> Result<(), DeviceError>;
    /// Set forbidden zones (no-go, no-sweep, no-mop areas).
    fn set_forbidden_zones(&mut self, zones: &[ForbiddenZone]) -> Result<(), DeviceError>;
    /// Clear all forbidden zones.
    fn clear_forbidden_zones(&mut self) -> Result<(), DeviceError>;
    /// Set virtual wall barriers.
    fn set_virtual_walls(&mut self, walls: &[Wall]) -> Result<(), DeviceError>;
    /// Clear all virtual walls.
    fn clear_virtual_walls(&mut self) -> Result<(), DeviceError>;
    /// Query the current room cleaning status.
    fn query_room_status(&mut self) -> Result<Option<RoomCleanStatusResponse>, DeviceError>;
    /// Set the suction power level.
    fn set_suction(&mut self, level: SuctionLevel) -> Result<(), DeviceError>;
    /// Set the mopping water level.
    fn set_mop(&mut self, level: MopLevel) -> Result<(), DeviceError>;
    /// Set the speaker volume (0-100).
    fn set_volume(&mut self, volume: u8) -> Result<(), DeviceError>;
    /// Enable or disable Do Not Disturb mode.
    fn set_dnd(&mut self, enabled: bool) -> Result<(), DeviceError>;
    /// Set a raw DP value and return any follow-up status update.
    fn set_value(&mut self, dp: u8, value: DpValue) -> Result<Option<DpsUpdate>, DeviceError>;
    /// Send a raw DP 15 sweeper command and return the response.
    fn send_raw_command(
        &mut self,
        cmd: u8,
        payload: &[u8],
    ) -> Result<Option<SweeperMessage>, DeviceError>;
}

/// Real-time listener for DPS changes.
pub trait DeviceListener {
    /// Start listening for DPS change events, calling the callback for each batch.
    fn listen<F>(&mut self, on_event: F) -> Result<(), DeviceError>
    where
        F: FnMut(Vec<DpsEvent>);
    /// Stop listening for events.
    fn stop(&mut self);
}

/// X-Plorer Serie 75 S / Serie 95 S vacuum cleaner.
///
/// Wraps a [`TuyaConnection`] and implements the [`Device`] trait,
/// mapping high-level commands to Tuya DPS values.
pub struct XPlorer {
    conn: TuyaConnection,
}

impl XPlorer {
    /// Connect to an X-Plorer vacuum at the given device config.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use xplorer_rs::XPlorer;
    /// use xplorer_rs::device::Device;
    /// use xplorer_rs::protocol::RoomCleanCommand;
    /// use tuya_rs::connection::DeviceConfig;
    ///
    /// let config = DeviceConfig {
    ///     dev_id: "my_device_id".into(),
    ///     address: "192.168.1.100".into(),
    ///     local_key: "0123456789abcdef".into(),
    ///     ..Default::default()
    /// };
    /// let mut robot = XPlorer::connect(&config).unwrap();
    /// let state = robot.status().unwrap();
    /// println!("battery: {}%", state.battery);
    ///
    /// let cmd = RoomCleanCommand { clean_times: 1, room_ids: vec![0, 2] };
    /// robot.clean_rooms(&cmd).unwrap();
    /// ```
    pub fn connect(config: &DeviceConfig) -> Result<Self, DeviceError> {
        Ok(Self {
            conn: TuyaConnection::connect(config)?,
        })
    }

    /// Wrap an existing Tuya TCP connection.
    pub fn new(conn: TuyaConnection) -> Self {
        Self { conn }
    }

    /// Return the device ID.
    pub fn dev_id(&self) -> &str {
        self.conn.dev_id()
    }

    /// Send Control command setting a single DP.
    fn set_dp(&mut self, dp: u8, value: serde_json::Value) -> Result<TuyaPacket, DeviceError> {
        let json = build_dps_json(self.conn.dev_id(), now(), &[(&dp.to_string(), value)]);
        self.conn.send(TuyaCommand::Control, json.into_bytes())
    }

    /// Drain follow-up STATUS pushes, collecting DPS data.
    fn drain_status(&mut self) -> HashMap<String, serde_json::Value> {
        let mut all = HashMap::new();
        for _ in 0..MAX_DRAIN_ATTEMPTS {
            match self.conn.recv() {
                Ok(pkt) => Self::collect_dps(&pkt.payload, &mut all),
                Err(DeviceError::Timeout) => break,
                Err(_) => break,
            }
        }
        all
    }

    /// Parse JSON payload, merge "dps" entries into map.
    fn collect_dps(payload: &[u8], map: &mut HashMap<String, serde_json::Value>) {
        let s = String::from_utf8_lossy(payload);
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&s)
            && let Some(dps) = val.get("dps").and_then(|v| v.as_object())
        {
            for (k, v) in dps {
                map.insert(k.clone(), v.clone());
            }
        }
    }

    /// Extract DP 15 sweeper response from packet payload.
    fn extract_sweeper(payload: &[u8]) -> Option<SweeperMessage> {
        let s = String::from_utf8_lossy(payload);
        let val: serde_json::Value = serde_json::from_str(&s).ok()?;
        let dp15 = val.get("dps")?.get("15")?.as_str()?;
        SweeperMessage::decode_base64(dp15).ok()
    }

    /// Drain follow-up pushes looking for a sweeper response on DP 15.
    fn drain_sweeper_response(&mut self) -> Option<RoomCleanStatusResponse> {
        for _ in 0..MAX_DRAIN_ATTEMPTS {
            match self.conn.recv() {
                Ok(pkt) => {
                    if let Some(msg) = Self::extract_sweeper(&pkt.payload)
                        && msg.cmd == 0x15
                    {
                        return RoomCleanStatusResponse::try_from(&msg).ok();
                    }
                }
                Err(DeviceError::Timeout) => break,
                Err(_) => break,
            }
        }
        None
    }
}

impl Device for XPlorer {
    fn status(&mut self) -> Result<DeviceState, DeviceError> {
        // DpQuery returns "parse data error" on this device — expected.
        // We send it to trigger STATUS pushes.
        let query = serde_json::json!({
            "gwId": self.conn.dev_id(),
            "devId": self.conn.dev_id(),
            "uid": self.conn.dev_id(),
            "t": now().to_string(),
        })
        .to_string();

        let mut all_dps: HashMap<String, serde_json::Value> = HashMap::new();

        match self.conn.send(TuyaCommand::DpQuery, query.into_bytes()) {
            Ok(pkt) => Self::collect_dps(&pkt.payload, &mut all_dps),
            Err(DeviceError::Timeout) => {}
            Err(e) => return Err(e),
        }

        // Read follow-up STATUS pushes
        let more = self.drain_status();
        all_dps.extend(more);

        DeviceState::from_dps(&all_dps)
            .map_err(|e| DeviceError::InvalidResponse(format!("DPS parse error: {e}")))
    }

    fn power_on(&mut self) -> Result<(), DeviceError> {
        self.set_dp(1, json!(true))?;
        Ok(())
    }

    fn power_off(&mut self) -> Result<(), DeviceError> {
        self.set_dp(1, json!(false))?;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), DeviceError> {
        self.set_dp(2, json!(false))?;
        Ok(())
    }

    fn resume(&mut self) -> Result<(), DeviceError> {
        self.set_dp(2, json!(true))?;
        Ok(())
    }

    fn charge_go(&mut self) -> Result<(), DeviceError> {
        self.set_dp(4, json!("chargego"))?;
        Ok(())
    }

    fn locate(&mut self) -> Result<(), DeviceError> {
        self.set_dp(13, json!(true))?;
        Ok(())
    }

    fn set_mode(&mut self, mode: Mode) -> Result<(), DeviceError> {
        self.set_dp(4, json!(mode.as_str()))?;
        Ok(())
    }

    fn clean_rooms(
        &mut self,
        cmd: &RoomCleanCommand,
    ) -> Result<Option<RoomCleanStatusResponse>, DeviceError> {
        let b64 = cmd.encode_base64();
        self.set_dp(15, json!(b64))?;
        Ok(self.drain_sweeper_response())
    }

    fn clean_zone(&mut self, cmd: &ZoneCleanCommand) -> Result<(), DeviceError> {
        let b64 = cmd.encode_base64();
        self.set_dp(15, json!(b64))?;
        self.drain_status();
        Ok(())
    }

    fn set_forbidden_zones(&mut self, zones: &[ForbiddenZone]) -> Result<(), DeviceError> {
        let cmd = ForbiddenZoneCommand {
            zones: zones.to_vec(),
        };
        let b64 = cmd.encode_base64();
        self.set_dp(15, json!(b64))?;
        self.drain_status();
        Ok(())
    }

    fn clear_forbidden_zones(&mut self) -> Result<(), DeviceError> {
        self.set_forbidden_zones(&[])
    }

    fn set_virtual_walls(&mut self, walls: &[Wall]) -> Result<(), DeviceError> {
        let cmd = VirtualWallCommand {
            walls: walls.to_vec(),
        };
        let b64 = cmd.encode_base64();
        self.set_dp(15, json!(b64))?;
        self.drain_status();
        Ok(())
    }

    fn clear_virtual_walls(&mut self) -> Result<(), DeviceError> {
        self.set_virtual_walls(&[])
    }

    fn query_room_status(&mut self) -> Result<Option<RoomCleanStatusResponse>, DeviceError> {
        // Query frame: aa 00 01 15 15
        let frame = vec![0xAA, 0x00, 0x01, 0x15, 0x15];
        let b64 = base64::engine::general_purpose::STANDARD.encode(&frame);
        self.set_dp(15, json!(b64))?;
        Ok(self.drain_sweeper_response())
    }

    fn set_suction(&mut self, level: SuctionLevel) -> Result<(), DeviceError> {
        self.set_dp(9, json!(level.as_str()))?;
        Ok(())
    }

    fn set_mop(&mut self, level: MopLevel) -> Result<(), DeviceError> {
        self.set_dp(10, json!(level.as_str()))?;
        Ok(())
    }

    fn set_volume(&mut self, volume: u8) -> Result<(), DeviceError> {
        self.set_dp(26, json!(volume))?;
        Ok(())
    }

    fn set_dnd(&mut self, enabled: bool) -> Result<(), DeviceError> {
        self.set_dp(25, json!(enabled))?;
        Ok(())
    }

    fn set_value(&mut self, dp: u8, value: DpValue) -> Result<Option<DpsUpdate>, DeviceError> {
        let json_val = match &value {
            DpValue::Boolean(b) => json!(*b),
            DpValue::Integer(n) => json!(*n),
            DpValue::String(s) => json!(s),
            DpValue::Raw(bytes) => {
                json!(base64::engine::general_purpose::STANDARD.encode(bytes))
            }
        };

        self.set_dp(dp, json_val)?;

        // Try to read follow-up status
        let dps = self.drain_status();
        if dps.is_empty() {
            return Ok(None);
        }

        let entries: Vec<(u8, String)> = dps
            .iter()
            .filter_map(|(k, v)| Some((k.parse::<u8>().ok()?, v.to_string())))
            .collect();

        Ok(Some(DpsUpdate {
            dps: entries,
            timestamp: None,
        }))
    }

    fn send_raw_command(
        &mut self,
        cmd: u8,
        data: &[u8],
    ) -> Result<Option<SweeperMessage>, DeviceError> {
        // Build sweeper frame: AA + len(2 BE) + cmd + data + checksum
        let payload_len = 1 + data.len();
        let mut frame = Vec::with_capacity(3 + payload_len + 1);
        frame.push(0xAA);
        frame.push((payload_len >> 8) as u8);
        frame.push(payload_len as u8);
        frame.push(cmd);
        frame.extend_from_slice(data);
        let checksum: u8 = frame[3..]
            .iter()
            .copied()
            .fold(0u16, |acc, b| acc + b as u16) as u8;
        frame.push(checksum);

        let b64 = base64::engine::general_purpose::STANDARD.encode(&frame);
        self.set_dp(15, json!(b64))?;

        // Try to read a sweeper response
        for _ in 0..MAX_DRAIN_ATTEMPTS {
            match self.conn.recv() {
                Ok(pkt) => {
                    if let Some(msg) = Self::extract_sweeper(&pkt.payload) {
                        return Ok(Some(msg));
                    }
                }
                Err(DeviceError::Timeout) => break,
                Err(_) => break,
            }
        }
        Ok(None)
    }
}

/// Parse a DPS JSON response into DpsEvent list.
///
/// # Examples
///
/// ```
/// use xplorer_rs::device::parse_dps_response;
/// use xplorer_rs::types::{DpsEvent, Mode};
///
/// let json = r#"{"dps":{"1":true,"4":"smart","8":72}}"#;
/// let events = parse_dps_response(json).unwrap();
/// assert!(events.contains(&DpsEvent::Power(true)));
/// assert!(events.contains(&DpsEvent::Mode(Mode::Smart)));
/// assert!(events.contains(&DpsEvent::Battery(72)));
/// ```
pub fn parse_dps_response(json: &str) -> Result<Vec<DpsEvent>, DeviceError> {
    let val: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| DeviceError::InvalidResponse(format!("invalid JSON: {e}")))?;

    let dps = val
        .get("dps")
        .and_then(|v| v.as_object())
        .ok_or_else(|| DeviceError::InvalidResponse("no dps object".into()))?;

    let mut events = Vec::new();
    for (k, v) in dps {
        let dp: u8 = k
            .parse()
            .map_err(|_| DeviceError::InvalidResponse(format!("invalid DP number: {k}")))?;
        match DpsEvent::parse(dp, v) {
            Ok(event) => events.push(event),
            Err(_) => {
                events.push(DpsEvent::Unknown {
                    dp,
                    value: v.to_string(),
                });
            }
        }
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── collect_dps ───────────────────────────────────────────

    #[test]
    fn collect_dps_basic() {
        let payload = br#"{"dps":{"1":true,"8":72}}"#;
        let mut map = HashMap::new();
        XPlorer::collect_dps(payload, &mut map);
        assert_eq!(map.len(), 2);
        assert_eq!(map["1"], json!(true));
        assert_eq!(map["8"], json!(72));
    }

    #[test]
    fn collect_dps_empty() {
        let payload = b"{}";
        let mut map = HashMap::new();
        XPlorer::collect_dps(payload, &mut map);
        assert!(map.is_empty());
    }

    #[test]
    fn collect_dps_merge() {
        let mut map = HashMap::new();
        map.insert("1".to_string(), json!(false));
        let payload = br#"{"dps":{"1":true,"4":"smart"}}"#;
        XPlorer::collect_dps(payload, &mut map);
        assert_eq!(map["1"], json!(true)); // overwritten
        assert_eq!(map["4"], json!("smart"));
    }

    // ── extract_sweeper ──────────────────────────────────────

    #[test]
    fn extract_sweeper_valid() {
        // base64 of [0xAA, 0x00, 0x04, 0x15, 0x01, 0x01, 0x04, 0x1B]
        let payload = br#"{"dps":{"15":"qgAEFQEBBBs="}}"#;
        let msg = XPlorer::extract_sweeper(payload).unwrap();
        assert_eq!(msg.cmd, 0x15);
        assert_eq!(msg.data, vec![0x01, 0x01, 0x04]);
    }

    #[test]
    fn extract_sweeper_no_dp15() {
        let payload = br#"{"dps":{"1":true}}"#;
        assert!(XPlorer::extract_sweeper(payload).is_none());
    }

    // ── parse_dps_response ───────────────────────────────────

    #[test]
    fn parse_dps_response_basic() {
        let json = r#"{"dps":{"1":true,"4":"smart","8":72}}"#;
        let events = parse_dps_response(json).unwrap();
        assert_eq!(events.len(), 3);
        assert!(events.contains(&DpsEvent::Power(true)));
        assert!(events.contains(&DpsEvent::Mode(Mode::Smart)));
        assert!(events.contains(&DpsEvent::Battery(72)));
    }

    #[test]
    fn parse_dps_response_unknown_dp() {
        let json = r#"{"dps":{"200":"mystery"}}"#;
        let events = parse_dps_response(json).unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], DpsEvent::Unknown { dp: 200, .. }));
    }

    #[test]
    fn parse_dps_response_invalid_json() {
        assert!(parse_dps_response("not json").is_err());
    }
}

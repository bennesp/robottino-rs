use std::collections::HashMap;

use base64::Engine;
use serde_json::json;

use tuya_rs::connection::{
    DeviceConfig, DeviceError, DpValue, DpsUpdate, Transport, TuyaCommand, TuyaConnection,
    TuyaPacket, build_dps_json, now,
};

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

// ── Helpers (transport-independent) ─────────────────────────

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

// ── XPlorer ─────────────────────────────────────────────────

/// X-Plorer Serie 75 S / Serie 95 S vacuum cleaner.
///
/// Generic over the transport layer: uses [`TuyaConnection`] by default
/// for real TCP communication, but accepts any [`Transport`] implementation
/// (useful for testing with mocks).
pub struct XPlorer<T: Transport = TuyaConnection> {
    conn: T,
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
}

impl<T: Transport> XPlorer<T> {
    /// Create an XPlorer from any [`Transport`] implementation.
    pub fn new(transport: T) -> Self {
        Self { conn: transport }
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
                Ok(pkt) => collect_dps(&pkt.payload, &mut all),
                Err(DeviceError::Timeout) => break,
                Err(_) => break,
            }
        }
        all
    }

    /// Drain follow-up pushes looking for a sweeper response on DP 15.
    fn drain_sweeper_response(&mut self) -> Option<RoomCleanStatusResponse> {
        for _ in 0..MAX_DRAIN_ATTEMPTS {
            match self.conn.recv() {
                Ok(pkt) => {
                    if let Some(msg) = extract_sweeper(&pkt.payload)
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

impl<T: Transport> Device for XPlorer<T> {
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
            Ok(pkt) => collect_dps(&pkt.payload, &mut all_dps),
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
                    if let Some(msg) = extract_sweeper(&pkt.payload) {
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
    use std::collections::VecDeque;

    use super::*;

    // ── MockTransport ───────────────────────────────────────

    struct MockTransport {
        dev_id: String,
        responses: VecDeque<Result<TuyaPacket, DeviceError>>,
        sent: Vec<(TuyaCommand, Vec<u8>)>,
    }

    impl MockTransport {
        fn new(responses: Vec<Result<TuyaPacket, DeviceError>>) -> Self {
            Self {
                dev_id: "mock_device".into(),
                responses: VecDeque::from(responses),
                sent: Vec::new(),
            }
        }
    }

    impl Transport for MockTransport {
        fn dev_id(&self) -> &str {
            &self.dev_id
        }

        fn send(
            &mut self,
            command: TuyaCommand,
            payload: Vec<u8>,
        ) -> Result<TuyaPacket, DeviceError> {
            self.sent.push((command, payload));
            self.responses
                .pop_front()
                .unwrap_or(Err(DeviceError::Timeout))
        }

        fn recv(&mut self) -> Result<TuyaPacket, DeviceError> {
            self.responses
                .pop_front()
                .unwrap_or(Err(DeviceError::Timeout))
        }
    }

    // ── Test helpers ────────────────────────────────────────

    fn ok_packet(payload: &[u8]) -> Result<TuyaPacket, DeviceError> {
        Ok(TuyaPacket {
            seq_num: 1,
            command: TuyaCommand::Status as u32,
            payload: payload.to_vec(),
        })
    }

    fn mock_robot(responses: Vec<Result<TuyaPacket, DeviceError>>) -> XPlorer<MockTransport> {
        XPlorer::new(MockTransport::new(responses))
    }

    /// Extract the DPS JSON sent in the last Control command.
    fn sent_dps(robot: &XPlorer<MockTransport>) -> serde_json::Value {
        let (cmd, payload) = robot.conn.sent.last().expect("no commands sent");
        assert_eq!(*cmd, TuyaCommand::Control);
        serde_json::from_slice(payload).expect("sent payload is not valid JSON")
    }

    // ── collect_dps ─────────────────────────────────────────

    #[test]
    fn collect_dps_basic() {
        let mut map = HashMap::new();
        collect_dps(br#"{"dps":{"1":true,"8":72}}"#, &mut map);
        assert_eq!(map.len(), 2);
        assert_eq!(map["1"], json!(true));
        assert_eq!(map["8"], json!(72));
    }

    #[test]
    fn collect_dps_empty() {
        let mut map = HashMap::new();
        collect_dps(b"{}", &mut map);
        assert!(map.is_empty());
    }

    #[test]
    fn collect_dps_merge() {
        let mut map = HashMap::new();
        map.insert("1".to_string(), json!(false));
        collect_dps(br#"{"dps":{"1":true,"4":"smart"}}"#, &mut map);
        assert_eq!(map["1"], json!(true));
        assert_eq!(map["4"], json!("smart"));
    }

    #[test]
    fn collect_dps_invalid_json() {
        let mut map = HashMap::new();
        collect_dps(b"not json", &mut map);
        assert!(map.is_empty());
    }

    // ── extract_sweeper ─────────────────────────────────────

    #[test]
    fn extract_sweeper_valid() {
        let msg = extract_sweeper(br#"{"dps":{"15":"qgAEFQEBBBs="}}"#).unwrap();
        assert_eq!(msg.cmd, 0x15);
        assert_eq!(msg.data, vec![0x01, 0x01, 0x04]);
    }

    #[test]
    fn extract_sweeper_no_dp15() {
        assert!(extract_sweeper(br#"{"dps":{"1":true}}"#).is_none());
    }

    #[test]
    fn extract_sweeper_invalid_json() {
        assert!(extract_sweeper(b"not json").is_none());
    }

    #[test]
    fn extract_sweeper_dp15_not_string() {
        assert!(extract_sweeper(br#"{"dps":{"15": 42}}"#).is_none());
    }

    // ── parse_dps_response ──────────────────────────────────

    #[test]
    fn parse_dps_response_basic() {
        let events = parse_dps_response(r#"{"dps":{"1":true,"4":"smart","8":72}}"#).unwrap();
        assert_eq!(events.len(), 3);
        assert!(events.contains(&DpsEvent::Power(true)));
        assert!(events.contains(&DpsEvent::Mode(Mode::Smart)));
        assert!(events.contains(&DpsEvent::Battery(72)));
    }

    #[test]
    fn parse_dps_response_unknown_dp() {
        let events = parse_dps_response(r#"{"dps":{"200":"mystery"}}"#).unwrap();
        assert!(matches!(&events[0], DpsEvent::Unknown { dp: 200, .. }));
    }

    #[test]
    fn parse_dps_response_invalid_json() {
        assert!(parse_dps_response("not json").is_err());
    }

    #[test]
    fn parse_dps_response_no_dps_key() {
        assert!(parse_dps_response(r#"{"other": 42}"#).is_err());
    }

    #[test]
    fn parse_dps_response_invalid_dp_number() {
        assert!(parse_dps_response(r#"{"dps":{"abc": true}}"#).is_err());
    }

    #[test]
    fn parse_dps_response_type_mismatch_falls_back_to_unknown() {
        let events = parse_dps_response(r#"{"dps":{"4": 123}}"#).unwrap();
        assert!(matches!(&events[0], DpsEvent::Unknown { dp: 4, .. }));
    }

    // ── OEM credentials ─────────────────────────────────────

    #[cfg(feature = "cloud")]
    #[test]
    fn xplorer_oem_credentials_values() {
        let creds = super::xplorer_oem_credentials("aabbccdd".repeat(5) + "aabb");
        assert_eq!(creds.client_id, "staxmyjjd8thqxypvr5v");
        assert_eq!(creds.package_name, "com.groupeseb.ext.xplorer");
        assert_eq!(creds.app_device_id.len(), 44);
    }

    // ── Device trait (via MockTransport) ────────────────────

    #[test]
    fn dev_id_returns_mock_id() {
        assert_eq!(mock_robot(vec![]).dev_id(), "mock_device");
    }

    #[test]
    fn power_on_sends_dp1_true() {
        let mut robot = mock_robot(vec![ok_packet(b"{}")]);
        robot.power_on().unwrap();
        assert_eq!(robot.conn.sent.len(), 1);
        assert_eq!(robot.conn.sent[0].0, TuyaCommand::Control);
        assert_eq!(sent_dps(&robot)["dps"]["1"], true);
    }

    #[test]
    fn power_off_sends_dp1_false() {
        let mut robot = mock_robot(vec![ok_packet(b"{}")]);
        robot.power_off().unwrap();
        assert_eq!(sent_dps(&robot)["dps"]["1"], false);
    }

    #[test]
    fn pause_sends_dp2_false() {
        let mut robot = mock_robot(vec![ok_packet(b"{}")]);
        robot.pause().unwrap();
        assert_eq!(sent_dps(&robot)["dps"]["2"], false);
    }

    #[test]
    fn resume_sends_dp2_true() {
        let mut robot = mock_robot(vec![ok_packet(b"{}")]);
        robot.resume().unwrap();
        assert_eq!(sent_dps(&robot)["dps"]["2"], true);
    }

    #[test]
    fn charge_go_sends_chargego() {
        let mut robot = mock_robot(vec![ok_packet(b"{}")]);
        robot.charge_go().unwrap();
        assert_eq!(sent_dps(&robot)["dps"]["4"], "chargego");
    }

    #[test]
    fn locate_sends_dp13_true() {
        let mut robot = mock_robot(vec![ok_packet(b"{}")]);
        robot.locate().unwrap();
        assert_eq!(sent_dps(&robot)["dps"]["13"], true);
    }

    #[test]
    fn set_mode_sends_mode_string() {
        let mut robot = mock_robot(vec![ok_packet(b"{}")]);
        robot.set_mode(Mode::WallFollow).unwrap();
        assert_eq!(sent_dps(&robot)["dps"]["4"], "wall_follow");
    }

    #[test]
    fn set_suction_sends_dp9() {
        let mut robot = mock_robot(vec![ok_packet(b"{}")]);
        robot.set_suction(SuctionLevel::Strong).unwrap();
        assert_eq!(sent_dps(&robot)["dps"]["9"], "strong");
    }

    #[test]
    fn set_mop_sends_dp10() {
        let mut robot = mock_robot(vec![ok_packet(b"{}")]);
        robot.set_mop(MopLevel::High).unwrap();
        assert_eq!(sent_dps(&robot)["dps"]["10"], "high");
    }

    #[test]
    fn set_volume_sends_dp26() {
        let mut robot = mock_robot(vec![ok_packet(b"{}")]);
        robot.set_volume(75).unwrap();
        assert_eq!(sent_dps(&robot)["dps"]["26"], 75);
    }

    #[test]
    fn set_dnd_sends_dp25() {
        let mut robot = mock_robot(vec![ok_packet(b"{}")]);
        robot.set_dnd(true).unwrap();
        assert_eq!(sent_dps(&robot)["dps"]["25"], true);
    }

    #[test]
    fn status_collects_dps_from_multiple_pushes() {
        let mut robot = mock_robot(vec![
            ok_packet(br#"{"dps":{"1":true,"8":72}}"#),
            ok_packet(br#"{"dps":{"4":"smart","5":"cleaning","9":"normal","10":"closed"}}"#),
            Err(DeviceError::Timeout),
        ]);
        let state = robot.status().unwrap();
        assert!(state.power);
        assert_eq!(state.battery, 72);
        assert_eq!(state.mode, Mode::Smart);
    }

    #[test]
    fn status_handles_timeout_on_query() {
        let mut robot = mock_robot(vec![
            Err(DeviceError::Timeout),
            ok_packet(br#"{"dps":{"1":false,"4":"smart","5":"cleaning","8":50,"9":"normal","10":"closed"}}"#),
            Err(DeviceError::Timeout),
        ]);
        let state = robot.status().unwrap();
        assert!(!state.power);
        assert_eq!(state.battery, 50);
    }

    #[test]
    fn clean_rooms_sends_dp15_and_reads_response() {
        let cmd = RoomCleanCommand {
            clean_times: 1,
            room_ids: vec![0, 2],
        };
        let mut robot = mock_robot(vec![
            ok_packet(b"{}"),
            ok_packet(br#"{"dps":{"15":"qgAEFQEBBBs="}}"#),
            Err(DeviceError::Timeout),
        ]);
        let resp = robot.clean_rooms(&cmd).unwrap();
        assert!(resp.is_some());
        assert_eq!(resp.unwrap().clean_times, 1);
    }

    #[test]
    fn clean_zone_sends_dp15() {
        let cmd = ZoneCleanCommand {
            clean_times: 1,
            zones: vec![crate::protocol::Zone::rect(10, 20, 100, 200)],
        };
        let mut robot = mock_robot(vec![ok_packet(b"{}"), Err(DeviceError::Timeout)]);
        robot.clean_zone(&cmd).unwrap();
        assert!(sent_dps(&robot)["dps"]["15"].is_string());
    }

    #[test]
    fn set_forbidden_zones_sends_dp15() {
        let zone = ForbiddenZone {
            mode: crate::protocol::ForbiddenMode::FullBan,
            zone: crate::protocol::Zone::rect(0, 0, 100, 100),
        };
        let mut robot = mock_robot(vec![ok_packet(b"{}"), Err(DeviceError::Timeout)]);
        robot.set_forbidden_zones(&[zone]).unwrap();
        assert!(sent_dps(&robot)["dps"]["15"].is_string());
    }

    #[test]
    fn clear_forbidden_zones_sends_empty() {
        let mut robot = mock_robot(vec![ok_packet(b"{}"), Err(DeviceError::Timeout)]);
        robot.clear_forbidden_zones().unwrap();
        assert!(sent_dps(&robot)["dps"]["15"].is_string());
    }

    #[test]
    fn set_virtual_walls_sends_dp15() {
        let wall = Wall {
            start: (0, 0),
            end: (100, 100),
        };
        let mut robot = mock_robot(vec![ok_packet(b"{}"), Err(DeviceError::Timeout)]);
        robot.set_virtual_walls(&[wall]).unwrap();
        assert!(sent_dps(&robot)["dps"]["15"].is_string());
    }

    #[test]
    fn clear_virtual_walls_sends_empty() {
        let mut robot = mock_robot(vec![ok_packet(b"{}"), Err(DeviceError::Timeout)]);
        robot.clear_virtual_walls().unwrap();
        assert_eq!(robot.conn.sent.len(), 1);
    }

    #[test]
    fn set_value_boolean_returns_update() {
        let mut robot = mock_robot(vec![
            ok_packet(b"{}"),
            ok_packet(br#"{"dps":{"1":true}}"#),
            Err(DeviceError::Timeout),
        ]);
        let update = robot.set_value(1, DpValue::Boolean(true)).unwrap().unwrap();
        assert_eq!(update.dps.len(), 1);
        assert_eq!(update.dps[0].0, 1);
    }

    #[test]
    fn set_value_no_followup_returns_none() {
        let mut robot = mock_robot(vec![ok_packet(b"{}"), Err(DeviceError::Timeout)]);
        assert!(
            robot
                .set_value(1, DpValue::Boolean(true))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn set_value_raw_sends_base64() {
        let mut robot = mock_robot(vec![ok_packet(b"{}"), Err(DeviceError::Timeout)]);
        robot
            .set_value(15, DpValue::Raw(vec![0xAA, 0x00, 0x01, 0x15, 0x15]))
            .unwrap();
        let dp15 = sent_dps(&robot)["dps"]["15"].as_str().unwrap().to_owned();
        assert!(
            base64::engine::general_purpose::STANDARD
                .decode(&dp15)
                .is_ok()
        );
    }

    #[test]
    fn send_raw_command_returns_sweeper_response() {
        let mut robot = mock_robot(vec![
            ok_packet(b"{}"),
            ok_packet(br#"{"dps":{"15":"qgAEFQEBBBs="}}"#),
            Err(DeviceError::Timeout),
        ]);
        let msg = robot.send_raw_command(0x15, &[]).unwrap().unwrap();
        assert_eq!(msg.cmd, 0x15);
    }

    #[test]
    fn send_raw_command_no_response() {
        let mut robot = mock_robot(vec![ok_packet(b"{}"), Err(DeviceError::Timeout)]);
        assert!(robot.send_raw_command(0x15, &[]).unwrap().is_none());
    }

    #[test]
    fn query_room_status_sends_query_frame() {
        let mut robot = mock_robot(vec![ok_packet(b"{}"), Err(DeviceError::Timeout)]);
        assert!(robot.query_room_status().unwrap().is_none());
        assert!(sent_dps(&robot)["dps"]["15"].is_string());
    }
}

//! Cloud-based vacuum control via Tuya OEM API.
//!
//! [`CloudXPlorer`] implements the same [`Device`] trait as the local TCP
//! [`LocalXPlorer`](crate::LocalXPlorer), but sends commands through the Tuya cloud
//! instead of a direct LAN connection.

use base64::Engine;
use serde_json::json;
use tuya_rs::api::{
    ApiError, CloudDeviceInfo, DeviceInfo, Home, HttpClient, ReqwestClient, StorageCredentials,
    TuyaApi, TuyaOemApi,
};
use tuya_rs::connection::DeviceError;

use crate::device::Device;
use crate::protocol::{
    ForbiddenZone, ForbiddenZoneCommand, RoomCleanCommand, RoomCleanStatusResponse, SweeperMessage,
    VirtualWallCommand, Wall, ZoneCleanCommand, build_sweeper_frame,
};
use crate::types::*;

/// X-Plorer vacuum controlled via the Tuya cloud API.
///
/// Unlike [`LocalXPlorer`](crate::LocalXPlorer) which uses local TCP, this sends
/// commands through the Tuya OEM Mobile API. Requires an active session
/// (call [`TuyaApi::login`] first).
pub struct CloudXPlorer<H: HttpClient = ReqwestClient> {
    api: TuyaOemApi<H>,
    dev_id: String,
}

impl CloudXPlorer {
    /// Log in to the Tuya cloud and create a cloud-controlled device.
    ///
    /// Handles API creation and login automatically.
    /// `app_device_id` is an arbitrary 44-character lowercase hex string
    /// identifying your API client (e.g. generated with `openssl rand -hex 22`).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() {
    /// use xplorer_rs::{CloudXPlorer, Device, xplorer_oem_credentials};
    ///
    /// let oem_creds = xplorer_oem_credentials("your_44char_hex_app_device_id_here");
    /// let mut robot = CloudXPlorer::login(oem_creds, "you@email.com", "password", "device_id")
    ///     .await.unwrap();
    /// let state = robot.status().await.unwrap();
    /// println!("battery: {}%", state.battery);
    /// # }
    /// ```
    pub async fn login(
        oem_creds: tuya_rs::api::OemCredentials,
        email: &str,
        password: &str,
        dev_id: impl Into<String>,
    ) -> Result<Self, DeviceError> {
        let mut api = TuyaOemApi::new(oem_creds);
        api.login(email, password)
            .await
            .map_err(api_to_device_error)?;
        Ok(Self::new(api, dev_id))
    }

    /// Create a new cloud-controlled device with the default HTTP client.
    ///
    /// Use this when you already have a logged-in [`TuyaOemApi`] instance.
    /// For a simpler approach, see [`CloudXPlorer::login`].
    pub fn new(api: TuyaOemApi, dev_id: impl Into<String>) -> Self {
        Self {
            api,
            dev_id: dev_id.into(),
        }
    }
}

impl<H: HttpClient> CloudXPlorer<H> {
    /// Create a new cloud-controlled device with a custom HTTP client.
    pub fn with_http(api: TuyaOemApi<H>, dev_id: impl Into<String>) -> Self {
        Self {
            api,
            dev_id: dev_id.into(),
        }
    }

    /// Return the device ID.
    pub fn dev_id(&self) -> &str {
        &self.dev_id
    }

    /// Get device info from the cloud.
    pub async fn device_info(&self) -> Result<CloudDeviceInfo, DeviceError> {
        self.api
            .device_info(&self.dev_id)
            .await
            .map_err(api_to_device_error)
    }

    /// Get AWS storage credentials for downloading map files.
    ///
    /// Returns temporary credentials for accessing the device's map files
    /// (`lay.bin`, `rou.bin`) on AWS S3. Use with [`generate_presigned_url`](crate::generate_presigned_url).
    pub async fn storage_config(&self) -> Result<StorageCredentials, DeviceError> {
        self.api
            .storage_config(&self.dev_id)
            .await
            .map_err(api_to_device_error)
    }

    /// Publish DPS values to the device via the cloud.
    async fn publish(&self, dps: serde_json::Value) -> Result<(), DeviceError> {
        self.api
            .publish_dps(&self.dev_id, &dps)
            .await
            .map_err(api_to_device_error)
    }
}

fn api_to_device_error(e: ApiError) -> DeviceError {
    DeviceError::ConnectionFailed(e.to_string())
}

impl<H: HttpClient> Device for CloudXPlorer<H> {
    async fn status(&mut self) -> Result<DeviceState, DeviceError> {
        let info = self
            .api
            .device_info(&self.dev_id)
            .await
            .map_err(api_to_device_error)?;

        let dps = info
            .dps
            .ok_or_else(|| DeviceError::InvalidResponse("device has no DPS data".into()))?;

        DeviceState::from_dps(&dps)
            .map_err(|e| DeviceError::InvalidResponse(format!("DPS parse error: {e}")))
    }

    async fn power_on(&mut self) -> Result<(), DeviceError> {
        self.publish(json!({"1": true})).await
    }

    async fn power_off(&mut self) -> Result<(), DeviceError> {
        self.publish(json!({"1": false})).await
    }

    async fn pause(&mut self) -> Result<(), DeviceError> {
        self.publish(json!({"2": false})).await
    }

    async fn resume(&mut self) -> Result<(), DeviceError> {
        self.publish(json!({"2": true})).await
    }

    async fn charge_go(&mut self) -> Result<(), DeviceError> {
        self.publish(json!({"4": "chargego"})).await
    }

    async fn locate(&mut self) -> Result<(), DeviceError> {
        self.publish(json!({"13": true})).await
    }

    async fn set_mode(&mut self, mode: Mode) -> Result<(), DeviceError> {
        self.publish(json!({"4": mode.as_str()})).await
    }

    async fn clean_rooms(
        &mut self,
        cmd: &RoomCleanCommand,
    ) -> Result<Option<RoomCleanStatusResponse>, DeviceError> {
        let b64 = cmd.encode_base64();
        self.publish(json!({"15": b64})).await?;
        // Cloud API doesn't return immediate sweeper responses
        Ok(None)
    }

    async fn clean_zone(&mut self, cmd: &ZoneCleanCommand) -> Result<(), DeviceError> {
        let b64 = cmd.encode_base64();
        self.publish(json!({"15": b64})).await
    }

    async fn set_forbidden_zones(&mut self, zones: &[ForbiddenZone]) -> Result<(), DeviceError> {
        let cmd = ForbiddenZoneCommand {
            zones: zones.to_vec(),
        };
        let b64 = cmd.encode_base64();
        self.publish(json!({"15": b64})).await
    }

    async fn clear_forbidden_zones(&mut self) -> Result<(), DeviceError> {
        self.set_forbidden_zones(&[]).await
    }

    async fn set_virtual_walls(&mut self, walls: &[Wall]) -> Result<(), DeviceError> {
        let cmd = VirtualWallCommand {
            walls: walls.to_vec(),
        };
        let b64 = cmd.encode_base64();
        self.publish(json!({"15": b64})).await
    }

    async fn clear_virtual_walls(&mut self) -> Result<(), DeviceError> {
        self.set_virtual_walls(&[]).await
    }

    async fn query_room_status(&mut self) -> Result<Option<RoomCleanStatusResponse>, DeviceError> {
        let frame = vec![0xAA, 0x00, 0x01, 0x15, 0x15];
        let b64 = base64::engine::general_purpose::STANDARD.encode(&frame);
        self.publish(json!({"15": b64})).await?;
        Ok(None)
    }

    async fn set_suction(&mut self, level: SuctionLevel) -> Result<(), DeviceError> {
        self.publish(json!({"9": level.as_str()})).await
    }

    async fn set_mop(&mut self, level: MopLevel) -> Result<(), DeviceError> {
        self.publish(json!({"10": level.as_str()})).await
    }

    async fn set_volume(&mut self, volume: u8) -> Result<(), DeviceError> {
        self.publish(json!({"26": volume})).await
    }

    async fn set_dnd(&mut self, enabled: bool) -> Result<(), DeviceError> {
        self.publish(json!({"25": enabled})).await
    }

    async fn set_value(
        &mut self,
        dp: u8,
        value: tuya_rs::connection::DpValue,
    ) -> Result<Option<tuya_rs::connection::DpsUpdate>, DeviceError> {
        use tuya_rs::connection::DpValue;

        let json_val = match &value {
            DpValue::Boolean(b) => json!(*b),
            DpValue::Integer(n) => json!(*n),
            DpValue::String(s) => json!(s),
            DpValue::Raw(bytes) => {
                json!(base64::engine::general_purpose::STANDARD.encode(bytes))
            }
        };

        self.publish(json!({dp.to_string(): json_val})).await?;
        // Cloud API doesn't return immediate DPS updates
        Ok(None)
    }

    async fn send_raw_command(
        &mut self,
        cmd: u8,
        data: &[u8],
    ) -> Result<Option<SweeperMessage>, DeviceError> {
        let frame = build_sweeper_frame(cmd, data);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&frame);
        self.publish(json!({"15": b64})).await?;
        Ok(None)
    }
}

/// Log in to the Tuya cloud and list all devices across all homes.
///
/// Returns a list of `(Home, Vec<DeviceInfo>)` pairs. Each [`DeviceInfo`]
/// includes the `local_key` needed for local TCP control.
///
/// # Examples
///
/// ```no_run
/// # async fn example() {
/// use xplorer_rs::{cloud_discover, xplorer_oem_credentials};
///
/// let oem_creds = xplorer_oem_credentials("your_44char_hex_app_device_id_here");
/// let results = cloud_discover(oem_creds, "you@email.com", "password").await.unwrap();
/// for (home, devices) in &results {
///     println!("Home: {}", home.name);
///     for dev in devices {
///         println!("  {} (key: {})", dev.name, dev.local_key);
///     }
/// }
/// # }
/// ```
pub async fn cloud_discover(
    oem_creds: tuya_rs::api::OemCredentials,
    email: &str,
    password: &str,
) -> Result<Vec<(Home, Vec<DeviceInfo>)>, DeviceError> {
    let mut api = TuyaOemApi::new(oem_creds);
    api.login(email, password)
        .await
        .map_err(api_to_device_error)?;

    let homes = api.list_homes().await.map_err(api_to_device_error)?;
    let mut result = Vec::new();
    for home in homes {
        let devices = api
            .list_devices(home.gid)
            .await
            .map_err(api_to_device_error)?;
        result.push((home, devices));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use tuya_rs::api::{HttpClient, OemCredentials, TuyaOemApi};

    struct MockHttpClient {
        responses: RefCell<VecDeque<String>>,
    }

    impl MockHttpClient {
        fn new(responses: Vec<&str>) -> Self {
            Self {
                responses: RefCell::new(responses.into_iter().map(String::from).collect()),
            }
        }
    }

    impl HttpClient for MockHttpClient {
        async fn post_form(
            &self,
            _endpoint: &str,
            _params: &[(String, String)],
        ) -> Result<String, ApiError> {
            self.responses
                .borrow_mut()
                .pop_front()
                .ok_or_else(|| ApiError::NetworkError("no more mock responses".into()))
        }
    }

    fn test_creds() -> OemCredentials {
        OemCredentials {
            client_id: "test_client".into(),
            app_secret: "test_secret_placeholder_here_xxx".into(),
            bmp_key: "test_bmp_key_placeholder_here_xx".into(),
            cert_hash: "AA:BB:CC:DD".into(),
            package_name: "com.test".into(),
            app_device_id: "test_device".into(),
        }
    }

    fn mock_cloud(responses: Vec<&str>) -> CloudXPlorer<MockHttpClient> {
        let api = TuyaOemApi::with_http(test_creds(), MockHttpClient::new(responses));
        CloudXPlorer::with_http(api, "dev1")
    }

    #[test]
    fn dev_id_returns_configured_id() {
        let robot = mock_cloud(vec![]);
        assert_eq!(robot.dev_id(), "dev1");
    }

    #[tokio::test]
    async fn power_on_publishes_dp1() {
        let mut robot = mock_cloud(vec![r#"{"result":true}"#]);
        robot.power_on().await.unwrap();
    }

    #[tokio::test]
    async fn power_off_publishes_dp1_false() {
        let mut robot = mock_cloud(vec![r#"{"result":true}"#]);
        robot.power_off().await.unwrap();
    }

    #[tokio::test]
    async fn charge_go_publishes_chargego() {
        let mut robot = mock_cloud(vec![r#"{"result":true}"#]);
        robot.charge_go().await.unwrap();
    }

    #[tokio::test]
    async fn locate_publishes_dp13() {
        let mut robot = mock_cloud(vec![r#"{"result":true}"#]);
        robot.locate().await.unwrap();
    }

    #[tokio::test]
    async fn status_parses_cloud_dps() {
        let mut robot = mock_cloud(vec![
            r#"{"result":{"devId":"dev1","name":"Robot","isOnline":true,"dps":{"1":true,"4":"smart","5":"cleaning","8":72,"9":"normal","10":"closed"}}}"#,
        ]);
        let state = robot.status().await.unwrap();
        assert!(state.power);
        assert_eq!(state.battery, 72);
        assert_eq!(state.mode, Mode::Smart);
    }

    #[tokio::test]
    async fn status_errors_when_no_dps() {
        let mut robot = mock_cloud(vec![
            r#"{"result":{"devId":"dev1","name":"Robot","isOnline":true}}"#,
        ]);
        assert!(robot.status().await.is_err());
    }

    #[tokio::test]
    async fn publish_error_propagates() {
        let mut robot = mock_cloud(vec![
            r#"{"errorCode":"USER_SESSION_INVALID","errorMsg":"expired"}"#,
        ]);
        assert!(robot.power_on().await.is_err());
    }

    #[tokio::test]
    async fn set_suction_publishes_dp9() {
        let mut robot = mock_cloud(vec![r#"{"result":true}"#]);
        robot.set_suction(SuctionLevel::Strong).await.unwrap();
    }

    #[tokio::test]
    async fn clean_rooms_returns_none() {
        let mut robot = mock_cloud(vec![r#"{"result":true}"#]);
        let cmd = RoomCleanCommand {
            clean_times: 1,
            room_ids: vec![0, 2],
        };
        let resp = robot.clean_rooms(&cmd).await.unwrap();
        assert!(resp.is_none());
    }
}

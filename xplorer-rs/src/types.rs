use std::collections::HashMap;
use std::fmt;

use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("unknown variant: {0}")]
    UnknownVariant(String),
    #[error("invalid DP value for DP {dp}: {reason}")]
    InvalidDpValue { dp: u8, reason: String },
}

// ── Mode (DP 4) ────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    ChargeGo,
    Standby,
    Smart,
    WallFollow,
    Spiral,
    SelectRoom,
    Zone,
    Part,
}

impl Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::ChargeGo => "chargego",
            Mode::Standby => "standby",
            Mode::Smart => "smart",
            Mode::WallFollow => "wall_follow",
            Mode::Spiral => "spiral",
            Mode::SelectRoom => "selectroom",
            Mode::Zone => "zone",
            Mode::Part => "part",
        }
    }
}

impl TryFrom<&str> for Mode {
    type Error = ParseError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "chargego" => Ok(Mode::ChargeGo),
            "standby" => Ok(Mode::Standby),
            "smart" => Ok(Mode::Smart),
            "wall_follow" => Ok(Mode::WallFollow),
            "spiral" => Ok(Mode::Spiral),
            "selectroom" => Ok(Mode::SelectRoom),
            "zone" => Ok(Mode::Zone),
            "part" | "pose" => Ok(Mode::Part),
            _ => Err(ParseError::UnknownVariant(s.to_string())),
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── Status (DP 5) ──────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    ChargeDone,
    Charging,
    Cleaning,
    SelectRoom,
    Repositing,
    GotoCharge,
    Paused,
    Fault,
    Smart,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::ChargeDone => "charge_done",
            Status::Charging => "charging",
            Status::Cleaning => "cleaning",
            Status::SelectRoom => "select_room",
            Status::Repositing => "repositing",
            Status::GotoCharge => "goto_charge",
            Status::Paused => "paused",
            Status::Fault => "fault",
            Status::Smart => "smart",
        }
    }
}

impl TryFrom<&str> for Status {
    type Error = ParseError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "charge_done" => Ok(Status::ChargeDone),
            "charging" => Ok(Status::Charging),
            "cleaning" => Ok(Status::Cleaning),
            "select_room" => Ok(Status::SelectRoom),
            "repositing" => Ok(Status::Repositing),
            "goto_charge" => Ok(Status::GotoCharge),
            "paused" => Ok(Status::Paused),
            "fault" => Ok(Status::Fault),
            "smart" => Ok(Status::Smart),
            _ => Err(ParseError::UnknownVariant(s.to_string())),
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── SuctionLevel (DP 9) ────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SuctionLevel {
    Gentle,
    Normal,
    Strong,
    Max,
}

impl SuctionLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            SuctionLevel::Gentle => "gentle",
            SuctionLevel::Normal => "normal",
            SuctionLevel::Strong => "strong",
            SuctionLevel::Max => "max",
        }
    }
}

impl TryFrom<&str> for SuctionLevel {
    type Error = ParseError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "gentle" => Ok(SuctionLevel::Gentle),
            "normal" => Ok(SuctionLevel::Normal),
            "strong" => Ok(SuctionLevel::Strong),
            "max" => Ok(SuctionLevel::Max),
            _ => Err(ParseError::UnknownVariant(s.to_string())),
        }
    }
}

impl fmt::Display for SuctionLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── MopLevel (DP 10) ───────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MopLevel {
    Closed,
    Low,
    Middle,
    High,
}

impl MopLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            MopLevel::Closed => "closed",
            MopLevel::Low => "low",
            MopLevel::Middle => "middle",
            MopLevel::High => "high",
        }
    }
}

impl TryFrom<&str> for MopLevel {
    type Error = ParseError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "closed" => Ok(MopLevel::Closed),
            "low" => Ok(MopLevel::Low),
            "middle" => Ok(MopLevel::Middle),
            "high" => Ok(MopLevel::High),
            _ => Err(ParseError::UnknownVariant(s.to_string())),
        }
    }
}

impl fmt::Display for MopLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── Consumable ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Consumable {
    pub remaining_minutes: u16,
}

// ── CleaningStats ──────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CleaningStats {
    pub total_area_m2: u32,
    pub total_sessions: u32,
    pub total_time_minutes: u32,
}

// ── SessionProgress ────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionProgress {
    pub area_m2: u16,
    pub time_minutes: u16,
}

// ── MapBitmap (DP 102) ─────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapBitmap(pub u16);

impl MapBitmap {
    pub fn split(&self) -> bool {
        self.0 & (1 << 0) != 0
    }
    pub fn merger(&self) -> bool {
        self.0 & (1 << 1) != 0
    }
    pub fn map(&self) -> bool {
        self.0 & (1 << 2) != 0
    }
    pub fn cleaning(&self) -> bool {
        self.0 & (1 << 3) != 0
    }
    pub fn active_split(&self) -> bool {
        self.0 & (1 << 4) != 0
    }
    pub fn not_by_human(&self) -> bool {
        self.0 & (1 << 5) != 0
    }
    pub fn save_fail(&self) -> bool {
        self.0 & (1 << 6) != 0
    }
    pub fn split_success(&self) -> bool {
        self.0 & (1 << 7) != 0
    }
    pub fn merger_success(&self) -> bool {
        self.0 & (1 << 8) != 0
    }
    pub fn choice_not_found(&self) -> bool {
        self.0 & (1 << 9) != 0
    }
    pub fn count_error(&self) -> bool {
        self.0 & (1 << 10) != 0
    }
    pub fn choice_set_ok(&self) -> bool {
        self.0 & (1 << 11) != 0
    }
}

// ── DpsEvent ───────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum DpsEvent {
    Power(bool),
    Start(bool),
    Mode(Mode),
    Status(Status),
    Area(u16),
    Time(u16),
    Battery(u8),
    Suction(SuctionLevel),
    Mop(MopLevel),
    Locate(bool),
    CommandTrans(Vec<u8>),
    SideBrush(Consumable),
    MainBrush(Consumable),
    Filter(Consumable),
    Dnd(bool),
    Volume(u8),
    Fault(u8),
    TotalArea(u32),
    TotalSessions(u32),
    TotalTime(u32),
    MapBitmapEvent(MapBitmap),
    EnvSettings(bool),
    Unknown { dp: u8, value: String },
}

impl DpsEvent {
    pub fn parse(dp: u8, value: &Value) -> Result<Self, ParseError> {
        let err = |reason: &str| ParseError::InvalidDpValue {
            dp,
            reason: reason.to_string(),
        };
        match dp {
            1 => value
                .as_bool()
                .map(DpsEvent::Power)
                .ok_or_else(|| err("expected bool")),
            2 => value
                .as_bool()
                .map(DpsEvent::Start)
                .ok_or_else(|| err("expected bool")),
            4 => {
                let s = value.as_str().ok_or_else(|| err("expected string"))?;
                Ok(DpsEvent::Mode(Mode::try_from(s)?))
            }
            5 => {
                let s = value.as_str().ok_or_else(|| err("expected string"))?;
                Ok(DpsEvent::Status(Status::try_from(s)?))
            }
            6 => {
                let n = value.as_u64().ok_or_else(|| err("expected number"))? as u16;
                Ok(DpsEvent::Area(n))
            }
            7 => {
                let n = value.as_u64().ok_or_else(|| err("expected number"))? as u16;
                Ok(DpsEvent::Time(n))
            }
            8 => {
                let n = value.as_u64().ok_or_else(|| err("expected number"))? as u8;
                Ok(DpsEvent::Battery(n))
            }
            9 => {
                let s = value.as_str().ok_or_else(|| err("expected string"))?;
                Ok(DpsEvent::Suction(SuctionLevel::try_from(s)?))
            }
            10 => {
                let s = value.as_str().ok_or_else(|| err("expected string"))?;
                Ok(DpsEvent::Mop(MopLevel::try_from(s)?))
            }
            13 => value
                .as_bool()
                .map(DpsEvent::Locate)
                .ok_or_else(|| err("expected bool")),
            15 => {
                let s = value.as_str().ok_or_else(|| err("expected string"))?;
                let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s)
                    .map_err(|e| ParseError::InvalidDpValue {
                    dp,
                    reason: format!("invalid base64: {e}"),
                })?;
                Ok(DpsEvent::CommandTrans(bytes))
            }
            17 => {
                let n = value.as_u64().ok_or_else(|| err("expected number"))? as u16;
                Ok(DpsEvent::SideBrush(Consumable {
                    remaining_minutes: n,
                }))
            }
            19 => {
                let n = value.as_u64().ok_or_else(|| err("expected number"))? as u16;
                Ok(DpsEvent::MainBrush(Consumable {
                    remaining_minutes: n,
                }))
            }
            21 => {
                let n = value.as_u64().ok_or_else(|| err("expected number"))? as u16;
                Ok(DpsEvent::Filter(Consumable {
                    remaining_minutes: n,
                }))
            }
            25 => value
                .as_bool()
                .map(DpsEvent::Dnd)
                .ok_or_else(|| err("expected bool")),
            26 => {
                let n = value.as_u64().ok_or_else(|| err("expected number"))? as u8;
                Ok(DpsEvent::Volume(n))
            }
            28 => {
                let n = value.as_u64().ok_or_else(|| err("expected number"))? as u8;
                Ok(DpsEvent::Fault(n))
            }
            29 => {
                let n = value.as_u64().ok_or_else(|| err("expected number"))? as u32;
                Ok(DpsEvent::TotalArea(n))
            }
            30 => {
                let n = value.as_u64().ok_or_else(|| err("expected number"))? as u32;
                Ok(DpsEvent::TotalSessions(n))
            }
            31 => {
                let n = value.as_u64().ok_or_else(|| err("expected number"))? as u32;
                Ok(DpsEvent::TotalTime(n))
            }
            102 => {
                let n = value.as_u64().ok_or_else(|| err("expected number"))? as u16;
                Ok(DpsEvent::MapBitmapEvent(MapBitmap(n)))
            }
            105 => value
                .as_bool()
                .map(DpsEvent::EnvSettings)
                .ok_or_else(|| err("expected bool")),
            _ => Ok(DpsEvent::Unknown {
                dp,
                value: value.to_string(),
            }),
        }
    }
}

// ── DeviceState ────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceState {
    pub power: bool,
    pub start: bool,
    pub mode: Mode,
    pub status: Status,
    pub session: SessionProgress,
    pub battery: u8,
    pub suction: SuctionLevel,
    pub mop: MopLevel,
    pub side_brush: Consumable,
    pub main_brush: Consumable,
    pub filter: Consumable,
    pub dnd: bool,
    pub volume: u8,
    pub fault: u8,
    pub stats: CleaningStats,
    pub map_bitmap: MapBitmap,
    pub env_settings: bool,
}

impl DeviceState {
    pub fn from_dps(dps: &HashMap<String, Value>) -> Result<Self, ParseError> {
        let get_bool = |key: &str, default: bool| -> bool {
            dps.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
        };
        let get_u64 = |key: &str, default: u64| -> u64 {
            dps.get(key).and_then(|v| v.as_u64()).unwrap_or(default)
        };
        let get_str = |key: &str| -> Option<&str> { dps.get(key).and_then(|v| v.as_str()) };

        let mode = get_str("4")
            .map(Mode::try_from)
            .transpose()?
            .unwrap_or(Mode::Standby);
        let status = get_str("5")
            .map(Status::try_from)
            .transpose()?
            .unwrap_or(Status::Charging);
        let suction = get_str("9")
            .map(SuctionLevel::try_from)
            .transpose()?
            .unwrap_or(SuctionLevel::Normal);
        let mop = get_str("10")
            .map(MopLevel::try_from)
            .transpose()?
            .unwrap_or(MopLevel::Closed);

        Ok(DeviceState {
            power: get_bool("1", false),
            start: get_bool("2", false),
            mode,
            status,
            session: SessionProgress {
                area_m2: get_u64("6", 0) as u16,
                time_minutes: get_u64("7", 0) as u16,
            },
            battery: get_u64("8", 0) as u8,
            suction,
            mop,
            side_brush: Consumable {
                remaining_minutes: get_u64("17", 0) as u16,
            },
            main_brush: Consumable {
                remaining_minutes: get_u64("19", 0) as u16,
            },
            filter: Consumable {
                remaining_minutes: get_u64("21", 0) as u16,
            },
            dnd: get_bool("25", false),
            volume: get_u64("26", 0) as u8,
            fault: get_u64("28", 0) as u8,
            stats: CleaningStats {
                total_area_m2: get_u64("29", 0) as u32,
                total_sessions: get_u64("30", 0) as u32,
                total_time_minutes: get_u64("31", 0) as u32,
            },
            map_bitmap: MapBitmap(get_u64("102", 0) as u16),
            env_settings: get_bool("105", false),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Mode ───────────────────────────────────────────────

    #[test]
    fn mode_as_str_roundtrip() {
        let variants = [
            (Mode::ChargeGo, "chargego"),
            (Mode::Standby, "standby"),
            (Mode::Smart, "smart"),
            (Mode::WallFollow, "wall_follow"),
            (Mode::Spiral, "spiral"),
            (Mode::SelectRoom, "selectroom"),
            (Mode::Zone, "zone"),
            (Mode::Part, "part"),
        ];
        for (mode, s) in &variants {
            assert_eq!(mode.as_str(), *s);
            assert_eq!(Mode::try_from(*s).unwrap(), *mode);
        }
    }

    #[test]
    fn mode_pose_alias() {
        assert_eq!(Mode::try_from("pose").unwrap(), Mode::Part);
    }

    #[test]
    fn mode_unknown_variant() {
        assert!(Mode::try_from("invalid").is_err());
    }

    // ── Status ─────────────────────────────────────────────

    #[test]
    fn status_as_str_roundtrip() {
        let variants = [
            (Status::ChargeDone, "charge_done"),
            (Status::Charging, "charging"),
            (Status::Cleaning, "cleaning"),
            (Status::SelectRoom, "select_room"),
            (Status::Repositing, "repositing"),
            (Status::GotoCharge, "goto_charge"),
            (Status::Paused, "paused"),
            (Status::Fault, "fault"),
            (Status::Smart, "smart"),
        ];
        for (status, s) in &variants {
            assert_eq!(status.as_str(), *s);
            assert_eq!(Status::try_from(*s).unwrap(), *status);
        }
    }

    // ── SuctionLevel ───────────────────────────────────────

    #[test]
    fn suction_as_str_roundtrip() {
        let variants = [
            (SuctionLevel::Gentle, "gentle"),
            (SuctionLevel::Normal, "normal"),
            (SuctionLevel::Strong, "strong"),
            (SuctionLevel::Max, "max"),
        ];
        for (level, s) in &variants {
            assert_eq!(level.as_str(), *s);
            assert_eq!(SuctionLevel::try_from(*s).unwrap(), *level);
        }
    }

    // ── MopLevel ───────────────────────────────────────────

    #[test]
    fn mop_as_str_roundtrip() {
        let variants = [
            (MopLevel::Closed, "closed"),
            (MopLevel::Low, "low"),
            (MopLevel::Middle, "middle"),
            (MopLevel::High, "high"),
        ];
        for (level, s) in &variants {
            assert_eq!(level.as_str(), *s);
            assert_eq!(MopLevel::try_from(*s).unwrap(), *level);
        }
    }

    // ── DpsEvent::parse ────────────────────────────────────

    #[test]
    fn parse_dp1_power() {
        assert_eq!(
            DpsEvent::parse(1, &json!(true)).unwrap(),
            DpsEvent::Power(true)
        );
        assert_eq!(
            DpsEvent::parse(1, &json!(false)).unwrap(),
            DpsEvent::Power(false)
        );
    }

    #[test]
    fn parse_dp4_mode() {
        assert_eq!(
            DpsEvent::parse(4, &json!("smart")).unwrap(),
            DpsEvent::Mode(Mode::Smart)
        );
    }

    #[test]
    fn parse_dp5_status() {
        assert_eq!(
            DpsEvent::parse(5, &json!("charging")).unwrap(),
            DpsEvent::Status(Status::Charging)
        );
    }

    #[test]
    fn parse_dp8_battery() {
        assert_eq!(
            DpsEvent::parse(8, &json!(85)).unwrap(),
            DpsEvent::Battery(85)
        );
    }

    #[test]
    fn parse_dp9_suction() {
        assert_eq!(
            DpsEvent::parse(9, &json!("strong")).unwrap(),
            DpsEvent::Suction(SuctionLevel::Strong)
        );
    }

    #[test]
    fn parse_dp10_mop() {
        assert_eq!(
            DpsEvent::parse(10, &json!("middle")).unwrap(),
            DpsEvent::Mop(MopLevel::Middle)
        );
    }

    #[test]
    fn parse_dp15_command_trans() {
        // base64 of [0xAA, 0x00, 0x04, 0x15, 0x01, 0x01, 0x04, 0x1B]
        let b64 = "qgAEFQEBBBs=";
        let event = DpsEvent::parse(15, &json!(b64)).unwrap();
        assert_eq!(
            event,
            DpsEvent::CommandTrans(vec![0xAA, 0x00, 0x04, 0x15, 0x01, 0x01, 0x04, 0x1B])
        );
    }

    #[test]
    fn parse_dp17_side_brush() {
        assert_eq!(
            DpsEvent::parse(17, &json!(1200)).unwrap(),
            DpsEvent::SideBrush(Consumable {
                remaining_minutes: 1200
            })
        );
    }

    #[test]
    fn parse_dp19_main_brush() {
        assert_eq!(
            DpsEvent::parse(19, &json!(500)).unwrap(),
            DpsEvent::MainBrush(Consumable {
                remaining_minutes: 500
            })
        );
    }

    #[test]
    fn parse_dp21_filter() {
        assert_eq!(
            DpsEvent::parse(21, &json!(300)).unwrap(),
            DpsEvent::Filter(Consumable {
                remaining_minutes: 300
            })
        );
    }

    #[test]
    fn parse_dp26_volume() {
        assert_eq!(
            DpsEvent::parse(26, &json!(50)).unwrap(),
            DpsEvent::Volume(50)
        );
    }

    #[test]
    fn parse_dp28_fault() {
        assert_eq!(DpsEvent::parse(28, &json!(0)).unwrap(), DpsEvent::Fault(0));
    }

    #[test]
    fn parse_dp29_total_area() {
        assert_eq!(
            DpsEvent::parse(29, &json!(42)).unwrap(),
            DpsEvent::TotalArea(42)
        );
    }

    #[test]
    fn parse_dp30_total_sessions() {
        assert_eq!(
            DpsEvent::parse(30, &json!(10)).unwrap(),
            DpsEvent::TotalSessions(10)
        );
    }

    #[test]
    fn parse_dp31_total_time() {
        assert_eq!(
            DpsEvent::parse(31, &json!(120)).unwrap(),
            DpsEvent::TotalTime(120)
        );
    }

    #[test]
    fn parse_dp102_map_bitmap() {
        let event = DpsEvent::parse(102, &json!(12)).unwrap();
        assert_eq!(event, DpsEvent::MapBitmapEvent(MapBitmap(12)));
        if let DpsEvent::MapBitmapEvent(bm) = event {
            assert!(!bm.split());
            assert!(!bm.merger());
            assert!(bm.map());
            assert!(bm.cleaning());
        }
    }

    #[test]
    fn parse_dp199_unknown() {
        let event = DpsEvent::parse(199, &json!("something")).unwrap();
        assert_eq!(
            event,
            DpsEvent::Unknown {
                dp: 199,
                value: "\"something\"".to_string()
            }
        );
    }

    // ── DeviceState::from_dps ──────────────────────────────

    #[test]
    fn device_state_from_dps_complete() {
        let dps: HashMap<String, Value> = serde_json::from_value(json!({
            "1": true,
            "2": false,
            "4": "smart",
            "5": "cleaning",
            "6": 15,
            "7": 22,
            "8": 72,
            "9": "strong",
            "10": "middle",
            "17": 1100,
            "19": 800,
            "21": 400,
            "25": true,
            "26": 60,
            "28": 0,
            "29": 200,
            "30": 50,
            "31": 1500,
            "102": 12,
            "105": true
        }))
        .unwrap();

        let state = DeviceState::from_dps(&dps).unwrap();
        assert!(state.power);
        assert!(!state.start);
        assert_eq!(state.mode, Mode::Smart);
        assert_eq!(state.status, Status::Cleaning);
        assert_eq!(state.session.area_m2, 15);
        assert_eq!(state.session.time_minutes, 22);
        assert_eq!(state.battery, 72);
        assert_eq!(state.suction, SuctionLevel::Strong);
        assert_eq!(state.mop, MopLevel::Middle);
        assert_eq!(state.side_brush.remaining_minutes, 1100);
        assert_eq!(state.main_brush.remaining_minutes, 800);
        assert_eq!(state.filter.remaining_minutes, 400);
        assert!(state.dnd);
        assert_eq!(state.volume, 60);
        assert_eq!(state.fault, 0);
        assert_eq!(state.stats.total_area_m2, 200);
        assert_eq!(state.stats.total_sessions, 50);
        assert_eq!(state.stats.total_time_minutes, 1500);
        assert_eq!(state.map_bitmap.0, 12);
        assert!(state.env_settings);
    }

    #[test]
    fn device_state_from_dps_defaults() {
        let dps: HashMap<String, Value> = HashMap::new();
        let state = DeviceState::from_dps(&dps).unwrap();
        assert!(!state.power);
        assert_eq!(state.mode, Mode::Standby);
        assert_eq!(state.status, Status::Charging);
        assert_eq!(state.battery, 0);
        assert_eq!(state.suction, SuctionLevel::Normal);
        assert_eq!(state.mop, MopLevel::Closed);
    }
}

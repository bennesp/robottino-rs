# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-03-05

### Changed

#### xplorer-rs
- **BREAKING**: `XPlorer` renamed to `LocalXPlorer` to clarify it is the local TCP implementation
- **BREAKING**: `Device` trait methods are now `async fn` (Rust 2024 native async traits)
- **BREAKING**: `DeviceListener` trait methods are now `async fn`
- `build_sweeper_frame()` extracted as public helper in `protocol` module

### Added

#### tuya-rs
- `publish_dps()` and `device_info()` methods on `TuyaApi` trait for cloud device control
- `CloudDeviceInfo` struct for cloud device state

#### xplorer-rs
- `CloudXPlorer` — cloud-based `Device` implementation via Tuya OEM Mobile API (`cloud` feature)
- `CloudXPlorer::login()` convenience constructor (no need to create `TuyaOemApi` manually)
- `CloudXPlorer::storage_config()` for downloading map files
- `cloud_discover()` standalone function for device enumeration
- Re-exported `DeviceConfig`, `DeviceError`, `DpValue`, `DpsUpdate`, `Transport`, `discovery` from `tuya-rs` — no direct `tuya-rs` imports needed
- Re-exported `Home`, `DeviceInfo`, `StorageCredentials`, `generate_presigned_url` under `cloud` feature
- `local_control` example: unified local TCP CLI (status, power, go home, locate, clean rooms)
- `cloud_control` example: unified cloud CLI (status, power, go home, locate, clean rooms)
- `cloud_discover` example: list all devices with local keys via cloud API

### Removed

#### xplorer-rs
- Separate examples `robot_status`, `clean_rooms`, `go_home`, `locate_robot` — merged into `local_control`
- `discover` example renamed to `local_discover`

## [0.1.0] - 2026-03-01

### Added

#### tuya-rs
- Tuya v3.3 local TCP protocol: packet encode/decode with AES-128-ECB encryption and CRC32 validation
- UDP device discovery on ports 6666 (plaintext) and 6667 (encrypted)
- `DeviceConfig` and `TuyaConnection` for connecting to devices on the local network
- `Transport` trait for abstracting TCP connections (enables mock testing)
- `cloud` feature: OEM Mobile API client with login flow, device/home listing, AWS STS storage credentials, and AWS4 pre-signed URL generation
- `HttpClient` trait for abstracting HTTP transport (enables mock testing)
- HMAC-SHA256 request signing with Tuya's custom sign string format

#### xplorer-rs
- `Device` trait and `XPlorer` struct for high-level vacuum control
- Room cleaning, zone cleaning, forbidden zones, and virtual walls via DP 15 binary protocol
- Device state model with DPS event parsing (DP 1-105)
- Map file decoder: LZ4-compressed layout (pixel grid + room metadata) and route (cleaning path)
- `render` feature: PNG rendering of layout maps and cleaning routes
- `cloud` feature: pre-filled OEM credentials for the X-Plorer app
- 8 runnable examples: discover, robot_status, clean_rooms, clean_zone, forbidden_zone, go_home, locate_robot, download_map

[0.2.0]: https://github.com/bennesp/robottino-rs/releases/tag/v0.2.0
[0.1.0]: https://github.com/bennesp/robottino-rs/releases/tag/v0.1.0

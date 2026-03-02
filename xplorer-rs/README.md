# xplorer-rs

[![Crates.io](https://img.shields.io/crates/v/xplorer-rs.svg)](https://crates.io/crates/xplorer-rs)
[![docs.rs](https://docs.rs/xplorer-rs/badge.svg)](https://docs.rs/xplorer-rs)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Local control library for **X-Plorer Serie 75 S / Serie 95 S** robot vacuum cleaners, built on top of [`tuya-rs`](../tuya-rs/) for Tuya v3.3 protocol communication.

Reverse-engineered from the official Android APK (Ghidra + packet sniffing).

Part of the [robottino-rs](https://github.com/bennesp/robottino-rs) workspace.

## Usage

```rust
use tuya_rs::connection::DeviceConfig;
use xplorer_rs::device::{Device, XPlorer};
use xplorer_rs::protocol::RoomCleanCommand;

let config = DeviceConfig::from_env()?;
let mut vacuum = XPlorer::connect(&config)?;

// Query device state
let state = vacuum.status()?;
println!("Battery: {}%, Status: {}", state.battery, state.status);

// Clean specific rooms
let cmd = RoomCleanCommand { clean_times: 1, room_ids: vec![0, 2] };
vacuum.clean_rooms(&cmd)?;
```

## Features

| Flag | Description |
|------|-------------|
| *(default)* | Local TCP control, map decoding (LZ4-compressed layout + route) |
| `cloud` | Cloud API access: login, device discovery, map download via AWS STS |
| `render` | PNG rendering of layout maps and cleaning routes |

## Modules

- **`device`** — `Device` trait and `XPlorer` struct: power, clean rooms/zones, forbidden zones, virtual walls
- **`protocol`** — DP 15 binary protocol: sweeper message codec, room/zone clean, forbidden zones, virtual walls
- **`types`** — Device state model: DPS event parsing, enums for Mode, Status, SuctionLevel, MopLevel
- **`map`** — Map file decoder: layout (LZ4 pixel grid + room metadata) and route (cleaning path), optional PNG rendering

## Examples

```bash
cargo run --example discover                               # find devices on the network
cargo run --example robot_status                           # full device dashboard
cargo run --example clean_rooms -- 0 2                     # clean rooms 0 and 2
cargo run --example clean_zone                             # clean a rectangular zone
cargo run --example forbidden_zone                         # set no-go zones
cargo run --example go_home                                # return to charger
cargo run --example locate_robot                           # trigger "find me" beep
cargo run --example download_map --features cloud,render   # download + render map
```

The `discover` example requires no credentials. All others need `DEVICE_IP`, `DEVICE_ID`, and `LOCAL_KEY` environment variables. The `download_map` example additionally requires OEM cloud credentials (`TUYA_*`) — see the [workspace README](../README.md#cloud-credentials) for details.

## Tuya OEM credentials for X-Plorer

The `cloud` feature requires OEM credentials extracted from the official Android APK. These are the values for the **Rowenta X-Plorer Serie 75 S / Serie 95 S** app (`com.groupeseb.ext.xplorer`):

| Variable | Value |
|----------|-------|
| `TUYA_CLIENT_ID` | `staxmyjjd8thqxypvr5v` |
| `TUYA_APP_SECRET` | `q39ksm4c5yps9atn9repakn4gxpja3vh` |
| `TUYA_BMP_KEY` | `4rkkvamwnhedxecyexd9t5cxkchxtqff` |
| `TUYA_CERT_HASH` | `1B:D3:2E:D5:5E:D7:47:E3:81:A1:AF:EC:66:FA:AC:7B:E4:C8:A6:B2:DD:1F:1A:17:48:5E:1E:D1:1E:37:DB:92` |
| `TUYA_PACKAGE_NAME` | `com.groupeseb.ext.xplorer` |
| `TUYA_APP_DEVICE_ID` | Any 44-char lowercase hex string (e.g. `openssl rand -hex 22`) |

These are static and identical for every installation of the app. See the [workspace README](../README.md#cloud-credentials) for the full API flow and how each credential is extracted.

## How it works

The vacuum exposes state as numbered **Data Points (DPS)** over Tuya's local TCP protocol. Complex commands (room clean, zone clean, forbidden zones, virtual walls) are sent as base64-encoded binary frames via DP 15:

```
0xAA + length (2 bytes BE) + command + data + checksum
```

Map files (`lay.bin`, `rou.bin`) are downloaded from Tuya's AWS S3 storage. The layout contains an LZ4-compressed pixel grid with room metadata; the route contains the cleaning path as signed 16-bit coordinate pairs.

## Disclaimer

This project is the result of independent reverse engineering for personal use and interoperability purposes. It is not affiliated with, endorsed by, or connected to Groupe SEB, Tuya, or any of their subsidiaries. Use at your own risk.

## License

[MIT](../LICENSE)

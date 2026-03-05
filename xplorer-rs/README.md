# xplorer-rs

[![Crates.io](https://img.shields.io/crates/v/xplorer-rs.svg)](https://crates.io/crates/xplorer-rs)
[![docs.rs](https://docs.rs/xplorer-rs/badge.svg)](https://docs.rs/xplorer-rs)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Control library for **X-Plorer Serie 75 S / Serie 95 S** robot vacuum cleaners, built on top of [`tuya-rs`](../tuya-rs/) for Tuya v3.3 protocol communication.

Two implementations of the `Device` trait are available:

- **`LocalXPlorer`** — local TCP control via the Tuya v3.3 protocol (default)
- **`CloudXPlorer`** — cloud control via the Tuya OEM Mobile API (`cloud` feature)

Reverse-engineered from the official Android APK (Ghidra + packet sniffing).

Part of the [robottino-rs](https://github.com/bennesp/robottino-rs) workspace.

## Features

| Flag | Description |
|------|-------------|
| *(default)* | Local TCP control, map decoding (LZ4-compressed layout + route) |
| `cloud` | Cloud API access: login, device discovery, map download via AWS STS. Adds `CloudXPlorer` for remote device control |
| `render` | PNG rendering of layout maps and cleaning routes |

## Modules

- **`device`** — `Device` trait and `LocalXPlorer` struct: power, clean rooms/zones, forbidden zones, virtual walls
- **`cloud_device`** *(cloud)* — `CloudXPlorer`: same `Device` trait, commands sent via Tuya cloud HTTP API
- **`protocol`** — DP 15 binary protocol: sweeper message codec, room/zone clean, forbidden zones, virtual walls
- **`types`** — Device state model: DPS event parsing, enums for Mode, Status, SuctionLevel, MopLevel
- **`map`** — Map file decoder: layout (LZ4 pixel grid + room metadata) and route (cleaning path), optional PNG rendering

---

## Local control

Direct TCP communication with the vacuum on your local network.

Requires `DEVICE_IP`, `DEVICE_ID`, and `LOCAL_KEY` environment variables.

### Usage

```rust
use xplorer_rs::{DeviceConfig, LocalXPlorer, Device};
use xplorer_rs::protocol::RoomCleanCommand;

let config = DeviceConfig::from_env()?;
let mut vacuum = LocalXPlorer::connect(&config)?;

// Query device state
let state = vacuum.status().await?;
println!("Battery: {}%, Status: {}", state.battery, state.status);

// Clean specific rooms
let cmd = RoomCleanCommand { clean_times: 1, room_ids: vec![0, 2] };
vacuum.clean_rooms(&cmd).await?;
```

### Examples

```bash
cargo run --example local_discover                 # find devices via UDP broadcast
cargo run --example local_control -- status        # device dashboard
cargo run --example local_control -- clean_rooms 0 2
cargo run --example clean_zone -- --x1 82 --y1 -13 --x2 453 --y2 203
cargo run --example forbidden_zone -- zone --x1 82 --y1 -13 --x2 453 --y2 203
```

| Example | Description |
|---------|-------------|
| `local_discover` | Find Tuya devices on the local network via UDP broadcast |
| `local_control` | Status, power on/off, go home, locate, clean rooms |
| `clean_zone` | Clean a rectangular zone (cmd 0x28) |
| `forbidden_zone` | Set no-go zones, no-sweep zones, virtual walls |

---

## Cloud control

Remote control via the Tuya OEM Mobile API. Requires `--features cloud`.

Requires `TUYA_EMAIL`, `TUYA_PASSWORD` (and `TUYA_DEV_ID` for device control).

### Usage

```rust
use xplorer_rs::{CloudXPlorer, Device, xplorer_oem_credentials};

let oem_creds = xplorer_oem_credentials("your_app_device_id_here");
let mut robot = CloudXPlorer::login(oem_creds, "you@email.com", "password", "your_device_id").await?;
let state = robot.status().await?;
println!("Battery: {}%, Status: {}", state.battery, state.status);
```

### Examples

```bash
cargo run --example cloud_discover --features cloud            # list devices + local keys
cargo run --example cloud_control --features cloud -- status   # device dashboard
cargo run --example cloud_control --features cloud -- clean_rooms 0 2
cargo run --example download_map --features cloud,render       # download + render map
```

| Example | Description |
|---------|-------------|
| `cloud_discover` | List all devices with local keys, names, product IDs |
| `cloud_control` | Status, power on/off, go home, locate, clean rooms |
| `download_map` | Download map from AWS S3 and render as PNG (requires `render`) |

### OEM credentials

The `cloud` feature requires OEM credentials extracted from the official Android APK. These are already embedded in `xplorer_oem_credentials()` for the X-Plorer app (`com.groupeseb.ext.xplorer`).

See the [workspace README](../README.md#oem-credentials-for-reference) for the full list and extraction methods.

---

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

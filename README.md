# robottino-rs

[![CI](https://github.com/bennesp/robottino-rs/actions/workflows/checks.yaml/badge.svg)](https://github.com/bennesp/robottino-rs/actions/workflows/checks.yaml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Rust workspace for local control of **X-Plorer Serie 75 S / Serie 95 S** robot vacuum cleaners via the Tuya v3.3 protocol.

## Crates

| Crate | Description |
|-------|-------------|
| [`tuya-rs`](tuya-rs/) | Tuya v3.3 protocol layer — TCP connection, packet codec, AES-ECB encryption, UDP discovery |
| [`xplorer-rs`](xplorer-rs/) | Vacuum control — room/zone cleaning, forbidden zones, virtual walls, map decoding |

## Quick start

### 1. Discover devices on your network

```bash
cargo run --example discover
```

This finds all Tuya devices via UDP broadcast and prints their `device_id` and `ip`.

### 2. Get the local key

The `local_key` (AES encryption key) is not included in the UDP broadcast. You can obtain it via:

- **[tinytuya](https://github.com/jasonacox/tinytuya) wizard** — easiest method, uses the Tuya Developer Console
- **The `cloud` feature of this crate** — uses the OEM Mobile API (same API the app uses), requires credentials extracted from the APK (see below)

### 3. Control the vacuum

```bash
# Check vacuum status
DEVICE_IP=192.168.1.42 DEVICE_ID=abc123 LOCAL_KEY=0123456789abcdef \
  cargo run --example robot_status

# Clean specific rooms (by room ID)
DEVICE_IP=192.168.1.42 DEVICE_ID=abc123 LOCAL_KEY=0123456789abcdef \
  cargo run --example clean_rooms -- 0 2 5

# Download and render the map (requires cloud credentials)
cargo run --example download_map --features cloud,render
```

## Building

```bash
# Default (local-only control)
cargo build

# With cloud API support (login, device discovery, map download)
cargo build --features cloud

# With map PNG rendering
cargo build --features render

# Everything
cargo build --all-features

# Run tests
cargo test --all-features
```

## Feature flags

| Flag | Crate | Description |
|------|-------|-------------|
| `cloud` | both | HTTP API client: login, device/home listing, AWS STS map storage |
| `render` | `xplorer-rs` | PNG rendering of layout maps and cleaning routes |

Default build is local-only TCP control with no network dependencies beyond `std::net`.

## Examples

| Example | Description |
|---------|-------------|
| `discover` | Find Tuya devices on the local network (no credentials needed) |
| `robot_status` | Connect and display full device state |
| `clean_rooms` | Send room cleaning command |
| `clean_zone` | Clean a rectangular zone |
| `forbidden_zone` | Set no-go zones |
| `go_home` | Send the vacuum back to the charger |
| `locate_robot` | Trigger the "find me" beep |
| `download_map` | Download and render map from cloud (`cloud` + `render`) |

## Cloud credentials

The `cloud` feature uses Tuya's **OEM Mobile API** — the same API the official app communicates with. This requires a set of credentials extracted from the Android APK.

### API flow

```
login(email, password)  →  Session { sid }
list_homes()            →  Vec<Home { gid }>
list_devices(gid)       →  Vec<DeviceInfo { dev_id, local_key, ... }>
storage_config(dev_id)  →  StorageCredentials { ak, sk, token, bucket, ... }
generate_presigned_url  →  signed S3 URL for lay.bin / rou.bin
```

### Environment variables

| Variable | Source | Description |
|----------|--------|-------------|
| `TUYA_EMAIL` | Your account | Email registered in the Rowenta / X-Plorer app |
| `TUYA_PASSWORD` | Your account | Password for the same account |
| `TUYA_DEV_ID` | `discover` example or app | Device ID (also found via UDP discovery) |
| `TUYA_CLIENT_ID` | APK decompilation | `appKey` from `SmartApplication.java` (jadx) |
| `TUYA_APP_SECRET` | APK decompilation | `appSecret` from `SmartApplication.java` (jadx) |
| `TUYA_BMP_KEY` | BMP steganography | Hidden in `assets/t_s.bmp`, extracted via Vandermonde matrix algorithm (see [tuya-sign-hacking](https://github.com/nalajcie/tuya-sign-hacking)) |
| `TUYA_CERT_HASH` | APK signing cert | SHA-256 of the APK signing certificate (`keytool -printcert -jarfile app.apk`), colon-separated uppercase hex |
| `TUYA_PACKAGE_NAME` | AndroidManifest.xml | App package name (e.g. `com.groupeseb.ext.xplorer`) |
| `TUYA_APP_DEVICE_ID` | Free choice | Arbitrary device identifier sent with API requests — any 44-character lowercase hex string works (e.g. generate with `openssl rand -hex 22`) |

> **Note:** `TUYA_CLIENT_ID`, `TUYA_APP_SECRET`, `TUYA_CERT_HASH`, `TUYA_PACKAGE_NAME`, and `TUYA_BMP_KEY` are static (same for all installations of the app). `TUYA_APP_DEVICE_ID` can be any hex string — it identifies your API client, not the vacuum.

### How to extract each credential

| Credential | Method |
|------------|--------|
| `TUYA_CLIENT_ID`, `TUYA_APP_SECRET` | Decompile the APK with [jadx](https://github.com/skylot/jadx), find `appKey`/`appSecret` in `SmartApplication.java` |
| `TUYA_BMP_KEY` | Extract from `assets/t_s.bmp` using the Vandermonde algorithm from [tuya-sign-hacking](https://github.com/nalajcie/tuya-sign-hacking) |
| `TUYA_CERT_HASH` | `keytool -printcert -jarfile app.apk` — SHA-256 digest, colon-separated |
| `TUYA_PACKAGE_NAME` | Read from `AndroidManifest.xml` in the APK |
| `TUYA_APP_DEVICE_ID` | Any 44-character lowercase hex string (e.g. `openssl rand -hex 22`) — identifies your API client |

## How it works

The vacuum exposes state as numbered **Data Points (DPS)** over Tuya's local TCP protocol:

- **DP 1**: power on/off
- **DP 4**: mode (smart, wall, spiral, ...)
- **DP 5**: status (standby, cleaning, charging, ...)
- **DP 8**: battery percentage
- **DP 9**: suction level
- **DP 15**: binary sweeper commands (room clean, zone clean, forbidden zones, virtual walls)

Complex commands are sent as base64-encoded binary frames via DP 15, using a custom protocol: `0xAA` + length (2 bytes BE) + command + data + checksum.

## Disclaimer

This project is the result of independent reverse engineering for personal use and interoperability purposes. It is not affiliated with, endorsed by, or connected to Groupe SEB, Tuya, or any of their subsidiaries. Use at your own risk.

## License

[MIT](LICENSE)

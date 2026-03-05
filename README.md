# robottino-rs

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Rust workspace for controlling **X-Plorer Serie 75 S / Serie 95 S** robot vacuum cleaners via the Tuya v3.3 protocol — locally over TCP or remotely via the Tuya cloud API.

## Crates

| Crate | Description |
|-------|-------------|
| [`tuya-rs`](tuya-rs/) | Tuya v3.3 protocol layer — TCP connection, packet codec, AES-ECB encryption, UDP discovery, cloud API |
| [`xplorer-rs`](xplorer-rs/) | Vacuum control — `LocalXPlorer` (TCP) and `CloudXPlorer` (cloud API), room/zone cleaning, forbidden zones, virtual walls, map decoding |

## Building

```bash
# Default (local-only control)
cargo build

# With cloud API support
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
| `cloud` | both | HTTP API client: login, device/home listing, device control, AWS STS map storage |
| `render` | `xplorer-rs` | PNG rendering of layout maps and cleaning routes |

Default build is local-only TCP control with no network dependencies beyond `std::net`.

---

## Local control

Direct TCP communication with the vacuum on your local network. No internet required, lowest latency. Uses the Tuya v3.3 protocol with AES-ECB encryption.

### Prerequisites

| Variable | Description | How to get |
|----------|-------------|------------|
| `DEVICE_IP` | Vacuum IP on your LAN | `local_discover` example or your router |
| `DEVICE_ID` | Tuya device ID | `local_discover` or `cloud_discover` |
| `LOCAL_KEY` | 16-byte AES encryption key | `cloud_discover`, [tinytuya](https://github.com/jasonacox/tinytuya) wizard, or cloud API |

> **Note:** The `LOCAL_KEY` is not available via local discovery — you need the cloud API or tinytuya to obtain it.

### Examples

```bash
# Discover devices on the local network (no credentials needed)
cargo run --example local_discover

# Device control
DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example local_control -- status
DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example local_control -- power_on
DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example local_control -- go_home
DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example local_control -- locate
DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example local_control -- clean_rooms 0 2 5

# Zone cleaning and map restrictions
DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example clean_zone -- --x1 82 --y1 -13 --x2 453 --y2 203
DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example forbidden_zone -- zone --x1 82 --y1 -13 --x2 453 --y2 203
DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example forbidden_zone -- wall --x1 100 --y1 100 --x2 400 --y2 100
```

| Example | Description |
|---------|-------------|
| `local_discover` | Find Tuya devices on the local network via UDP broadcast |
| `local_control` | Status, power on/off, go home, locate, clean rooms |
| `clean_zone` | Clean a rectangular zone (cmd 0x28) |
| `forbidden_zone` | Set no-go zones, no-sweep zones, virtual walls |

---

## Cloud control

Remote control via the Tuya OEM Mobile API — the same API the official app uses. Works from anywhere, no local network access needed.

Requires `--features cloud` at build time.

### Prerequisites

| Variable | Description |
|----------|-------------|
| `TUYA_EMAIL` | Email registered in the Rowenta / X-Plorer app |
| `TUYA_PASSWORD` | Password for the same account |
| `TUYA_DEV_ID` | Device ID (from `local_discover` or `cloud_discover`) |

The OEM API credentials (client ID, app secret, BMP key, certificate hash) are hardcoded in `xplorer_oem_credentials()` — they are static values extracted from the Android APK and identical for every installation.

### Examples

```bash
# Discover devices and get local keys
TUYA_EMAIL=... TUYA_PASSWORD=... \
  cargo run --example cloud_discover --features cloud

# Device control
TUYA_EMAIL=... TUYA_PASSWORD=... TUYA_DEV_ID=... \
  cargo run --example cloud_control --features cloud -- status
TUYA_EMAIL=... TUYA_PASSWORD=... TUYA_DEV_ID=... \
  cargo run --example cloud_control --features cloud -- power_on
TUYA_EMAIL=... TUYA_PASSWORD=... TUYA_DEV_ID=... \
  cargo run --example cloud_control --features cloud -- clean_rooms 0 2

# Download and render the map as PNG
TUYA_EMAIL=... TUYA_PASSWORD=... TUYA_DEV_ID=... \
  cargo run --example download_map --features cloud,render
```

| Example | Description |
|---------|-------------|
| `cloud_discover` | List all devices with local keys, names, and product IDs |
| `cloud_control` | Status, power on/off, go home, locate, clean rooms |
| `download_map` | Download map from AWS S3 and render as PNG (requires `render`) |

### Cloud API flow

```
login(email, password)  →  Session { sid }
list_homes()            →  Vec<Home { gid }>
list_devices(gid)       →  Vec<DeviceInfo { dev_id, local_key, ... }>
storage_config(dev_id)  →  StorageCredentials { ak, sk, token, bucket, ... }
generate_presigned_url  →  signed S3 URL for lay.bin / rou.bin
```

### OEM credentials (for reference)

The following values are extracted from the Android APK (`com.groupeseb.ext.xplorer`) and already embedded in the library:

| Credential | Value |
|------------|-------|
| Client ID | `staxmyjjd8thqxypvr5v` |
| App Secret | `q39ksm4c5yps9atn9repakn4gxpja3vh` |
| BMP Key | `4rkkvamwnhedxecyexd9t5cxkchxtqff` |
| Cert Hash | `1B:D3:2E:D5:5E:D7:...` (SHA-256, colon-separated) |
| Package Name | `com.groupeseb.ext.xplorer` |

<details>
<summary>How to extract these from any Tuya OEM app</summary>

| Credential | Method |
|------------|--------|
| Client ID, App Secret | Decompile the APK with [jadx](https://github.com/skylot/jadx), find `appKey`/`appSecret` in `SmartApplication.java` |
| BMP Key | Extract from `assets/t_s.bmp` using the Vandermonde algorithm from [tuya-sign-hacking](https://github.com/nalajcie/tuya-sign-hacking) |
| Cert Hash | `keytool -printcert -jarfile app.apk` — SHA-256 digest, colon-separated |
| Package Name | Read from `AndroidManifest.xml` in the APK |

</details>

---

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

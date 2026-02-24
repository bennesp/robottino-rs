# tuya-rs

Tuya v3.3 local protocol implementation in Rust.

Provides UDP device discovery, TCP connection, packet encoding/decoding, and AES-ECB encryption for communicating with Tuya-based IoT devices on the local network.

Part of the [robottino-rs](https://github.com/bennesp/robottino-rs) workspace.

## Usage

### Discover devices on the network

```rust
use tuya_rs::discovery;
use std::time::Duration;

let devices = discovery::discover(Duration::from_secs(10))?;
for dev in &devices {
    println!("{} @ {} (v{})", dev.device_id, dev.ip, dev.version);
}
```

No credentials needed — devices broadcast their ID and IP via UDP every ~5 seconds. The `local_key` must be obtained separately (via cloud API or [tinytuya](https://github.com/jasonacox/tinytuya)).

### Connect and send commands

```rust
use tuya_rs::connection::{DeviceConfig, TuyaConnection, TuyaCommand};

let config = DeviceConfig {
    dev_id: "your_device_id".into(),
    address: "192.168.1.42".into(),
    local_key: "0123456789abcdef".into(),
    ..Default::default()
};

let mut conn = TuyaConnection::connect(&config)?;
let response = conn.send(TuyaCommand::DpQuery, b"{}".to_vec())?;
println!("{:?}", response);
```

### Cloud API: get the local key

With the `cloud` feature, you can obtain the `local_key` via Tuya's OEM Mobile API:

```rust
use tuya_rs::api::{OemCredentials, TuyaOemApi, TuyaApi};

let creds = OemCredentials { /* from APK — see workspace README */ };
let mut api = TuyaOemApi::new(creds);
let _session = api.login("you@email.com", "password")?;

let homes = api.list_homes()?;
let devices = api.list_devices(homes[0].gid)?;
println!("local_key: {}", devices[0].local_key);
```

The OEM credentials must be extracted from the Android APK — see the [workspace README](../README.md#cloud-credentials) for details. The `xplorer-rs` crate provides pre-filled credentials for the X-Plorer app via `xplorer_oem_credentials()`.

## Features

| Flag | Description |
|------|-------------|
| *(default)* | UDP device discovery, TCP packet codec, AES-128-ECB encryption, CRC32 validation |
| `cloud` | OEM Mobile API client: login flow, device/home listing, HMAC-SHA256 request signing, AWS STS credentials for map storage |

## Modules

- **`discovery`** — UDP broadcast listener: find devices on the local network (ports 6666/6667)
- **`connection`** — `DeviceConfig`, `TuyaConnection` (TCP), `TuyaPacket` encode/decode, `TuyaCommand` enum
- **`crypto`** — AES-128-ECB with PKCS7 padding, hex encoding
- **`api`** *(cloud)* — OEM Mobile API: login, device listing, AWS4 pre-signed URL generation
- **`signing`** *(cloud)* — HMAC-SHA256 request signing with Tuya's custom sign string format

## License

[MIT](../LICENSE)

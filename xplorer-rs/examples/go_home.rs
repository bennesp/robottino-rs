//! Send the vacuum cleaner back to the charging dock.
//!
//!   DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example go_home

use tuya_rs::connection::DeviceConfig;
use xplorer_rs::device::{Device, XPlorer};

fn main() {
    let config = DeviceConfig::from_env().unwrap_or_else(|e| {
        eprintln!("Missing env var: {e}");
        eprintln!("Usage: DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example go_home");
        std::process::exit(1);
    });

    print!("Connecting to {}:{}... ", config.address, config.port);
    let mut vacuum = XPlorer::connect(&config).unwrap_or_else(|e| {
        eprintln!("FAILED: {e}");
        std::process::exit(1);
    });
    println!("OK");

    print!("Sending go home... ");
    vacuum.charge_go().unwrap_or_else(|e| {
        eprintln!("FAILED: {e}");
        std::process::exit(1);
    });
    println!("ACK (vacuum is returning to dock)");
}

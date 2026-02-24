//! Make the vacuum cleaner beep — connect and send locate command.
//!
//!   DEVICE_IP=192.168.1.x DEVICE_ID=xxx LOCAL_KEY=16charkey cargo run --example locate_robot

use tuya_rs::connection::DeviceConfig;
use xplorer_rs::device::{Device, XPlorer};

fn main() {
    let config = DeviceConfig::from_env().unwrap_or_else(|e| {
        eprintln!("Missing env var: {e}");
        eprintln!(
            "Usage: DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example locate_robot"
        );
        std::process::exit(1);
    });

    print!("Connecting to {}:{}... ", config.address, config.port);
    let mut vacuum = XPlorer::connect(&config).unwrap_or_else(|e| {
        eprintln!("FAILED: {e}");
        std::process::exit(1);
    });
    println!("OK");

    print!("Sending locate... ");
    vacuum.locate().unwrap_or_else(|e| {
        eprintln!("FAILED: {e}");
        std::process::exit(1);
    });
    println!("ACK (vacuum should beep now)");
}

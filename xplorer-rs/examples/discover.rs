//! Discover Tuya devices on the local network via UDP broadcast.
//!
//!   cargo run --example discover

use std::time::Duration;
use tuya_rs::discovery;

fn main() {
    let timeout = Duration::from_secs(10);
    println!(
        "Scanning for Tuya devices ({} seconds)...\n",
        timeout.as_secs()
    );

    match discovery::discover(timeout) {
        Ok(devices) if devices.is_empty() => {
            println!("No devices found.");
            println!("Make sure you're on the same network as your Tuya devices.");
        }
        Ok(devices) => {
            println!("Found {} device(s):\n", devices.len());
            for dev in &devices {
                println!("  ID:       {}", dev.device_id);
                println!("  IP:       {}", dev.ip);
                println!("  Version:  {}", dev.version);
                println!("  Product:  {}", dev.product_key);
                println!();
            }
        }
        Err(e) => {
            eprintln!("Discovery failed: {e}");
            std::process::exit(1);
        }
    }
}

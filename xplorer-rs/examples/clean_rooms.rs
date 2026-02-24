//! Clean specific rooms — connect and send room cleaning command.
//!
//!   DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example clean_rooms -- 0 5
//!   DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example clean_rooms       # lists room IDs

use tuya_rs::connection::DeviceConfig;
use xplorer_rs::device::{Device, XPlorer};
use xplorer_rs::protocol::RoomCleanCommand;

fn main() {
    let config = DeviceConfig::from_env().unwrap_or_else(|e| {
        eprintln!("Missing env var: {e}");
        eprintln!("Usage: DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example clean_rooms -- <id> [id...]");
        eprintln!("Use download_map to discover room IDs from the map metadata.");
        std::process::exit(1);
    });

    // Parse room IDs from CLI args
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("No room IDs specified.");
        eprintln!("Usage: cargo run --example clean_rooms -- 0 2 5");
        eprintln!("Use download_map to discover room IDs from the map metadata.");
        std::process::exit(1);
    }

    let room_ids: Vec<u8> = args
        .iter()
        .map(|s| {
            s.parse::<u8>().unwrap_or_else(|_| {
                eprintln!("Invalid room ID: {s} (expected a number 0-255)");
                std::process::exit(1);
            })
        })
        .collect();

    println!("Room IDs: {:?}", room_ids);

    let cmd = RoomCleanCommand {
        clean_times: 1,
        room_ids,
    };

    print!("Connecting to {}:{}... ", config.address, config.port);
    let mut vacuum = XPlorer::connect(&config).unwrap_or_else(|e| {
        eprintln!("FAILED: {e}");
        std::process::exit(1);
    });
    println!("OK");

    print!("Sending clean command... ");
    match vacuum.clean_rooms(&cmd) {
        Ok(Some(resp)) => {
            println!("OK");
            println!(
                "  Status: clean_times={}, rooms={:?}",
                resp.clean_times, resp.room_ids
            );
        }
        Ok(None) => println!("ACK (no status response)"),
        Err(e) => {
            eprintln!("FAILED: {e}");
            std::process::exit(1);
        }
    }
}

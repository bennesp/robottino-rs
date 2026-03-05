//! Zone cleaning — send a rectangular zone cleaning command (cmd 0x28).
//!
//!   DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example clean_zone -- --x1 82 --y1 -13 --x2 453 --y2 203
//!   DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example clean_zone -- --x1 82 --y1 -13 --x2 453 --y2 203 --times 2

use xplorer_rs::protocol::{Zone, ZoneCleanCommand};
use xplorer_rs::{Device, DeviceConfig, LocalXPlorer};

fn parse_arg(args: &[String], flag: &str) -> Option<i16> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}

#[tokio::main]
async fn main() {
    let config = DeviceConfig::from_env().unwrap_or_else(|e| {
        eprintln!("Missing env var: {e}");
        eprintln!(
            "Usage: DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example clean_zone -- --x1 X --y1 Y --x2 X --y2 Y [--times N]"
        );
        std::process::exit(1);
    });

    let args: Vec<String> = std::env::args().collect();

    let x1 = parse_arg(&args, "--x1").unwrap_or_else(|| {
        eprintln!("Missing --x1");
        std::process::exit(1);
    });
    let y1 = parse_arg(&args, "--y1").unwrap_or_else(|| {
        eprintln!("Missing --y1");
        std::process::exit(1);
    });
    let x2 = parse_arg(&args, "--x2").unwrap_or_else(|| {
        eprintln!("Missing --x2");
        std::process::exit(1);
    });
    let y2 = parse_arg(&args, "--y2").unwrap_or_else(|| {
        eprintln!("Missing --y2");
        std::process::exit(1);
    });

    let times: u8 = args
        .iter()
        .position(|a| a == "--times")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);

    let zone = Zone::rect(x1, y1, x2, y2);
    let cmd = ZoneCleanCommand {
        clean_times: times,
        zones: vec![zone],
    };

    println!("Zone: ({x1}, {y1}) -> ({x2}, {y2})");
    println!("Clean times: {times}");
    println!("Encoded (hex): {:02x?}", cmd.encode());
    println!("Encoded (b64): {}", cmd.encode_base64());

    print!("Connecting to {}:{}... ", config.address, config.port);
    let mut vacuum = LocalXPlorer::connect(&config).unwrap_or_else(|e| {
        eprintln!("FAILED: {e}");
        std::process::exit(1);
    });
    println!("OK");

    print!("Sending zone clean command (cmd 0x28)... ");
    match vacuum.clean_zone(&cmd).await {
        Ok(()) => println!("OK"),
        Err(e) => {
            eprintln!("FAILED: {e}");
            std::process::exit(1);
        }
    }
}

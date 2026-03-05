//! Map restrictions — forbidden zones, no-sweep zones, and virtual walls.
//!
//! Forbidden zones (cmd 0x1a):
//!   cargo run --example forbidden_zone -- zone --x1 82 --y1 -13 --x2 453 --y2 203
//!   cargo run --example forbidden_zone -- zone --x1 82 --y1 -13 --x2 453 --y2 203 --mode nosweep
//!   cargo run --example forbidden_zone -- zone --x1 82 --y1 -13 --x2 453 --y2 203 --angle 30
//!   cargo run --example forbidden_zone -- clear-zones
//!
//! Virtual walls (cmd 0x12):
//!   cargo run --example forbidden_zone -- wall --x1 100 --y1 100 --x2 400 --y2 100
//!   cargo run --example forbidden_zone -- clear-walls
//!
//! Clear everything:
//!   cargo run --example forbidden_zone -- clear-all

use xplorer_rs::protocol::{ForbiddenMode, ForbiddenZone, Wall, Zone};
use xplorer_rs::{Device, DeviceConfig, LocalXPlorer};

fn parse_arg<T: std::str::FromStr>(args: &[String], flag: &str) -> Option<T> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}

fn require_coords(args: &[String]) -> (i16, i16, i16, i16) {
    let x1: i16 = parse_arg(args, "--x1").unwrap_or_else(|| {
        eprintln!("Missing --x1");
        std::process::exit(1);
    });
    let y1: i16 = parse_arg(args, "--y1").unwrap_or_else(|| {
        eprintln!("Missing --y1");
        std::process::exit(1);
    });
    let x2: i16 = parse_arg(args, "--x2").unwrap_or_else(|| {
        eprintln!("Missing --x2");
        std::process::exit(1);
    });
    let y2: i16 = parse_arg(args, "--y2").unwrap_or_else(|| {
        eprintln!("Missing --y2");
        std::process::exit(1);
    });
    (x1, y1, x2, y2)
}

const USAGE: &str = "\
Usage: forbidden_zone <command> [options]

Commands:
  zone         Set a forbidden/no-sweep zone (cmd 0x1a)
               --x1 X --y1 Y --x2 X --y2 Y [--mode ban|nosweep|nomop] [--angle DEG]
  wall         Set a virtual wall (cmd 0x12)
               --x1 X --y1 Y --x2 X --y2 Y
  clear-zones  Clear all forbidden zones
  clear-walls  Clear all virtual walls
  clear-all    Clear zones and walls";

#[tokio::main]
async fn main() {
    let config = DeviceConfig::from_env().unwrap_or_else(|e| {
        eprintln!("Missing env var: {e}");
        eprintln!("{USAGE}");
        std::process::exit(1);
    });

    let args: Vec<String> = std::env::args().collect();
    let subcommand = args.get(1).map(|s| s.as_str()).unwrap_or_else(|| {
        eprintln!("{USAGE}");
        std::process::exit(1);
    });

    print!("Connecting to {}:{}... ", config.address, config.port);
    let mut vacuum = LocalXPlorer::connect(&config).unwrap_or_else(|e| {
        eprintln!("FAILED: {e}");
        std::process::exit(1);
    });
    println!("OK");

    match subcommand {
        "zone" => {
            let (x1, y1, x2, y2) = require_coords(&args);
            let angle: f64 = parse_arg(&args, "--angle").unwrap_or(0.0);
            let mode_str: String = parse_arg(&args, "--mode").unwrap_or_else(|| "ban".to_string());
            let mode = match mode_str.as_str() {
                "ban" => ForbiddenMode::FullBan,
                "nosweep" => ForbiddenMode::NoSweep,
                "nomop" => ForbiddenMode::NoMop,
                other => {
                    eprintln!("Unknown mode: {other} (use: ban, nosweep, nomop)");
                    std::process::exit(1);
                }
            };

            let zone = if angle.abs() < f64::EPSILON {
                Zone::rect(x1, y1, x2, y2)
            } else {
                Zone::rotated_rect(x1, y1, x2, y2, angle)
            };

            println!("Forbidden zone ({mode_str}): ({x1},{y1}) -> ({x2},{y2}), angle: {angle}°");
            println!("Vertices: {:?}", zone.vertices);

            print!("Sending (cmd 0x1a)... ");
            match vacuum
                .set_forbidden_zones(&[ForbiddenZone { mode, zone }])
                .await
            {
                Ok(()) => println!("OK"),
                Err(e) => {
                    eprintln!("FAILED: {e}");
                    std::process::exit(1);
                }
            }
        }
        "wall" => {
            let (x1, y1, x2, y2) = require_coords(&args);
            println!("Virtual wall: ({x1},{y1}) -> ({x2},{y2})");

            print!("Sending (cmd 0x12)... ");
            match vacuum
                .set_virtual_walls(&[Wall {
                    start: (x1, y1),
                    end: (x2, y2),
                }])
                .await
            {
                Ok(()) => println!("OK"),
                Err(e) => {
                    eprintln!("FAILED: {e}");
                    std::process::exit(1);
                }
            }
        }
        "clear-zones" => {
            print!("Clearing all forbidden zones... ");
            match vacuum.clear_forbidden_zones().await {
                Ok(()) => println!("OK"),
                Err(e) => {
                    eprintln!("FAILED: {e}");
                    std::process::exit(1);
                }
            }
        }
        "clear-walls" => {
            print!("Clearing all virtual walls... ");
            match vacuum.clear_virtual_walls().await {
                Ok(()) => println!("OK"),
                Err(e) => {
                    eprintln!("FAILED: {e}");
                    std::process::exit(1);
                }
            }
        }
        "clear-all" => {
            print!("Clearing all forbidden zones... ");
            match vacuum.clear_forbidden_zones().await {
                Ok(()) => println!("OK"),
                Err(e) => {
                    eprintln!("FAILED: {e}");
                    std::process::exit(1);
                }
            }
            print!("Clearing all virtual walls... ");
            match vacuum.clear_virtual_walls().await {
                Ok(()) => println!("OK"),
                Err(e) => {
                    eprintln!("FAILED: {e}");
                    std::process::exit(1);
                }
            }
        }
        _ => {
            eprintln!("Unknown command: {subcommand}");
            eprintln!("{USAGE}");
            std::process::exit(1);
        }
    }
}

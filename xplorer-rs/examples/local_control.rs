//! Control the vacuum cleaner via local TCP — connect and send commands.
//!
//!   DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example local_control -- status
//!   DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example local_control -- power_on
//!   DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example local_control -- clean_rooms 0 2

use xplorer_rs::protocol::{GotoPointCommand, RoomCleanCommand};
use xplorer_rs::{Device, DeviceConfig, LocalXPlorer};

const USAGE: &str = "\
Usage: local_control <command> [args...]

Commands:
  status       Show device status
  power_on     Turn the vacuum on
  power_off    Turn the vacuum off
  go_home      Send to charging dock
  locate       Make the vacuum beep
  clean_rooms  Clean specific rooms (e.g. clean_rooms 0 2 5)
  goto_point   Go to a map point (e.g. goto_point 645 -651)

Env: DEVICE_IP, DEVICE_ID, LOCAL_KEY";

#[tokio::main]
async fn main() {
    let config = DeviceConfig::from_env().unwrap_or_else(|e| {
        eprintln!("Missing env var: {e}");
        eprintln!("{USAGE}");
        std::process::exit(1);
    });

    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(|s| s.as_str()).unwrap_or_else(|| {
        eprintln!("{USAGE}");
        std::process::exit(1);
    });

    print!("Connecting to {}:{}... ", config.address, config.port);
    let mut robot = LocalXPlorer::connect(&config).unwrap_or_else(|e| {
        eprintln!("FAILED: {e}");
        std::process::exit(1);
    });
    println!("OK");

    match command {
        "status" => {
            print!("Querying status... ");
            let state = robot.status().await.unwrap_or_else(|e| {
                eprintln!("FAILED: {e}");
                std::process::exit(1);
            });
            println!("OK\n");
            println!("  Status:      {}", state.status);
            println!("  Mode:        {}", state.mode);
            println!("  Battery:     {}%", state.battery);
            println!("  Suction:     {}", state.suction);
            println!("  Mop:         {}", state.mop);
            println!(
                "  Session:     {} m2, {} min",
                state.session.area_m2, state.session.time_minutes
            );
            println!(
                "  Fault:       {}",
                if state.fault == 0 {
                    "none".to_string()
                } else {
                    format!("code {}", state.fault)
                }
            );
            println!();
            println!("  Consumables:");
            println!(
                "    Side brush:  {} h remaining",
                state.side_brush.remaining_minutes / 60
            );
            println!(
                "    Main brush:  {} h remaining",
                state.main_brush.remaining_minutes / 60
            );
            println!(
                "    Filter:      {} h remaining",
                state.filter.remaining_minutes / 60
            );
            println!();
            println!(
                "  Lifetime:    {} m2, {} sessions, {} h",
                state.stats.total_area_m2,
                state.stats.total_sessions,
                state.stats.total_time_minutes / 60,
            );
        }
        "power_on" => {
            print!("Powering on... ");
            robot.power_on().await.unwrap_or_else(|e| {
                eprintln!("FAILED: {e}");
                std::process::exit(1);
            });
            println!("OK");
        }
        "power_off" => {
            print!("Powering off... ");
            robot.power_off().await.unwrap_or_else(|e| {
                eprintln!("FAILED: {e}");
                std::process::exit(1);
            });
            println!("OK");
        }
        "go_home" => {
            print!("Sending go home... ");
            robot.charge_go().await.unwrap_or_else(|e| {
                eprintln!("FAILED: {e}");
                std::process::exit(1);
            });
            println!("ACK (vacuum is returning to dock)");
        }
        "locate" => {
            print!("Sending locate... ");
            robot.locate().await.unwrap_or_else(|e| {
                eprintln!("FAILED: {e}");
                std::process::exit(1);
            });
            println!("ACK (vacuum should beep now)");
        }
        "clean_rooms" => {
            let room_ids: Vec<u8> = args[1..]
                .iter()
                .map(|s| {
                    s.parse::<u8>().unwrap_or_else(|_| {
                        eprintln!("Invalid room ID: {s} (expected a number 0-255)");
                        std::process::exit(1);
                    })
                })
                .collect();

            if room_ids.is_empty() {
                eprintln!("No room IDs specified.");
                eprintln!("Usage: local_control clean_rooms 0 2 5");
                std::process::exit(1);
            }

            let cmd = RoomCleanCommand {
                clean_times: 1,
                room_ids: room_ids.clone(),
            };

            print!("Cleaning rooms {:?}... ", room_ids);
            match robot.clean_rooms(&cmd).await {
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
        "goto_point" => {
            let x: i16 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or_else(|| {
                eprintln!("Usage: local_control goto_point <x> <y>");
                std::process::exit(1);
            });
            let y: i16 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or_else(|| {
                eprintln!("Usage: local_control goto_point <x> <y>");
                std::process::exit(1);
            });

            let cmd = GotoPointCommand { x, y };
            print!("Going to ({x}, {y})... ");
            robot.goto_point(&cmd).await.unwrap_or_else(|e| {
                eprintln!("FAILED: {e}");
                std::process::exit(1);
            });
            println!("ACK (robot navigating to target)");
        }
        _ => {
            eprintln!("Unknown command: {command}");
            eprintln!("{USAGE}");
            std::process::exit(1);
        }
    }
}

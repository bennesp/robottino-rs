//! Control the vacuum cleaner via the Tuya cloud API — no local network needed.
//!
//!   TUYA_EMAIL=... TUYA_PASSWORD=... TUYA_DEV_ID=... cargo run --example cloud_control --features cloud -- status
//!   TUYA_EMAIL=... TUYA_PASSWORD=... TUYA_DEV_ID=... cargo run --example cloud_control --features cloud -- power_on
//!   TUYA_EMAIL=... TUYA_PASSWORD=... TUYA_DEV_ID=... cargo run --example cloud_control --features cloud -- clean_rooms 0 2

use xplorer_rs::protocol::RoomCleanCommand;
use xplorer_rs::{xplorer_oem_credentials, CloudXPlorer, Device};

const USAGE: &str = "\
Usage: cloud_control <command> [args...]

Commands:
  status       Show device status
  power_on     Turn the vacuum on
  power_off    Turn the vacuum off
  go_home      Send to charging dock
  locate       Make the vacuum beep
  clean_rooms  Clean specific rooms (e.g. clean_rooms 0 2 5)

Env: TUYA_EMAIL, TUYA_PASSWORD, TUYA_DEV_ID";

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        eprintln!("Missing env var: {name}");
        eprintln!("{USAGE}");
        std::process::exit(1);
    })
}

#[tokio::main]
async fn main() {
    let email = env("TUYA_EMAIL");
    let password = env("TUYA_PASSWORD");
    let dev_id = env("TUYA_DEV_ID");

    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(|s| s.as_str()).unwrap_or_else(|| {
        eprintln!("{USAGE}");
        std::process::exit(1);
    });

    // Login
    let oem_creds = xplorer_oem_credentials("cd43f3353956c29131a9327dad5c84c2a93ebacaf16e");
    print!("Logging in as {email}... ");
    let mut robot =
        CloudXPlorer::login(oem_creds, &email, &password, &dev_id)
            .await
            .unwrap_or_else(|e| {
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
            println!("  Status:   {}", state.status);
            println!("  Mode:     {}", state.mode);
            println!("  Battery:  {}%", state.battery);
            println!("  Suction:  {}", state.suction);
            println!("  Mop:      {}", state.mop);
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
            print!("Sending home... ");
            robot.charge_go().await.unwrap_or_else(|e| {
                eprintln!("FAILED: {e}");
                std::process::exit(1);
            });
            println!("OK");
        }
        "locate" => {
            print!("Locating... ");
            robot.locate().await.unwrap_or_else(|e| {
                eprintln!("FAILED: {e}");
                std::process::exit(1);
            });
            println!("OK (vacuum should beep)");
        }
        "clean_rooms" => {
            let room_ids: Vec<u8> = args[1..]
                .iter()
                .map(|s| {
                    s.parse::<u8>().unwrap_or_else(|_| {
                        eprintln!("Invalid room ID: {s}");
                        std::process::exit(1);
                    })
                })
                .collect();

            if room_ids.is_empty() {
                eprintln!("No room IDs specified.");
                eprintln!("Usage: cloud_control clean_rooms 0 2 5");
                std::process::exit(1);
            }

            let cmd = RoomCleanCommand {
                clean_times: 1,
                room_ids: room_ids.clone(),
            };

            print!("Cleaning rooms {:?}... ", room_ids);
            robot.clean_rooms(&cmd).await.unwrap_or_else(|e| {
                eprintln!("FAILED: {e}");
                std::process::exit(1);
            });
            println!("OK");
        }
        _ => {
            eprintln!("Unknown command: {command}");
            eprintln!("{USAGE}");
            std::process::exit(1);
        }
    }
}

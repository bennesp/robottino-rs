//! What is the vacuum cleaner doing? — connect, query all DPS, show dashboard.
//!
//!   DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example robot_status

use tuya_rs::connection::DeviceConfig;
use xplorer_rs::device::{Device, XPlorer};

fn main() {
    let config = DeviceConfig::from_env().unwrap_or_else(|e| {
        eprintln!("Missing env var: {e}");
        eprintln!(
            "Usage: DEVICE_IP=... DEVICE_ID=... LOCAL_KEY=... cargo run --example robot_status"
        );
        std::process::exit(1);
    });

    print!("Connecting to {}:{}... ", config.address, config.port);
    let mut vacuum = XPlorer::connect(&config).unwrap_or_else(|e| {
        eprintln!("FAILED: {e}");
        std::process::exit(1);
    });
    println!("OK");

    print!("Querying status... ");
    let state = vacuum.status().unwrap_or_else(|e| {
        eprintln!("FAILED: {e}");
        std::process::exit(1);
    });
    println!("OK");

    println!();
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

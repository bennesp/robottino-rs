//! Discover devices via the Tuya cloud API — login, list homes, list devices.
//!
//! Prints device ID, local key, name, and product ID for each device.
//! The local key is what you need for local TCP control.
//!
//!   TUYA_EMAIL=... TUYA_PASSWORD=... cargo run --example cloud_discover --features cloud

use xplorer_rs::{cloud_discover, xplorer_oem_credentials};

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        eprintln!("Missing env var: {name}");
        eprintln!(
            "Usage: TUYA_EMAIL=... TUYA_PASSWORD=... cargo run --example cloud_discover --features cloud"
        );
        std::process::exit(1);
    })
}

#[tokio::main]
async fn main() {
    let email = env("TUYA_EMAIL");
    let password = env("TUYA_PASSWORD");

    let oem_creds = xplorer_oem_credentials("cd43f3353956c29131a9327dad5c84c2a93ebacaf16e");

    print!("Logging in as {email}... ");
    let results = cloud_discover(oem_creds, &email, &password)
        .await
        .unwrap_or_else(|e| {
            eprintln!("FAILED: {e}");
            std::process::exit(1);
        });
    println!("OK\n");

    let mut total = 0;
    for (home, devices) in &results {
        println!("Home: {} (gid={})", home.name, home.gid);

        if devices.is_empty() {
            println!("  (no devices)\n");
            continue;
        }

        for dev in devices {
            println!("  ID:        {}", dev.dev_id);
            println!("  Name:      {}", dev.name);
            println!("  Local Key: {}", dev.local_key);
            println!("  Product:   {}", dev.product_id);
            println!();
        }
        total += devices.len();
    }

    println!(
        "Total: {} device(s) across {} home(s)",
        total,
        results.len()
    );
}

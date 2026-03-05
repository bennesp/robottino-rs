//! Download and decode the vacuum cleaner's map — full cloud flow.
//!
//! Without credentials: decodes embedded testdata to show the result.
//! With credentials: logs in, gets storage config, downloads and decodes the live map.
//!
//!   cargo run --example download_map --features cloud
//!   TUYA_EMAIL=you@mail.com TUYA_PASSWORD=pass cargo run --example download_map --features cloud

use std::collections::HashMap;
use xplorer_rs::map::{LayoutMap, MapDecoder, PixelType, Route, TuyaMapDecoder};
use xplorer_rs::{CloudXPlorer, generate_presigned_url, xplorer_oem_credentials};

static EMBEDDED_LAY: &[u8] = include_bytes!("../testdata/lay.bin");
static EMBEDDED_ROU: &[u8] = include_bytes!("../testdata/rou.bin");

#[tokio::main]
async fn main() {
    let email = std::env::var("TUYA_EMAIL");
    let password = std::env::var("TUYA_PASSWORD");

    match (email, password) {
        (Ok(email), Ok(password)) => live_flow(&email, &password).await,
        _ => demo_flow(),
    }
}

/// Demo mode: decode embedded testdata to show what the output looks like.
fn demo_flow() {
    println!("No TUYA_EMAIL/TUYA_PASSWORD set — decoding embedded testdata\n");
    println!("The live flow would be:");
    println!("  1. login(email, password)    -> Session");
    println!("  2. storage_config(dev_id)    -> AWS credentials");
    println!("  3. generate_presigned_url()  -> signed S3 URL");
    println!("  4. HTTP GET lay.bin/rou.bin  -> raw bytes");
    println!("  5. decode_layout/route       -> map data\n");

    let decoder = TuyaMapDecoder;
    let layout = print_layout(&decoder, EMBEDDED_LAY);
    let route = print_route(&decoder, EMBEDDED_ROU);

    save_png(&layout, route.as_ref(), "map.png");
}

/// Live mode: full end-to-end cloud flow.
async fn live_flow(email: &str, password: &str) {
    let env = |name: &str| -> String {
        std::env::var(name).unwrap_or_else(|_| {
            eprintln!("Missing env var: {name}");
            eprintln!("Required: TUYA_DEV_ID (run discover example first)");
            std::process::exit(1);
        })
    };
    // For demo purposes, the device id here is hardcoded.
    // In a real implementation, you would generate one once and reuse it for all API calls.
    let oem_creds = xplorer_oem_credentials("cd43f3353956c29131a9327dad5c84c2a93ebacaf16e");
    let dev_id = env("TUYA_DEV_ID");

    // Step 1: Login
    print!("1. Logging in as {email}... ");
    let robot = CloudXPlorer::login(oem_creds, email, password, &dev_id)
        .await
        .expect("login failed");
    println!("OK");

    // Step 2: Get storage credentials
    print!("2. Getting storage config... ");
    let storage = robot.storage_config().await.expect("storage config failed");
    println!(
        "OK (bucket={}, expires={})",
        storage.bucket, storage.expiration
    );

    // Step 3: Generate pre-signed URLs
    let now = chrono_now();
    let layout_path = format!("{}/layout/lay.bin", storage.path_prefix);
    let route_path = format!("{}/route/rou.bin", storage.path_prefix);

    let layout_url = generate_presigned_url(
        &layout_path,
        &storage.ak,
        &storage.sk,
        &storage.token,
        &storage.bucket,
        &storage.region,
        &now,
        86400,
    );
    let route_url = generate_presigned_url(
        &route_path,
        &storage.ak,
        &storage.sk,
        &storage.token,
        &storage.bucket,
        &storage.region,
        &now,
        86400,
    );

    // Step 4: Download
    let client = reqwest::Client::new();
    print!("3. Downloading lay.bin... ");
    let lay_resp = client
        .get(&layout_url)
        .send()
        .await
        .expect("download failed");
    if !lay_resp.status().is_success() {
        eprintln!("HTTP {}", lay_resp.status());
        eprintln!("{}", lay_resp.text().await.unwrap_or_default());
        std::process::exit(1);
    }
    let lay_data = lay_resp.bytes().await.expect("read failed");
    println!("{} bytes", lay_data.len());

    print!("   Downloading rou.bin... ");
    let rou_resp = client
        .get(&route_url)
        .send()
        .await
        .expect("download failed");
    if !rou_resp.status().is_success() {
        eprintln!("HTTP {}", rou_resp.status());
        eprintln!("{}", rou_resp.text().await.unwrap_or_default());
        std::process::exit(1);
    }
    let rou_data = rou_resp.bytes().await.expect("read failed");
    println!("{} bytes", rou_data.len());

    // Step 5: Decode
    println!();
    let decoder = TuyaMapDecoder;
    let layout = print_layout(&decoder, &lay_data);
    let route = print_route(&decoder, &rou_data);

    save_png(&layout, route.as_ref(), "map.png");
}

fn print_layout(decoder: &TuyaMapDecoder, data: &[u8]) -> LayoutMap {
    let layout = decoder.decode_layout(data).expect("layout decode failed");
    let h = &layout.header;

    println!(
        "Layout: {}x{} px, {}cm/px, charger at ({}, {})",
        h.width, h.height, h.resolution, h.charge_x, h.charge_y
    );

    let mut room_counts: HashMap<u8, usize> = HashMap::new();
    let mut walls = 0usize;
    for px in &layout.pixels {
        match px {
            PixelType::Room(id) => *room_counts.entry(*id).or_default() += 1,
            PixelType::Wall => walls += 1,
            _ => {}
        }
    }

    println!("  Rooms:");
    for r in &layout.rooms {
        let pixel_val = if r.id == 0 { 0 } else { r.id * 4 };
        let px_count = room_counts.get(&pixel_val).copied().unwrap_or(0);
        let verts = if !r.vertices.is_empty() {
            format!(", {} vertices", r.vertices.len())
        } else {
            String::new()
        };
        println!(
            "    [{}] {} = {} px{}",
            r.id,
            r.name.as_deref().unwrap_or("?"),
            px_count,
            verts
        );
    }
    println!("  Walls: {walls} px");
    layout
}

fn print_route(decoder: &TuyaMapDecoder, data: &[u8]) -> Option<Route> {
    let route = decoder.decode_route(data).expect("route decode failed");
    println!("\nRoute: {} points", route.points.len());

    if route.points.len() < 2 {
        println!("  (no active route)");
        return None;
    }

    let first = route.points.first().unwrap();
    let last = route.points.last().unwrap();
    let min_x = route
        .points
        .iter()
        .map(|p| p.x)
        .fold(f32::INFINITY, f32::min);
    let max_x = route
        .points
        .iter()
        .map(|p| p.x)
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = route
        .points
        .iter()
        .map(|p| p.y)
        .fold(f32::INFINITY, f32::min);
    let max_y = route
        .points
        .iter()
        .map(|p| p.y)
        .fold(f32::NEG_INFINITY, f32::max);
    println!("  Bounding box: x=[{min_x:.1}, {max_x:.1}] y=[{min_y:.1}, {max_y:.1}]");
    println!(
        "  Start: ({:.1}, {:.1}), End: ({:.1}, {:.1})",
        first.x, first.y, last.x, last.y
    );
    Some(route)
}

fn save_png(layout: &LayoutMap, route: Option<&Route>, path: &str) {
    let png_data = layout
        .to_png_with_route(route)
        .expect("PNG rendering failed");
    std::fs::write(path, &png_data).expect("failed to write PNG");
    println!("\nSaved: {path} ({} bytes)", png_data.len());
}

fn chrono_now() -> String {
    use std::time::SystemTime;
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // Simple UTC date formatting: YYYYMMDD'T'HHMMSS'Z'
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let h = time_of_day / 3600;
    let m = (time_of_day % 3600) / 60;
    let s = time_of_day % 60;

    // Days since epoch to Y/M/D (simplified Gregorian)
    let mut y = 1970u64;
    let mut remaining = days;
    loop {
        let days_in_year =
            if y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400)) {
                366
            } else {
                365
            };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }
    let leap = y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400));
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut mo = 0usize;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining < md {
            mo = i + 1;
            break;
        }
        remaining -= md;
    }
    let d = remaining + 1;

    format!("{y:04}{mo:02}{d:02}T{h:02}{m:02}{s:02}Z")
}

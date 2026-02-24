use thiserror::Error;

/// Map decoder errors.
#[derive(Debug, Error)]
pub enum MapError {
    #[error("data too short for header: need {needed}, got {actual}")]
    HeaderTooShort { needed: usize, actual: usize },
    #[error("LZ4 decompression failed: {0}")]
    DecompressionFailed(String),
    #[error("pixel count mismatch: expected {expected}, got {actual}")]
    PixelCountMismatch { expected: usize, actual: usize },
    #[error("invalid format: {0}")]
    InvalidFormat(String),
    #[error("PNG rendering failed: {0}")]
    RenderFailed(String),
}

/// Read a big-endian u16 from `data[offset..offset+2]`.
fn read_u16_be(data: &[u8], offset: usize) -> u16 {
    ((data[offset] as u16) << 8) | data[offset + 1] as u16
}

// ── Layout ─────────────────────────────────────────────────

/// Layout file header (24 bytes = 12 x 2-byte BE pairs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutHeader {
    pub version: u8,
    pub map_id: u32,
    pub map_type: u8,
    pub width: u16,
    pub height: u16,
    pub origin_x: u16,
    pub origin_y: u16,
    pub resolution: u16,
    pub charge_x: u16,
    pub charge_y: u16,
    pub total_count: u32,
    pub compressed_length: u32,
}

impl LayoutHeader {
    pub fn parse(data: &[u8]) -> Result<Self, MapError> {
        if data.len() < 24 {
            return Err(MapError::HeaderTooShort {
                needed: 24,
                actual: data.len(),
            });
        }

        Ok(LayoutHeader {
            version: data[0],
            map_id: ((data[2] as u32) << 8) | data[1] as u32,
            map_type: data[3],
            width: read_u16_be(data, 4),
            height: read_u16_be(data, 6),
            origin_x: read_u16_be(data, 8),
            origin_y: read_u16_be(data, 10),
            resolution: read_u16_be(data, 12),
            charge_x: read_u16_be(data, 14),
            charge_y: read_u16_be(data, 16),
            total_count: read_u16_be(data, 20) as u32,
            compressed_length: read_u16_be(data, 22) as u32,
        })
    }
}

/// Pixel type in the layout map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PixelType {
    Outside,
    Wall,
    /// Room pixel. Value 0 = room_id 0 (default/first room), others = room_id * 4.
    Room(u8),
    Unknown(u8),
}

impl PixelType {
    pub fn from_byte(b: u8) -> Self {
        match b {
            0xFF => PixelType::Outside,
            0xF4 | 0xF9 => PixelType::Wall,
            v if v < 0xF4 => PixelType::Room(v),
            v => PixelType::Unknown(v),
        }
    }
}

/// Room metadata from the layout file.
#[derive(Debug, Clone, PartialEq)]
pub struct RoomMeta {
    pub id: u8,
    pub name: Option<String>,
    pub color: Option<u8>,
    pub fan: u8,
    pub water_level: u8,
    pub vertices: Vec<(u16, u16)>,
}

/// Decoded layout map.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutMap {
    pub header: LayoutHeader,
    pub pixels: Vec<PixelType>,
    pub rooms: Vec<RoomMeta>,
}

/// Default room color palette (RGB).
#[cfg(feature = "render")]
const ROOM_COLORS: &[[u8; 3]] = &[
    [76, 175, 80],  // green
    [33, 150, 243], // blue
    [255, 152, 0],  // orange
    [156, 39, 176], // purple
    [0, 188, 212],  // cyan
    [255, 87, 34],  // deep orange
    [139, 195, 74], // light green
    [233, 30, 99],  // pink
];

impl LayoutMap {
    /// Look up room metadata by pixel value.
    ///
    /// Pixel values encode room ID as `pixel = room_id * 4`.
    /// Room id=0 cannot be represented (pixel 0 = unexplored);
    /// use vertex data from the room metadata to draw its boundary.
    pub fn room_for_pixel(&self, pixel_value: u8) -> Option<&RoomMeta> {
        if pixel_value == 0 {
            // Pixel 0 = room_id 0 (first room, can't use *4 formula)
            return self.rooms.iter().find(|r| r.id == 0);
        }
        if !pixel_value.is_multiple_of(4) {
            return None;
        }
        let room_id = pixel_value / 4;
        self.rooms.iter().find(|r| r.id == room_id)
    }

    /// Render the layout map to a PNG image (rooms + walls + charger).
    #[cfg(feature = "render")]
    pub fn to_png(&self) -> Result<Vec<u8>, MapError> {
        self.to_png_with_route(None)
    }

    /// Render the layout map with an optional route overlay.
    ///
    /// The charger is marked as a red dot. If a route is provided,
    /// it is drawn on top as a white path.
    #[cfg(feature = "render")]
    pub fn to_png_with_route(&self, route: Option<&Route>) -> Result<Vec<u8>, MapError> {
        use image::{ImageBuffer, ImageEncoder, Rgb, codecs::png::PngEncoder};

        let w = self.header.width as u32;
        let h = self.header.height as u32;

        let mut img = ImageBuffer::from_fn(w, h, |x, y| {
            let px = &self.pixels[(x + y * w) as usize];
            match px {
                PixelType::Outside => Rgb([20u8, 20, 20]),
                PixelType::Wall => Rgb([50, 50, 50]),
                PixelType::Room(0) => Rgb(ROOM_COLORS[0]),
                PixelType::Room(v) => {
                    let idx = (*v as usize / 4) % ROOM_COLORS.len();
                    Rgb(ROOM_COLORS[idx])
                }
                PixelType::Unknown(_) => Rgb([100, 100, 100]),
            }
        });

        // Draw route as connected lines (Y is inverted: device Y-up, image Y-down)
        if let Some(route) = route {
            let origin_x = self.header.origin_x as f32 / 10.0;
            let origin_y = self.header.origin_y as f32 / 10.0;
            let color = Rgb([255u8, 255, 255]);

            let to_pixel = |p: &RoutePoint| -> (i32, i32) {
                ((p.x + origin_x) as i32, (origin_y - p.y) as i32)
            };

            for pair in route.points.windows(2) {
                let (x0, y0) = to_pixel(&pair[0]);
                let (x1, y1) = to_pixel(&pair[1]);
                draw_line(&mut img, x0, y0, x1, y1, color);
            }
        }

        // Draw charger (3x3 red dot)
        let cx = (self.header.charge_x / 10) as i32;
        let cy = (self.header.charge_y / 10) as i32;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let px = cx + dx;
                let py = cy + dy;
                if px >= 0 && py >= 0 && (px as u32) < w && (py as u32) < h {
                    img.put_pixel(px as u32, py as u32, Rgb([255, 40, 40]));
                }
            }
        }

        let mut buf = Vec::new();
        let encoder = PngEncoder::new(&mut buf);
        encoder
            .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgb8)
            .map_err(|e| MapError::RenderFailed(e.to_string()))?;
        Ok(buf)
    }
}

impl Route {
    /// Render the route as a standalone PNG (white path on black background).
    ///
    /// `layout_header` provides the map dimensions and origin for coordinate mapping.
    #[cfg(feature = "render")]
    pub fn to_png(&self, layout_header: &LayoutHeader) -> Result<Vec<u8>, MapError> {
        use image::{ImageBuffer, ImageEncoder, Rgb, codecs::png::PngEncoder};

        let w = layout_header.width as u32;
        let h = layout_header.height as u32;
        let origin_x = layout_header.origin_x as f32 / 10.0;
        let origin_y = layout_header.origin_y as f32 / 10.0;
        let color = Rgb([255u8, 255, 255]);

        let mut img: ImageBuffer<Rgb<u8>, _> = ImageBuffer::new(w, h);

        let to_pixel =
            |p: &RoutePoint| -> (i32, i32) { ((p.x + origin_x) as i32, (origin_y - p.y) as i32) };

        for pair in self.points.windows(2) {
            let (x0, y0) = to_pixel(&pair[0]);
            let (x1, y1) = to_pixel(&pair[1]);
            draw_line(&mut img, x0, y0, x1, y1, color);
        }

        let mut buf = Vec::new();
        let encoder = PngEncoder::new(&mut buf);
        encoder
            .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgb8)
            .map_err(|e| MapError::RenderFailed(e.to_string()))?;
        Ok(buf)
    }
}

/// Draw a line between two points using Bresenham's algorithm.
#[cfg(feature = "render")]
fn draw_line(
    img: &mut image::ImageBuffer<image::Rgb<u8>, Vec<u8>>,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: image::Rgb<u8>,
) {
    let w = img.width() as i32;
    let h = img.height() as i32;

    let mut plot = |x: i32, y: i32| {
        if x >= 0 && y >= 0 && x < w && y < h {
            img.put_pixel(x as u32, y as u32, color);
        }
    };

    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut x = x0;
    let mut y = y0;

    loop {
        plot(x, y);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

// ── Route ──────────────────────────────────────────────────

/// Route file header (13 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteHeader {
    pub version: u8,
    pub route_id: u16,
    pub force_update: bool,
    pub route_type: u8,
    pub total_count: u32,
    pub theta: u16,
    pub compressed_length: u16,
}

impl RouteHeader {
    pub fn parse(data: &[u8]) -> Result<Self, MapError> {
        if data.len() < 13 {
            return Err(MapError::HeaderTooShort {
                needed: 13,
                actual: data.len(),
            });
        }
        Ok(RouteHeader {
            version: data[0],
            route_id: read_u16_be(data, 1),
            force_update: data[3] != 0,
            route_type: data[4],
            total_count: ((data[5] as u32) << 24)
                | ((data[6] as u32) << 16)
                | ((data[7] as u32) << 8)
                | data[8] as u32,
            theta: read_u16_be(data, 9),
            compressed_length: read_u16_be(data, 11),
        })
    }
}

/// A single point in the cleaning route.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoutePoint {
    pub x: f32,
    pub y: f32,
}

impl RoutePoint {
    /// Decode from 4 raw bytes: [x_high, x_low, y_high, y_low].
    /// Values are signed 16-bit integers (two's complement), divided by 10.
    pub fn decode_bytes(bytes: [u8; 4]) -> Self {
        RoutePoint {
            x: read_u16_be(&bytes, 0) as i16 as f32 / 10.0,
            y: read_u16_be(&bytes, 2) as i16 as f32 / 10.0,
        }
    }
}

/// Decoded cleaning route.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub header: RouteHeader,
    pub points: Vec<RoutePoint>,
}

// ── MapDecoder trait ───────────────────────────────────────

pub trait MapDecoder {
    fn decode_layout(&self, data: &[u8]) -> Result<LayoutMap, MapError>;
    fn decode_route(&self, data: &[u8]) -> Result<Route, MapError>;
}

/// Concrete map decoder implementation.
pub struct TuyaMapDecoder;

impl MapDecoder for TuyaMapDecoder {
    fn decode_layout(&self, data: &[u8]) -> Result<LayoutMap, MapError> {
        let header = LayoutHeader::parse(data)?;

        let compressed_data = &data[24..];
        let decompressed = if header.compressed_length > 0 {
            lz4_flex::decompress(compressed_data, header.total_count as usize)
                .map_err(|e| MapError::DecompressionFailed(e.to_string()))?
        } else {
            compressed_data.to_vec()
        };

        let area = header.width as usize * header.height as usize;
        if decompressed.len() < area {
            return Err(MapError::PixelCountMismatch {
                expected: area,
                actual: decompressed.len(),
            });
        }

        let pixels: Vec<PixelType> = decompressed[..area]
            .iter()
            .map(|&b| PixelType::from_byte(b))
            .collect();

        let rooms = if decompressed.len() > area + 2 {
            parse_room_metadata(&decompressed[area..])
        } else {
            Vec::new()
        };

        Ok(LayoutMap {
            header,
            pixels,
            rooms,
        })
    }

    fn decode_route(&self, data: &[u8]) -> Result<Route, MapError> {
        let header = RouteHeader::parse(data)?;

        let point_data = if header.compressed_length > 0 {
            let compressed = &data[13..];
            lz4_flex::decompress(compressed, header.total_count as usize * 4)
                .map_err(|e| MapError::DecompressionFailed(e.to_string()))?
        } else {
            data[13..].to_vec()
        };

        let mut points = Vec::with_capacity(header.total_count as usize);
        for chunk in point_data.chunks_exact(4) {
            points.push(RoutePoint::decode_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3],
            ]));
        }

        Ok(Route { header, points })
    }
}

/// Parse room metadata from the bytes following the pixel data.
///
/// Format (from tuya-vacuum / @ray-js/robot-protocol):
///   [0]       version (u8)
///   [1]       room_count (u8)
///   Per room (47 fixed bytes + vertex_count * 4):
///     [0..2]    id (u16 BE)
///     [2..4]    order (u16 BE)
///     [4..6]    sweep_count (u16 BE)
///     [6..8]    mop_count (u16 BE)
///     [8]       color_order
///     [9..11]   sweep_forbidden, mop_forbidden
///     [11]      fan
///     [12]      water_level
///     [13]      y_mode
///     [14..26]  reserved (12 bytes)
///     [26]      name_length
///     [27..47]  name (20 bytes UTF-8, zero-padded)
///     [47]      vertex_count
///     [48..]    vertices (vertex_count * 4 bytes, u16 BE pairs)
fn parse_room_metadata(data: &[u8]) -> Vec<RoomMeta> {
    if data.len() < 2 {
        return Vec::new();
    }
    let room_count = data[1] as usize;
    let mut rooms = Vec::with_capacity(room_count);
    let mut pos = 2;

    for _ in 0..room_count {
        if pos + 47 > data.len() {
            break;
        }
        let id = data[pos + 1]; // low byte of u16 BE — room IDs fit in u8
        let color = data[pos + 8];
        let fan = data[pos + 11];
        let water_level = data[pos + 12];
        let name_len = data[pos + 26] as usize;
        let name = if name_len > 0 && name_len <= 20 {
            let name_bytes = &data[pos + 27..pos + 27 + name_len];
            String::from_utf8(name_bytes.to_vec()).ok()
        } else {
            None
        };
        let vertex_count = data[pos + 46] as usize;
        pos += 47;

        let mut vertices = Vec::with_capacity(vertex_count);
        for _ in 0..vertex_count {
            if pos + 4 > data.len() {
                break;
            }
            let x = read_u16_be(data, pos);
            let y = read_u16_be(data, pos + 2);
            vertices.push((x, y));
            pos += 4;
        }

        rooms.push(RoomMeta {
            id,
            name,
            color: Some(color),
            fan,
            water_level,
            vertices,
        });
    }

    rooms
}

#[cfg(test)]
mod tests {
    use super::*;

    static LAY_BIN: &[u8] = include_bytes!("../testdata/lay.bin");
    static ROU_BIN: &[u8] = include_bytes!("../testdata/rou.bin");

    // ── LayoutHeader ───────────────────────────────────────

    #[test]
    fn layout_header_parse() {
        let header = LayoutHeader::parse(LAY_BIN).unwrap();
        assert_eq!(header.version, 1);
        assert_eq!(header.map_id, 42);
        assert_eq!(header.map_type, 1);
        assert_eq!(header.width, 172);
        assert_eq!(header.height, 264);
        assert_eq!(header.origin_x, 680);
        assert_eq!(header.origin_y, 700);
        assert_eq!(header.resolution, 5);
        assert_eq!(header.charge_x, 620);
        assert_eq!(header.charge_y, 880);
        assert!(
            header.compressed_length > 0,
            "data should be LZ4-compressed"
        );
    }

    #[test]
    fn layout_header_too_short() {
        assert!(LayoutHeader::parse(&[0u8; 20]).is_err());
    }

    // ── PixelType ──────────────────────────────────────────

    #[test]
    fn pixel_type_from_byte() {
        assert_eq!(PixelType::from_byte(0x00), PixelType::Room(0));
        assert_eq!(PixelType::from_byte(0xFF), PixelType::Outside);
        assert_eq!(PixelType::from_byte(0xF4), PixelType::Wall);
        assert_eq!(PixelType::from_byte(0xF9), PixelType::Wall);
        assert_eq!(PixelType::from_byte(0x08), PixelType::Room(8));
    }

    // ── RouteHeader ────────────────────────────────────────

    #[test]
    fn route_header_parse() {
        let header = RouteHeader::parse(ROU_BIN).unwrap();
        assert_eq!(header.version, 0);
        assert_eq!(header.route_id, 100);
        assert!(!header.force_update);
        assert_eq!(header.route_type, 2);
        assert_eq!(header.total_count, 675);
        assert_eq!(header.theta, 270);
        assert_eq!(header.compressed_length, 0);
    }

    // ── RoutePoint ─────────────────────────────────────────

    #[test]
    fn route_point_negative_coords() {
        let p = RoutePoint::decode_bytes([0xFF, 0xD8, 0xFF, 0xBC]);
        assert!((p.x - (-4.0)).abs() < 0.01);
        assert!((p.y - (-6.8)).abs() < 0.01);
    }

    #[test]
    fn route_point_mixed_sign() {
        let p = RoutePoint::decode_bytes([0x00, 0x08, 0xFF, 0xEA]);
        assert!((p.x - 0.8).abs() < 0.01);
        assert!((p.y - (-2.2)).abs() < 0.01);
    }

    #[test]
    fn route_point_signed_boundary() {
        // Value exactly at 32769 should be negative
        // 32769 as i16 = -32767 → / 10 = -3276.7
        let p = RoutePoint::decode_bytes([0x80, 0x01, 0x00, 0x00]);
        assert!(p.x < 0.0);
    }

    #[test]
    fn route_point_exact_0x8000() {
        // 0x8000 = 32768 as u16 → -32768 as i16 → / 10 = -3276.8
        let p = RoutePoint::decode_bytes([0x80, 0x00, 0x80, 0x00]);
        assert!((p.x - (-3276.8)).abs() < 0.01);
        assert!((p.y - (-3276.8)).abs() < 0.01);
    }

    // ── Full decode: lay.bin ───────────────────────────────

    #[test]
    fn decode_layout_full() {
        let decoder = TuyaMapDecoder;
        let layout = decoder.decode_layout(LAY_BIN).unwrap();
        assert_eq!(layout.header.width, 172);
        assert_eq!(layout.header.height, 264);
        assert_eq!(layout.pixels.len(), 172 * 264);

        // Should have all pixel types present
        let outside = layout
            .pixels
            .iter()
            .filter(|p| **p == PixelType::Outside)
            .count();
        let walls = layout
            .pixels
            .iter()
            .filter(|p| matches!(p, PixelType::Wall))
            .count();
        let rooms = layout
            .pixels
            .iter()
            .filter(|p| matches!(p, PixelType::Room(_)))
            .count();
        assert!(outside > 0, "should have Outside pixels");
        assert!(walls > 0, "should have Wall pixels");
        assert!(rooms > 0, "should have Room pixels");
    }

    #[test]
    fn decode_layout_rooms() {
        let decoder = TuyaMapDecoder;
        let layout = decoder.decode_layout(LAY_BIN).unwrap();
        assert_eq!(layout.rooms.len(), 5);

        let names: Vec<_> = layout
            .rooms
            .iter()
            .map(|r| (r.id, r.name.as_deref().unwrap_or("?")))
            .collect();
        assert_eq!(
            names,
            vec![
                (0, "Room A"),
                (2, "Room B"),
                (3, "Room C"),
                (4, "Room D"),
                (5, "Hallway"),
            ]
        );

        // Hallway (id=5) has water_level=2
        let hallway = layout.rooms.iter().find(|r| r.id == 5).unwrap();
        assert_eq!(hallway.water_level, 2);

        // Room C (id=3) has fan=2, water_level=1
        let room_c = layout.rooms.iter().find(|r| r.id == 3).unwrap();
        assert_eq!(room_c.fan, 2);
        assert_eq!(room_c.water_level, 1);

        // All rooms have 4 vertices
        for room in &layout.rooms {
            assert_eq!(
                room.vertices.len(),
                4,
                "room {} should have 4 vertices",
                room.id
            );
        }

        // Pixel-to-room mapping: pixel = room_id * 4
        assert_eq!(
            layout.room_for_pixel(0).unwrap().name.as_deref(),
            Some("Room A")
        ); // id=0
        assert_eq!(
            layout.room_for_pixel(8).unwrap().name.as_deref(),
            Some("Room B")
        ); // id=2
        assert_eq!(
            layout.room_for_pixel(12).unwrap().name.as_deref(),
            Some("Room C")
        ); // id=3
        assert_eq!(
            layout.room_for_pixel(16).unwrap().name.as_deref(),
            Some("Room D")
        ); // id=4
        assert_eq!(
            layout.room_for_pixel(20).unwrap().name.as_deref(),
            Some("Hallway")
        ); // id=5
        assert!(layout.room_for_pixel(3).is_none());
    }

    // ── Full decode: rou.bin ───────────────────────────────

    #[test]
    fn decode_route_full() {
        let decoder = TuyaMapDecoder;
        let route = decoder.decode_route(ROU_BIN).unwrap();
        assert_eq!(route.points.len(), 675);

        // First point is (-45.0, 35.0) — x=-450/10, y=350/10
        assert!((route.points[0].x - (-45.0)).abs() < 0.01);
        assert!((route.points[0].y - 35.0).abs() < 0.01);

        // Route should have some negative x values (starts at x=-50)
        let has_negative_x = route.points.iter().any(|p| p.x < 0.0);
        assert!(has_negative_x, "route should include negative x coords");
    }
}

//! Builds the app icons from one square picture:
//!   cargo run --example make_icons -- <picture.png|webp>
//! Writes assets/icon.png (1024), assets/icon-256.png (window icon),
//! assets/icon.ico (Windows: 16…256 px) and docs/icon.png.
//! The tile is cut with a clean rounded square (superellipse), so the edge is
//! smooth and the outside is transparent.

use image::{imageops::FilterType, Rgba, RgbaImage};

fn main() {
    let input = std::env::args().nth(1).expect("usage: make_icons <picture>");
    let src = image::open(&input).expect("cannot open the picture").to_rgba8();
    let (w, h) = src.dimensions();

    // find the tile: rows/columns where most pixels are not (almost) black/transparent
    let transparent = src.pixels().filter(|p| p[3] < 10).count() as f32 / (w * h) as f32;
    let use_alpha = transparent > 0.03;
    println!("transparent share: {:.1}%", transparent * 100.0);
    let solid = |p: &Rgba<u8>| if use_alpha { p[3] > 128 } else { (p[0] as u32 + p[1] as u32 + p[2] as u32) > 12 };
    let row_count: Vec<u32> = (0..h).map(|y| (0..w).filter(|&x| solid(src.get_pixel(x, y))).count() as u32).collect();
    let col_count: Vec<u32> = (0..w).map(|x| (0..h).filter(|&y| solid(src.get_pixel(x, y))).count() as u32).collect();
    let rmax = *row_count.iter().max().unwrap_or(&1);
    let cmax = *col_count.iter().max().unwrap_or(&1);
    let top = row_count.iter().position(|&c| c > rmax * 6 / 10).unwrap_or(0) as f32;
    let bottom = (h as usize - 1 - row_count.iter().rev().position(|&c| c > rmax * 6 / 10).unwrap_or(0)) as f32;
    let left = col_count.iter().position(|&c| c > cmax * 6 / 10).unwrap_or(0) as f32;
    let right = (w as usize - 1 - col_count.iter().rev().position(|&c| c > cmax * 6 / 10).unwrap_or(0)) as f32;
    // keep it square and inset slightly to drop the ragged border
    let side = (right - left).min(bottom - top) * 0.95;
    let cx = (left + right) / 2.0;
    let cy = (top + bottom) / 2.0;
    println!("tile: x {left}..{right}, y {top}..{bottom}, side {side}");

    // crop a square around the tile with a small margin, then mask
    let margin = side * 0.04;
    let crop = side + margin * 2.0;
    let x0 = cx - crop / 2.0;
    let y0 = cy - crop / 2.0;
    let size = 1024u32;
    let mut out = RgbaImage::new(size, size);
    let half = side / 2.0;
    let n = 5.0f32; // superellipse exponent (iOS/macOS-like squircle)
    for oy in 0..size {
        for ox in 0..size {
            // sample the source (bilinear)
            let sx = x0 + (ox as f32 + 0.5) / size as f32 * crop;
            let sy = y0 + (oy as f32 + 0.5) / size as f32 * crop;
            let px = sample(&src, sx, sy);
            // superellipse coverage with ~1.5 px soft edge (in output pixels)
            let dx = ((sx - cx) / half).abs();
            let dy = ((sy - cy) / half).abs();
            let r = (dx.powf(n) + dy.powf(n)).powf(1.0 / n);
            let edge = 1.5 * crop / size as f32 / half;
            let cov = ((1.0 - r) / edge + 0.5).clamp(0.0, 1.0);
            let a = (px[3] as f32 * cov) as u8;
            out.put_pixel(ox, oy, Rgba([px[0], px[1], px[2], a]));
        }
    }
    std::fs::create_dir_all("assets").unwrap();
    std::fs::create_dir_all("docs").unwrap();
    out.save("assets/icon.png").unwrap();
    out.save("docs/icon.png").unwrap();
    image::imageops::resize(&out, 256, 256, FilterType::Lanczos3).save("assets/icon-256.png").unwrap();

    // Windows .ico with the usual sizes
    let frames: Vec<image::codecs::ico::IcoFrame> = [16u32, 24, 32, 48, 64, 128, 256]
        .iter()
        .map(|&s| {
            let img = image::imageops::resize(&out, s, s, FilterType::Lanczos3);
            image::codecs::ico::IcoFrame::as_png(img.as_raw(), s, s, image::ExtendedColorType::Rgba8).unwrap()
        })
        .collect();
    let f = std::fs::File::create("assets/icon.ico").unwrap();
    image::codecs::ico::IcoEncoder::new(f).encode_images(&frames).unwrap();
    println!("done: assets/icon.png, assets/icon-256.png, assets/icon.ico, docs/icon.png");
}

fn sample(img: &RgbaImage, x: f32, y: f32) -> [u8; 4] {
    let (w, h) = img.dimensions();
    let x = x.clamp(0.0, w as f32 - 1.001);
    let y = y.clamp(0.0, h as f32 - 1.001);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let g = |xx: u32, yy: u32| img.get_pixel(xx.min(w - 1), yy.min(h - 1)).0;
    let (a, b, c, d) = (g(x0, y0), g(x0 + 1, y0), g(x0, y0 + 1), g(x0 + 1, y0 + 1));
    let mut o = [0u8; 4];
    for i in 0..4 {
        let top = a[i] as f32 * (1.0 - fx) + b[i] as f32 * fx;
        let bot = c[i] as f32 * (1.0 - fx) + d[i] as f32 * fx;
        o[i] = (top * (1.0 - fy) + bot * fy).round() as u8;
    }
    o
}

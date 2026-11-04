//! Cloud preview: renders the procedural sky (`src/cloudgen.rs`) to a BMP
//! image, so cloud shapes can be reviewed without running the app.
//!
//! ```text
//! cargo run --bin clouds -- [preset|all] [seed] [palette] [moon phase 0-7]
//! ```
//!
//! `preset` is clear, puffs, scattered, broken, overcast, storm, or `all`
//! (every preset stacked top to bottom; the default). Defaults: seed 1,
//! palette vga, moon phase 4 (full). Writes
//! `target/cloud-preview/<preset>-<seed>-<palette>.bmp` at the app's
//! default 1080p size with 3-pixel cells, and prints the path.
//!
//! The image is composed the way the app will be: sky and stars, then the
//! moon, then each cloud layer back to front, composited in linear light
//! with dithered edges and the moon's light through the clouds.

use std::error::Error;
use std::path::PathBuf;

use starry_journey::cloudgen::{
    dithered_alpha, light_at, sky_fields, tone_index, tone_ramp, Light, LAYERS,
};
use starry_journey::clouds::{Cover, COVERS};
use starry_journey::endless::decode_srgb;
use starry_journey::model::grid::{CellScaling, Grid};
use starry_journey::model::palette::{palette_by_key, Palette};
use starry_journey::moon::{moon_sprite, MoonState, MOON_DIAMETER};
use starry_journey::raster::{generate_stars, CityRaster};
use starry_journey::station::StationFate;

/// Device pixels per cell, as the app's default.
const CELL: usize = 3;

fn encode(c: f64) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let v = if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (v * 255.0).round() as u8
}

fn linear(c: [u8; 3]) -> [f64; 3] {
    c.map(|v| decode_srgb(f64::from(v) / 255.0))
}

/// Renders one preset's sky as linear RGB cells, `cols x rows`.
fn render_sky(
    cover: Cover,
    seed: u32,
    palette: &Palette,
    phase_step: u64,
) -> (usize, usize, Vec<[f64; 3]>) {
    let grid = Grid::with_scaling(1920, 1080, true, CellScaling::Fixed(CELL as u32));
    let (cols, rows) = (grid.cols as usize, grid.ground_row as usize);
    let sky = CityRaster::render(&grid, &[], &generate_stars(seed, &grid), palette).rgba;
    let mut image: Vec<[f64; 3]> = (0..cols * rows)
        .map(|i| linear([sky[i * 4], sky[i * 4 + 1], sky[i * 4 + 2]]))
        .collect();

    // The moon, behind the clouds.
    let (mx, my) = (cols as f64 * 0.62, rows as f64 * 0.22);
    let phase = phase_step as f64 / 8.0;
    let moon = MoonState {
        x: mx as i32,
        row: my as i32,
        phase,
        exact_x: mx.floor(),
        exact_row: my.floor(),
        ..MoonState::default()
    };
    let sprite = moon_sprite(&moon, palette, StationFate::Survive);
    for (i, px) in sprite.rgba.chunks(4).enumerate() {
        if px[3] == 0 {
            continue;
        }
        let x = sprite.left as i64 + (i % sprite.size) as i64;
        let y = sprite.top as i64 + (i / sprite.size) as i64;
        if (0..cols as i64).contains(&x) && (0..rows as i64).contains(&y) {
            image[y as usize * cols + x as usize] = linear([px[0], px[1], px[2]]);
        }
    }
    let brightness = 1.0 - (phase - 0.5).abs() * 2.0;
    let light = Light {
        x: mx + 0.5,
        y: my + 0.5,
        radius: f64::from(MOON_DIAMETER) / 2.0,
        strength: brightness,
    };

    // Clouds, back (high) to front (low).
    let fields = sky_fields(cover, rows as i32, seed);
    let sky_color = linear(palette.bg.0);
    for (field, layer) in fields.iter().zip(LAYERS) {
        let ramp = tone_ramp(layer, cover, palette).map(|c| linear(c.0));
        // Each sub-layer separately, far to near, so nearer heaps overlap
        // farther ones with their own lit edges.
        for sub in (0..field.sublayers()).rev() {
            let haze = field.haze(sub);
            for row in 0..rows {
                for col in 0..cols {
                    let (x, y) = (col as f64 + 0.5, row as f64 + 0.5);
                    let density = field.sublayer_density(x, y, sub);
                    if density <= 0.0 {
                        continue;
                    }
                    let alpha = dithered_alpha(field.opacity(density), col as i32, row as i32);
                    if alpha <= 0.0 {
                        continue;
                    }
                    let lit = light_at(field, sub, x, y, x, Some(light));
                    let tone = ramp[tone_index(lit)];
                    let cell = &mut image[row * cols + col];
                    for k in 0..3 {
                        let hazy = tone[k] + (sky_color[k] - tone[k]) * haze;
                        cell[k] = hazy * alpha + cell[k] * (1.0 - alpha);
                    }
                }
            }
        }
    }
    (cols, rows, image)
}

/// Writes 24-bit BMP pixels (`width x height`, top row first).
fn write_bmp(path: &PathBuf, width: usize, height: usize, rgb: &[[u8; 3]]) -> std::io::Result<()> {
    let row_bytes = (width * 3).div_ceil(4) * 4;
    let size = 54 + row_bytes * height;
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    // BMP rows run bottom to top, pixels as BGR.
    for y in (0..height).rev() {
        let start = out.len();
        for px in &rgb[y * width..(y + 1) * width] {
            out.extend_from_slice(&[px[2], px[1], px[0]]);
        }
        out.resize(start + row_bytes, 0);
    }
    std::fs::write(path, out)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let which = args.first().map_or("all", String::as_str);
    let seed: u32 = args.get(1).map_or(Ok(1), |s| s.parse())?;
    let palette_key = args.get(2).map_or("vga", String::as_str);
    let phase: u64 = args.get(3).map_or(Ok(4), |s| s.parse())?;
    let palette =
        palette_by_key(palette_key).ok_or_else(|| format!("unknown palette '{palette_key}'"))?;
    let covers: Vec<Cover> = if which == "all" {
        COVERS.iter().skip(1).copied().collect()
    } else {
        vec![Cover::parse(which).map_err(|e| format!("preset {e}"))?]
    };

    // Each sky scaled up by CELL, stacked with a thin divider.
    let mut width = 0;
    let mut pixels: Vec<[u8; 3]> = Vec::new();
    for cover in &covers {
        let (cols, rows, image) = render_sky(*cover, seed, palette, phase % 8);
        width = cols * CELL;
        for row in 0..rows {
            let line: Vec<[u8; 3]> = (0..cols * CELL)
                .map(|px| image[row * cols + px / CELL].map(encode))
                .collect();
            for _ in 0..CELL {
                pixels.extend_from_slice(&line);
            }
        }
        pixels.extend(std::iter::repeat_n([60, 60, 60], width * 6));
        println!("{}", cover.label());
    }
    let height = pixels.len() / width.max(1);
    let dir = PathBuf::from("target/cloud-preview");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{which}-{seed}-{palette_key}.bmp"));
    write_bmp(&path, width, height, &pixels)?;
    println!("wrote {}", path.display());
    Ok(())
}

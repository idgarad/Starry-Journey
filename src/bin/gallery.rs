//! Building gallery (GitHub issue #2: "A gallery makes meaningful shape
//! differences visible at equal virtual-pixel scale").
//!
//! Usage: `cargo run --release --bin gallery -- [seed] [palette] [pixel size]`
//! (defaults: seed 1, the default palette, 3 screen pixels per cell).
//!
//! Draws every building kind with the real rasterizer, windows lit and
//! masts placed, in three labelled rows:
//!
//! 1. the 19 kinds shared with the Python reference at a typical width;
//! 2. the same 19 at their minimum widths, where shapes are tightest;
//! 3. the 9 Starry Journey extension kinds, kept separate.
//!
//! Writes `target/building-gallery/gallery-<seed>-<palette>.png` and prints
//! the path. Each building gets its own seed (the gallery seed plus its
//! position), so a kind looks the same however the rows change.

use std::error::Error;
use std::path::PathBuf;

use starry_journey::message::message_font;
use starry_journey::model::building::{Building, BuildingKind, KIND_WEIGHTS, SPEC_KIND_COUNT};
use starry_journey::model::grid::{CellScaling, Grid};
use starry_journey::model::mast::plan_mast;
use starry_journey::model::palette::{palette_by_key, Palette, BODY_COUNT, DEFAULT_PALETTE_KEY};
use starry_journey::model::rng::Mulberry32;
use starry_journey::model::shapes::build_tiers;
use starry_journey::model::windows::generate_windows;
use starry_journey::raster::CityRaster;
use starry_journey::snapshot::encode_png;

/// Typical footprint width for the first row (cells).
const TYPICAL_WIDTH: i32 = 13;
/// Target height for every building (cells); low-rise kinds stay lower.
const HEIGHT: i32 = 56;
/// Building height limit, leaving headroom for masts.
const MAX_H: i32 = 74;
/// Cells between buildings, and around each row.
const GAP: i32 = 6;
/// Label strip under each row, in screen pixels.
const LABEL_PX: u32 = 40;
/// Row title strip above each row, in screen pixels.
const TITLE_PX: u32 = 28;
/// Label text size in screen pixels.
const TEXT_PX: f32 = 13.0;

struct Row {
    title: &'static str,
    items: Vec<(BuildingKind, i32)>,
}

/// One building of `kind` and `width` at `x0`, built as the generator
/// builds it (tiers, mast, windows lit to their targets).
fn building(kind: BuildingKind, x0: i32, width: i32, seed: u32, index: usize) -> Building {
    let mut rng = Mulberry32::new(seed.wrapping_add(index as u32 * 7919));
    let kind = kind.fit_to_width(width);
    let (tiers, kind) = build_tiers(kind, x0, width, HEIGHT, MAX_H, &mut rng);
    let mast = plan_mast(kind, &tiers, MAX_H, &mut rng);
    let mut building = Building {
        layer: 3,
        x0,
        x1: x0 + width,
        kind,
        tiers,
        body_index: (index % BODY_COUNT) as u8,
        tone: rng.rand_int(0, 255) as u8,
        accent_roll: rng.next_f64(),
        windows: Default::default(),
        mast,
        decoration: None,
    };
    building.windows = generate_windows(&building, &mut rng);
    building.windows.light_to_target();
    building
}

/// Draws `text` centered on `cx` with its top at `y`, in `color`.
fn label(image: &mut [u8], width: u32, height: u32, text: &str, cx: f32, y: u32, color: [u8; 3]) {
    let font = message_font();
    let glyphs: Vec<_> = text.chars().map(|c| font.rasterize(c, TEXT_PX)).collect();
    let total: f32 = glyphs.iter().map(|(m, _)| m.advance_width).sum();
    let mut pen = cx - total / 2.0;
    let baseline = y as f32 + TEXT_PX;
    for (metrics, coverage) in glyphs {
        let left = (pen + metrics.xmin as f32).round() as i32;
        let top = (baseline - metrics.height as f32 - metrics.ymin as f32).round() as i32;
        for gy in 0..metrics.height {
            for gx in 0..metrics.width {
                let a = f32::from(coverage[gy * metrics.width + gx]) / 255.0;
                let (px, py) = (left + gx as i32, top + gy as i32);
                if a <= 0.0 || px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                    continue;
                }
                let at = (py as usize * width as usize + px as usize) * 4;
                for c in 0..3 {
                    let old = f32::from(image[at + c]);
                    image[at + c] = (old + (f32::from(color[c]) - old) * a).round() as u8;
                }
            }
        }
        pen += metrics.advance_width;
    }
}

/// Renders one row: its buildings at screen scale, with a title above and
/// a kind name and width under each building.
/// Width of `text` as drawn by [`label`], in screen pixels.
fn text_width(text: &str) -> f32 {
    let font = message_font();
    text.chars()
        .map(|c| font.metrics(c, TEXT_PX).advance_width)
        .sum()
}

/// Columns a building's slot takes: its width or its label's, whichever is
/// wider, plus the gap.
fn slot_cols(kind: BuildingKind, width: i32, scale: u32) -> i32 {
    let label = (text_width(&format!("{kind:?}")) / scale as f32).ceil() as i32 + 2;
    width.max(label) + GAP
}

fn render_row(
    row: &Row,
    palette: &Palette,
    seed: u32,
    scale: u32,
    row_index: usize,
) -> (u32, u32, Vec<u8>) {
    let cols = row
        .items
        .iter()
        .map(|&(kind, width)| slot_cols(kind, width, scale))
        .sum::<i32>()
        + GAP;
    let rows = MAX_H + 4;
    let grid = Grid::with_scaling(cols as u32, rows as u32, false, CellScaling::Fixed(1));
    let mut x = GAP;
    let mut buildings = Vec::new();
    let mut centers = Vec::new();
    for (i, &(kind, width)) in row.items.iter().enumerate() {
        let slot = slot_cols(kind, width, scale) - GAP;
        let left = x + (slot - width) / 2;
        buildings.push(building(kind, left, width, seed, row_index * 100 + i));
        centers.push((kind, width, left as f32 + width as f32 / 2.0));
        x += slot + GAP;
    }
    let raster = CityRaster::render(&grid, &buildings, &[], palette);
    let (width, city_h) = (grid.cols as u32 * scale, grid.rows as u32 * scale);
    let height = TITLE_PX + city_h + LABEL_PX;
    let mut image = vec![0u8; (width * height * 4) as usize];
    let bg = palette.bg.0;
    for pixel in image.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[bg[0], bg[1], bg[2], 255]);
    }
    for py in 0..city_h {
        for px in 0..width {
            let cell = ((py / scale) * grid.cols as u32 + px / scale) as usize * 4;
            let at = (((TITLE_PX + py) * width + px) * 4) as usize;
            image[at..at + 3].copy_from_slice(&raster.rgba[cell..cell + 3]);
        }
    }
    let text = palette.star.0;
    label(
        &mut image,
        width,
        height,
        row.title,
        width as f32 / 2.0,
        6,
        text,
    );
    for (kind, w, center) in centers {
        let y = TITLE_PX + city_h + 4;
        label(
            &mut image,
            width,
            height,
            &format!("{kind:?}"),
            center * scale as f32,
            y,
            text,
        );
        label(
            &mut image,
            width,
            height,
            &format!("{w}"),
            center * scale as f32,
            y + 16,
            text,
        );
    }
    (width, height, image)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let seed: u32 = args.first().map_or(Ok(1), |s| s.parse())?;
    let key = args.get(1).map_or(DEFAULT_PALETTE_KEY, String::as_str);
    let scale: u32 = args.get(2).map_or(Ok(3), |s| s.parse())?;
    if !(1..=8).contains(&scale) {
        return Err("pixel size must be 1 to 8".into());
    }
    let palette = palette_by_key(key).ok_or_else(|| format!("unknown palette '{key}'"))?;
    let shared: Vec<BuildingKind> = KIND_WEIGHTS[..SPEC_KIND_COUNT]
        .iter()
        .map(|(k, _)| *k)
        .collect();
    let extensions: Vec<BuildingKind> = KIND_WEIGHTS[SPEC_KIND_COUNT..]
        .iter()
        .map(|(k, _)| *k)
        .collect();
    let rows = [
        Row {
            title: "Shared with the Python reference (19), typical width",
            items: shared
                .iter()
                .map(|&k| (k, TYPICAL_WIDTH.max(k.min_width())))
                .collect(),
        },
        Row {
            title: "Shared kinds at their minimum widths",
            items: shared.iter().map(|&k| (k, k.min_width().max(3))).collect(),
        },
        Row {
            title: "Starry Journey extensions (9)",
            items: extensions
                .iter()
                .map(|&k| {
                    (
                        k,
                        k.own_width_range(3)
                            .map_or(TYPICAL_WIDTH.max(k.min_width()), |(lo, _)| lo as i32),
                    )
                })
                .collect(),
        },
    ];
    let images: Vec<(u32, u32, Vec<u8>)> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| render_row(row, palette, seed, scale, i))
        .collect();
    let width = images.iter().map(|(w, _, _)| *w).max().unwrap_or(1);
    let height: u32 = images.iter().map(|(_, h, _)| *h).sum();
    let bg = palette.bg.0;
    let mut sheet: Vec<u8> = [bg[0], bg[1], bg[2], 255].repeat((width * height) as usize);
    let mut top = 0;
    for (w, h, image) in &images {
        for y in 0..*h {
            let from = (y * w * 4) as usize;
            let to = (((top + y) * width) * 4) as usize;
            sheet[to..to + (*w * 4) as usize]
                .copy_from_slice(&image[from..from + (*w * 4) as usize]);
        }
        top += h;
    }
    let dir = PathBuf::from("target/building-gallery");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("gallery-{seed}-{key}.png"));
    std::fs::write(&path, encode_png(width, height, &sheet))?;
    println!("{}", path.display());
    Ok(())
}

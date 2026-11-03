//! Text preview of a whole generated Normal-mode city.
//!
//! Usage: `cargo run --bin city -- [columns] [rows] [seed]`
//! (defaults: 160 columns, 60 rows, seed 1).
//!
//! Draws the four layers far to near, so nearer buildings hide farther ones.
//! Legend: `.` `:` `+` `#` buildings in layers 0 (farthest) to 3 (nearest),
//! `o` a window that will be lit, `|` a mast, `*` landmark lights, space for sky.

use std::process::ExitCode;

use starry_journey::model::building::Building;
use starry_journey::model::city::generate_normal_city;
use starry_journey::model::grid::Grid;

const LAYER_CHARS: [char; 4] = ['.', ':', '+', '#'];

fn parse_arg(args: &[String], index: usize, default: i64, name: &str) -> Result<i64, String> {
    match args.get(index) {
        None => Ok(default),
        Some(text) => text
            .parse()
            .map_err(|_| format!("{name} must be a whole number, got '{text}'")),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let parsed = (|| {
        let cols = parse_arg(&args, 0, 160, "columns")?;
        let rows = parse_arg(&args, 1, 60, "rows")?;
        let seed = parse_arg(&args, 2, 1, "seed")?;
        if !(20..=1000).contains(&cols) || !(10..=240).contains(&rows) {
            return Err("columns must be 20..=1000 and rows 10..=240".to_string());
        }
        Ok((cols as u32, rows as u32, seed as u32))
    })();
    let (cols, rows, seed) = match parsed {
        Ok(values) => values,
        Err(message) => {
            eprintln!("{message}\nusage: cargo run --bin city -- [columns] [rows] [seed]");
            return ExitCode::from(2);
        }
    };

    // A 2-pixel cell on a (2*cols x 2*rows) surface gives exactly cols x rows.
    let grid = Grid::new(cols * 2, rows * 2, false);
    let city = generate_normal_city(seed, &grid);
    let mut canvas = vec![vec![' '; grid.cols as usize]; grid.rows as usize];
    for layer in 0..4 {
        for building in city.iter().filter(|b| b.layer == layer) {
            draw(&mut canvas, &grid, building);
        }
    }

    let landmarks = city.iter().filter(|b| b.decoration.is_some()).count();
    let masts = city.iter().filter(|b| b.mast.is_some()).count();
    println!(
        "seed {seed}: {} buildings, {landmarks} landmarks, {masts} masts  (layers . : + #, o lit window, | mast, * landmark)",
        city.len()
    );
    for row in canvas {
        println!("{}", String::from_iter(row).trim_end());
    }
    ExitCode::SUCCESS
}

fn draw(canvas: &mut [Vec<char>], grid: &Grid, building: &Building) {
    let mut put = |x: i32, h: i32, ch: char| {
        let row = grid.row_for_height(h);
        if (0..grid.rows).contains(&row) && (0..grid.cols).contains(&x) {
            canvas[row as usize][x as usize] = ch;
        }
    };
    let body = LAYER_CHARS[usize::from(building.layer)];
    for tier in &building.tiers {
        for h in tier.h0..tier.h1 {
            for x in tier.x0..tier.x1 {
                put(x, h, body);
            }
        }
    }
    for i in 0..building.windows.len() {
        if building.windows.target[i] {
            put(building.windows.x[i], building.windows.h[i], 'o');
        }
    }
    if let Some(mast) = building.mast {
        for h in mast.h0..mast.h1 {
            put(mast.x, h, '|');
        }
    }
    if let Some(decoration) = &building.decoration {
        for (x, h) in decoration.iter_points() {
            put(x, h, '*');
        }
    }
}

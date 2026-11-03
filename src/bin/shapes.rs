//! Text preview of every building shape.
//!
//! Usage: `cargo run --bin shapes -- [width] [height] [seed]`
//! (defaults: width 15, height 40, seed 1).
//!
//! Prints each kind side by side as text so shapes can be reviewed before the
//! rasterizer exists, with each building's generated windows drawn on top.
//! Legend: `#` wall, `o` window that will be lit, `_` window that stays
//! dark, `-` dark roof/spire/eave (no windows), `*` dense lit tier, `.` sky.

use std::process::ExitCode;

use starry_journey::model::building::{Building, WindowSet, KIND_WEIGHTS};
use starry_journey::model::rng::Mulberry32;
use starry_journey::model::shapes::{ascii_rows, build_tiers, MAX_SHAPE_WIDTH};
use starry_journey::model::windows::generate_windows;

/// Columns of output per row of previews before wrapping.
const LINE_WIDTH: usize = 120;

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
        let width = parse_arg(&args, 0, 15, "width")?;
        let height = parse_arg(&args, 1, 40, "height")?;
        let seed = parse_arg(&args, 2, 1, "seed")?;
        if !(1..=i64::from(MAX_SHAPE_WIDTH)).contains(&width) || !(1..=240).contains(&height) {
            return Err(format!(
                "width must be 1..={MAX_SHAPE_WIDTH} and height 1..=240"
            ));
        }
        Ok((width as i32, height as i32, seed as u32))
    })();
    let (width, height, seed) = match parsed {
        Ok(values) => values,
        Err(message) => {
            eprintln!("{message}\nusage: cargo run --bin shapes -- [width] [height] [seed]");
            return ExitCode::from(2);
        }
    };

    let columns = (LINE_WIDTH / (width as usize + 2)).max(1);
    let previews: Vec<(String, Vec<String>)> = KIND_WEIGHTS
        .iter()
        .map(|(kind, _)| {
            let mut rng = Mulberry32::new(seed);
            let (tiers, used) = build_tiers(*kind, 0, width, height, height, &mut rng);
            let label = if used == *kind {
                format!("{kind:?}")
            } else {
                format!("{kind:?}>Box")
            };
            let mut rows: Vec<Vec<char>> = ascii_rows(&tiers, 0, width, height)
                .iter()
                .map(|row| row.chars().collect())
                .collect();
            let building = Building {
                layer: 3,
                x0: 0,
                x1: width,
                kind: used,
                tiers,
                body_index: 0,
                tone: 0,
                accent_roll: 0.0,
                windows: WindowSet::default(),
                mast: None,
            };
            let windows = generate_windows(&building, &mut rng);
            for i in 0..windows.len() {
                let row = (height - 1 - windows.h[i]) as usize;
                rows[row][windows.x[i] as usize] = if windows.target[i] { 'o' } else { '_' };
            }
            (label, rows.into_iter().map(String::from_iter).collect())
        })
        .collect();

    println!(
        "width {width}, height {height}, seed {seed}  (# wall, o lit window, _ dark window, - roof, * dense tier)\n"
    );
    for group in previews.chunks(columns) {
        let cell = width as usize + 2;
        let labels: String = group
            .iter()
            .map(|(label, _)| format!("{:<cell$}", truncate(label, cell - 1)))
            .collect();
        println!("{}", labels.trim_end());
        for row in 0..height as usize {
            let line: String = group
                .iter()
                .map(|(_, rows)| format!("{:<cell$}", rows[row]))
                .collect();
            println!("{}", line.trim_end());
        }
        println!();
    }
    ExitCode::SUCCESS
}

fn truncate(text: &str, max: usize) -> &str {
    &text[..text.len().min(max)]
}

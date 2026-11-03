# Active work

## REBUILD-003: Normal-mode rendering

| Branch | Contents | Status |
| --- | --- | --- |
| `feature/normal-rasterizer` | CPU rasterizer; GPU presentation of the real city; `P` and `R` keys | Merged |
| `feature/normal-reveal` | 240-second cell-by-cell construction; speed keys; progress in the title | Merged |
| `feature/pixel-size-option` | `--pixel-size N` fixed virtual pixel size (option C) | Merged |
| `feature/default-pixel-size` | Fixed 3-pixel virtual pixels by default; `--pixel-size spec` for the spec formula | Merged |
| `feature/construction-timer` | `--timer`/`--deadline` duration clock; time left in the title | Merged |
| `feature/default-window-1080p` | Opens at 1920 × 1080 (testing size) | Merged |
| `feature/normal-ambience` | Window flicker after completion, beacon blinking, landmark color cycling, star twinkles, shooting stars | Planned |

### feature/default-window-1080p

- **Decision (owner, 2026-11-03):** the app opens at 1920 × 1080, the assumed screen size for testing. At the default 3-pixel cells that is a 640 × 360 grid.
- **Verification:** `cargo build` PASS; `cargo test` PASS (118 on main).

### feature/construction-timer

- **Request (owner, 2026-11-03):** a `--deadline` option so the city finishes in a set time, for testing (seconds) and for meeting screens ("the meeting starts in 10 minutes").
- **Delivered:**
  - [src/clock.rs](../src/clock.rs): `ConstructionClock::{Untimed, Duration}`. For a duration, progress is elapsed monotonic time over the duration, independent of speed.
  - `parse_duration` accepts a bare number as minutes, or forms like `90s`, `25m`, `1h30m`, from 1 s to 24 h.
  - `format_remaining` formats the time left, rounding up.
  - [src/config.rs](../src/config.rs): `--timer D`, with `--deadline D` as an alias. Only one may be given, and invalid input exits with code 2.
  - [src/main.rs](../src/main.rs): restarting resets the timer; resizing keeps it. With a timer the title shows `building N% - m:ss left`, and the speed keys have no effect (as the spec requires).
- **Verification (2026-11-03):**
  - `cargo test`: PASS, 118 tests. Parsing tests cover the forms, errors, and range. A 10-second timer is half built at 5 s and complete at 10 s at 0.25×, 1×, and 4×.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: clean.
  - CLI: `--deadline soon` exited 2, and `--help` lists both names.
  - GUI: not run by the agent, because the owner was using the machine. The owner can check with `cargo run -- --deadline 10s`.
- **Units:** a bare `--deadline 10` means 10 minutes, following the spec's `--timer`. Use `10s` for seconds.
- **Not yet:** `--ends-at TIME` (finish at a local clock time) needs a timezone dependency and arrives with the full launch configuration.

### feature/default-pixel-size

- **Decision (owner, 2026-11-03):** pixel size 3 is the default, measured on both of the owner's systems. Bigger windows and screens now show more city and sky at the same pixel size. This matches the owner's recollection of the Python version.
- **Delivered:** [src/config.rs](../src/config.rs) has `PixelSize::{Spec, Fixed(n)}` defaulting to `Fixed(3)` (`DEFAULT_PIXEL_SIZE`). `--pixel-size spec` (case-insensitive) restores the Rebuild Prompt formula of about 240 rows. The model's `Grid::new` keeps spec scaling as the library default.
- **Verification (2026-11-03):**
  - `cargo test`: PASS, 110 tests.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: clean.
  - CLI: `--pixel-size bogus` exited 2, and `--help` lists both forms.
  - GUI run with no options, maximized: `854x457 @ 3px`, surface 2560 × 1369.

### fix/maximize-rebuild

- **Report (owner, 2026-11-03):** maximize/minimize "scales the pixels up", while dragging a handle rebuilds the scene correctly.
- **Findings:**
  - In default (spec) mode this is the formula working as designed. Maximizing 1280 × 720 to 2560 × 1369 makes the window taller, so the cell goes from 3 to 6 (427 × 229 at 6 px). Dragging mostly changes width, so it adds city.
  - In one scripted run a maximize during startup left a stale, stretched frame. A resize event that arrives before the renderer exists is dropped. This could not be reproduced on demand, but the code path was real.
- **Fix:**
  - Every frame compares the window's real size with the configured surface and resizes if they differ, so a lost event can't leave a stretched frame.
  - Drawing is skipped while the window is minimized.
  - The title now shows the grid, cell size, and surface size, for example `854x457 @ 3px - surface 2560x1369`.
- **Verification (2026-11-03):**
  - `cargo test`: PASS, 109 tests.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: clean.
  - Scripted GUI runs:
    - default mode, maximize: `427x229 @ 6px` (spec behavior);
    - `--pixel-size 3`, maximize: `854x457 @ 3px`;
    - `--pixel-size 3`, minimize then restore: `427x240 @ 3px` with surface 1280 × 720 and construction progress kept.

### feature/pixel-size-option

- **Decision (owner, 2026-11-03):** option C. Spec scaling stays the default, and a launch option gives fixed-size virtual pixels, so bigger screens show more city.
- **Delivered:**
  - [src/model/grid.rs](../src/model/grid.rs): `CellScaling::{Spec, Fixed(n)}` and `Grid::with_scaling`. In fixed mode, `max_h` is computed from the smaller of the real ground row and the 240-row reference, so taller screens add sky instead of stretching towers. Widths are unchanged.
  - [src/config.rs](../src/config.rs): `parse_args` handles `--pixel-size N` (1–16, also `--pixel-size=N`) and `-h`/`--help`. Unknown options, bad values, and repeats are errors. `cell_scaling` multiplies N by the display scale factor, never below 1 pixel.
  - [src/main.rs](../src/main.rs): `--help` exits 0 and invalid options exit 2 with the usage text. The scene uses the chosen scaling, and `ScaleFactorChanged` (moving to a monitor with different scaling) updates it.
- **Verification (2026-11-03):**
  - `cargo test`: PASS, 109 tests. In fixed mode at 4, 1080p gives 480 × 270 and 4K gives 960 × 540, with `max_h` of 196 for both; a 1440p scene has 640 × 360 cells and over 1.5× the buildings of 720p.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: clean.
  - CLI: `--help` exited 0; `--pixel-size 0` and `--speed 2` were rejected, with exit 2 for the bad value.
  - GUI run with `--pixel-size 2` in a 1280 × 720 window: a 640 × 360 grid with more buildings and more sky, construction complete at 4×.

### feature/normal-reveal

- **Delivered:**
  - [src/reveal.rs](../src/reveal.rs):
    - `Reveal`: a seeded Fisher–Yates order over every owned cell (body, window, and mast). It shows `floor(f × total)` cells at progress `f` and all of them at `f ≥ 1`, and progress never decreases.
    - `untimed_step` advances progress by `dt × speed / 240`, with `dt` clamped to 0–0.25 s.
    - `light_revealed` turns on target-lit windows as their cells appear.
    - `ConstructionView` updates the shown image incrementally from a built-city image and a sky-only image, so each frame costs only the newly revealed cells.
  - [src/raster.rs](../src/raster.rs): `render_with(..., WindowLighting::Target)` paints the finished look without touching the `lit` flags.
  - [src/main.rs](../src/main.rs):
    - New cities start as empty sky and build up over 240 s at 1×.
    - `+`/`-` change speed from 0.25× to 4× in 0.25× steps.
    - The window title shows `building N% - Sx` or `complete`.
    - Resizing rebuilds with the same seed and keeps progress. `P` keeps the reveal; `R` restarts construction.
    - The texture is uploaded only when cells change.
- **Verification (2026-11-03):**
  - `cargo test`: PASS, 101 tests.
    - Every owned cell is revealed exactly once, and no sky cell is.
    - The order is seeded and shuffled.
    - Exact floor counts are shown at each progress.
    - Construction is complete at 240 s (1×) and 60 s (4×).
    - Lit windows match revealed targets.
    - The incremental view equals a rebuilt view.
    - The scene is half built at 120 s and equals the built image at the end.
    - Resize keeps progress.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: clean.
  - Windows GUI run: at 4× for 15 s the title read "building 25% - 4x", and the screenshot showed the city partly materialized cell by cell.
- **Assumptions:** AS-R007: the spec does not name the stream for the reveal order. It uses `seed XOR 0x5bd1e995`, separate from the city stream.
- **Not yet:** duration and deadline timers (`--timer`, `--ends-at`) arrive with launch configuration. Window flicker after completion arrives in `feature/normal-ambience`.

### feature/normal-rasterizer

- **Delivered:**
  - [src/raster.rs](../src/raster.rs): `CityRaster::render` builds a logical-grid RGBA buffer plus `owner`, `window_index`, and `depth` arrays. Ownership is resolved before any painting, so nearer layers hide farther ones and hidden windows never paint through. It draws:
    - the palette `bg` sky;
    - seeded stars from the ambient stream, `round(cols × groundRow × 0.0035)` of them with fade 0–0.6;
    - bodies faded toward `bg` by 1.5 × the layer fade;
    - lit windows in their tone color, faded toward the body by their shade (unlit windows are body-colored);
    - masts in the antenna color with a beacon tip;
    - landmark lights in the palette decor color, or HSL from hue and decor lightness.
  - [src/main.rs](../src/main.rs) and [src/present.wgsl](../src/present.wgsl): the window now shows the generated city.
    - The buffer is uploaded as an sRGB texture (one texel per cell), and the shader draws each cell as an exact `cell × cell` pixel block from the top-left.
    - Resizing rebuilds the city with the same seed. `P` cycles palettes without regenerating; `R` restarts with a new seed.
    - The SKY-001 gradient (`sky.wgsl`) and its 320 × 180 constants were removed. Its test `virtual_canvas_is_320_by_180` was removed with it, because it tested the superseded canvas. New tests cover the scene grid instead.
  - `App` now derives `Default`, which clears the long-standing clippy warning.
- **Verification (2026-11-03):**
  - `cargo test`: PASS, 90 tests.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: clean, with no warnings.
  - `cargo build`: PASS.
  - Windows GUI run: the app launched at 1280 × 720 (427 × 240 cells, cell 3), and screenshots were reviewed in Classic and ArtDeco (after pressing `P`). The window closed cleanly.
- **For now:** lights are steady. The reveal is in `feature/normal-reveal`.
- **Assumptions:**
  - AS-R005: an accent building's body moves 35% toward the palette accent color. The spec gives the accent and its chance but not how it is applied.
  - AS-R006: generated landmark colors use HSL saturation 80%.
  - Interpretation: a window's shade fades its lit color toward the building body, not the sky.
- **Tuning notes against the reference screenshot:**
  - Bodies are darker than in the reference, possibly because the ArtDeco material isn't built yet or because of the photo's exposure.
  - Lower-floor windows look dimmer than in the reference.

## REBUILD-002: Building generation (complete)

| Branch | Contents | Status |
| --- | --- | --- |
| `feature/building-records` | Building, tier, and window records; kind weights and width fallback; layer specs and height formula | Merged |
| `feature/tier-shapes` | Tier builders for all 19 kinds; text shape preview | Merged |
| `feature/expanded-buildings` | 9 extension kinds; wide footprints (see product.md) | Merged |
| `feature/masts-and-tints` | Masts and beacons; body-tint neighbor contrast | Merged |
| `feature/window-patterns` | Band, stripe, and grid patterns; shades; tone bytes | Merged |
| `feature/landmark-decorations` | Crown, band, spire, and roofline decorations | Merged |
| `feature/city-generator` | Layer cursors and generation; acceptance check 1 | Merged |

### feature/city-generator

- **Delivered:**
  - [src/model/city.rs](../src/model/city.rs):
    - `CityGenerator` gives each selected layer a cursor starting at `startX - randInt(0, 10)` and always extends the leftmost cursor first. For each building it picks, in order: kind, then width, height, tiers, tint, tone, accent roll, mast, windows, decoration, and gap.
    - Tint neighbors are pruned once they can no longer touch a new building.
    - `extend_to` continues the same city in steps; `prune_before` bounds memory for Endless mode.
    - `generate_normal_city` uses one stream for all four layers, starts with windows dark, then runs `ensure_landmarks` (1–3 landmarks if none) and `apply_cast_fade`. The cast fade adds 0.3 to windows with nearer geometry up to 3 columns to the left, level or above.
    - `endless_layer_generator` gives each layer its own seed (`seed XOR (0x85ebca6b + L)`), starts at x = -28, and creates windows lit to their targets.
  - [tests/city_model.rs](../tests/city_model.rs): acceptance check 1, ported and run against whole cities at 720p, 1080p, and 4K with 6 seeds each. It checks:
    - the same seed reproduces the city;
    - every building validates;
    - widths stay within limits (25 general, 50 low-rise);
    - layers never overlap and cover the screen;
    - low-rise kinds stay in the near layers;
    - fewer than 1% of touching same or adjacent-layer pairs share a color;
    - each city has at least 10 kinds, at least one landmark, and masts, with a varied lit ratio and shades;
    - Endless layers extend continuously and use independent streams.
  - [src/bin/city.rs](../src/bin/city.rs): text preview of a whole city, `cargo run --bin city -- [columns] [rows] [seed]`.
- **Verification (2026-11-03):**
  - `cargo test`: PASS, 80 tests.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: no new warnings.
  - City preview reviewed: seed 7 at 150 × 56 has 38 buildings, 3 landmarks, and 5 masts.
- **Interpretations:**
  - Ties between equal cursors go to the farther layer.
  - The cast fade applies to the quantized shade (level/15 + 0.3, capped at 0.9, then re-quantized), so it can differ from the original by at most one level.

### feature/landmark-decorations

- **Delivered:**
  - [src/model/building.rs](../src/model/building.rs):
    - `Decoration` record (outline kind, light mode, hue, and flattened `(x, h)` points) and the `DecorationKind`/`LightMode` enums.
    - The optional `Building::decoration` field.
    - Validation that every point lies on the building.
  - [src/model/decorations.rs](../src/model/decorations.rs):
    - `is_eligible`: the main tier, meaning the widest grounded tier, is at least 5 wide, and the roof reaches 40% of `max_h`.
    - `maybe_decorate`: a 6% roll on eligible buildings.
    - `make_decoration`, with four outlines:
      - crown: top row of the highest tier plus 2 rows of its sides;
      - band: one row at 55–80% of the main tier;
      - spire: center column of the highest tier;
      - roofline: the top cell of every covered column.
- **Verification (2026-11-03):**
  - `cargo test`: PASS, 66 tests.
    - Decorations validate for all 28 kinds over 30 seeds.
    - The measured decoration share is 5% to 7%.
    - All four outline kinds and both light modes occur.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: no new warnings.
- **Interpretation:** the spec lists crown, band, spire, and roofline without exact geometry. These outline definitions are this rebuild's choice and satisfy the spec's "outlines must lie on geometry" rule.

### feature/window-patterns

- **Delivered:**
  - [src/model/windows.rs](../src/model/windows.rs):
    - `choose_pattern`: bands 20%, stripes 22%, grid 58%, with the spec's pitches and target ratios.
    - `generate_windows`: places windows on every tier that isn't dark. Rules:
      - no windows on the ground row;
      - no windows on exposed top edges or exterior side columns of ordinary tiers 4 or more cells wide;
      - each cell belongs to the topmost tier covering it;
      - solid tiers get dense stripes on alternate rows, with a 92% target-on chance;
      - 12% of windows get their own tone.
    - `window_shade` follows the spec formula, with a 0.22 right-face fade on the rightmost 38%. `quantize_shade` gives 16 levels. `with_cast_fade` applies the standard-mode 0.3 cast fade, which the city generator will use.
  - [src/bin/shapes.rs](../src/bin/shapes.rs): the preview now draws generated windows (`o` lit, `_` stays dark).
- **Verification (2026-11-03):**
  - `cargo test`: PASS, 61 tests.
    - Measured pattern shares are within ±2% of 20/22/58.
    - The independent tone share is 10% to 13%.
    - Placement rules hold for all 28 kinds over 12 seeds, and every building still validates.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: no new warnings.
  - The preview was reviewed at width 14, height 26.
- **Interpretations:**
  - Solid tiers keep their top row lit, so bright crowns read at the roofline. Ordinary tiers reserve it.
  - Patterns are anchored to the footprint's left edge and the ground row, so floors line up across tiers. This makes the right-hand wall column one cell wider on some widths.

### feature/masts-and-tints

- **Delivered:**
  - [src/model/building.rs](../src/model/building.rs): `Mast` record (column, base, top, beacon period and phase), the optional `Building::mast` field, and mast validation. A mast must sit on the roof, stay inside the footprint, and stay under `max_h`.
  - [src/model/mast.rs](../src/model/mast.rs): `mast_rule` and `plan_mast`.
    - Needles and TV towers always get a mast.
    - Other kinds get one 30% of the time when tall.
    - Factories and all low-rise extension kinds never get one; the user approved this on 2026-11-03.
    - Masts are 4 to 16 cells, clipped to headroom, and centered on the highest tier. Beacon periods are 1.2 to 2.5 s, with a seeded phase.
  - [src/model/tint.rs](../src/model/tint.rs): `choose_body_index` with the spec's three-step relaxation. It first excludes touching buildings in the same, adjacent, and farther layers, then only the same and adjacent layers, then excludes nothing. Shared edges count as touching.
- **Verification (2026-11-03):**
  - `cargo test`: PASS, 55 tests.
    - The measured mast share for tall buildings is 27% to 33%.
    - Planned masts validate for every kind across 40 seeds.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: no new warnings.
- **Assumptions:**
  - AS-R003: the spec's "explicit tower kinds" are Needle and Tvtower. Spire already ends in its own spire.
  - AS-R004: "tall" means the roof reaches 50% of `max_h`.

### feature/expanded-buildings

- **Design decisions (user, 2026-11-03):**
  - All four proposed groups: 5-over-1; industrial low-rise; commercial low-rise; modern towers.
  - Occasional wide footprints.
  - New kinds at about 10% of picks.
  - Low-rise also in Normal mode's front layer.
- **Delivered:**
  - [src/model/building.rs](../src/model/building.rs): 9 new `BuildingKind`s (weights total 16; the spec's 143 are unchanged).
    - `is_extension`, `is_low_rise`, and `allowed_in_layer` decide which kinds may appear where.
    - `pick(rng, layer)` now chooses only among kinds allowed in that layer.
    - `own_width_range` gives each low-rise kind its own width range, up to 50.
  - [src/model/layers.rs](../src/model/layers.rs): `sample_footprint_width`. Low-rise kinds use their own range. Other buildings take a wide 19–25 footprint 15% of the time and otherwise use the layer's range, never above 25.
  - [src/model/shapes.rs](../src/model/shapes.rs): builders for the 9 new kinds.
- **Verification (2026-11-03):**
  - `cargo test`: PASS, 47 tests. The contract test now covers all 28 kinds × widths 1 to 50 (100,800 buildings).
  - Measured shares:
    - The wide-footprint share is between 13% and 17% (test bound).
    - Warehouses reach the 50-cell limit.
    - No low-rise kind is picked in layer 0.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: no new warnings.
  - Previews reviewed at width 34, height 22.

### feature/tier-shapes

- **Delivered:**
  - [src/model/shapes.rs](../src/model/shapes.rs): `build_tiers(kind, x0, width, height, max_h, rng)` covers all 19 kinds, with per-kind minimum heights and a fallback to box.
    - Roofs, eaves, domes, spires, and water tanks are `dark` (no windows).
    - Crowns, observation decks, and clock faces are `solid` (densely lit).
    - Shapes are behaviorally faithful: the original tier dimensions aren't available, so the builders follow the spec's shape descriptions and contracts.
  - [src/bin/shapes.rs](../src/bin/shapes.rs) previews every kind as text: `cargo run --bin shapes -- [width] [height] [seed]`.
- **Verification (2026-11-03):**
  - `cargo test`: PASS, 44 tests. The contract test builds 68,400 buildings (19 kinds × widths 1 to 50 × 12 heights × 6 seeds, including negative world x) and validates each one.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: no new warnings.
  - The preview was reviewed at width 13, height 30.

### feature/building-records

- **Delivered:**
  - [src/model/building.rs](../src/model/building.rs):
    - `BuildingKind`: the 19 kinds with the spec's weights (total 143), weighted `pick`, and minimum widths with a fallback to box.
    - `Tier`: a half-open rectangle with solid and dark flags.
    - `WindowSet`: parallel arrays of x, height, shade, tone, target, and lit.
    - `Building::validate`: checks that the footprint is non-empty, the layer and body index are in range, tiers stay inside the footprint and under `max_h`, every tier is grounded or rests on another tier, and every window lies inside a tier.
  - [src/model/layers.rs](../src/model/layers.rs): the four `LayerSpec`s, with width, gap, and height sampling. Height is `max(4, round(maxH × (base + span × r^power)))`, clamped to `max_h`.
- **Verification (2026-11-03):**
  - `cargo test`: PASS, 36 tests.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: no new warnings.
- **Assumptions:**
  - AS-R002: the spec says "generic setbacks require width 7" without naming the kinds. Stepped, spire, pointed, ziggurat, and crownblock are treated as generic setbacks; box has no minimum. This is non-blocking and easy to change in `BuildingKind::min_width`.
- **Deferred to later branches:** which kinds count as "explicit tower kinds" for masts (`feature/masts-and-tints`), and roof metadata (`feature/tier-shapes`; not needed, because roof height is derived from the tiers).

## REBUILD-001: Model foundation (RNG, grid, palettes)

- **Outcome:** Start the pure city model from [Rebuild Prompt.md](Rebuild%20Prompt.md) with no graphics dependencies: the random generator, logical grid math, and palette data.
- **Delivered:**
  - [src/model/rng.rs](../src/model/rng.rs): Mulberry32 with wrapping 32-bit arithmetic; `next_f64` and `rand_int` with the specified formulas; seed helpers for the ambient, layer, cloud, and tree-slot streams (negative slots supported); `js_round`, which is JavaScript-style `floor(x + 0.5)`.
  - [src/model/grid.rs](../src/model/grid.rs): `Grid::new(width, height, endless)`. Cell size is about device height ÷ 240, with columns and rows rounded up. The ground row reserves 7 road rows in Endless mode, and `max_h` follows the spec. Also includes row and screen-x conversions.
  - [src/model/palette.rs](../src/model/palette.rs): all 12 palettes in selector order, with weighted tone-byte → window color, layer tint targets and overrides, depth-fade constants, key lookup, and next-palette cycling.
  - [src/lib.rs](../src/lib.rs): a library crate holding the model, so the binary and tests share it.
- **Acceptance criteria:**
  - The RNG matches the canonical JavaScript Mulberry32 output for fixed seeds.
  - The grid matches the spec formulas at 1080p, 720p, 1366 × 768, 4K, and degenerate sizes.
  - The palette table matches the spec's JSON exactly, checked automatically.
  - The legacy `blackonwhite` key is labeled Ink on Parchment.
  - Tone mapping follows `(b + 0.5) / 256 × totalWeight`.
- **Verification (2026-11-03):**
  - `cargo fmt --check`: PASS.
  - `cargo test`: PASS, 24 tests. That is 21 model unit tests, the 2 existing binary tests, and 1 integration test.
  - The integration test [tests/palette_spec.rs](../tests/palette_spec.rs) parses the JSON block in the Rebuild Prompt and compares every palette field. A deliberately wrong lane color made it fail, which shows it catches transcription errors.
  - `cargo clippy --all-targets`: only the existing `derivable_impls` warning on `App` in `main.rs`; the new code is clean.
  - `cargo build`: PASS.
  - The RNG reference vectors came from an independent Python emulation of the JavaScript algorithm. Node.js is not installed, so they were not cross-checked against a real JavaScript engine. Seed 0's first value, 0.26643, matches the commonly published Mulberry32 output.
- **Dependencies:** `serde_json = "1"` as a dev-dependency only, used to parse the spec's palette JSON in tests. It is not part of the shipped app.
- **Assumptions:**
  - AS-R001: when an Endless surface has fewer than 7 rows, `ground_row` is clamped to 0 instead of going negative. The spec doesn't cover this, and the window's minimum size makes it unreachable.
- **Not done yet:** the app still shows SKY-001's 320 × 180 gradient. The model isn't wired into rendering until the Normal-mode rasterizer step.
- **Status:** Implemented and verified.

## Next candidates

1. Launch configuration: command-line parsing for `--speed`, `--palette`, `--message`, `--timer`, `--ends-at`, and `--endless`. `--ends-at` needs a local-timezone crate.
2. The Normal-mode rasterizer replacing the SKY-001 canvas: palette `bg` sky, building and window colors, stars, and the reveal.

## Superseded

### SKY-001: Virtual sky canvas

Implemented and verified on Windows at the default window size. It drew a fixed 320 × 180 canvas, nearest-neighbor scaled with a hard-coded gradient. The Rebuild Prompt, now the primary spec, replaces it with a window-derived grid and a palette `bg` sky. Its window, surface, and fullscreen/monitor code are kept.

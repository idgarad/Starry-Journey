# Active work

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

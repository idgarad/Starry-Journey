# Active work

## REBUILD-002: Building generation (split into feature branches)

| Branch | Contents | Status |
| --- | --- | --- |
| `feature/building-records` | Building, tier, and window records; kind weights and width fallback; layer specs and height formula | Merged |
| `feature/tier-shapes` | Tier builders for all 19 kinds | Planned |
| `feature/masts-and-tints` | Masts and beacons; body-tint neighbor contrast | Planned |
| `feature/window-patterns` | Band, stripe, and grid patterns; shades; tone bytes | Planned |
| `feature/landmark-decorations` | Crown, band, spire, and roofline decorations | Planned |
| `feature/city-generator` | Layer cursors and generation; acceptance check 1 | Planned |

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
- **Deferred to later branches:** which kinds count as "explicit tower kinds" for masts (`feature/masts-and-tints`), and roof metadata (`feature/tier-shapes`).

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

1. The remaining REBUILD-002 branches listed above.
2. Launch configuration: command-line parsing for `--speed`, `--palette`, `--message`, `--timer`, `--ends-at`, and `--endless`. `--ends-at` needs a local-timezone crate.
3. The Normal-mode rasterizer replacing the SKY-001 canvas.

## Superseded

### SKY-001: Virtual sky canvas

Implemented and verified on Windows at the default window size. It drew a fixed 320 × 180 canvas, nearest-neighbor scaled with a hard-coded gradient. The Rebuild Prompt, now the primary spec, replaces it with a window-derived grid and a palette `bg` sky. Its window, surface, and fullscreen/monitor code are kept.

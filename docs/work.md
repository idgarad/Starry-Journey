# Active work

## REBUILD-004: Endless mode

| Branch | Contents | Status |
| --- | --- | --- |
| `feature/endless-layers` | `--endless` and `E`; built cell by cell first, then four parallax layers scroll; bounded buffers; `Space` pause; startup quips | Merged |
| `feature/endless-compositor` | Layers and the moon placed at fractional device-pixel positions and blended in linear light on the GPU (smooth scrolling); one draw order; station radio calls; slower trench run | Merged (with `feature/endless-layers`) |
| `feature/endless-street` | Road, trees, and traffic scroll with the road camera at sub-pixel positions; cars glide as sprites; headlights keep lighting trees | Merged |
| `feature/clouds` | Puffs, banks, and decks; six coverage presets (`--clouds`, `C`); both modes; in front of the moon, behind the city; moonlit, four tones | In review |
| `feature/cloud-lights` | City glow on cloud undersides; rooftop searchlights with visible beams; sun as a light source (shown with the day/night cycle) | Planned |
| `feature/lightning` | Flashes inside clouds, cloud-to-cloud bolts, strikes to the tallest building; flash rate capped for photosensitive viewers | Planned |

### feature/clouds

- **Decisions (owner, 2026-11-03):**
  - Clouds need variety and density: occasional puffs, and heavy coverage for the rain and thunderstorms to come.
  - Clouds appear in both modes (the spec has them in Endless only).
  - Coverage presets are chosen with `--clouds` and cycled with `C`.
  - The moon sits behind the clouds.
  - Clouds are lit dynamically by the moon, the city's light, searchlights, the sun, and lightning, with "all the lights" tackled. Lightning comes in three kinds: inside a cloud, between clouds, and to the ground at the tallest building.
  - Split into three branches: this one (shapes, presets, motion, ambient and moonlight), then `feature/cloud-lights`, then `feature/lightning`.
- **Agent defaults (for the owner to tune):**
  - Default cover is scattered. It follows the spec: `max(3, ceil(cols / 130))` clouds, mostly banks of 38-90 x 6-13.
  - Four tones per cloud, between the palette's sky and star colors.
  - Edges are 55% opaque against open sky.
  - The wind is the spec's 0.35 cells/s. In Endless the clouds also follow a tenth of the camera, slower than the farthest city layer.
- **Measured coverage** (share of the whole sky down to the ground, 1080p at 3 px, mean of 8 seeds):

  | Preset | Coverage |
  | --- | --- |
  | Clear | 0% |
  | Puffs | 0.2% |
  | Scattered | 0.6% |
  | Broken | 8.0% |
  | Overcast | 23.9% |
  | Storm | 45.5% |
- **Delivered:**
  - [src/clouds.rs](../src/clouds.rs):
    - `Cover` (the six presets, with `next`, `label`, `key`, `parse`, and `cloud_count`) and `CloudKind`.
    - Shapes are stepped domes in two-row strips:
      - puffs: rounded, at most about half as tall as wide;
      - banks: a main dome giving the spec height, plus smaller ones;
      - decks: a tall body with wandering sides and a lumpy top. Deck height scales with the sky (25-40% for storms), so heavy skies stay heavy on any screen.
    - Domes are capped at 0.55 rows per cell of half-width, so they stay rounded.
    - `CloudField` holds the wind, Endless parallax, the `cols + 160` wrap span (with new shapes on wrap), a strip two columns wider than the screen on each side, and `coverage()`.
    - `cell_light`: the top two rows of each column are lit and the bottom two shaded. The moon adds a glow by phase that fades with the square of the distance within 2.2 radii (dimmer inside thick cloud), and a rim on the edges facing it.
    - `draw` snaps light to four tones. Edges are soft only against open sky.
  - [src/config.rs](../src/config.rs): `--clouds` (exit code 2 for an unknown preset).
  - [src/main.rs](../src/main.rs):
    - The renderer owns the `CloudField`, rebuilt with the scene; `C` cycles the cover and names it in the message box.
    - Clouds are redrawn only when they move a whole cell, a cloud wraps, or the moon moves a cell or changes phase. The station lap is a new moon, so it gives no light.
  - [src/present.wgsl](../src/present.wgsl): the cloud strip is drawn after the moon and before the city, slid by its fractional offset with linear-light reconstruction.
- **Tuning by looking (off-screen renders reviewed by the agent):**
  - The first version had tall arched "tombstone" decks, a thin hairline under each bank, banded undersides, bullseye rings around the moon, and storms nearly invisible on black.
  - Fixes:
    - flatter domes;
    - ragged deck sheets;
    - per-column shading;
    - a squared, shorter moon glow;
    - no bank slab;
    - solid edges where clouds overlap (they had drawn diagonal seams);
    - slightly brighter storm and overcast tones.
  - Overcast still reads as flat-topped decks rather than a full ceiling; it needs the owner's eye.
- **Verification (2026-11-03):**
  - `cargo test`: PASS, 238 tests. New tests:
    - preset cycling, parsing, and the spec count;
    - shapes are paired two-row strips within their size ranges, reach their top, and have flat bases;
    - puffs stay rounder than tall, and banks have no thin ledge;
    - edge depth;
    - coverage rises strictly from clear to storm, with storm over 40%;
    - clouds stay above the ground row;
    - wind rate and parallax;
    - an hour of 4x Endless wraps with new shapes, keeps the count, and leaves nothing behind;
    - determinism;
    - a full moon lights nearby clouds and a new moon doesn't; the moon-facing rim; undersides darker than tops;
    - four palette tones with translucent edges in all 12 palettes; storms darker than fair weather;
    - the strip offset;
    - `--clouds` parsing.

    The fixes above changed two of my own new tests: the bank-ledge test now checks for a ledge rather than any full-width base, and the deck height bound allows trimmed lumpy tops.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: clean.
  - GUI: not run by the agent. For the owner: `cargo run -- --clouds broken`, then press `C` to cycle; also try `--endless`.

### feature/endless-street

- **Goal:** the road, trees, and traffic move with the scrolling city. The spec: "Trees share the road camera, not vehicle speed."
- **Design:**
  - The road camera is the root camera, which is also the nearest layer's view (multiplier 1), so the road moves exactly with the nearest buildings.
  - The street is drawn in world columns into a strip from 2 columns left of the view to 2 past the right edge. The strip is rebuilt whenever the view crosses a whole column (2–8 times a second; cheap).
  - The GPU slides the strip by the remaining fraction with the same linear-light reconstruction as the layers, so the road glides.
  - In Normal mode and during construction the strip is the screen with no offset, which gives exactly the old picture.
  - Vehicles keep world positions and drive at their own speed relative to the road. They enter and leave relative to the moving view, so the upper lane's cars approach faster on screen and the lower lane's slower. Their cells snap to whole road cells, the same as in Normal mode.
  - Headlight glow on trees works in world columns, so it follows along.
- **Delivered:**
  - [src/street.rs](../src/street.rs):
    - `Span` (a world strip: origin, width, rows) with `index`, `position`, and `cells`.
    - `StreetLayer::for_span`; `new` is the screen span.
    - The `apply_*_lit` functions take a span.
    - Fix: the street now includes trees that straddle a strip's edge (clipped). `trees_in` returns only trees wholly inside a range, so trees used to pop in at strip edges; in Normal mode, half-trees now also appear at the screen edges.
  - [src/traffic.rs](../src/traffic.rs): `set_view` and `view_x`. Spawns and exits are relative to the view, and `paint_span` paints into a strip. Normal mode keeps view 0.
  - [src/main.rs](../src/main.rs):
    - `Scene::road_view`, `street_span_now`, `follow_road_camera` (called on scroll and when traffic starts), and `street_offset_px`.
    - The overlay buffer and texture are sized to the strip.
    - `Params.cell.w` carries the overlay offset.
  - [src/present.wgsl](../src/present.wgsl): the overlay is drawn with `sharp_sample` at its offset.
- **Verification (2026-11-03):**
  - `cargo test`: PASS, 219 tests. New tests:
    - strips at different origins agree on every world cell (this is what caught the tree-edge bug);
    - span index and position round-trip;
    - traffic keeps flowing past a view moving at 8 cells/s for 10 minutes: 6 vehicles always, none left behind, something on screen over 90% of the time;
    - spawns are relative to the view, and a NaN view is ignored;
    - in the app, the street strip and traffic follow the road camera after 30 s, the fractional offset stays within the margin, and the center dashes sit at world positions;
    - Normal mode keeps the screen span with no offset.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: clean.
  - GUI: reviewed by the owner, who moved on to clouds.
- **Smooth cars (owner request, 2026-11-03: "we should smooth the cars"):**
  - [src/traffic.rs](../src/traffic.rs) `Traffic::sprites` draws each vehicle into its own 13 × 2 sprite (the longest truck plus its beam), at its unrounded world position. Its beam and body are kept separate.
  - `beam_cells` and `body_cells` now define both the cell painter and the sprites, so they can't drift apart; a test checks that a sprite at a whole cell draws exactly what the painter draws, for both lanes and a truck.
  - The street is split into a back layer (road and background trees) and a front layer (foreground trees).
  - The shader draws, between those two layers, every vehicle's beam and then every vehicle's body, each at its fractional position with linear-light reconstruction. A follower's beam never covers the car ahead, as before.
  - Vehicle sprites live in one 13 × 24 atlas, uploaded when traffic moves. Each frame passes six slot offsets computed from the current road view.
  - Normal mode's cars glide too, since both modes share the draw path.
  - `cargo test`: PASS, 221 tests. Also new: the back and front street layers carry exactly the Normal colors, and the front layer holds exactly the foreground trees.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: clean.
  - GUI: not run by the agent.

### feature/endless-compositor

- **Report (owner, 2026-11-03):** "the scrolling is terrible", as it was in the Python version before sub-pixel positioning and "some light trick to prevent flickering".
- **Cause:** layers moved in whole cells (3 device pixels). The far layer at 0.4 cells/s stepped once every 2.5 s, and the four layers stepped at different moments.
- **Fix (the Rebuild Prompt's compositor):**
  - **Sub-pixel positions:** each layer is placed at its exact fractional device-pixel offset `(viewX - originX) × cell`. A device pixel takes the two pixels of the layer's sharp device-resolution image that it overlaps, weighted by overlap.
  - **The light trick:** texels are decoded from sRGB to linear light (the `Rgba8UnormSrgb` texture format decodes) and premultiplied by alpha, then interpolated, then composited far to near with "over". The result is encoded once by the sRGB surface. Interpolating encoded values instead darkens a moving edge whenever it sits between pixels, which reads as flicker.
- **Deviation (equivalent result):** the spec keeps a sharp device-resolution cache texture per layer. Here the shader computes the same values from the cell-resolution layer texture, because the cache's pixel `x` is just cell `floor(x / cell)`. This saves memory and avoids texture-size limits on wide surfaces (planned multi-monitor support): the cache at 1080p would be 4032 px wide, and over 8192 beyond about 4K wide.
- **Delivered:**
  - [src/present.wgsl](../src/present.wgsl):
    - The background texture holds the Normal/construction frame, or the sky, stars, and moon while Endless scrolls.
    - Four layer textures are reconstructed as above, followed by the overlay and the message box.
    - `Params` carries the four layer offsets and a layers-on flag.
  - [src/endless.rs](../src/endless.rs):
    - Layer buffers are transparent on sky.
    - `layer_image` exposes each buffer with a process-unique revision; rebases, palette changes, and new cities all get new revisions.
    - `source_offset_px` is computed in f64 from the buffer origin, so it stays precise however far the camera goes.
    - `reconstruct_row` is a CPU mirror of the shader math, used for tests.
  - [src/main.rs](../src/main.rs):
    - While scrolling, the cell-resolution frame is only the sky and moon (buildings hide the moon on the GPU now). The street, trees, and traffic go to a transparent overlay.
    - Layer textures are uploaded only when their revision changes, reusing storage when the size matches. Scrolling itself writes just the 96-byte uniform each frame.
    - The device requests the adapter's real `max_texture_dimension_2d`.
  - [src/street.rs](../src/street.rs): `mark_opaque` for overlay cells.
- **Verification (2026-11-03):**
  - `cargo test`: PASS, 210 tests. New tests:
    - whole-pixel offsets reproduce the sharp image exactly;
    - a grey cell moving across black in quarter-pixel steps keeps its linear-light brightness constant (to 1e-9), while the encoded-space alternative dims by more than 5% between pixels;
    - coverage at the buffer edges;
    - layer alpha (0 on sky, 255 on buildings) and revisions after a palette change, a rebase, and a new city;
    - fractional source offsets;
    - the scrolling background is sky only, and the overlay is opaque exactly on street cells, with Normal-mode colors.
  - The shader passes the existing WGSL validation test.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: clean.
  - GUI: not run by the agent. The GPU output is not compared against the CPU mirror automatically. For the owner: `cargo run -- --endless --deadline 20s`, then watch the far layers glide at `-` (0.25×) and at 4×.
- **Smooth moon (owner request, 2026-11-03: "we need to do the same for the moon"):**
  - `moon_at` now also keeps the unrounded `exact_x` and `exact_row`.
  - [src/moon.rs](../src/moon.rs) `moon_sprite` draws the moon, or the station with its battle, into its own small image: 41 × 41 cells for a plain moon, 161 × 161 on the station lap (the debris reaches about 76 cells out; a test checks a whole destroyed lap fits).
  - The shader places the sprite at the exact fractional x and y with the same linear-light reconstruction, now bilinear over the four overlapped source pixels.
  - The sprite is redrawn only on a new lap or phase (every frame on the station lap); otherwise each frame just moves it.
- **Radio calls (owner idea, 2026-11-03):** on a station lap that ends in destruction, the message box shows "Red-5 Going In" while the red ship flies the trench (5 s), then "Great shot, kid! That was one in a million!" from the climb away until 1 s after the blast (5.5 s). [src/station.rs](../src/station.rs) `radio_line` follows the same schedule as the ship. The calls use the status line, so they take priority over the Endless startup lines; a `--message` hides them (agent default, consistent with the quips). They work in both modes.
  - Owner feedback: the attack needs to be "at least twice as long", so Red-5 is slowed in the trench. The trench run went from 2.5 s to 5 s (the climb away stays 1.5 s, the fuse 3 s). `--station-fate destroy` now blows up about 15 s after opening, not 12 s, so the run still starts about 5 s in.
  - Tested: exact start and end times, the ship in the trench during the first call, no calls when the station survives, and the length limit.
- **One draw order for every mode:** sky and stars, then the moon, then the city, then the overlay (street, trees, traffic), then the message box. The city is the still city in Normal mode and during construction (transparent where nothing is built), or the four layers while Endless scrolls. Buildings hide the moon because they are drawn after it, which replaces the old CPU masking. The still city is recomposed only when construction reveals cells; the overlay only when traffic moves.
- **More verification (2026-11-03):**
  - `cargo test`: PASS, 214 tests. New tests:
    - the exact moon position moves continuously and rounds to the cell position;
    - the sprite equals the cell-painted moon at whole-cell positions;
    - the station sprite holds the whole battle;
    - a sprite gliding diagonally keeps constant linear-light brightness, and the 2D sampler matches the row form;
    - the still city is opaque exactly on built cells at 0%, 50%, and 100%;
    - the street overlay covers the horizon.
    
    Tests that used to read the single CPU frame now read `Scene::flattened` (sky, still city, overlay).
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: clean.
  - GUI: reviewed and approved by the owner ("perfect"), including the radio calls and the 5-second trench run.
- **Not yet:** the road and trees still stand still (`feature/endless-street`). The stars don't move, by design.

### feature/endless-layers

- **Decisions (owner, 2026-11-03):**
  - Roadside trees in every palette in Endless mode, as in Normal (the spec has them for Ink on Parchment only). They arrive with `feature/endless-street`.
  - The moon and the new-moon battle station appear in Endless mode too, unchanged.
  - Switching modes: `--endless` at launch, or `E` at any time.
  - Endless "should start like normal mode and once the city is complete then continue on": the city is built cell by cell over 240 s at 1× (or the timer), then scrolls.
  - While an Endless city is first building, the message box shows silly startup lines ("Calibrating zebra stripes", "Unpacking packaging materials", ...).
- **Agent defaults (non-blocking, easy to change):**
  - A quip changes every 4 s of real time, in a seeded shuffled order of 32 lines; all are shown before any repeats. Quips stop when the city is complete.
  - A `--message` hides the quips, so a meeting screen keeps its message. A palette flash shows over either.
  - `--endless` may be combined with `--timer`, which times the construction phase (the spec forbids the combination, but the spec has no Endless construction).
  - `E` rebuilds the city in the other mode with the same seed and keeps construction progress; the Endless camera starts at 0. Normal and Endless cities differ (Endless uses one generator per layer), so the buildings change when switching.
  - `Space` pauses scrolling and traffic in Endless mode only. The moon keeps real time, and construction continues.
- **Delivered:**
  - [src/endless.rs](../src/endless.rs): `EndlessCity` holds four layers, each with its own generator (`seed XOR (0x85ebca6b + L)`, start x -28) and a buffer `2 × cols + 64` wide starting at origin -4, generated through `origin + bufW + 32`.
    - The camera moves at `2 × speed` cells/s, using at most 1/30 s per frame. Layers follow at 0.2, 0.4, 0.65, and 1.
    - When a view would leave its buffer, the buffer is rebased to `floor(viewX) - 4`, generation continues ahead, buildings behind `origin - 4` are dropped, and the buffer is redrawn.
    - `initial_buildings` gives the buildings on screen at camera 0 for the construction phase.
    - `compose` draws the layers far to near over the sky at whole-cell positions and marks the building cells. It skips rows above `max_h`.
    - `with_camera` rebuilds at a camera position, so a resize keeps the scroll position.
  - [src/raster.rs](../src/raster.rs): `RasterOptions` (window state, world origin, buffer width, Endless tint) and `render_options`; `endless_body_color` adds the spec's 10% blend toward each layer's tint target. `CityRaster` has an `origin_x`.
  - [src/quips.rs](../src/quips.rs): the 32 lines, `quip_order` (a seeded Fisher–Yates), and `quip_at`.
  - [src/message.rs](../src/message.rs): `Notice` has a status line, shown only when there is no message and no flash.
  - [src/config.rs](../src/config.rs): `--endless`.
  - [src/main.rs](../src/main.rs):
    - An Endless `Scene` reveals the Endless buildings with their windows dark at first, using Endless body colors. On the completing frame it switches to the layers, which then scroll.
    - The composed layers are cached and recomposed only when a layer moves a whole column. A full compose measured about 10 ms at 1080p in a debug build, and Normal frames cost one copy.
    - `E`, `Space`, pause, and quips are wired in. The title shows `endless - building N%`, `scrolling Sx`, or `paused`.
    - Fix: `resize` built its comparison grid without the road rows, so every resize event rebuilt the scene even when the grid was unchanged.
- **Verification (2026-11-03):**
  - `cargo test`: PASS, 204 tests. They cover:
    - the 2-cells/s camera with parallax ratios at 1× and 4×;
    - the 1/30 s motion clamp, with negative and NaN frames ignored;
    - whole-column change reporting;
    - 2 hours at 4×, with buffers always covering the view and building count bounded;
    - a 20-minute scroll at 4× matching a city built directly at that camera (continuity across rebases);
    - the construction image equal to the first scrolling frame, pixel for pixel, plus the mask;
    - per-layer speeds;
    - palette change without movement;
    - the 10% layer tint.
    
    In the app: an Endless scene starts building with dark windows, doesn't move until complete, then scrolls seamlessly; a resize keeps the camera. Quip tests cover the permutation, the 4 s hold, no repeat before the list is exhausted, the length limit, and bad times. `Notice` priority is tested, and so is `--endless` parsing, including with `--timer`.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: clean.
  - GUI: not run by the agent. For the owner: `cargo run -- --endless --deadline 30s`, then watch the quips and the hand-off to scrolling; try `Space`, `E`, `P`, and `+`.
- **Not yet:** layers snap to whole cells (fixed by `feature/endless-compositor`; the owner called this scrolling "terrible"). The road and trees stay still while the layers move (cars still drive), until `feature/endless-street`. There are no clouds yet.

## REBUILD-003: Normal-mode rendering

| Branch | Contents | Status |
| --- | --- | --- |
| `feature/normal-rasterizer` | CPU rasterizer; GPU presentation of the real city; `P` and `R` keys | Merged |
| `feature/normal-reveal` | 240-second cell-by-cell construction; speed keys; progress in the title | Merged |
| `feature/pixel-size-option` | `--pixel-size N` fixed virtual pixel size (option C) | Merged |
| `feature/default-pixel-size` | Fixed 3-pixel virtual pixels by default; `--pixel-size spec` for the spec formula | Merged |
| `feature/construction-timer` | `--timer`/`--deadline` duration clock; time left in the title | Merged |
| `feature/default-window-1080p` | Opens at 1920 × 1080 (testing size) | Merged |
| `feature/normal-street` | Road and roadside trees in Normal mode; tree colors for every palette | Merged |
| `feature/moon` | 39-cell moon crossing in 10 minutes; phase fixed per lap, 8-phase cycle; `--moon-phase`; battle station with fighters, trench run, and `--station-fate` | Merged |
| `feature/night-traffic` | Cars and trucks after completion; headlights with road beams and tree lighting (top lane lights top trees, bottom lane bottom trees) | Merged |
| `feature/message-box` | `--message` box over the scene; palette name shown for 2 s on `P`, then the message returns | Merged |
| `feature/normal-ambience` | Window flicker after completion, beacon blinking, landmark color cycling, star twinkles, shooting stars | Planned |

### feature/message-box

- **Decisions (owner, 2026-11-03):**
  - Build the message box before Endless mode.
  - Reuse the box for the palette name: on a palette change it briefly shows the name, then restores the message. The agent chose 2 seconds, with no fade.
  - Text uses a font crate and a bundled font (rather than a hand-drawn pixel font or egui). The agent chose `fontdue` 0.9.5 (pure Rust; MIT/Apache-2.0/Zlib) and Noto Sans Regular (SIL OFL 1.1, 607 KB, from the Noto project).
  - The message is set at launch only (`--message`). Editing in the app waits for the native UI decision.
- **Delivered:**
  - [src/message.rs](../src/message.rs):
    - `parse_message` trims, treats blank text as no box, and rejects more than 240 UTF-16 units (supplementary characters count as 2).
    - `Notice` shows the message steadily; `flash` shows a palette name for `PALETTE_NOTICE_SECONDS` (2 s). A new flash restarts the timer.
    - `wrap_text` breaks at spaces and at newlines; words too long for a line break between characters, never before a combining mark or joiner.
    - `render_message_box` follows the spec's styling: centered at 50% / 42%, at most 640 CSS px wide with 16 px side clearance, palette `bg` at 75% opacity, `star` text, border halfway between them, lines centered. Box sizes are in CSS px times the scale factor: 24 px text, 18 × 10 px padding, 2 px border. Output is premultiplied sRGB RGBA.
  - [src/fonts/](../src/fonts/): `NotoSans-Regular.ttf` compiled into the binary with `include_bytes!`, and `NotoSans-OFL.txt`.
  - [src/config.rs](../src/config.rs): `--message TEXT` (also `--message=TEXT`), with an error and exit code 2 for repeats, a missing value, or too-long text. `LaunchOptions` is now `Clone` instead of `Copy`.
  - [src/main.rs](../src/main.rs) and [src/present.wgsl](../src/present.wgsl): a second, device-resolution texture holds the box. The shader blends it over the city in sRGB space, the way a browser blends a 75%-opacity box, and decodes the result for the sRGB surface. The box is re-rendered only when its text, palette, surface size, or scale changes. `P` flashes the palette label.
- **Verification (2026-11-03):**
  - `cargo test`: PASS, 185 tests. New tests cover trimming and blank text, literal markup, the 240-unit boundary with supplementary characters, flash and restore (with and without a message, and a re-flash), wrapping (spaces, newlines, long words, combining marks), the font's Latin/Greek/Cyrillic coverage, centering at 50% / 42%, palette colors and the 75% background in all 12 palettes, the border midpoint, the 640 px limit at 1× and 2×, side clearance on a 320 × 180 window, height clipping, and determinism. Config tests cover `--message` parsing and errors. The existing shader-validation test covers the updated WGSL.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: clean.
  - Box images rendered off-screen (ArtDeco, Ink on Parchment) were inspected by the agent: text is crisp, wrapping is centered, "Café" with a combining accent is placed correctly, and tags show literally.
  - GUI: reviewed and approved by the owner ("looks good").
- **Not yet:** in-app editing, scrolling for tall content, Amber phosphor message styling (display-effects step), and text shaping for complex scripts.

### feature/night-traffic

- **Decisions (owner, 2026-11-03):**
  - Cars drive by in Normal mode once the city is finished. The spec has no traffic in Normal mode.
  - "If this is at night, the car's headlights should be on": the owner felt the Python version missed this.
  - "The headlights should illuminate the trees as they approach."
    - Each headlight casts a cone up to 8 cells ahead, widening one row on each side every 2 cells.
    - Tree pixels inside it are tinted toward the headlight color (up to 45%), fading with distance ahead and with distance off the beam's row.
    - Nothing behind a car is lit, and overlapping cones take the brightest, never a sum.
    - `StreetLayer` now keeps background trees apart from the road so they can be lit.
    - Owner feedback: only the bottom trees were getting light. Each lane now lights its own verge: the top lane lights the top (background) trees and the bottom lane the bottom (foreground) trees.
    - The top lane's cone rises 0.75 rows per cell ahead, aiming at mid-tree at full reach. This is deliberately more than realistic.
- **Delivered:**
  - [src/traffic.rs](../src/traffic.rs): `Traffic`, following the spec's road rules:
    - two opposing lanes (upper drives left, lower drives right) starting at ground + 1 and ground + 4;
    - cars 4–5 cells and trucks 7–9 cells, with 20% trucks;
    - speeds of 9–18 cells/s times the speed setting, with motion steps clamped to 1/30 s;
    - 2–6 vehicles, `clamp(round(cols / 100), 2, 6)`, which is 6 at the default 640 columns.
    - Vehicles start off-screen and drive in staggered. Followers keep a 2-cell gap, so there is no overlap, and vehicles are replaced after leaving.
    - Each vehicle is 2 rows tall: a car has a cabin inset at both ends, and a truck a trailer roof with a lower cab front.
    - The headlight and taillight cells use the palette's colors. A beam fans out ahead over 4 cells (55%, 38%, 22%, 10% toward the headlight color), low row first, then both rows.
  - [src/street.rs](../src/street.rs): `apply_paint` is now public.
  - [src/main.rs](../src/main.rs): traffic starts when construction completes (immediately for a finished city after a resize), is cleared by `R`, steps every frame with the speed setting, and is drawn between the street's back layer (road and background trees) and front layer (foreground trees). `Scene` now remembers its palette.
- **Verification (2026-11-03):**
  - `cargo test`: PASS, 162 tests, including the lane-to-verge pairing (the lower lane never lights top trees and the upper lane never lights bottom trees), the upper cone centered mid-tree at full reach, and the tree glow: strongest at the bumper, fading to the reach limit, nothing behind, a widening cone weaker off-axis, left-driving cars lighting to their left, and in the app a trunk ahead of a parked car lit. They also check:
    - vehicle count by width;
    - starting off-screen in both lanes and arriving;
    - opposite directions and spec speeds;
    - no overlap over 2 minutes at 4×, with the count kept;
    - sizes and truck share within 17–23%;
    - clamped frames and speed scaling;
    - headlight front, taillight rear, and the beam ahead and spreading, for both directions and for trucks;
    - determinism;
    - in the app, no cars during construction and lit headlights in the frame afterwards.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: clean.
  - GUI: reviewed and approved by the owner ("so much better").
- **Assumptions:** AS-R009: one vehicle per 100 columns, 2 to 6. The spec says only "2..6 vehicles based on viewport width".

### feature/moon

- **Decisions (owner, 2026-11-03):**
  - A pass crosses the sky in 5 minutes.
  - Revised after the first version: "way bigger" (at least double), so the diameter is now 39 virtual pixels.
  - Also revised: the phase stays fixed for a whole lap and advances one step per lap, instead of changing during the pass.
  - The new-moon lap shows a battle station (owner idea): a grey sphere with an equatorial trench, panel bands, darker edges, and a concave dish (rim, interior, and bright focus) in the upper left.
  - The shadowed side was "a tad bright", so earthshine was reduced from 8% to 4%.
  - "Too fast": the moon now moves at half speed, so a lap takes 10 minutes and the full cycle 80 minutes.
  - The station is under attack (owner idea):
    - About 8 one-pixel fighters orbit it on tilted orbits, hidden when behind the station.
    - Each fighter lives 3–10 s, then explodes in a 0.6 s burst (flash, sparks, embers), and a new fighter takes its slot.
    - Each station lap has a 30% chance that the station is destroyed at a random point 35–65% of the way through the lap: a 0.25 s full-disk flash, then a fireball, an expanding shockwave, and 30 debris particles fading over 4 s. After that the sky is empty for the rest of the lap.
    - Destruction always follows a trench run (owner request: "give Red-5 credit where credit is due"):
      - a single two-cell ship in pure red `#ff2020`, whatever the palette, flies the equatorial trench left to right in 2.5 s;
      - it climbs away up and to the right in 1.5 s;
      - 3 s later the station explodes.
      - There is no trench run without destruction.
    - `--station-fate chance|destroy|survive` controls this (`destroy` blows up about 12 s after opening on the station lap). The battle is seeded per lap.
  - A `--moon-phase` option picks the opening phase, by name (`new` … `waning-crescent`) or step 0–7. Phases still advance each lap. `--moon-phase new` shows the station at launch.
  - The moon lives on the sky layer, and passing behind buildings is a feature. The sun in Endless mode will be handled the same way later.
- **Headroom at 1080p** (640 × 360 cells at 3 px): the top 162 rows (45%) are always clear of every possible building.
- **Delivered:**
  - [src/moon.rs](../src/moon.rs):
    - `moon_at` gives the position on a shallow arc. The moon rises behind the street on the left, peaks at 22% of screen height, and sets on the right.
    - The phase step is fixed per lap and runs through the 8-phase cycle: new, waxing crescent, first quarter, waxing gibbous, full, waning gibbous, last quarter, waning crescent. A cycle takes 40 minutes, and the app opens on a waxing crescent.
    - `moon_pixel` gives the disk (`dx² + dy² ≤ 19.5²`). Waxing moons are lit right of `cos θ` across each row, and waning moons left of `-cos θ`, with `θ = 2π · phase`. The seas are the 19-cell design scaled to 2 × 2 blocks.
    - `moon_cells` colors the lit side in the palette's star color, the seas 30% toward the sky, and the unlit side 8% earthshine.
  - [src/main.rs](../src/main.rs): the moon follows real time from app start and starts 30% into a pass, so a crescent is visible at launch. It is painted on cells that currently show sky, before the street, and recomputed only when its cell position or quantized phase changes. Palette change, resize, and restart redraw it.
- **Verification (2026-11-03):**
  - `cargo test`: PASS, 152 tests, including the trench run (trench row, edge to edge, climb away, gone before the blast, red in 5 palettes, absent when the station survives) and the battle: fighters stay near the station and explode, the battle is deterministic per lap, bursts go through their stages, the measured destruction share is 27–33% within its window, the destruction sequence ends in an empty sky, a surviving station stays under attack, and `--station-fate` parses correctly. Also the 10-minute lap, `--moon-phase` parsing (names, numbers, errors, repeats), the opening phase followed by per-lap advance, and that the station appears on the new-moon lap with the same silhouette as the moon, the dish in the upper left, the trench across the equator, and colors visible against the sky. They also check:
    - the 39-cell disk;
    - waxing on the right and waning on the left, with mirrored crescents;
    - the phase fixed within a lap, stepping each lap, and the cycle repeating every 8 laps;
    - rising hidden behind the street;
    - the top of the arc clearing every tower at the default 3 px;
    - the moon showing on sky but hidden behind buildings.
  - With `--pixel-size spec` (about 216 rows) the big moon can dip 1–2 rows behind a maximum-height tower at the top of its arc. This is accepted, because the moon lives on the sky layer.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: clean.
  - GUI: reviewed and approved by the owner ("looks good").
- **Test fixes during development:** a 25% start could hide the opening crescent behind the tallest possible tower, so the start moved to 30%. One test compared phases exactly across passes and now allows for floating-point rounding. Another test placed the moon where buildings covered it completely, and now searches for a partly covered height.

### feature/normal-street

- **Decisions (owner, 2026-11-03):**
  - The street and trees belong in Normal mode too. The spec has no road or trees there.
  - Trees appear in every palette.
  - Night trees were near-black at first, so each palette now has its own tree colors. The owner reviewed them: "trees look good".
- **Delivered:**
  - [src/model/trees.rs](../src/model/trees.rs): the spec's sprites, slot spacing (23 back, 29 front), and hashed per-slot seeds. Overlapping and negative queries agree. `tree_colors` uses per-palette colors, or the spec's parchment rule.
  - [src/model/palette.rs](../src/model/palette.rs): `TreePalette` gives each night palette leaves (3 tones), shade, and trunk, mostly reusing its own colors. A contrast test keeps every tree color at least 1.8:1 against the sky.
  - [src/street.rs](../src/street.rs): `StreetLayer` holds a 7-row road with curbs and 4-on/4-off center dashes. Background trees are rooted on the road's top edge; foreground trees are rooted on the bottom row and drawn after traffic.
  - [src/main.rs](../src/main.rs): the Normal grid reserves the road rows. Each frame draws the city view, then road and background trees, then foreground trees. The street is present from the start; only the city is revealed.
- **Verification (2026-11-03):**
  - `cargo test`: PASS, 129 tests.
  - `cargo fmt --check`: PASS.
  - `cargo clippy --all-targets`: clean.
  - The owner reviewed the result visually across palettes.
- **Git:** this was the first merge conflict. `main` (with the timer) was merged into this branch, and both sides had added an import and a test in the same spots. Both conflicts were resolved by keeping both sides.

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

**Endless mode is in progress (REBUILD-004 above).** The Rebuild Prompt's plan:
- four independent parallax layers;
- a sharp device-resolution cache per layer, reconstructed at fractional scroll in linear light;
- clouds, the road camera, and traffic;
- buffer extension and pruning.

Normal mode's remaining polish (`feature/normal-ambience`: window flicker, blinking beacons, landmark color cycling, star twinkles, shooting stars) is still planned.


1. Launch configuration: command-line parsing for `--speed`, `--palette`, `--message`, `--timer`, `--ends-at`, and `--endless`. `--ends-at` needs a local-timezone crate.
2. The Normal-mode rasterizer replacing the SKY-001 canvas: palette `bg` sky, building and window colors, stars, and the reveal.

## Superseded

### SKY-001: Virtual sky canvas

Implemented and verified on Windows at the default window size. It drew a fixed 320 × 180 canvas, nearest-neighbor scaled with a hard-coded gradient. The Rebuild Prompt, now the primary spec, replaces it with a window-derived grid and a palette `bg` sky. Its window, surface, and fullscreen/monitor code are kept.

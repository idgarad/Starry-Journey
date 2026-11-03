# Product intent

## Purpose

Starry Journey is a native Rust rebuild of the Python/browser *Starry Night* application: a pixel-art night-city screensaver and meeting/work-break display inspired by the general idea of After Dark's *Starry Night*. It uses original, seeded procedural art. It does not require proprietary original artwork or claim hardware-accurate emulation of historical displays.

## Authoritative specification

[Rebuild Prompt.md](Rebuild%20Prompt.md) is the primary behavioral specification. It is kept unchanged as a reference copy; its links point to files in the original Python repository, not this one. Its coordinates, numeric conventions, palette data, timing rules, effects, and acceptance checks apply unless this document records a deviation. This file summarizes that scope and records the Rust-specific choices.

| Rebuild choice | Value |
| --- | --- |
| Target language | Rust (wgpu for graphics, winit for windowing) |
| Target platforms | Windows, macOS, Linux |
| Runtime | Native desktop |
| Fidelity | Behaviorally faithful. Seed-exact output is not possible here because the original `city.js`/`main.js` source (tier builders and RNG call order) is not available in this workspace. |

## Modes

1. **Normal:** a fixed city assembled cell by cell in randomized order over 240 seconds at 1x speed, or over a requested duration or until a local-clock deadline. After completion, windows flicker, landmark lights animate, stars twinkle, and occasional shooting stars appear. No road, traffic, clouds, or trees.
2. **Endless:** a fully built city that scrolls forever in four independent parallax layers, with stepped clouds, a road, cars and trucks, and roadside trees only in the Ink on Parchment palette. Lights and stars are steady: no window flicker, blinking beacons, color-cycling landmarks, twinkles, or shooting stars.

## Capabilities

- **Pixel grid:** sized from the window, not fixed. Cell size is `max(2, round(deviceHeight / 240))`. Columns and rows fill the window, so there are no letterbox bars. Endless layers are drawn sharp at device resolution, then composited in linear light at fractional scroll positions to avoid shimmer.
- **Deterministic generation:** Mulberry32 with independent seeded streams, 19 weighted building kinds (including landmarks), masts and beacons, window patterns, body-tint contrast between neighbors, and landmark decorations.
- **12 live-switchable palettes,** in this order: Classic, ArtDeco, Ink on Parchment (legacy key `blackonwhite`), CGA Cyan/Magenta, CGA Green/Red, EGA, VGA, Commodore 64, Apple II, Game Boy, Amber Terminal, and Green Terminal. Exact color data is in the Rebuild Prompt.
- **Sky:** the palette's flat `bg` color, with seeded stars.
- **Palette materials and display effects:**
  - ArtDeco building sheen.
  - Game Boy cell grid.
  - Apple II and C64 color reconstruction.
  - CGA RGB stripes.
  - Amber scanlines and a phosphor message effect.
  - Green Terminal falling bitmap code.

  A Display Effects toggle controls the effects. Classic, EGA, VGA, and Ink on Parchment have no display effect.
- **Timing:** speed from 0.25 to 4. Untimed, duration (`--timer`), or absolute local-clock deadline (`--ends-at`) construction. Pause and reduced-motion handling.
- **Message:** optional literal on-screen text, at most 240 UTF-16 code units.
- **Controls:** speed, palette, restart, pause, renderer selection, Display Effects, and message editing. Keyboard: `P` cycles palettes, `R` restarts, `+`/`-` change speed, `Space` pauses or resumes Endless mode.
- **Launch presets:** Standard, Endless, Meeting, Meeting-at, and the custom MWF meeting message.

## Native platform deviations

These parts of the Rebuild Prompt are replaced or do not apply in a native build:

- **Browser host contract:** not applicable. This covers the loopback HTTP server, `/config.json`, security headers, the `/ready` token, `--port`, `--no-browser`, and `--close-after-load`. The native app reads its launch configuration directly from the command line.
- **Other command-line flags:** `--speed`, `--palette`, `--message`, `--timer`, `--ends-at`/`--end-time`, and `--endless` keep their documented meanings, validation, and exit code 2 for invalid input.
- **CSS and device pixels:** measurements given in CSS pixels or device pixel ratio use winit's window scale factor.
- **On-screen controls:** need a native UI. The approach, an immediate-mode UI library or a keyboard-only interface, is an open architectural decision.
- **Photo skyline:** dropped from the product. The Rebuild Prompt's optional image import (`--image`, image analysis, and its acceptance checks) is not implemented, so no image-decoding dependency is needed.
- **Expanded buildings (requested 2026-11-03):** more building kinds than the spec's 19, and wider footprints. General buildings may be up to 25 cells wide, and near-road low-rise buildings (factories, warehouses, 5-over-1 apartments, and other low odd shapes) up to 50. Confirmed design:
  - **New kinds (9):** 5-over-1 apartment, warehouse, Quonset hut, silos, big-box store, parking garage, tapered tower, cantilever, and terraced tower. Together they make up about 10% of picks in the nearest layer; the spec's 19 kinds keep their weights.
  - **Placement:** low-rise kinds appear only in the nearest layer, in both Normal and Endless modes. 5-over-1s may also appear one layer back. The three new towers may appear in any layer.
  - **Widths:** ordinary buildings keep the spec's per-layer ranges, but 15% roll a wide footprint of 19–25 cells. Low-rise kinds use their own ranges, up to 50 cells. Factories widen to 10–50 cells in the nearest layer only.
  - **Heights:** low-rise kinds choose their own low height and treat the layer's height sample only as an upper bound.
- **Fixed pixel size by default (owner decision, 2026-11-03):** every virtual pixel is 3 screen pixels (times display scaling), so bigger windows and screens get more city and more sky. Building heights use the 240-row reference so towers keep their proportions. `--pixel-size N` (1–16) picks another size, and `--pixel-size spec` restores the Rebuild Prompt formula of about 240 rows, which matches the reference screenshot's density.
- **Native additions:** multi-monitor fullscreen (`F` toggles fullscreen, `Tab` selects the next display, `Esc` quits).
- **Green Terminal glyphs:** require GNU Unifont 16.0.04 bitmaps, or another suitably licensed equivalent, shipped with their SIL OFL 1.1 notice. They are not yet in this repository.

## Future extensions (beyond the rebuild baseline)

These earlier Starry Journey ideas are not part of the Rebuild Prompt. They are deferred until the rebuild baseline is complete and each one is separately approved:

- Natural landscape below a wave-shaped city boundary, distant mountains and forests with palette-tinted depth layers.
- Weather (rain, wind) and seasonal variation.
- A moon and an automatic day/night cycle, including sunset sky gradients.
- An Atari palette.
- Roadside trees in palettes other than Ink on Parchment.
- Android as a later target.
- Near-side foreground layers (owner reference, 2026-11-03), drawn after traffic and overlapping the road and bottom edge, so the 240-row grid and road position are unchanged:
  - **Silhouettes:** bushes along the bottom edge and occasional framing trees, in a near-black color derived from the palette background, with little interior detail so they never compete with the city.
  - **Motion:** in Endless mode, faster parallax than the road (about 1.3 to 1.5 times road speed). Trees sway by shifting crown rows one cell at a time with a per-tree seeded phase.
  - **Street lamps:** warm lamps with a small glow along the road edge, colored from the palette's headlight color.
- A river with window reflections below the road. This needs rows below the road, which means raising the city and road, so it is a separate, larger layout change.
- Background music. The Rebuild Prompt specifies no sound, so this is a deliberate addition. Users supply their own tracks in `Sounds/Music/`, which is gitignored; the app ships no music. Playback needs an audio dependency and a decision on controls (mute, volume, track choice).

## Current increment status

SKY-001 built a fixed 320 × 180 canvas with a hard-coded blue gradient sky. Both conflict with the Rebuild Prompt, which requires a window-derived grid and a palette `bg` sky. The window, wgpu surface, and fullscreen/monitor handling were kept. The canvas and gradient were replaced by the Normal-mode rasterizer, which draws the real city in the window.

REBUILD-001 delivered the first part of step 1: the random generator and stream seeds, the window-derived grid math, and the 12 palettes. See [work.md](work.md).

Planned order, from the Rebuild Prompt:

1. Pure model and configuration.
2. Normal-mode rasterizer.
3. Timing and UI.
4. Endless reconstruction and traffic.
5. Display and material effects.
6. Parchment trees.
7. Platform lifecycle.

## Visual references

Reference images live in `.ai/` on the owner's machine only. They are gitignored and not part of the repository, so they are used for tuning but never distributed.

- **Fidelity target:** `.ai/starry-night-detailed-cityscape-reference.jpg`, a photo of the Python version running Endless mode with the ArtDeco palette. Its grid is the spec's (about 240 rows), so the Rust grid formula reproduces its pixel density; no rescaling is needed.
- `.ai/starry-night-cityscape-reference.png`.
- `.ai/starry-night-sunset-cityscape-reference.png`. The sunset sky belongs to the future day/night extension.
- Further mood references added on 2026-11-03 (`ref9.jpg`, `refimage7.bmp`, `reimage8.bmp`, and an `.avif`): simple blocks, sparse windows, antennas, a moon, clouds, and haze. Some are watermarked stock images; they must not be committed.

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
- **Shared baseline with the Python version (GitHub issue #2; owner decisions, 2026-11-05):** the 19 shared kinds keep the Python names, weights (total 143), and minimum widths. Their shapes follow the issue's descriptions; they need not match exactly, because the Python source is not available here.
  - Masts follow Python: spire, pointed, needle, TV tower, twin, onion, clock tower, and pagoda always get one; other buildings at least 45% of the height limit get one 30% of the time.
  - `--buildings baseline` generates only the shared kinds at the per-layer widths, for comparison; the extensions above stay the default.
  - `cargo run --bin gallery` draws every kind, with the extensions in their own row.
- **Fixed pixel size by default (owner decision, 2026-11-03):** every virtual pixel is 3 screen pixels (times display scaling), so bigger windows and screens get more city and more sky. Building heights use the 240-row reference so towers keep their proportions. `--pixel-size N` (1–16) picks another size, and `--pixel-size spec` restores the Rebuild Prompt formula of about 240 rows, which matches the reference screenshot's density.
- **Street and trees in Normal mode (owner decision, 2026-11-03):** Normal mode has the 7-row road and roadside trees, which the spec reserves for Endless mode. Trees appear in every palette. Night palettes use their own tree colors; Ink on Parchment keeps the spec's rule.
- **Moon (owner decisions, 2026-11-03):** a 39-pixel moon on the sky layer crosses the sky in 10 minutes on a shallow arc. Its phase is fixed for each lap and advances one step per lap through the 8 lunar phases (an 80-minute cycle). `--moon-phase` picks the opening phase. On the new-moon lap a battle-station easter egg crosses instead of a nearly invisible disk. Tiny fighters attack it and explode, and with a 30% chance per lap the station blows up (`--station-fate` controls this). Each destruction is preceded by a single red ship's trench run (always red, in every palette). It is an homage to Lucasfilm's Death Star design: fine for personal use, but consider making it opt-in before distributing builds or making the repository public. It is drawn behind the city and street, so buildings and trees pass in front of it (a feature). During the trench run and escape, the message box shows two short radio calls quoted from the film ("Red-5 Going In"; "Great shot, kid! That was one in a million!"), which are part of the same homage. The sun in a future Endless-mode day/night cycle will live on the same sky layer.
- **Traffic in Normal mode with headlights (owner decisions, 2026-11-03):** once the city is finished, cars and trucks drive along the road in Normal mode too. Since it is night, headlights are on and cast a short beam on the road ahead, lighting up roadside trees as cars approach; taillights glow at the rear.
- **Message box text (owner decision, 2026-11-03):** text is drawn with the `fontdue` rasterizer and a bundled Noto Sans Regular font (SIL OFL 1.1, notice shipped in `src/fonts/`), at device resolution over the scene, as the spec intends. There is no text shaping: Latin, Greek, and Cyrillic render well; characters the font lacks (emoji, CJK) show the font's missing-glyph box. The spec's scrolling for tall content is replaced by leaving out lines that don't fit the window, since the box isn't interactive yet. For now the message is set only at launch with `--message`; editing in the app waits for the native UI decision.
- **Palette name notice (owner decision, 2026-11-03):** pressing `P` shows the new palette's name in the message box for 2 seconds, then the launch message returns (or the box hides if there is none).
- **Endless mode (owner decisions, 2026-11-03):**
  - An Endless city is first built cell by cell like a Normal one, then scrolls. The spec starts Endless fully built. The construction uses the Endless layers' own buildings, so the hand-off is seamless. `--timer` may therefore be combined with `--endless`.
  - While it builds, the message box shows silly startup lines, unless a `--message` is set.
  - `E` switches between Normal and Endless at any time, in addition to `--endless`.
  - Roadside trees appear in every palette, and the moon and battle station appear, as in Normal mode.
- **Clouds (owner decisions, 2026-11-03 and 2026-11-04):**
  - Clouds are procedural: layered density fields (high streaks, low scud) and metaball cumulus heaps that fill the sky to the horizon with perspective and overlap in depth.
  - There are no outlines; edges are dithered and stay translucent, so the moon always shows faintly.
  - They appear in both modes, in front of the moon and behind the city.
  - Six coverage presets (clear, puffs, scattered, broken, overcast, storm) are chosen with `--clouds` and cycled with `C`.
  - Moonlight is self-shadowing and passes through thin cloud, following the Python version's GitHub issue #1.
  - The city's window glow lights cloud undersides.
  - Ground searchlights (`--searchlights`) put on occasional shows of two or three sweeping beams. A show lasts at most 2 minutes; shows are very rare in Endless. Beams pass in front of distant clouds and pool on the near clouds overhead.
  - Lightning in storms (`--lightning auto|off`): mostly flickers inside clouds, some cloud-to-cloud bolts, and rare strikes on the tallest point of the skyline. Pixel-art style: one-cell stair-step bolts with a glow ring, banded and dithered flash light, and fades in three brightness steps. Flashes are local, soft, and capped: events at least 4 s apart, never more than three flashes a second (WCAG 2.3.1 allows at most three).
  - The sun is planned with a day/night cycle.
- **Rain, snow, and wind (owner decisions, 2026-11-05):**
  - Precipitation falls in the foreground only, in four layers of slightly different size: farther layers are smaller, slower, fainter, and denser. Density per layer scales with how hard it falls (light in overcast, heavy in storms).
  - Showers come by chance with the cloud cover: every 10 minutes, a 10% chance under overcast and 60% under a storm (none under lighter cover). A shower eases in over a minute, holds 5 minutes, and eases out over a minute.
  - `--precip rain|snow|off` and `W` choose what falls.
  - The wind gusts, slants the fall (snow more than rain), and drives the clouds' drift.
  - Searchlights are never on while anything falls. Lightning comes only in rainstorms; snowstorms are quiet.
  - The cloud cover itself stays as set (`--clouds`, `C`).
- **Ambience in both modes (owner decision, 2026-11-05; the spec keeps Endless steady):** once the city is built, windows flicker (the spec's Poisson toggles at 0.02 per building per second times speed, steered toward each building's target), beacons blink on their own periods, landmark lights cycle or chase through hues, 4% of stars twinkle, and shooting stars cross (first after 4-12 s, then every 8-28 s). `Shift+S` sends one at once.
- **Special vehicles and swaying trees (owner decisions, 2026-11-05):**
  - Now and then a police car (about every 5-8 minutes) or a utility truck (about every 10-15) joins the traffic.
  - Police light bars alternate a warm and a cool color picked from the palette; utility trucks blink an amber beacon. They flash about once a second, under the photosensitive limit.
  - The flashers light nearby trees and the nearest buildings.
  - Tree canopies lean and rock with the wind.
  - `Shift+E` and `Shift+U` send a police car or utility truck at once, and `--events` fires test events at chosen moments.
- **Palette display effects (Rebuild Prompt; owner decisions, 2026-11-05):** a second pass applies each palette's monitor effect to the finished scene: Amber, Apple II, C64, and both CGAs get screen-fixed scanlines and their own color treatment, and Game Boy an LCD grid. The message box is drawn after the effect, so it stays clear. `D` or `--effects off` turns them off (the Game Boy grid stays). Owner additions (2026-11-05):
  - Game Boy LCD ghosting;
  - Amber and Green phosphor persistence, message-text bloom, and a slow, tight refresh beam sweeping down the screen every 15 seconds;
  - edge convergence error (chromatic aberration) on CGA, C64, and Apple II;
  - a vignette on every CRT palette;
  - a CRT power-on when switching into one;
  - scanlines on EGA and VGA (the spec leaves them unprocessed).
  - Green Terminal's falling code rain across the whole scene, with 157 GNU Unifont 16.0.04 glyphs.
  - Classic and Parchment stay unprocessed. Still to come: ArtDeco's sheen material and Amber's message animation.
- **Native additions:** multi-monitor fullscreen (`F` toggles fullscreen, `Tab` selects the next display, `Esc` quits).
- **Green Terminal glyphs:** 157 GNU Unifont 16.0.04 bitmaps compiled into the app ([src/rain_glyph_data.rs](../src/rain_glyph_data.rs)), shipped with their SIL OFL 1.1 notice in [src/fonts/unifont-OFL.txt](../src/fonts/unifont-OFL.txt).

## Future extensions (beyond the rebuild baseline)

These earlier Starry Journey ideas are not part of the Rebuild Prompt. They are deferred until the rebuild baseline is complete and each one is separately approved:

- Natural landscape below a wave-shaped city boundary, distant mountains and forests with palette-tinted depth layers.
- Seasonal variation.
- An automatic day/night cycle with a sunrise and sunset (sun on the sky layer, like the moon), including sunset sky gradients.
- An Atari palette.
- Android as a later target.
- Near-side foreground layers (owner reference, 2026-11-03), drawn after traffic and overlapping the road and bottom edge, so the 240-row grid and road position are unchanged:
  - **Silhouettes:** bushes along the bottom edge and occasional framing trees, in a near-black color derived from the palette background, with little interior detail so they never compete with the city.
  - **Motion:** in Endless mode, faster parallax than the road (about 1.3 to 1.5 times road speed). Trees sway by shifting crown rows one cell at a time with a per-tree seeded phase.
  - **Street lamps:** warm lamps with a small glow along the road edge, colored from the palette's headlight color.
- A river with window reflections below the road. This needs rows below the road, which means raising the city and road, so it is a separate, larger layout change.
- **Torn Paper effect (owner idea, 2026-11-05):** under Ink on Parchment the puffy clouds look like torn paper, so each building layer could get a torn-paper edge, as if the skyline were layers of torn paper stacked in depth. The technique may also be worth reusing in another project.
  - **Why the clouds read as torn paper:**
    - metaball heaps give soft, lumpy outlines;
    - billowy noise roughens only the edges, giving a fibrous, irregular border;
    - dithered, translucent edge cells read as the thin, frayed rim of torn paper;
    - flat, self-shadowed tones in the palette's pale paper colors stack in layers with a slightly darker underside, like layered paper with depth.
    - (`cloudgen::heap_field`, `fractal(..., billow)`, `dithered_alpha`, and `tone_ramp`.)
  - **For buildings:** tear the top edge of each layer's silhouette (or each building) with the same billow-noise roughening and dithered rim, add a faint paper-colored shadow under each layer, and keep the windows inside. Probably only on paper-like palettes (Ink on Parchment), or as a palette effect.
- Separate toggles for each display effect (ghosting, persistence, convergence error, vignette, power-on, bloom, scanlines) once the app has menu options (owner, 2026-11-05). For now `D` switches them all.
- The building parts catalog (the rest of GitHub issue #2): a versioned catalog of the shared kinds as parts (a Rust table with a JSON export), portable fixtures, and per-deviation classification tests. Deferred by the owner on 2026-11-05.
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

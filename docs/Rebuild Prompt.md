# Rebuild Prompt: Starry Night

## How To Use This Prompt

Reconstruct the application described below in **[TARGET_LANGUAGE]**, for
**[TARGET_PLATFORM]**, using **[TARGET_RUNTIME: browser, native desktop, or hybrid]**.
Choose **[FIDELITY: behaviorally faithful or seed-exact]**. If these choices are not
specified, ask for them before selecting a framework or generating a project.

Act as a senior graphics/application engineer. Implement the usable application, not a
landing page or a static screenshot. Use idiomatic modules and suitable existing graphics,
image-decoding, timezone, and UI libraries in the chosen language. Preserve the behaviors,
coordinates, numerical conventions, palette data, and acceptance criteria below. Do not
add speculative features, migrate every abstraction literally, or replace seeded procedural
art with stock images. Explain unavoidable platform differences.

This is a reconstruction snapshot of the implemented project on **2026-11-03**, including
the parchment roadside trees. It is a derived, portable specification, not a new work
backlog or competing authority for the original repository. Original source and its tests
remain authoritative when available. Standalone use supports a behaviorally faithful
rebuild; exact procedural geometry/RNG consumption requires porting the original tier
builders and generator call order, not merely matching the descriptions of their shapes.

Reference implementation:
- [README.md](README.md): launch options, controls, and operation.
- [docs/product.md](docs/product.md): implemented product behavior.
- [docs/dataDictionary.md](docs/dataDictionary.md): data and numeric contracts.
- [docs/apiReference.md](docs/apiReference.md): module/function boundaries.
- [starry_night.py](starry_night.py): local launcher and HTTP lifecycle.
- [skyline.py](skyline.py): optional image analysis.
- [web/js/city.js](web/js/city.js): deterministic model and palette definitions.
- [web/js/main.js](web/js/main.js): simulation, rasterization, controls, and tree sprites.
- [web/js/compositor.js](web/js/compositor.js): linear city compositing and display shaders.
- [web/js/rain-glyphs.js](web/js/rain-glyphs.js): bundled bitmap glyph data.
- [tests/city.test.mjs](tests/city.test.mjs), [tests/renderer.test.mjs](tests/renderer.test.mjs),
  [tests/test_starry_night.py](tests/test_starry_night.py), and
  [tests/test_skyline.py](tests/test_skyline.py): behavioral checks to port.

## Product And Boundaries

Build a pixel-art night-city screensaver and meeting/work-break display inspired by the
general idea of After Dark's Starry Night. Buildings have varied silhouettes, shaded
windows, occasional antennas, and landmark lights under a starry sky. Do not require
proprietary original artwork or claim hardware-accurate historical-display emulation.

Two modes:
1. **Normal:** fixed city assembled in randomized cell order. Every covered visible
   building cell, including body/mast/window cells, participates in construction. Untimed
   construction lasts 240 seconds at 1x; after completion windows flicker and landmark
   lights animate. No road, traffic, clouds, or roadside trees.
2. **Endless:** fully built city scrolling forever in four independent parallax layers,
   with stepped clouds, road, cars/trucks, and parchment-only roadside trees. Lights and
   stars are steady; no window flicker, blinking beacons, animated landmark color cycling,
   twinkles, or shooting stars. Camera/traffic/cloud movement remains continuous at low speed.

Provide 12 live-switchable palettes, optional literal meeting text, duration timers,
absolute local-clock completion deadlines, image-derived skylines, pause/reduced-motion
handling, responsive controls, and the palette-specific effects specified below.

No sound, alarms, full calendar scheduler, persistent browser edits, automatic expansion
of trees to other palettes, additional shaders for EGA/VGA, or marketing screen.

## Architecture

Keep these ownership boundaries even if the target language uses different file names:

| Component | Responsibility |
| --- | --- |
| Pure city model | Seeded randomness, building geometry, window metadata, palette data, deterministic roadside tree queries. No UI or graphics-context dependence. |
| Scene/simulation | Timers, reveal progress, cameras, bounded movement, traffic, pause, visibility, independent effect clock. |
| Rasterizer | Logical RGBA city buffers, ownership/depth resolution, materials, sharp device-resolution caches, sprite drawing. |
| City compositor | Smooth fractional reconstruction and far-to-near blending in linear light; graphics fallback independent of display effects. |
| Display postprocessor | Palette-specific monitor effects after scene reconstruction; separate resources and fallback. |
| UI adapter | Controls, message text, accessibility, resize, user interaction and idle state. |
| Launcher/platform adapter | CLI/configuration, optional image analysis, browser/local-server startup or native equivalent, cleanup and exit codes. |

A browser rebuild can retain HTML/CSS/Canvas/WebGL with a launcher written in the target
language. A native rebuild may replace browser controls and HTTP with native equivalents,
but must preserve observable behavior and document which browser-specific contracts do not
apply. Do not move the per-frame simulation into HTTP requests: the original server only
delivers static assets/configuration; the loaded client owns animation.

## Configuration And Launcher

Portable launch configuration:

```json
{
  "mode": "normal",
  "palette": "classic",
  "speed": 1,
  "timerSeconds": null,
  "endsAtMs": null,
  "skyline": null,
  "message": "",
  "closeAfterLoad": false,
  "readyToken": null
}
```

Preserve the original CLI, or supply a documented equivalent for native applications:
- `--speed N`: initial value 0.25..4, default 1.
- `--palette KEY`: one of the palette keys below, default classic.
- `--message TEXT`: trim surrounding whitespace; at most 240 UTF-16 code units, including
  two units for supplementary Unicode characters. Empty/whitespace hides the notice.
- `--timer D`: duration from 1 second through 24 hours. Bare numbers mean minutes;
  support forms such as `25`, `25m`, `90s`, and `1h30m`.
- `--ends-at TIME`, alias `--end-time`: 12-hour AM/PM or 24-hour local time, optional
  seconds. Resolve the next occurrence once using the computer's local timezone. Equality
  to an already reached target rolls to tomorrow. No timezone suffix/argument.
- `--endless`: excludes duration timer, image mode, and absolute deadline.
- `--image PATH`: analyze an image into normalized skyline segments; permitted with either
  normal timing option, but not endless mode.
- `--port N`: loopback port, default automatically assigned free port.
- `--no-browser`: print the URL without opening it.
- `--close-after-load`: stop the launcher server after authenticated client readiness.

Reject incompatible options, malformed values, and unusable image input with exit 2.
Duration timer and absolute deadline are mutually exclusive. Normal Ctrl+C/auto-close
shutdown returns 0. Print the resolved deadline date, time, and local timezone.

Browser-host contract:
- Bind only to `127.0.0.1`; serve the application asset directory, not the workspace.
- `GET /config.json` returns JSON; `GET /favicon.ico` returns 204.
- Static files use correct MIME types, including JavaScript. Reject directory listings,
  traversal, and access outside the asset root.
- Send no-store caching, nosniff, no-referrer, and CSP equivalent to
  `default-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'`.
- Auto-close creates a fresh URL-safe token from 32 random bytes in the server's serialized
  config, without mutating the caller's configuration object.
- After the first visible drawn frame, the client sends one `POST /ready` with
  `X-Starry-Ready: TOKEN`. A paused scene can still draw and signal; hidden pages defer it.
- Valid auto-close readiness returns bodyless 204, then initiates shutdown without
  deadlocking the request handler. Wrong/missing token returns 403; disabled/unknown POST
  returns 404. GET must never cause shutdown.
- Readiness errors warn but do not stop animation or retry automatically. Restart/resize
  do not reset the once-per-page readiness flag.
- After server shutdown the loaded page still supports animation, controls, restart, and
  resizing; reload/new tabs require relaunch. Close listeners/threads/sockets reliably,
  including browser-open failures and partial initialization.

Original launch presets, where platform-appropriate:
- Standard: no extra defaults; endless: prepend `--endless`.
- Meeting: endless, speed 4, ArtDeco, message `This meeting will start shortly`.
- Meeting-at: normal, ArtDeco, the same message, requires a deadline unless help is requested.
- MWF meeting is only a custom meeting preset, not a day-of-week scheduler. Its message is
  `Welcome to the MWF Test Environment Meeting. The meeting will begin shortly. Recording Disabled. Copilot Disabled.`
  Those words are notice text, not functionality that disables recording or AI tools.
- Forward later arguments so they override preset palette/speed/message defaults. Resolve
  executable/source relative to the script; preserve the caller's working directory and
  relative image paths. Original Windows scripts prefer a local virtualenv interpreter.

## Coordinates, Arithmetic, And Data

Use integer logical cells for art and continuous world x for scrolling:

```text
deviceWidth  = max(1, round(viewportWidth  * devicePixelRatio))
deviceHeight = max(1, round(viewportHeight * devicePixelRatio))
cell         = max(2, round(deviceHeight / 240))
cols         = ceil(deviceWidth / cell)
rows         = ceil(deviceHeight / cell)
groundRow    = endless ? rows - 7 : rows
maxH         = max(8, floor(groundRow * 0.82))
row          = groundRow - 1 - buildingHeightCoordinate
bufferIndex  = row * bufW + (worldX - originX)
screenX      = (worldX - viewX) * cell
```

The grid can overrun the device canvas slightly; final raster output clips at its bounds.
Return an invalid index for out-of-buffer coordinates; never let negative indices wrap.
Preserve unsigned 32-bit overflow and logical right shifts where specified. JavaScript
rounding is not bankers' rounding: for matching integer calculations use its semantics
(normally `floor(x + 0.5)`). Image-analysis rounding follows the source's Python behavior
if seed/pixel-exact compatibility is required. Normalize modulo explicitly where negative
world positions occur; do not assume language remainder equals mathematical modulo.

Portable records:
- Building: layer 0..3, footprint `[x0,x1)`, kind, tiers, roof metadata, optional mast/beacon,
  decoration, body index 0..5, base tone byte, accent roll, window arrays, target/current counts.
- Tier: nonempty half-open rectangle `[x0,x1) x [h0,h1)` plus optional solid/dark flags.
  Every tier is grounded or supported by another tier. Geometry/masts stay in the footprint
  and do not exceed maxH. Skip empty roof tiers when the base already reaches the limit.
- Windows: parallel arrays for world x, height, byte shade, byte tone, target bit, and lit bit.
  Store tone independently of palette so live palette changes do not regenerate geometry.
- Decoration: crown/band/spire/roofline, cycle/chase, initial hue, flattened coordinate pairs.
- Per-city state: seed/RNG, grid, camera/origin, buildings/ordered buildings, RGBA buffer,
  depth byte array, owner integer array, window-index integer array, dirty/revision flags.
- Normal-only: reveal bits, randomized reveal order, count/progress and completion state.
- Endless root: four independent child city states, root road camera, clouds, cars,
  stars and optional lazy tree-sprite cache. Children do not recursively own layers.

Use `owner = -1` for sky, `windowIndex = -1` for body, and `-2` for mast.
Resolve topmost ownership before painting any pixels so occlusion and shadows see complete
geometry. Draw stable far-to-near layer order. Invisible/occluded window updates must not
paint through another building. Preserve alpha masks when changing materials/palettes.

## Deterministic Generation

For cross-language RNG compatibility, implement Mulberry32 with wrapping 32-bit arithmetic:

```text
state = uint32(seed)
next():
    state = uint32(state + 0x6d2b79f5)
    t = state
    t = multiplyLow32(t XOR logicalShiftRight(t, 15), t OR 1)
    t = t XOR uint32(t + multiplyLow32(t XOR logicalShiftRight(t, 7), t OR 61))
    return uint32(t XOR logicalShiftRight(t, 14)) / 4294967296
randInt(lo, hi) = lo + floor(next() * (hi - lo + 1))
```

Independent streams:
- Normal city geometry: scene seed.
- Root ambient/traffic stream: `seed XOR 0x9e3779b9`.
- Endless child layer L: `seed XOR (0x85ebca6b + L)`.
- Clouds: `seed XOR 0xc2b2ae35`.
- Trees: per-slot/per-side hashing below; never consume city or traffic RNG.

Four generator layer specifications, ordered farthest to nearest:

| Layer | Width | Gap | Height base | Height span | RNG power |
| --- | --- | --- | --- | --- | --- |
| 0 | 6..16 | 0..3 | 0.45 | 0.55 | 1.2 |
| 1 | 6..18 | 1..5 | 0.32 | 0.50 | 1.2 |
| 2 | 6..18 | 2..7 | 0.18 | 0.40 | 1.0 |
| 3 | 5..16 | 3..12 | 0.07 | 0.25 | 1.0 |

Height is `max(4, round(maxH * (base + span * random^power)))`, then geometry is clamped.
Note that the farthest layer intentionally contains the tallest general silhouettes.
Each generator starts its layer cursors at `startX - randInt(0,10)`. Extend left-to-right
until each selected layer reaches the requested world x, advancing by building width plus
its sampled gap. Buildings in a layer never overlap. Maintain/prune only nearby tint
neighbors using the shortest reach of the generator's selected layers.

Weighted kinds:
`box:18, stepped:18, spire:12, pointed:12, ziggurat:6, gable:8, arch:8, slant:8,
crownblock:10, needle:4, tvtower:3, twin:5, onion:4, watertank:5, invpyramid:4,
clocktower:4, jenga:5, factory:5, pagoda:4`.

Shapes must visually include slabs, setbacks, pyramidal/rounded/slanted roofs, bright crowns,
needle observation decks, spherical TV towers, linked twins, onion domes, water tanks,
inverted pyramids, clock towers, offset stacks, sawtooth factories, and pagoda eaves.
Generic setbacks require width 7; gable/arch/slant require 5. Landmark minimum widths:
needle/twin/invpyramid/jenga/pagoda 9, factory 10, other special kinds 7. Narrow footprints
fall back to a box rather than create invalid tiers. Exact special tier dimensions are
source-dependent in seed-exact mode; preserve these shape/support contracts otherwise.

Masts: explicit tower kinds get them; otherwise tall buildings have 30% eligibility,
factories never get them. Length is 4..16 cells clipped to available headroom. Center the
mast over the highest tier. Beacon periods are 1.2..2.5 seconds with seeded phase.

Choose body tints to avoid matching touching/overlapping footprints: first exclude colors
used in same/adjacent layers and farther touching layers, then relax farther-layer exclusions,
then allow all six if necessary. Independent endless layers guarantee this only inside each
generator, not for arbitrary future parallax alignments.

Window patterns:
- 20% horizontal bands: x pitch/width 1, y pitch 2 (60%) or 3, target ratio 0.70..0.92.
- Next 22% vertical stripes: x pitch 2, width 1, y pitch 1, target ratio 0.70..0.92.
- Otherwise grid: x pitch 2 (75%) or 3; pitch-3 windows may be width 2 (50%);
  y pitch 2 (70%) or 3, target ratio 0.45..0.80.
- Avoid normal exterior borders where width permits. Start base-tier windows above ground;
  reserve roof/top edges. Solid tiers are densely lit; dark tiers have no windows. Crown
  blocks and alternate roof rows become dense stripes. Dense cells have 92% target-on chance.
- Give 12% of windows an independent tone byte; others use their building's tone byte.
- Fixed shade is `min(0.9, layerFade + 0.6*(1-h/max(1,roofH))^1.6 + rightFaceShade)`;
  rightFaceShade is 0.22 on the rightmost 38% when tier width is at least four.
- Standard mode adds 0.3 cast fade when nearer geometry lies within three cells to the
  left/upper-left. Keep window shades quantized through 16 precomputed fade levels.

Eligible landmark decoration needs a main tier at least five cells wide and total height
at least 40% of maxH. Generation has a 6% decoration chance; a normal city ensures 1..3
eligible decorated buildings if none already exist. Roofline/crown/band/spire outlines
must lie on geometry. Standard mode cycles/chases hue; endless caches steady lights.

## Time, Reveal, And Endless Motion

Normal reveal order is a Fisher-Yates shuffle of every topmost owned buffer cell, not just
window locations. At progress f, reveal `floor(f * total)` cells, but all cells at f >= 1.
If a revealed cell is a target-lit window, turn it on and maintain lit counts.

Three construction clocks:
- Untimed: advance progress by `dt * speed / 240`.
- Duration: monotonic elapsed time / requested duration, independent of speed. Restart
  restarts this timer; resizing preserves its original timing/progress.
- Deadline: wall-clock epoch elapsed relative to a baseline / time to the fixed `endsAtMs`.
  The baseline resets on Restart, not the deadline. Clamp 0..1 and prevent progress decreasing
  if the wall clock moves backwards. Late load, elapsed target, and Restart after target
  immediately complete. Hidden tabs complete on their next visible frame, not in background
  paint. Speed must not affect deadline or duration completion.

After normal completion, use Poisson window toggles at 0.02 per building per second times
speed, steering on/off choices toward each building's target lit count. Twinkles and
shooting stars remain normal-only. First shooting-star wait is 4..12 seconds; subsequent
waits 8..28 seconds. Preserve seeded scheduling and short fading trails.

Endless root camera moves at `2 * speed` cells/second. Child multipliers are
`[0.2, 0.4, 0.65, 1]`. Buffer width is `2*cols + 64`; child origin initially -4 and
generator startX -28. Generate ahead through `originX + bufW + 32`, setting all windows
to target states. If the camera approaches buffer exhaustion, rebase origin to
`floor(viewX)-4`, extend ahead, discard old buildings behind origin-4, and redraw the cache.
Do not accumulate an unbounded history or regenerate visible geometry at each frame.

Visible frame dt is clamped to 0..0.25 seconds; camera/cloud/car motion uses at most
1/30 second per frame, not a fixed 30fps timestep. High-frequency frames use their real
smaller dt. Hidden documents skip simulation/drawing and reset the clock; resuming starts
with a fresh frame without catch-up camera jumps. Pausing freezes all endless motion but
continues drawing/UI/readiness. Honor reduced-motion preferences from startup; changing
back does not automatically resume animation.

Stars: approximately `round(cols * groundRow * 0.0035)` seeded cells, brightness fade
0..0.6. Four percent are potential twinkles; they remain steady in endless mode.
Clouds: `max(3, ceil(cols/130))`, widths 38..90 and heights 6..13 cells, composed of
two-cell-high stepped strips. Wrap in span cols+160 outside the viewport; drift 0.35
cells/second times speed behind buildings, tinted from palette stars toward background.

Road: seven rows at groundRow, top/bottom curb lines, central four-cell lane dashes every
eight world cells. Two opposing traffic lanes start at groundRow+1 and groundRow+4.
Keep 2..6 vehicles based on viewport width; cars are 4..5 cells long, trucks 7..9 with
20% truck chance. Speeds are 9..18 world cells/second times speed, with palette-specific
body/head/tail colors. Respawn offscreen. Trees share the road camera, not vehicle speed.

## Rasterization And Color Space

Do not upscale a logical pixel buffer directly at a fractionally moving coordinate: that
can snap pixel art and produce shimmer. For endless mode, first nearest-neighbor-rasterize
each logical city to a sharp device-resolution cache, including its steady lights. Reuse
that cache until dirty. Reconstruct it at 1:1 device scale with fractional horizontal
positions; stars/clouds/traffic/tree sprites also use continuous coordinates as appropriate.

Preferred GPU compositor uses one background texture and exactly four layer textures.
For each fractional source x, fetch the two neighboring encoded texels, decode RGB to
linear light, premultiply by alpha, interpolate, and composite far-to-near with source-over.
Encode the final opaque output once. Alpha itself is not gamma corrected.

```text
decode(c) = c/12.92                         if c <= 0.04045
            ((c+0.055)/1.055)^2.4           otherwise
encode(c) = 12.92*c                        if c <= 0.0031308
            1.055*c^(1/2.4)-0.055          otherwise
over(front, back) = front + back*(1-front.alpha)
```

Use nearest texel fetches, explicit transparent out-of-range layer samples, and correct
top-origin raster/bottom-origin GPU row mapping. Do not rely on hardware filtering in
encoded sRGB to fix shimmer. Upload background each frame; upload city layers only when
identity/dimensions/revision change, reusing texture storage when dimensions match.

The Canvas/native CPU fallback interpolates the same sharp device-resolution caches but
may differ in brightness due to encoded-space filtering. Keep a renderer selector for A/B
comparison. Standard city rasterization remains nearest-neighbor/integer-grid. GPU setup,
loss, size-limit rejection, or draw failure falls back cleanly and independently of display
effects; release partial allocations, shaders, textures, programs, and contexts idempotently.

Final draw order:
1. Opaque background, stars, normal twinkles/trails; endless clouds.
2. Far-to-near reconstructed cities, with appropriate lights/materials.
3. Endless road, parchment background trees, traffic, parchment foreground trees.
4. Game Boy grid when selected; separate monitor postprocessor when selected/enabled.
5. UI/message above the scene, unaffected by full-scene filters except explicit Amber
   message phosphor styling. No cards around the primary scene.

## Exact Palette Data

Keep this selector/key order: classic, artdeco, blackonwhite, cga, cga2, ega, vga,
c64, appleii, gameboy, amber, green. Preserve the legacy `blackonwhite` key but display
**Ink on Parchment**. Retro palettes are inspired looks, not strict historical color counts.

Palette fields: background, six unique bodies, weighted windows, antenna/star/beacon,
optional building accent/probability, road/lane, vehicles/headlight/taillight, decoration
lightness and optional fixed layer/decor tint. Missing accent defaults to null/probability 0;
missing layerTint/decor means normal layer tint/generated landmark HSL.

```json
{
  "classic": {
    "label":"Classic","bg":"#000000","antenna":"#7a7a70","star":"#ffffff","beacon":"#ff2a2a",
    "bodies":["#141412","#201f1b","#111419","#1c1814","#171b15","#26231c"],
    "windows":[["#ffff55",6],["#ffd75f",2],["#ffffff",1]],
    "road":"#101010","lane":"#5a5a5a","vehicles":["#e03030","#3060e0","#d8d8d8","#e0c020"],
    "headlight":"#ffffc0","taillight":"#ff2020","decorLightness":55
  },
  "artdeco": {
    "label":"ArtDeco","bg":"#000000","antenna":"#8a8478","star":"#fff6e0","beacon":"#ff3020",
    "bodies":["#181510","#261f15","#13151b","#221814","#191d16","#2c251a"],
    "windows":[["#ffffff",4],["#fff0c8",3],["#f0c050",3],["#e89040",1]],"accent":"#d83a2a","accentChance":0.08,
    "road":"#0c0c0c","lane":"#6a5a3a","vehicles":["#d83a2a","#d83a2a","#3a6fd9","#f0e6c8"],
    "headlight":"#fff6d0","taillight":"#ff2a1a","decorLightness":58
  },
  "blackonwhite": {
    "label":"Ink on Parchment","bg":"#efe4c8","antenna":"#5b5549","star":"#24221e","beacon":"#24221e",
    "bodies":["#b7a16e","#b88870","#899777","#a59382","#aa7971","#a6a071"],
    "windows":[["#1b1916",1]],"layerTint":"#a3977d","decor":"#24221e",
    "road":"#dbceb2","lane":"#7a705d","vehicles":["#24221e","#4b453a"],
    "headlight":"#82745a","taillight":"#24221e","decorLightness":30
  },
  "cga": {
    "label":"CGA Cyan/Magenta","bg":"#000000","antenna":"#aa00aa","star":"#ffffff","beacon":"#ff55ff",
    "bodies":["#092729","#290929","#112025","#251124","#102f2f","#301230"],
    "windows":[["#55ffff",4],["#ff55ff",4],["#ffffff",2]],
    "road":"#100c18","lane":"#55ffff","vehicles":["#ff55ff","#55ffff","#ffffff"],
    "headlight":"#ffffff","taillight":"#ff55ff","decorLightness":60
  },
  "cga2": {
    "label":"CGA Green/Red","bg":"#000000","antenna":"#aa5500","star":"#ffff55","beacon":"#ff5555",
    "bodies":["#102810","#2b1110","#29260c","#17321a","#331a12","#34300f"],
    "windows":[["#55ff55",4],["#ff5555",3],["#ffff55",3]],
    "road":"#12140c","lane":"#ffff55","vehicles":["#55ff55","#ff5555","#ffff55"],
    "headlight":"#ffff55","taillight":"#ff5555","decorLightness":60
  },
  "ega": {
    "label":"EGA","bg":"#000000","antenna":"#aaaaaa","star":"#ffffff","beacon":"#ff5555",
    "bodies":["#10102c","#102c2c","#2c102c","#102c10","#2c1010","#2c2210"],
    "windows":[["#55ffff",2],["#ff55ff",2],["#ffff55",3],["#55ff55",2],["#ffffff",1]],"accent":"#5555ff","accentChance":0.12,
    "road":"#111111","lane":"#aaaaaa","vehicles":["#5555ff","#ff5555","#55ff55","#aa5500"],
    "headlight":"#ffffff","taillight":"#ff5555","decorLightness":60
  },
  "vga": {
    "label":"VGA","bg":"#060614","antenna":"#8a92b8","star":"#e8f4ff","beacon":"#ff4060",
    "bodies":["#171c3a","#301631","#10332e","#302712","#20243d","#351c25"],
    "windows":[["#40c8ff",3],["#ff70c8",2],["#ffc850",3],["#70f0b0",2],["#e8f4ff",1]],"accent":"#a080ff","accentChance":0.14,
    "road":"#101020","lane":"#6478a8","vehicles":["#3868e8","#e84878","#38b890","#e8a838"],
    "headlight":"#e8f4ff","taillight":"#ff4060","decorLightness":65
  },
  "c64": {
    "label":"Commodore 64","bg":"#100e20","antenna":"#6c6c6c","star":"#b8c76f","beacon":"#9a6759",
    "bodies":["#1c1738","#2d1929","#142927","#2b2318","#22263e","#302116"],
    "windows":[["#70a4b2",3],["#b8c76f",3],["#6f3d86",2],["#9a6759",2],["#ffffff",1]],"accent":"#6f3d86","accentChance":0.12,
    "road":"#211b2e","lane":"#6c6c6c","vehicles":["#352879","#68372b","#588d43","#6f4f25"],
    "headlight":"#b8c76f","taillight":"#9a6759","decorLightness":55
  },
  "appleii": {
    "label":"Apple II","bg":"#000000","antenna":"#808080","star":"#ffffff","beacon":"#ff6600",
    "bodies":["#24102d","#102b15","#301a0a","#10242f","#291e32","#163120"],
    "windows":[["#dd22dd",3],["#22dd22",3],["#ff6600",2],["#11aaff",2],["#ffffff",1]],
    "road":"#101410","lane":"#808080","vehicles":["#dd22dd","#22dd22","#ff6600","#11aaff"],
    "headlight":"#ffffff","taillight":"#ff6600","decorLightness":58
  },
  "gameboy": {
    "label":"Game Boy","bg":"#0f380f","antenna":"#306230","star":"#9bbc0f","beacon":"#8bac0f",
    "bodies":["#173f16","#204b1b","#2a5520","#346127","#3d6c2b","#47782f"],
    "windows":[["#9bbc0f",3],["#8bac0f",2]],"layerTint":"#306230","decor":"#9bbc0f",
    "road":"#0b2b0b","lane":"#306230","vehicles":["#8bac0f","#306230","#9bbc0f"],
    "headlight":"#9bbc0f","taillight":"#8bac0f","decorLightness":50
  },
  "amber": {
    "label":"Amber Terminal","bg":"#100900","antenna":"#805000","star":"#ffb000","beacon":"#ffd070",
    "bodies":["#241500","#301d02","#3b2503","#472c04","#523305","#603c08"],
    "windows":[["#ffb000",4],["#ffd070",1]],"layerTint":"#805000","decor":"#ffb000",
    "road":"#160d00","lane":"#805000","vehicles":["#a06a00","#d09000","#ffb000"],
    "headlight":"#ffd070","taillight":"#ffb000","decorLightness":55
  },
  "green": {
    "label":"Green Terminal","bg":"#001005","antenna":"#007028","star":"#33ff66","beacon":"#99ffb0",
    "bodies":["#00240b","#03300e","#043b12","#054716","#07521b","#096021"],
    "windows":[["#33ff66",4],["#99ffb0",1]],"layerTint":"#007028","decor":"#33ff66",
    "road":"#001607","lane":"#007028","vehicles":["#008830","#10b848","#33ff66"],
    "headlight":"#99ffb0","taillight":"#33ff66","decorLightness":55
  }
}
```

Depth fades are `[0.3,0.2,0.1,0]`. Bodies blend toward background by 1.5 times that fade;
endless bodies additionally blend 10% toward layer targets
`[[70,94,130],[130,76,74],[74,110,96],[128,112,86]]` or the palette's layerTint override.
Do not tint windows/lamps with those body hue targets. Fixed decor overrides generated HSL.
For each tone byte b, choose a weighted window entry at `(b+0.5)/256 * totalWeight`.

## Palette-Specific Materials And Display Effects

Display Effects defaults enabled, survives palette switches, and appears only for ArtDeco,
both CGAs, C64, Apple II, Amber, and Green. Game Boy grid is always palette-scoped rather
than controlled by this checkbox. Parchment has no display shader. Classic/EGA/VGA remain
unprocessed. No effect should silently leak into another palette.

**ArtDeco material, before compositing:** only owned body/unlit-window surfaces, never
lit windows, masts, sky, stars, clouds, road, or UI. Let u be building-local x across its
footprint. Warm sheen is `exp(-((u-0.3)/0.18)^2)`; cool edge reflection is
`exp(-((u-0.8)/0.25)^2)`. RGB body scale is `0.96 + sheen*0.22`, with additive warm
`[226,196,146]*sheen*0.055` and cool `[138,155,177]*reflection*0.018`. Add horizontal grain
from a height-anchored, normalized modulo-17 phase, bounded +/-0.7 encoded levels.
For each revealed same-owner axial neighboring lit window, add up to 2.5% of shaded RGB,
scaled by `max(0,(max(windowRGB)/255-0.55)/0.45)`. Take the strongest spill per channel,
not an accumulating blur. Clamp/round RGB, preserve alpha. Refresh neighboring owned
surfaces on window reveal/flicker. Disabling redraws original colors without changing geometry
or reveal progress. Sheen/grain move with buildings and have no animation clock.

**Game Boy:** stationary cell-aligned dark horizontal/vertical gaps over the completed
canvas. Pitch is cell; gap `max(1,round(cell*0.2))`, ending at each cell boundary. Use
`rgba(3,19,3,0.55)` independently for each direction; intersections compound. No ghosting,
curvature, or CRT effect. DOM text/controls stay clear.

Monitor scanline geometry: period `max(2,round(2*devicePixelRatio))`; dark thickness
`max(1,round(devicePixelRatio))`. Apply fixed rows after reconstruction in encoded RGB.
Patterns are screen-fixed, never moving bands. Neighbor reach is
`max(1,round(devicePixelRatio))` device pixels with clamped edge samples.

**Apple II:** RGB-to-luminance/chroma coefficients:
`Y=dot(RGB,[0.299,0.587,0.114])`, `I=dot(RGB,[0.596,-0.274,-0.322])`,
`Q=dot(RGB,[0.211,-0.523,0.312])`. Chroma uses 0.4 center, 0.24 each immediate horizontal
neighbor, 0.06 each outer neighbor at twice the reach; Y uses 0.84 center + 0.08 each
immediate neighbor. Add stationary alternating-column fringe to I/Q of
`[0.12,0.08]*(rightY-leftY)*phase`, phase -1/+1 by reach-sized column. Reconstruct RGB:
`[Y+0.956I+0.621Q, Y-0.272I-0.647Q, Y-1.106I+1.703Q]`, clamp, then dark rows retain 70%.
Fallback: source weights 0.76 center/0.12 each horizontal neighbor, additive blending,
then 30% scanlines; separate chroma/fringes are not reproduced exactly.

**C64:** same conversion, chroma 0.7 center/0.15 each immediate neighbor, Y unchanged,
no alternating-column artifacts. Tight highlight halo uses four axial samples, each decoded
to linear RGB and weighted by `smoothstep(0.55,0.85,max(encodedRGB))*0.045`; screen blend
against the reconstructed linear color, then encode. Dark rows retain 80%. Fallback uses
0.88 center/0.06 each horizontal neighbor plus 20% scanlines, omitting selective glow.

**Both CGAs:** identical crisp RGB-monitor effect, no chroma reconstruction/color bleed.
Use the same four-neighbor highlight function at weight 0.012 rather than 0.045. After
linear-light screen blending/encoding, apply repeating reach-wide R/G/B stripes: matching
channel retains 100%, other channels 96%. Dark rows retain 75%. Fallback multiplies the
source by repeating `[255,245,245]`, `[245,255,245]`, `[245,245,255]` masks and adds 25%
scanlines, omitting selective glow. No curvature, noise, or flicker.

**Amber:** whole-scene 25% scanlines, with dark rows retaining 75% of source encoded RGB.
Meeting message remains complete literal text, split by grapheme only for styling. Refresh
one grapheme every 0.09 seconds; charge decays as `exp(-age/0.55)`, with 2.4 seconds between
passes. Do not erase/retype text or announce changing live-region content. Reduced motion
keeps the message steady. Canvas fallback uses the same scanline geometry.

**Green:** sparse falling bitmap code across the entire scene, not just the message.
Use 18x20 CSS-pixel cells with 8x16 glyphs at offsets (5,2), column participation 42%,
seed-like deterministic hash `fract(sin(value*12.9898)*43758.5453)`. Column phase controls
head speed 3..6 glyph rows/second; wrap span is visible rows + 12. Tails last nine rows,
strength `0.5*(1-distance/9)^2`; glyph selection is stable by cell. Green heads approach
pale green while tails remain green. Keep message text steady and literal.

Use narrow local dark outlines/under-glyph darkening based on scene linear luminance,
then bounded linear-light screen blend with maximum weight 0.7. Glyph emission carries
25% alternate-row darkening and a restrained local mask halo: four neighbors at one CSS
pixel weighted 0.045, four at two pixels weighted 0.015, capped 0.12. Do not blur/dim the
whole city or use global brightness pulses. Canvas fallback copies the scene, draws local
black outlines, then green screen-blended glyphs with a two-CSS-pixel phosphor shadow;
restore alpha/blend/shadow state. Approximate GPU/CPU differences must be documented.

Green glyphs are 157 bundled GNU Unifont 16.0.04 bitmap entries: non-space printable ASCII
U+0021..007E and half-width punctuation/katakana U+FF61..FF9F. Each bitmap is 16 rows of
eight bits, encoded as 32 hex characters, MSB left. Atlas: 128x160, 16 columns, 8x16 slots,
three empty padding slots. Preserve the supplied SIL OFL 1.1 attribution/license in
[web/assets/rain-glyphs-OFL.txt](web/assets/rain-glyphs-OFL.txt). If rebuilding without the
source asset, obtain an appropriately licensed equivalent and disclose glyph differences;
this prompt does not reproduce the full glyph dataset. No runtime font/network dependency.

The effect clock is independent of speed, advances at most 1/30 second per visible frame,
and freezes for endless pause, hidden documents, reduced motion, or disabled effects.
Reset it on message/palette/effect/preference changes. Setup a separate full-scene
postprocessor once per page; on failure dispose it and use a CPU/Canvas fallback, without
disabling the city compositor. Reload can retry. Release graphics resources on page exit.

## Parchment Roadside Trees

Enable only for endless `blackonwhite`. No tree UI toggle and no trees in other palettes.
Use these exact five-column sprites, with `.` transparency, `l` leaf, `s` leaf shade,
`t` trunk; first three are deciduous, last three conifers:

```json
[
  [".lll.","lssll","lllsl",".lll.","..t..","..t.."],
  ["..l..",".lll.","lssll","lllsl",".lll.","..t..","..t.."],
  [".lll.","llsll",".lll.","..t..","..t.."],
  ["..l..",".lsl.","..l..",".lll.","llsll","..t..","..t.."],
  ["..l..",".lsl.","llsll",".lsl.","llsll","..t.."],
  ["..l..",".lsl.","..l..",".lll.","..t.."]
]
```

Each tree is five to seven cells high, never taller than the seven-cell road. Query only
visible world slots, with spacing 23 for background and 29 for foreground. For slot i:

```text
rngSeed = sceneSeed XOR multiplyLow32(i, 0x45d9f3b)
          XOR (foreground ? 0x632be5ab : 0x27d4eb2d)
x = i*spacing + randInt(0,7)
shape = (oddSlot ? 3 : 0) + randInt(0,2)
tintByte = randInt(0,255)
```

Query slots from `floor((left-5)/spacing)` through `floor(right/spacing)`; discard sprites
outside the requested interval, including their width. Empty/inverted intervals return none.
Negative slots work. Querying overlapping ranges must give identical overlapping trees.
Do not maintain an infinite growing tree list or disturb city/traffic RNG streams.

Deciduous trees use tintByte modulo six body colors. Conifers choose sage/olive body indices
2/5 by byte parity. Blend leaf RGB 6% toward ink for background, 18% for foreground; shade
adds another 22% ink; trunks use antenna color. Background roots meet groundRow*cell,
foreground roots meet canvas bottom. Draw background after road/before cars, foreground
after cars. Sparse foreground foliage may naturally occlude passing vehicles.

Lazily cache sharp device-resolution sprites by side/shape/tint, at most 48 combinations.
Draw 1:1 at fractional road-camera x with interpolation, not direct fractional upscaling.
Resize/restart rebuilds the scene cache; palette switches hide/reuse it without growth.

## Optional Photo Skyline

Decode locally using a mature image library; validate missing/unreadable/oversized input.
Original decoder limit is 50 million pixels. Convert to grayscale and box-resample to
480 columns by `2*max(16,round(originalHeight*240/originalWidth))` rows.

For each 240-column logical analysis cell, average absolute rightward plus downward
grayscale gradients over its 2x2 samples. Mark texture when gradient > 10; horizontally
dilate by one logical column to bridge window gaps. Ground is floor(analysisRows*0.88).
Trace upward from ground in each column, stopping at an untextured run longer than
`max(3,round(analysisRows*0.06))` to avoid treating floating clouds as buildings. With at
least five columns, copy the third column to the first two and third-from-last to last two.

Convert top coordinates to heights; median-filter over neighborhoods of three columns.
Split segments when height differs from the current segment median by more than
`max(2,peakHeight*0.06)`. Merge segments narrower than two columns into their left neighbor.
Normalize x/width by 240 and height by peak; discard heights < 0.03. Round x/width to five
decimals and h to four. Return ordered `{x,w,h}` records, not the original image.
Reject a flat/empty skyline. Build non-overlapping box footprints on front layer 3 from
normalized segments, minimum width one cell and height two; maintain body tint contrast
and eligible landmark decorations. The original image never goes to the browser.

## Controls, Message, And Responsive Behavior

Full-viewport scene. Compact top-centered toolbar, wrapping on mobile; cursor/toolbar
auto-hide after three seconds idle, wake on pointer/touch/keyboard interaction. Controls:
speed dial, palette selector, restart icon, endless pause/resume icon, endless renderer
selector, optional Display Effects checkbox, and editable message field. Numeric speed
range 0.25..4 in 0.25 increments, with pointer capture, keyboard arrows/Home/End and
accessible value text. Dial arc is -135..135 degrees; clamp, never jump across the dead zone.

Keyboard outside form controls: P cycles palettes, R restarts, +/- change speed, Space
pauses/resumes endless. Do not intercept typing or native button/select/input keys.
Use accessible control names, pressed state, focus visibility and tooltips. Keep fixed
tool dimensions to avoid layout jumps. No visible instructional/marketing text in the scene.

Message is plain literal Unicode, never HTML. Empty hides its box; preserve edits through
palette changes/restart/resize, but reload restores launch text. Use one consistent 240
UTF-16-unit limit in CLI and browser. Normally center at 50% width/42% height; maximum
640 CSS pixels wide with at least 16px side clearance. Background is palette bg at 75%
opacity, text palette star, border halfway between star and bg. Wrap unbroken text; bound
tall content with scrolling. Observe toolbar bottom plus 12px to keep visible/focused
controls above the message. Text stays visible when controls hide.

Resize is debounced and rebuilds using the same seed, preserving progress, absolute
deadline, duration baseline, message, palette, speed and pause state. Restart chooses a
new random uint32 seed, resets untimed/duration progress and scene motion, but preserves
the fixed absolute deadline and message. Hidden-window restore must refresh rendering
without accumulating camera movement. Dispose observers/input hooks/resources appropriately.

## Verification And Deliverables

Implement in small runnable increments: pure model/configuration, normal rasterizer,
timing/UI, endless reconstruction/traffic, display/material effects, parchment trees,
photo analysis, then platform lifecycle integration. This ordering does not permit omitting
later required features or silently changing requirements.

Port these acceptance checks using the target's test framework:
1. Same seed reproduces model records; validate widths, supports, height limits, no overlap,
   window containment, body-color neighbors, and varied landmark/roof/window patterns.
2. Validate all palette roles, exact supplied RGB values, weighted tone mapping, parchment
   ink contrast, muted earth pigments, legacy key compatibility, and no colored ink accents.
3. Reveal all owned cells, not just lit windows; preserve ownership/alpha under materials.
   Test duration/deadline completion at 0.25x and 4x, exact boundary frames, late load,
   hidden time, backwards wall-clock changes, restart, resize, and deadline rollover.
4. Test bounded delta, smooth fractional motion, no hidden/resume jumps, pause/reduced motion,
   steady endless lights, buffer extension continuity, and bounded memory/cache growth.
5. Test tree heights/families/spacing, negative and overlapping world queries, independent
   RNG, verge rooting/draw order, fractional motion, cache reuse, resizing, evergreen color
   roles, parchment-only scope, and roadless normal mode.
6. Test message literal markup, blank/whitespace, supplementary characters/combining marks,
   length boundary, palette changes, restart, wrapping, idle controls, and grapheme animation.
7. Test effect toggle/isolation, unchanged EGA/VGA/Classic, unchanged material sky/windows,
   exact ArtDeco off/on reversibility, and incremental glow matching a full redraw.
8. Use real raster/GPU readbacks, not only mocked draw calls: nonblank desktop/mobile scenes,
   correct framing and assets, stationary scanlines/triads, selective halo, and no text overlap.
   GPU white scanline checks before other masks: Amber 255/191, Apple II 255/178,
   C64 255/204. CGA white triads 255/245/245 rotated across columns; dark rows 191/184/184.
   DPR 2 doubles stripe/row widths; same frozen scene/effect time is repeatable.
9. Compare decoded luminance during subpixel scroll; an encoded-space brightness sum is
   not sufficient evidence of linear-light stability. Avoid nearest-neighbor snapping.
10. Inject GPU setup/context-loss/texture failures and verify fallback plus resource cleanup.
    Validate server confinement, headers/MIME, token rejection, one-shot readiness and
    shutdown/socket release on success/failure. Always clean owned test resources.
11. Test image gradients/cloud rejection, empty profiles, aspect ratios, segmentation,
    unreadable/oversized images, and consistent normalized footprints.

Original local checks, when working in the source repository:

```powershell
node --test tests/city.test.mjs tests/renderer.test.mjs
.venv/Scripts/python.exe -m unittest discover -s tests
git diff --check
```

Do not present passing mocked tests as proof of actual pixel rendering, or authored tests
as executed tests. Separate implementation, executed verification, human visual acceptance,
and unverified hardware/browser differences.

Deliver runnable source, target-specific setup/run/test commands, a concise architecture
and configuration reference, required licensed assets/notices, and a compatibility/deviation
report. Name any intentionally unavailable platform feature rather than simulating it
with misleading UI. For browser builds provide the local URL and a clear server shutdown
procedure; avoid retaining duplicate test servers. Keep the visual experience full-screen,
quiet, and faithful to the supplied art and timing behavior.
# Starry Journey

A native Rust rebuild of the *Starry Night* pixel-art night-city screensaver and meeting/work-break display. It uses original, seeded procedural art and is inspired by the general idea of After Dark's *Starry Night*. It is built with [wgpu](https://wgpu.rs/) and [winit](https://github.com/rust-windowing/winit) for Windows, macOS, and Linux.

The planned app has two modes:

- **Normal:** a city assembles cell by cell, then its windows flicker under twinkling stars.
- **Endless:** a fully built city scrolls forever in four parallax layers, with clouds, a road, and traffic.

It will have 12 live-switchable retro palettes with palette-specific display effects, an optional on-screen message, and duration or clock-time timers.

## Status

Early rebuild. The app now draws a real generated city in Normal mode, building it up one virtual pixel at a time over 4 minutes above a street lined with trees, under a moon that crosses the sky every 10 minutes. Once the city is finished, cars drive by with their headlights on (building lights are steady for now). The pure city model is done: the deterministic random generator, the window-derived grid, all 12 palettes, and full city generation. City generation covers 28 building kinds (the spec's 19 plus 9 additions), masts, tints, windows, landmarks, and both Normal and Endless layouts, and is checked by the spec's first acceptance test. Window flicker, blinking beacons, and twinkling stars are next.

Each launch uses a new random city.

## Requirements

- Rust stable, installed with [rustup](https://rustup.rs/). The project is developed with Rust 1.90; the minimum supported version hasn't been tested.
- A GPU and driver supported by wgpu: Vulkan, Metal, DirectX 12, or OpenGL.

## Run

```powershell
cargo run
```

| Key | Action |
| --- | --- |
| `P` | Next palette (its name shows in the message box for 2 seconds) |
| `R` | New city (construction restarts) |
| `C` | Cycle the cloud cover (its name shows in the message box) |
| `W` | Cycle what falls in showers: rain, snow, or nothing |
| `D` | Toggle the palette display effects (scanlines, color bleed, RGB stripes; the Game Boy grid always stays) |
| `Shift+S` | Test key: a shooting star now |
| `Shift+E` | Test key: a police car now (once traffic runs) |
| `Shift+U` | Test key: a utility truck now (once traffic runs) |
| `Shift+L` | Test key: a lightning strike on the tallest point now (any cover) |
| `Shift+B` | Test key: a cloud-to-cloud bolt now |
| `Shift+I` | Test key: an in-cloud flicker now |
| `Shift+R` | Test key: a shower now (eases in over a minute; at least overcast strength) |
| `E` | Switch between Normal and Endless mode |
| `Space` | Pause or resume Endless scrolling |
| `+` / `-` | Construction speed, 0.25× to 4× |
| `F` | Toggle fullscreen on the selected display |
| `Tab` | Select the next display |
| `Esc` | Quit |

To preview the building shapes as text (until the renderer draws them):

```powershell
cargo run --bin shapes -- 15 40 1   # width, height, seed
```

To preview a whole generated city as text:

```powershell
cargo run --bin city -- 160 60 1    # columns, rows, seed
```

To see every building kind side by side (GitHub issue #2), labelled, in three rows: the 19 kinds shared with the Python version at a typical width, the same at their minimum widths, and the 9 Starry Journey extensions:

```powershell
cargo run --release --bin gallery -- 1 classic 3   # seed, palette, screen pixels per cell
```

It writes `target/building-gallery/gallery-<seed>-<palette>.png` and prints the path. To compare whole skylines with the Python version, run the app with `--buildings baseline --pixel-size spec`.

To preview the procedural clouds as an image (BMP, opens in Windows Photos):

```powershell
cargo run --release --bin clouds -- all 1 vga   # preset (or all), seed, palette, [moon phase 0-7]
```

It writes `target/cloud-preview/<preset>-<seed>-<palette>.bmp` and prints the path.

To see the real app at chosen moments without opening a window (snapshot mode):

```powershell
cargo run --release -- --snapshot 0,30s,12m --clouds storm --seed 7
```

It runs the app on simulated time through the real GPU shader and writes one PNG per moment into `target/snapshots/<YYYYMMDD-HHMMSS>/`, printing each path. The same options and seed always give the same pictures. Runs take longer for storms and big sizes (a 12-minute storm at 1920x1080 took about 6 minutes); `--size 960x540` is about four times faster.

### Options

| Option | Effect |
| --- | --- |
| `--palette KEY` | Start in a palette: `classic` (default), `artdeco`, `blackonwhite`, `cga`, `cga2`, `ega`, `vga`, `c64`, `appleii`, `gameboy`, `amber`, `green`. `P` cycles. |
| `--pixel-size N` | Each virtual pixel is N screen pixels (1–16, times display scaling). Default 3. Bigger windows show more city and more sky. |
| `--pixel-size spec` | Scale pixels with screen height instead, so every screen shows about 240 virtual rows (the Rebuild Prompt formula). |
| `--timer D`, `--deadline D` | Finish building the city in exactly D, whatever the speed. A bare number is minutes; `90s`, `25m`, `1h30m` also work. |
| `--buildings B` | Building kinds: `all` (default), or `baseline`: only the 19 kinds shared with the Python version, at its per-layer widths (no extension kinds, no wide footprints), for comparing the two. |
| `--clouds C` | Cloud cover: `clear`, `puffs`, `scattered` (default), `broken`, `overcast`, or `storm`. |
| `--endless` | Endless mode: the city is built, then scrolls forever in four parallax layers. Silly status lines show while it builds (unless `--message` is set). Works with `--timer`. |
| `--lightning L` | Lightning in storms: `auto` (default) or `off`. Flashes are soft and capped well under three a second. |
| `--message TEXT` | Show TEXT in a box over the city, for example `--message "This meeting will start shortly"`. Up to 240 UTF-16 units; blank text shows no box. Shown literally (no markup). |
| `--precip P` | What falls in showers: `rain` (default), `snow`, or `off`. Press `W` to cycle it. Every 10 minutes overcast clouds have a 10% chance of a shower and storms 60%; a shower eases in over a minute, holds 5 minutes, and eases out over a minute. |
| `--moon-phase P` | Phase the moon opens on: `new`, `waxing-crescent`, `first-quarter`, `waxing-gibbous`, `full`, `waning-gibbous`, `last-quarter`, `waning-crescent` (or 0–7). It advances one phase per 10-minute lap. |
| `--station-fate F` | On the new-moon lap a battle station is under attack: `chance` (30% it is destroyed, default), `destroy` (blows up about 15 s after opening on that lap), or `survive`. |
| `--searchlights S` | Ground searchlights: `auto` (now and then a show of two or three sweeping beams for up to 2 minutes, very rarely in Endless; default), `always`, or `off`. |
| `--effects E` | Palette display effects: `on` (default) or `off`. `D` toggles them. |
| `--events E@T,...` | Fire test events at moments of the run, as the Shift test keys do: `meteor`, `police`, `utility`, `strike`, `bolt`, `flicker`, `shower`. For example `--events strike@30s --snapshot 30.15s` (moments here and in `--snapshot` may be decimal seconds, such as `30.15s`, to catch a flash). |
| `--seed N` | Build city number N (0 to 4294967295) instead of a random one. The same seed gives the same city and clouds. |
| `--snapshot T,...` | Open no window; save a PNG of each moment T (durations as for `--timer`, or `0`). See above. |
| `--size WxH` | Snapshot size in pixels (default `1920x1080`; each side 16 to 8192). Only with `--snapshot`. |
| `--snapshot-dir D` | Folder for snapshot runs (default `target/snapshots`). Only with `--snapshot`. |
| `-h`, `--help` | Show usage. |

Pass options after `--` when using cargo: `cargo run -- --pixel-size 3`.

## Test

```powershell
cargo fmt --check
cargo clippy --all-targets
cargo test
```

`cargo test` runs the model and rasterizer unit tests, the shader validation test, and [tests/palette_spec.rs](tests/palette_spec.rs). That last test checks every palette value against the JSON in the spec.

## Git workflow (feature branches)

`main` always builds and passes its tests. Each feature is developed on its own branch and merged back with a merge commit.

1. Start from an up-to-date `main`: `git switch main`.
2. Create the feature branch: `git switch -c feature/<short-name>`, for example `feature/building-generation`.
3. Commit in small, working steps using [Conventional Commits](https://www.conventionalcommits.org/) prefixes: `feat:`, `fix:`, `test:`, `docs:`, `refactor:`, `chore:`.
4. Before merging, run the checks in [Test](#test) on the branch.
5. Merge: `git switch main`, then `git merge --no-ff feature/<short-name>`. `--no-ff` always creates a merge commit, so each feature remains visible as a unit in `git log --graph`.
6. Delete the merged branch: `git branch -d feature/<short-name>`.

Never commit `target/` or music files in `Sounds/Music/`; `.gitignore` excludes both.

## Layout

| Path | Contents |
| --- | --- |
| [src/post.wgsl](src/post.wgsl) | The display-effects pass: each palette's monitor effect (Amber, Apple II, C64, CGA scanlines and color, the Game Boy grid), then the message box. |
| [src/main.rs](src/main.rs), [src/present.wgsl](src/present.wgsl) | Window, input, fullscreen/monitor handling, and GPU presentation of the city buffer; also snapshot mode's offscreen rendering on simulated time. |
| [src/snapshot.rs](src/snapshot.rs) | Snapshot mode helpers: a dependency-free PNG writer and run folder names. |
| [src/message.rs](src/message.rs), [src/fonts/](src/fonts/) | Message box: `--message` validation, the palette-name notice, text wrapping, and CPU rasterization with the bundled Noto Sans font (SIL OFL 1.1, notice in `src/fonts/NotoSans-OFL.txt`). |
| [src/clouds.rs](src/clouds.rs) | Cloud shapes, coverage presets, wind and parallax, and moonlit four-tone drawing. |
| [src/weather.rs](src/weather.rs) | Rain, snow, and wind: the four precipitation layers' travel and look, gusts, and the slant. The shader draws the drops. |
| [src/lightning.rs](src/lightning.rs) | Storm lightning: when flashes happen (capped for photosensitive viewers), their kinds, and bolt shapes. The shader draws them. |
| [src/searchlights.rs](src/searchlights.rs) | Ground searchlight shows: when they run, where the lights stand, and how they sweep. The shader draws the beams. |
| [src/ambience.rs](src/ambience.rs) | Ambience after construction: window flicker, blinking beacons, landmark light cycles and chases, twinkling stars, and the shooting-star schedule. |
| [src/cityglow.rs](src/cityglow.rs) | City glow on the clouds: each column's lit-window light, blurred into a glow row the shader reads under every cloud pixel. |
| [src/endless.rs](src/endless.rs), [src/quips.rs](src/quips.rs) | Endless mode's four scrolling layer cities with bounded buffers, and the startup quips. |
| [src/raster.rs](src/raster.rs) | CPU rasterizer: city to logical-grid RGBA buffer with ownership arrays. |
| [src/lib.rs](src/lib.rs), [src/model/](src/model/) | Pure city model with no graphics dependencies: `rng` (Mulberry32 and stream seeds), `grid` (logical cell grid), `palette` (palette data and window tones), `building` (building, tier, and window records), `layers` (per-layer generator parameters), `shapes` (tier builders for each building kind), `mast`, `tint`, `windows`, `decorations`, and `city` (the generator that assembles them). |
| [src/bin/shapes.rs](src/bin/shapes.rs), [src/bin/city.rs](src/bin/city.rs) | Text previews of building shapes and of a whole generated city. |
| [src/bin/gallery.rs](src/bin/gallery.rs) | The building gallery: every kind drawn with the real rasterizer, labelled, as a PNG. |
| [tests/](tests/) | Integration tests. |
| [docs/Rebuild Prompt.md](docs/Rebuild%20Prompt.md) | Primary behavioral specification, carried over from the Python/browser version. |
| [docs/product.md](docs/product.md) | Product scope, Rust-specific deviations, and deferred extensions. |
| [docs/work.md](docs/work.md) | Current and completed work increments with verification results. |
| [docs/user-notes.md](docs/user-notes.md) | The owner's reminders, to-do list, and git tips. |
| `.ai/` | The agent handoff note (tracked) and local reference images (gitignored). |
| [Sounds/Music/](Sounds/Music/README.md) | User-supplied background music for a planned later feature. Gitignored except its README; not used by the app yet. |

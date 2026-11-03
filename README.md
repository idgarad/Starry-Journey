# Starry Journey

A native Rust rebuild of the *Starry Night* pixel-art night-city screensaver and meeting/work-break display. It uses original, seeded procedural art and is inspired by the general idea of After Dark's *Starry Night*. It is built with [wgpu](https://wgpu.rs/) and [winit](https://github.com/rust-windowing/winit) for Windows, macOS, and Linux.

The planned app has two modes:

- **Normal:** a city assembles cell by cell, then its windows flicker under twinkling stars.
- **Endless:** a fully built city scrolls forever in four parallax layers, with clouds, a road, and traffic.

It will have 12 live-switchable retro palettes with palette-specific display effects, an optional on-screen message, and duration or clock-time timers.

## Status

Early rebuild. The app now draws a real generated city in Normal mode, building it up one virtual pixel at a time over 4 minutes (steady lights for now). The pure city model is done: the deterministic random generator, the window-derived grid, all 12 palettes, and full city generation. City generation covers 28 building kinds (the spec's 19 plus 9 additions), masts, tints, windows, landmarks, and both Normal and Endless layouts, and is checked by the spec's first acceptance test. Window flicker, blinking beacons, and twinkling stars are next.

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
| `P` | Next palette |
| `R` | New city (construction restarts) |
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

### Options

| Option | Effect |
| --- | --- |
| `--pixel-size N` | Each virtual pixel is N screen pixels (1–16, times display scaling). Default 3. Bigger windows show more city and more sky. |
| `--pixel-size spec` | Scale pixels with screen height instead, so every screen shows about 240 virtual rows (the Rebuild Prompt formula). |
| `--timer D`, `--deadline D` | Finish building the city in exactly D, whatever the speed. A bare number is minutes; `90s`, `25m`, `1h30m` also work. |
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
| [src/main.rs](src/main.rs), [src/present.wgsl](src/present.wgsl) | Window, input, fullscreen/monitor handling, and GPU presentation of the city buffer. |
| [src/raster.rs](src/raster.rs) | CPU rasterizer: city to logical-grid RGBA buffer with ownership arrays. |
| [src/lib.rs](src/lib.rs), [src/model/](src/model/) | Pure city model with no graphics dependencies: `rng` (Mulberry32 and stream seeds), `grid` (logical cell grid), `palette` (palette data and window tones), `building` (building, tier, and window records), `layers` (per-layer generator parameters), `shapes` (tier builders for each building kind), `mast`, `tint`, `windows`, `decorations`, and `city` (the generator that assembles them). |
| [src/bin/shapes.rs](src/bin/shapes.rs), [src/bin/city.rs](src/bin/city.rs) | Text previews of building shapes and of a whole generated city. |
| [tests/](tests/) | Integration tests. |
| [docs/Rebuild Prompt.md](docs/Rebuild%20Prompt.md) | Primary behavioral specification, carried over from the Python/browser version. |
| [docs/product.md](docs/product.md) | Product scope, Rust-specific deviations, and deferred extensions. |
| [docs/work.md](docs/work.md) | Current and completed work increments with verification results. |
| [docs/user-notes.md](docs/user-notes.md) | The owner's reminders, to-do list, and git tips. |
| `.ai/` | The agent handoff note (tracked) and local reference images (gitignored). |
| [Sounds/Music/](Sounds/Music/README.md) | User-supplied background music for a planned later feature. Gitignored except its README; not used by the app yet. |

# Starry Journey

A native Rust rebuild of the *Starry Night* pixel-art night-city screensaver and meeting/work-break display. It uses original, seeded procedural art and is inspired by the general idea of After Dark's *Starry Night*. It is built with [wgpu](https://wgpu.rs/) and [winit](https://github.com/rust-windowing/winit) for Windows, macOS, and Linux.

The planned app has two modes:

- **Normal:** a city assembles cell by cell, then its windows flicker under twinkling stars.
- **Endless:** a fully built city scrolls forever in four parallax layers, with clouds, a road, and traffic.

It will have 12 live-switchable retro palettes with palette-specific display effects, an optional on-screen message, and duration or clock-time timers.

## Status

Early rebuild. The pure model foundation is done: the deterministic random generator, the window-derived logical grid, and all 12 palettes, verified against the spec. Building generation and rendering are next.

The running window still shows an earlier placeholder: a fixed 320 × 180 gradient sky. The Normal-mode rasterizer will replace it.

## Requirements

- Rust stable, installed with [rustup](https://rustup.rs/). The project is developed with Rust 1.90; the minimum supported version hasn't been tested.
- A GPU and driver supported by wgpu: Vulkan, Metal, DirectX 12, or OpenGL.

## Run

```powershell
cargo run
```

`F` toggles fullscreen on the selected display, `Tab` selects the next display, and `Esc` quits.

## Test

```powershell
cargo fmt --check
cargo clippy --all-targets
cargo test
```

`cargo test` runs the model unit tests, the shader validation tests, and [tests/palette_spec.rs](tests/palette_spec.rs). That last test checks every palette value against the JSON in the spec.

## Git workflow (feature branches)

`main` always builds and passes its tests. Each feature is developed on its own branch and merged back with a merge commit.

1. Start from an up-to-date `main`: `git switch main`.
2. Create the feature branch: `git switch -c feature/<short-name>`, for example `feature/building-generation`.
3. Commit in small, working steps using [Conventional Commits](https://www.conventionalcommits.org/) prefixes: `feat:`, `fix:`, `test:`, `docs:`, `refactor:`, `chore:`.
4. Before merging, run the checks in [Test](#test) on the branch.
5. Merge: `git switch main`, then `git merge --no-ff feature/<short-name>`. `--no-ff` always creates a merge commit, so each feature remains visible as a unit in `git log --graph`.
6. Delete the merged branch: `git branch -d feature/<short-name>`.

Never commit `target/` or the MP3s in `Sounds/Music/`; `.gitignore` excludes both.

## Layout

| Path | Contents |
| --- | --- |
| [src/main.rs](src/main.rs) | Window, GPU surface, fullscreen/monitor handling, and the current placeholder renderer. |
| [src/lib.rs](src/lib.rs), [src/model/](src/model/) | Pure city model with no graphics dependencies: `rng` (Mulberry32 and stream seeds), `grid` (logical cell grid), `palette` (palette data and window tones). |
| [src/*.wgsl](src/) | Placeholder sky and presentation shaders, to be replaced. |
| [tests/](tests/) | Integration tests. |
| [docs/Rebuild Prompt.md](docs/Rebuild%20Prompt.md) | Primary behavioral specification, carried over from the Python/browser version. |
| [docs/product.md](docs/product.md) | Product scope, Rust-specific deviations, and deferred extensions. |
| [docs/work.md](docs/work.md) | Current and completed work increments with verification results. |
| `.ai/` | Visual references and the agent handoff note. |
| `Sounds/Music/` | Background music for a planned later extension. It is not used by the app yet. |

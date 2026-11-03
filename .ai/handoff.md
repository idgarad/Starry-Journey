# Handoff

- Primary spec: [docs/Rebuild Prompt.md](../docs/Rebuild%20Prompt.md). Rust-specific deviations and owner decisions are in [docs/product.md](../docs/product.md); work history and verification are in [docs/work.md](../docs/work.md).
- State (2026-11-03): Normal mode is feature-complete for now. Merged features:
  - the city reveal;
  - 3 px pixels by default (`--pixel-size`);
  - `--timer`/`--deadline`;
  - street and trees in all palettes;
  - a 39-cell moon with 10-minute laps, per-lap phases, and `--moon-phase`;
  - the battle-station easter egg with fighters, a red trench run, and `--station-fate`;
  - traffic with headlights, road beams, and lane-paired tree lighting.

  162 tests pass; clippy is clean.
- **Next session:** start Endless mode (owner request). `feature/normal-ambience` (flicker, beacons, landmark cycling, twinkles, shooting stars) is still planned.
- Open decisions:
  - the native UI approach for controls (`egui` or keyboard only);
  - adopting the rest of the `AGENTS.md` process;
  - whether the battle-station homage stays on by default if the repo goes public.
- Git: feature branches (`feature/<name>`, `fix/<name>`) merged with `--no-ff`. Stage explicit paths, never `git add -A`. The owner pushes; the agent never does.
- Builds: the agent uses `CARGO_TARGET_DIR=target/agent` so it never collides with the owner's running `cargo run`. Don't run GUI captures that send keys or steal focus while the owner is at the machine.

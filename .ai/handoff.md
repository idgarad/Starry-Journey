# Handoff

- Primary spec: [docs/Rebuild Prompt.md](../docs/Rebuild%20Prompt.md), with Rust-specific deviations and deferred extensions recorded in [docs/product.md](../docs/product.md). The photo skyline is dropped. Background music (`Sounds/Music/`) is a deferred extension.
- Last completed: REBUILD-001 (RNG, grid, and palettes in `src/model/`), described in [docs/work.md](../docs/work.md). 24 tests pass.
- The running app still shows SKY-001's 320 × 180 gradient, which is superseded and will be replaced in the Normal-mode rasterizer step.
- Next candidates are listed in work.md. Building generation (REBUILD-002) is the natural next step.
- Open decisions: native UI approach for controls (`egui` or keyboard only); adopting the rest of the `AGENTS.md` process (`Docs/` layout, backlog).
- Git: feature-branch workflow. Work on `feature/<name>` branches and merge to `main` with `git merge --no-ff`. See the README "Git workflow" section. `Sounds/Music/` is gitignored except its README; users supply their own music.

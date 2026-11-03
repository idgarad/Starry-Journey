# Handoff

- Primary spec: [docs/Rebuild Prompt.md](../docs/Rebuild%20Prompt.md), with Rust-specific deviations and deferred extensions recorded in [docs/product.md](../docs/product.md). The photo skyline is dropped. Background music (`Sounds/Music/`) is a deferred extension.
- Last completed: `feature/night-traffic` committed on its branch, UNMERGED pending owner visual review (traffic after completion, headlights with road beams). 161 tests pass. Next: `feature/normal-ambience`.
- Rendering: CPU raster (`src/raster.rs`) uploaded as a texture; `present.wgsl` scales each cell to an exact cell×cell block.
- Next candidates are listed in work.md. `feature/normal-ambience` (flicker after completion, beacons, landmark cycling, twinkles, shooting stars) is next.
- Open decisions: native UI approach for controls (`egui` or keyboard only); adopting the rest of the `AGENTS.md` process (`Docs/` layout, backlog).
- Git: feature-branch workflow. Work on `feature/<name>` branches and merge to `main` with `git merge --no-ff`. See the README "Git workflow" section. `Sounds/Music/` is gitignored except its README; users supply their own music.

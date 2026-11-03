# Notes for the end user

Your personal reference for working on Starry Journey: reminders, a to-do list, and tips. Edit freely; it's yours.

## Reminders

### The feature loop

```
git switch main                          # start from the stable line
git pull                                 # get anything new from GitHub
git switch -c feature/<short-name>       # new branch for this feature
# ...edit, then repeat as needed:
git status                               # what changed?
git add <files>                          # stage (or: git add -A for everything)
git commit -m "feat: what this step does"
# ...when the feature is finished and checks pass:
git switch main
git merge --no-ff feature/<short-name>   # always make a merge commit
git branch -d feature/<short-name>       # remove the merged label
git push                                 # send main to GitHub
```

### Before merging a feature

- [ ] `cargo fmt --check` (formatting)
- [ ] `cargo clippy --all-targets` (lint)
- [ ] `cargo test` (all tests pass)
- [ ] `git status` is clean, so nothing is left uncommitted
- [ ] Docs updated if behavior changed (`docs/work.md`, README)

### Commit message prefixes

| Prefix | Use for |
| --- | --- |
| `feat:` | New behavior |
| `fix:` | Bug fix |
| `test:` | Tests only |
| `docs:` | Documentation only |
| `refactor:` | Restructuring without changing behavior |
| `chore:` | Housekeeping (config, ignore files, dependencies) |

Write the message as what the commit *does*: `feat: add building generator`, not `added stuff`.

## To-do (decisions waiting on you)

- [ ] **Native UI for controls:** an immediate-mode library such as `egui` (full toolbar, dial, message box) or keyboard-only controls. Needed before the "timing and UI" step.
- [ ] **Green Terminal glyphs:** source GNU Unifont 16.0.04 bitmaps (SIL OFL 1.1 licence) before the display-effects step.
- [ ] **Background music:** pick an audio crate and decide on controls (mute, volume, track choice). Deferred until the rebuild baseline is done.
- [ ] **AGENTS.md process:** decide whether to adopt the rest of it for this project (`Docs/` capitalization, backlog, decision log) or keep the lighter `docs/work.md` approach.
- [ ] **Battle-station easter egg:** it's a homage to the Death Star (Lucasfilm's design). Decide whether it stays on by default if the repo or builds go public.
- [ ] **GitHub visibility:** if the repository is public, remember that everything committed is public. `Sounds/Music/` is already ignored.

## Tips

### Doing it in VS Code instead of the terminal

Open **Source Control** with the branch icon in the left bar, or `Ctrl+Shift+G`. The branch name is shown at the bottom-left of the status bar.

| Terminal | VS Code |
| --- | --- |
| `git status` | The Source Control panel lists changed files: **M** modified, **U** untracked, **A** added. |
| `git diff` | Click a changed file to see a side-by-side diff. |
| `git add <file>` | Hover the file and click **+** (Stage Changes). |
| `git commit -m "..."` | Type the message in the box at the top, then click **Commit**. |
| `git switch -c feature/x` | Click the branch name in the status bar, choose **Create new branch...**, and type `feature/x`. |
| `git switch main` | Click the branch name in the status bar and pick `main`. |
| `git merge --no-ff feature/x` | `...` menu in Source Control → **Branch → Merge...** and pick the feature branch. This makes a merge commit only if the one-time setup below is done; otherwise use the terminal. |
| `git branch -d feature/x` | `...` menu → **Branch → Delete Branch...** |
| `git push` / `git pull` | **Sync Changes** button, or the ↑↓ arrows in the status bar. |
| `git log --graph` | The **Source Control Graph** section under the changes list. |

**One-time setup so VS Code merges like `--no-ff`.** Run this in the terminal from the project folder:

```
git config merge.ff false
git config pull.ff only
```

- The first line makes every merge create a merge commit, including merges started from VS Code's menu.
- The second line keeps `git pull` and Sync from creating unexpected merge commits. If GitHub has changes you don't have locally, the pull stops and asks you to deal with them instead of merging silently.
- Both settings apply only to this repository.

**Watch the branch name before you commit.** If the status bar says `main`, the commit goes straight onto `main`. When you're starting a feature, create the branch *first*.

### Looking around (safe: these change nothing)

| Command | Shows |
| --- | --- |
| `git status` | Current branch and changed, staged, or untracked files. Run it often. |
| `git log --oneline --graph --all` | History as a drawing, including other branches. |
| `git diff` | Unstaged changes. |
| `git diff --staged` | What the next commit will contain. |
| `git branch` | Local branches; `*` marks the current one. |
| `git remote -v` | Where `push` and `pull` go (GitHub). |

### Undo recipes

| Situation | Command |
| --- | --- |
| Unstage a file but keep its edits | `git restore --staged <file>` |
| Throw away unsaved edits to a file (**permanent**) | `git restore <file>` |
| Fix the message of the last commit, before pushing | `git commit --amend -m "new message"` |
| Undo a commit that was already pushed | `git revert <commit-id>` (adds a new commit that reverses it) |
| Abandon a merge that has conflicts | `git merge --abort` |
| Delete an unmerged branch you don't want (**permanent**) | `git branch -D feature/<name>` |

Avoid `git reset --hard` and `git push --force` until you're comfortable. They can discard work.

### Merge conflicts

If a merge stops with `CONFLICT`, Git marks the clashing lines in the file:

```
<<<<<<< HEAD
the version on main
=======
the version on your feature branch
>>>>>>> feature/<name>
```

Edit the file to keep the right result and delete the marker lines, then run `git add <file>` and `git commit`. VS Code shows **Accept Current / Accept Incoming / Accept Both** buttons above each conflict.

### Line-ending warnings

`LF will be replaced by CRLF` is harmless on Windows. The repository stores LF line endings, and Git converts them to Windows CRLF in your working files.

### Running the app

- The window opens at 1920 × 1080 (our assumed test screen); at 3-pixel cells that is 640 × 360 virtual pixels.

- The title bar shows build progress, speed, and `columns x rows @ pixel size`. Pixels are 3 screen pixels by default, so maximizing adds city. `--pixel-size spec` makes pixels scale with window height instead.
- `cargo run -- --deadline 10s` builds the whole city in 10 seconds (testing); `--deadline 10` means 10 minutes (meetings). The title shows the time left.
- `cargo run -- --moon-phase new --station-fate destroy` shows the station under attack and blowing up about 12 seconds in (`survive` keeps it; default is a 30% chance).
- `cargo run -- --moon-phase full` opens on a full moon; `--moon-phase new` shows the battle station right away. Names: new, waxing-crescent, first-quarter, waxing-gibbous, full, waning-gibbous, last-quarter, waning-crescent (or 0-7).
- `cargo run -- --pixel-size 4` tries another pixel size (default 3); `cargo run -- --pixel-size spec` uses the spec's about 240 rows. The `--` tells cargo that the options are for the app, not for cargo.

- `cargo run` builds and launches the app.
- `P` next palette, `R` new city, `+`/`-` speed (0.25× to 4×; 4× builds the city in 1 minute), `F` fullscreen, `Tab` next monitor, `Esc` quit. The title bar shows build progress.
- `cargo run --bin shapes -- 15 40 1` previews the building shapes as text (width, height, seed). `o` marks a window that will be lit and `_` one that stays dark. Try different seeds to see variations.
- `cargo run --bin city -- 160 60 1` previews a whole city (columns, rows, seed). Layers draw as `.` `:` `+` `#` from far to near.

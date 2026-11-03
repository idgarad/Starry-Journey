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
- [ ] **GitHub visibility:** if the repository is public, remember that everything committed is public. `Sounds/Music/` is already ignored.

## Tips

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

- `cargo run` builds and launches the app.
- `F` toggles fullscreen, `Tab` selects the next monitor, and `Esc` quits.

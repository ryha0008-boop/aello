# Recording the demo GIF

A short asciinema → GIF for the README and the launch post. Two takes: the TUI
(the visual hero shot) and the CLI isolation story (the proof). Record whichever
sells best; the TUI one is the better top-of-README hero.

## One-time setup

```sh
# Linux
sudo apt install asciinema           # or: brew install asciinema
cargo install --git https://github.com/asciinema/agg   # cast -> gif
```

Keep the terminal small (≈90×28) so the GIF stays crisp and embeds well.

## Take A — the TUI hero shot (~20s, no Claude login needed)

The TUI never launches Claude until you hit `↵`, so this records cleanly with no
auth. Rehearse the keystrokes once, then:

```sh
asciinema rec --idle-time-limit 1.5 -c 'aello' tui.cast
```

Suggested sequence (slow, deliberate — let each screen breathe ~1s):
1. Land on the registry (the "Kinetic Command" palette).
2. `A` → walk the guided add: name → model → persona → capability checklist.
3. Back on the list, `?` → scroll the in-app docs reader, `Esc`.
4. `F` → toggle the placed-here filter.
5. `Q` to quit (ends the recording).

```sh
agg --theme monokai tui.cast docs/assets/demo-tui.gif
```

## Take B — the isolation story (~30s, needs `aello login` done)

Shows the actual pitch: two blueprints in one repo that don't clobber each other.

```sh
asciinema rec --idle-time-limit 1.5 demo-cli.cast
# then, inside the recording shell:
aello add backend  --model opus   --claude-md coder    --github
aello add frontend --model sonnet --claude-md coder    --github
aello list
mkdir -p /tmp/demo-app && cd /tmp/demo-app && git init -q
aello run backend  -p 'print a one-line hello as the backend agent'
aello run frontend -p 'print a one-line hello as the frontend agent'
# the money shot — two isolated envs + a per-blueprint tracked mirror:
ls -a                                    # .claude-env-backend  .claude-env-frontend
ls claude-internal                       # backend/  frontend/  (namespaced, no clobber)
git log --format='%an %s' | head         # commits attributed per blueprint
exit
```

```sh
agg --theme monokai demo-cli.cast docs/assets/demo-cli.gif
```

## Wire it into the README

Add under the tagline (top of `README.md`):

```md
![aello demo](docs/assets/demo-tui.gif)
```

Commit the `.gif` under `docs/assets/` (create the dir). Keep each GIF under
~2 MB so GitHub renders it inline; trim with `agg --speed 1.3` or a tighter
`--idle-time-limit` if needed. Delete the `.cast` files after converting.

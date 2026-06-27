# Contributing to aello

Thanks for your interest! aello is a small, focused Rust CLI, and contributions of all sizes are welcome — bug reports, docs fixes, and features alike.

## Before you start

- **Architecture lives in [CLAUDE.md](CLAUDE.md).** It documents every module (`src/` map), the blueprint/capability model, the env-dir layout, auth, contextdb, and the release flow. Read the relevant section before changing behavior — it'll save you reverse-engineering.
- **Good first issues** are labeled [`good first issue`](https://github.com/ryha0008-boop/aello/labels/good%20first%20issue). Comment to claim one.
- For anything non-trivial, **open an issue first** to align on the approach before writing code.

## Dev loop

aello is plain Rust — no extra toolchain. Requires a recent stable Rust ([rustup](https://rustup.rs/)).

```sh
git clone https://github.com/ryha0008-boop/aello
cd aello
cargo build --release      # writes to target/ — safe even while aello is running
cargo test                 # unit tests, no Claude launch needed
cargo install --path . --force   # try your build as the real `aello`
```

Most logic (templates, placement, launch, capability scaffolding) is unit-testable without ever launching Claude Code — see the `#[cfg(test)]` blocks in `src/`.

## Conventions

These are enforced by review (and largely mirror [CLAUDE.md](CLAUDE.md)'s development rules):

- **`cargo build --release` and `cargo test` must be green** before you open a PR.
- **Add a test for new behavior.** Template/placement/launch logic is all testable in-process.
- **Every user-facing change gets a [CHANGELOG.md](CHANGELOG.md) entry**, in the same commit as the code.
- **Keep docs in sync.** `README.md` for user-facing commands/capabilities; `docs/` for deeper reference (it's also the in-app help via `aello docs`, so a new `docs/*.md` appears automatically). Update whatever your change touches.
- **Small, scoped commits** — one logical change per commit; don't bundle unrelated edits.
- **Surgical edits** — minimum code that solves the problem; match the surrounding style; touch only what's required.

## Pull requests

1. Fork and branch off `main`.
2. Make your change, with tests and a changelog entry.
3. Ensure `cargo build --release` and `cargo test` pass.
4. Open the PR with a clear description of *what* and *why*. Link the issue it closes.

CI bumps the patch version on merge to `main` automatically — **don't bump `Cargo.toml`'s version yourself** (see the release process in [CLAUDE.md](CLAUDE.md)).

## License

By contributing, you agree that your contributions will be dual licensed under [MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE), matching the project license.

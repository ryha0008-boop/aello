# aello landing page

A static Next.js site. Nothing in the CLI depends on it — it builds to plain HTML.

```sh
npm install
npm run dev     # http://localhost:3000
npm run build   # static HTML in out/
```

## Design system

The **structure** — token names, the type scale, spacing, radii, motion — comes from a
captured GitHub design system (`agent_run_results/design-systems/github1/DESIGN-SYSTEM.md`)
and is transcribed into the `:root` block of `app/globals.css`. **Edit that file rather
than hard-coding values in a component**, and keep the token names as captured so the two
stay comparable.

The **colours** are no longer the captured ones. They were retuned to the brand: orange on
near-black — one warm accent family over neutral greys, nothing pure `#000` (it flattens
every border and shadow drawn on it). The token count is unchanged; three tokens that named
their own hue were renamed, because `--color-decorative-indigo` holding an orange is worse
than no name at all:

| Was | Now |
|---|---|
| `--color-decorative-indigo` | `--color-decorative-orange-mid` |
| `--color-decorative-purple-soft` | `--color-decorative-orange-soft` |
| `--color-decorative-purple-mid` | `--color-decorative-rust` |
| `--color-canvas-green-subtle` | `--color-canvas-accent-subtle` |
| `--color-canvas-green-dark` | `--color-canvas-accent-dark` |

Values were measured rather than eyeballed — the old green palette shipped a pair below AA
(`--color-fg-subtle` on `--color-canvas-subtle`, 2.79:1) and the retune fixed it. `/design-system`
reports every ratio.

**`/design-system` renders the whole thing**, generated from `app/globals.css` at build
time by `lib/design-system.ts` — swatches, type specimens, easing curves, the spacing
ramp, per-token usage counts, measured WCAG contrast, and the breakpoints. There is no
second copy to update: change a value in `globals.css` and the page follows.

Two rules are **enforced by the build**, not merely documented, because a convention
nobody can violate accidentally is worth more than one written down:

1. **No literal colour outside `globals.css`.** A hex or `rgba()` in any component
   stylesheet throws during `next build`, naming the file, the line, and the fix.
2. **Only the documented breakpoints** (`48rem`, `63.25rem`, plus the component-local
   `34rem`). Custom properties don't work inside `@media`, so the literals are repeated
   per file — which is exactly why an invented one has to fail loudly. A stray `64rem`
   in the docs stylesheet had already opened a 0.75rem band where that page laid out
   differently from every other.

Both guards were checked by introducing a violation and watching the build fail, not by
reading the code. Adding a new breakpoint means adding it to `BREAKPOINTS` in
`lib/design-system.ts`, which also publishes it on the page.

A third invariant guards the parser itself: **unbalanced `/*` and `*/` inside `:root` fail
the build.** An unterminated comment swallows every declaration up to the next close, and
the page then renders the smaller set as though that were the design system — which is
exactly what happened here (six tokens vanished, silently, and the count looked plausible).
Counting parsed-vs-declared tokens *cannot* catch it, since both sides share the same
comment model and agree by construction; marker balance is what distinguishes the two
states.

Contrast is **reported, not enforced**: below 4.5:1 is legitimate for some pairs — a button
fill is not text, a display-only accent needs 3:1 — so the judgement belongs at the call
site. No pair fails AA today.

Type is Mona Sans (brand) with Hubot Sans for display headings, both self-hosted through
`next/font`. The system's `Mona Sans Mono` isn't published, so mono text falls back to the
`ui-monospace` stack the same system declares.

## Structure

| Path | What it is |
|---|---|
| `app/globals.css` | Design tokens, reset, and the shared `.container` / `.section` / `.eyebrow` primitives |
| `app/page.tsx` | Section order for the single page |
| `components/EnvDemo.tsx` | The interactive hero demo — blueprints, their env dirs, and their commits |
| `components/Feature.tsx` | Alternating text/visual row, used for the skills and voice sections |
| `components/Reveal.tsx` | Scroll-in reveal; the hidden state is gated on `@media (scripting: enabled)`, so the page still reads without JavaScript |
| `lib/docs.ts` | Reads `../docs/*.md` at build time — slug, title, rendered HTML, headings |
| `app/docs/` | The docs reader: index, `[slug]` page, and the shared stylesheet |
| `lib/design-system.ts` | Parses `globals.css` into tokens, counts their uses, measures contrast, and holds the two build-time guards |
| `app/design-system/` | The `/design-system` reference page |

## The docs pages

`/docs` is generated from the repo's **`docs/` directory** at build time — the same files
`docs.rs` embeds into the binary. There is no second copy: add a `.md` there and it appears
on the site, in `aello docs`, and in the TUI reader together.

`lib/docs.ts` owns the details worth knowing:

- **Reading order** mirrors `docs.rs::ORDER`; anything unlisted sorts after it alphabetically.
- **Heading ids** are GitHub-style slugs, so in-page links written in the markdown resolve.
- **Sibling `*.md` links** become site routes; a link that escapes `docs/` points at a repo
  file and is rewritten to GitHub rather than 404ing.
- `BLURBS` supplies the one-liners on the docs index — add an entry when you add a page.

The landing page's **Workflows** section resolves its anchors from the rendered headings of
`docs/workflows.md` rather than hardcoding them, so renaming a heading fails the build
instead of quietly leaving a dead link.

## Deploying

`npm run build` writes `out/`. Serve that directory as-is.

CI does this on every push to `main` that touches `site/**` or `docs/**`
(`.github/workflows/pages.yml`) and publishes to GitHub Pages. Because it's a **project**
site served from `/aello`, the workflow sets `NEXT_PUBLIC_BASE_PATH=/aello`; `next.config.ts`
and `lib/docs.ts` both read that variable, so local `npm run dev` stays at the root and a
custom domain later needs only the variable dropped.

**Pages has to exist before the first deploy.** The workflow passes `enablement: true` to
`actions/configure-pages`, but `GITHUB_TOKEN` is not allowed to *create* a Pages site —
it fails with `Resource not accessible by integration`. Enable it once, either in
Settings → Pages → Source → **GitHub Actions**, or with:

```sh
gh api -X POST repos/<owner>/<repo>/pages -f build_type=workflow
```

`enablement: true` stays because it is a no-op once the site exists and does work where the
token has the rights.

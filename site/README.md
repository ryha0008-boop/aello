# aello landing page

A static Next.js site. Nothing in the CLI depends on it — it builds to plain HTML.

```sh
npm install
npm run dev     # http://localhost:3000
npm run build   # static HTML in out/
```

## Design system

Colour, type, spacing, radii, and motion come from a captured GitHub design system
(`agent_run_results/design-systems/github1/DESIGN-SYSTEM.md`). Its tokens are transcribed
verbatim into the `:root` block of `app/globals.css` — **edit that file rather than
hard-coding values in a component**, and keep the token names as captured so the two stay
comparable.

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

**One-time manual step:** the repo's Settings → Pages → Source must be set to
**GitHub Actions**, or the deploy job fails.

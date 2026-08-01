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

## Deploying

`npm run build` writes `out/`. Serve that directory as-is.

For a GitHub Pages **project** site (`<user>.github.io/aello`) the assets need a path
prefix — set `basePath: '/aello'` and `assetPrefix: '/aello'` in `next.config.ts` first.
A user site or custom domain serving from the root needs no change.

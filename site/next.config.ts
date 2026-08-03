import type { NextConfig } from 'next'

// A GitHub Pages *project* site is served from `/<repo>`, so assets and routes
// need that prefix — but a local `npm run dev` and a custom domain do not. CI
// sets NEXT_PUBLIC_BASE_PATH; leaving it unset gives root-relative output.
// `lib/docs.ts` reads the same variable when rewriting links inside rendered
// markdown, so there is one place to change it.
const basePath = process.env.NEXT_PUBLIC_BASE_PATH || ''

const nextConfig: NextConfig = {
  // Static HTML in `out/` — no server needed to host the landing page.
  output: 'export',
  images: { unoptimized: true },
  ...(basePath ? { basePath, assetPrefix: basePath } : {}),
  // Emit `about/index.html` rather than `about.html`, so any static host serves
  // clean URLs without rewrite rules.
  trailingSlash: true,
}

export default nextConfig

import type { NextConfig } from 'next'

const nextConfig: NextConfig = {
  // Static HTML in `out/` — no server needed to host the landing page.
  output: 'export',
  images: { unoptimized: true },
  // Emit `about/index.html` rather than `about.html`, so any static host serves
  // clean URLs without rewrite rules.
  trailingSlash: true,
}

export default nextConfig

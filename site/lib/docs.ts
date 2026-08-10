import fs from 'node:fs'
import path from 'node:path'
import { Marked, type Tokens } from 'marked'

/**
 * The docs site is generated from the repo's `docs/` directory at build time —
 * the same files `docs.rs` embeds into the binary. There is no second copy to
 * keep in sync: add a `.md` there and it appears here, in `aello docs`, and in
 * the TUI reader together.
 */
const DOCS_DIR = path.join(process.cwd(), '..', 'docs')

/**
 * Reading order, mirroring `docs.rs::ORDER`. Anything not listed sorts after,
 * alphabetically — so a new doc still appears without touching this list.
 */
const ORDER = [
  'concepts',
  'roles',
  'workflows',
  'skills',
  'voice',
  'tokens',
  'cline',
  'upgrading',
  'migrate',
  'development',
  'troubleshooting',
]

/** One-line blurbs for the docs index; falls back to the page's first sentence. */
const BLURBS: Record<string, string> = {
  concepts: 'The isolation model, auth, and what lives where on disk.',
  roles: 'What each role owns, and what /sync does for it.',
  workflows: 'Task-shaped walkthroughs, start to finish.',
  skills: 'The four seeded slash commands, in detail.',
  voice: 'How every env speaks, and what to check when it does not.',
  tokens: 'What each env has spent, and what the cost figure does not claim.',
  cline: 'The second agent: what a Cline env gets, and what it cannot.',
  upgrading: 'Coming from a pre-0.2 setup? Read this once per environment.',
  migrate: 'Putting an existing repo onto aello.',
  development: 'Building, testing, and releasing aello itself.',
  troubleshooting: 'Failure modes and what they actually mean.',
}

/** Set in CI for the GitHub Pages project site; empty for local dev. */
export const BASE = process.env.NEXT_PUBLIC_BASE_PATH ?? ''

export type Heading = { id: string; text: string; depth: number }
export type Doc = {
  slug: string
  title: string
  blurb: string
  html: string
  headings: Heading[]
}

/** GitHub-style heading id, so in-page links written in the markdown resolve. */
function slugify(text: string): string {
  return text
    .toLowerCase()
    .replace(/<[^>]+>/g, '')
    .replace(/[^\w\s-]/g, '')
    .trim()
    .replace(/\s+/g, '-')
}

function rank(slug: string): number {
  const i = ORDER.indexOf(slug)
  return i === -1 ? ORDER.length : i
}

export function slugs(): string[] {
  return fs
    .readdirSync(DOCS_DIR)
    .filter((f) => f.endsWith('.md'))
    .map((f) => f.replace(/\.md$/, ''))
    .sort((a, b) => rank(a) - rank(b) || a.localeCompare(b))
}

/**
 * Rewrite the links the markdown was written with. Sibling `*.md` links are how
 * the pages cross-reference each other in `aello docs`; on the site they have to
 * become routes. A link that escapes `docs/` points at a repo file, so it goes
 * to GitHub rather than 404ing here.
 */
function rewriteHref(href: string): string {
  if (/^(https?:|mailto:|#)/.test(href)) return href
  if (href.startsWith('../')) {
    return `https://github.com/ryha0008-boop/aello/blob/main/${href.slice(3)}`
  }
  const m = href.match(/^([\w-]+)\.md(#.*)?$/)
  if (m) return `${BASE}/docs/${m[1]}/${m[2] ?? ''}`
  return href
}

function render(markdown: string): { html: string; headings: Heading[] } {
  const headings: Heading[] = []
  const marked = new Marked({ gfm: true })

  marked.use({
    renderer: {
      heading({ tokens, depth }: Tokens.Heading) {
        const text = this.parser.parseInline(tokens)
        const id = slugify(text)
        // Only h2/h3 reach the on-page contents; h1 is the page title.
        if (depth === 2 || depth === 3) headings.push({ id, text, depth })
        return `<h${depth} id="${id}">${text}</h${depth}>\n`
      },
      link({ href, title, tokens }: Tokens.Link) {
        const text = this.parser.parseInline(tokens)
        const to = rewriteHref(href)
        const external = /^https?:/.test(to)
        const attrs = [
          `href="${to}"`,
          title ? `title="${title}"` : '',
          external ? 'target="_blank" rel="noreferrer"' : '',
        ]
          .filter(Boolean)
          .join(' ')
        return `<a ${attrs}>${text}</a>`
      },
    },
  })

  // Tables can be wider than the column; wrap each so it scrolls on its own
  // rather than making the whole page scroll sideways on a phone.
  const html = (marked.parse(markdown) as string).replace(
    /<table>[\s\S]*?<\/table>/g,
    (t) => `<div class="tableWrap">${t}</div>`,
  )
  return { html, headings }
}

export function getDoc(slug: string): Doc {
  const raw = fs.readFileSync(path.join(DOCS_DIR, `${slug}.md`), 'utf8')
  const { html, headings } = render(raw)
  const title = raw.match(/^#\s+(.+)$/m)?.[1]?.trim() ?? slug
  const firstPara = raw
    .split('\n')
    .find((l) => l.trim() && !l.startsWith('#'))
    ?.replace(/\[([^\]]+)\]\([^)]+\)/g, '$1')
    .replace(/[*`]/g, '')
  return {
    slug,
    title,
    blurb: BLURBS[slug] ?? firstPara?.split('. ')[0] ?? '',
    html,
    headings,
  }
}

export function allDocs(): Doc[] {
  return slugs().map(getDoc)
}

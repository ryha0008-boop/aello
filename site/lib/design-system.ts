import fs from 'node:fs'
import path from 'node:path'

/**
 * The /design-system page is generated from `app/globals.css` at build time,
 * the same way /docs is generated from the repo's `docs/`. There is no second
 * copy of the palette to keep in sync: edit a value in the `:root` block and
 * the page, and every component, move together.
 *
 * This module also carries the coherence guards. They `throw` during
 * `next build`, which is deliberate — a design system nobody can violate by
 * accident is worth more than one written down. The failure shape this repo
 * keeps hitting is the silent one, so these name the file, the line, and the
 * fix rather than warning into a log nobody reads.
 */

const SITE_ROOT = process.cwd()
const GLOBALS = path.join(SITE_ROOT, 'app', 'globals.css')

export type TokenKind = 'color' | 'font' | 'size' | 'duration' | 'easing' | 'shadow' | 'number'

export type Token = {
  /** The custom property, e.g. `--color-canvas-default`. */
  name: string
  /** Exactly as written in globals.css. */
  value: string
  /** With `var()` chains followed, so a swatch has something to paint. */
  resolved: string
  kind: TokenKind
  /** How many `var(--name)` references exist across the site source. */
  uses: number
  /** Which files those references are in, site-relative, most-used first. */
  usedIn: string[]
}

export type TokenGroup = { title: string; tokens: Token[] }

/**
 * The two breakpoints the layout actually turns on, plus the one component-local
 * exception. CSS custom properties are not usable inside `@media` queries, so
 * these cannot be tokens — the literals have to be repeated in each file. That
 * is exactly why they are listed here and enforced below: an invented third
 * breakpoint is invisible until a layout breaks in a 12px window nobody resizes
 * to.
 */
export const BREAKPOINTS = [
  {
    value: '48rem',
    px: 768,
    label: 'medium',
    note: 'Phone to tablet. Sections drop to condensed padding and headings step down a size.',
  },
  {
    value: '63.25rem',
    px: 1012,
    label: 'large',
    note: "The system's own large breakpoint. Multi-column grids collapse to one column below it.",
  },
  {
    value: '34rem',
    px: 544,
    label: 'compact (component-local)',
    note: 'Used once, by the commit row inside the hero demo. Not a layout breakpoint — do not reach for it elsewhere.',
  },
]

const ALLOWED_WIDTHS = new Set(BREAKPOINTS.map((b) => b.value))

/* ------------------------------------------------------------------ parsing */

function stripComments(src: string): string {
  // Blanks the comment out but keeps every newline, so reported line numbers
  // still point at the real line.
  return src.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
}

type RawToken = { title: string; name: string; value: string }

/** Longest a comment may be and still be read as a group heading rather than prose. */
const HEADING_MAX = 80

function readRoot(css: string): RawToken[] {
  const block = css.match(/:root\s*\{([\s\S]*?)\n\}/)
  if (!block) {
    throw new Error('design-system: no `:root { … }` block found in app/globals.css')
  }
  const body = block[1]

  const out: RawToken[] = []
  let title = 'Ungrouped'

  // One scan over comments and declarations together. The line-at-a-time version
  // this replaces only recognised a comment that opened and closed on one line;
  // a multi-line note swallowed the declaration after it and the token vanished
  // with no error. Six went missing that way, and the page rendered the smaller
  // number as though it were the truth — hence the count check below.
  for (const m of body.matchAll(/\/\*([\s\S]*?)\*\/|(--[\w-]+)\s*:\s*([^;]+);/g)) {
    if (m[1] !== undefined) {
      const text = m[1].trim()
      // Single line and short = a section heading. Anything longer is prose
      // explaining the section, and is not a title.
      if (!text.includes('\n') && text.length <= HEADING_MAX) title = text
      continue
    }
    out.push({ title, name: m[2], value: m[3].trim().replace(/\s+/g, ' ') })
  }

  if (!out.length) throw new Error('design-system: parsed 0 tokens from app/globals.css')

  // The one way this parser silently loses tokens: an unterminated comment.
  // `/*` with no `*/` runs on to the next close and swallows every declaration
  // in between, and the page then renders the smaller set as though that were
  // the design system. Six tokens vanished exactly this way while this file was
  // being written.
  //
  // Counting parsed-vs-declared cannot catch it — both sides would use the same
  // comment model and agree with each other by construction. Balance is the
  // check that actually distinguishes the two states.
  const opens = (body.match(/\/\*/g) ?? []).length
  const closes = (body.match(/\*\//g) ?? []).length
  if (opens !== closes) {
    throw new Error(
      `design-system: unbalanced comment markers in the :root block of app/globals.css — ` +
        `${opens} \`/*\` against ${closes} \`*/\`. An unterminated comment swallows every ` +
        'token declaration up to the next close, and they disappear from /design-system ' +
        'without any other error. Close the comment.'
    )
  }

  return out
}

function resolve(value: string, byName: Map<string, string>): string {
  let v = value
  // Bounded rather than recursive: an accidental cycle should not hang a build.
  for (let i = 0; i < 5 && v.includes('var(--'); i++) {
    const next = v.replace(/var\((--[\w-]+)\)/g, (whole, n: string) => byName.get(n) ?? whole)
    if (next === v) break
    v = next
  }
  return v
}

function kindOf(name: string, resolved: string): TokenKind {
  if (/^#|^rgba?\(/.test(resolved)) return 'color'
  if (name.includes('fontStack')) return 'font'
  if (name.includes('easing')) return 'easing'
  if (name.includes('duration')) return 'duration'
  if (name.includes('shadow')) return 'shadow'
  // letterSpacing is em-suffixed and often negative; a size bar would be
  // meaningless, so it reads as a plain value.
  if (name.includes('letterSpacing')) return 'number'
  if (/(rem|px|em|ch)$/.test(resolved)) return 'size'
  return 'number'
}

/* -------------------------------------------------------------- source scan */

/** Every file that may reference a token: the page source, not the build output. */
function sourceFiles(): string[] {
  const roots = [path.join(SITE_ROOT, 'app'), path.join(SITE_ROOT, 'components')]
  const found: string[] = []
  const walk = (dir: string) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, e.name)
      if (e.isDirectory()) walk(full)
      else if (/\.(css|tsx|ts)$/.test(e.name)) found.push(full)
    }
  }
  for (const r of roots) if (fs.existsSync(r)) walk(r)
  return found
}

function rel(file: string): string {
  return path.relative(SITE_ROOT, file).replace(/\\/g, '/')
}

/* ------------------------------------------------------------------- guards */

/**
 * No literal colour outside globals.css. Every colour on the page has to come
 * from a token, or the palette is a suggestion rather than a system.
 */
function assertNoRawColors(files: string[]): void {
  const offences: string[] = []
  for (const file of files) {
    if (file === GLOBALS) continue
    if (!file.endsWith('.css')) continue
    const lines = stripComments(fs.readFileSync(file, 'utf8')).split('\n')
    lines.forEach((line, i) => {
      const hit = line.match(/#[0-9a-fA-F]{3,8}\b|\brgba?\([^)]*\)/)
      if (hit) offences.push(`  ${rel(file)}:${i + 1}  ${hit[0]}`)
    })
  }
  if (offences.length) {
    throw new Error(
      `design-system: ${offences.length} literal colour(s) outside app/globals.css:\n` +
        `${offences.join('\n')}\n` +
        '  Fix: add or reuse a token in the :root block of app/globals.css and ' +
        'reference it with var(--token). Colours live in one file on purpose.'
    )
  }
}

/**
 * Every `@media` width must be one of the documented breakpoints. A one-off
 * `64rem` sitting 0.75rem from the real breakpoint opens a dead zone where one
 * page lays out differently from every other, and nothing reports it.
 */
function assertKnownBreakpoints(files: string[]): void {
  const offences: string[] = []
  for (const file of files) {
    if (!file.endsWith('.css')) continue
    const lines = stripComments(fs.readFileSync(file, 'utf8')).split('\n')
    lines.forEach((line, i) => {
      for (const m of line.matchAll(/\((?:min|max)-width:\s*([^)]+)\)/g)) {
        const w = m[1].trim()
        if (!ALLOWED_WIDTHS.has(w)) offences.push(`  ${rel(file)}:${i + 1}  ${w}`)
      }
    })
  }
  if (offences.length) {
    throw new Error(
      `design-system: ${offences.length} undocumented breakpoint(s):\n` +
        `${offences.join('\n')}\n` +
        `  Fix: use one of ${[...ALLOWED_WIDTHS].join(', ')}, or add the new one to ` +
        'BREAKPOINTS in lib/design-system.ts so it appears on /design-system.'
    )
  }
}

/* -------------------------------------------------------------------- public */

export type DesignSystem = {
  groups: TokenGroup[]
  /** Tokens with no `var()` reference anywhere — dead weight, or a missed hookup. */
  unused: Token[]
  total: number
}

export function designSystem(): DesignSystem {
  const css = fs.readFileSync(GLOBALS, 'utf8')
  const raw = readRoot(css)

  const byName = new Map(raw.map((t) => [t.name, t.value]))
  const files = sourceFiles()

  assertNoRawColors(files)
  assertKnownBreakpoints(files)

  // The /design-system page is EXCLUDED from the counts. It is documentation
  // about the tokens, not the site using them, and counting it made a token
  // that only its own swatch referenced read as "used 2×" — the page quietly
  // citing itself as evidence. Excluding it is what makes "unused" mean
  // anything.
  const counted = files.filter((f) => !rel(f).startsWith('app/design-system/'))
  const bodies = counted.map((f) => ({ file: rel(f), body: fs.readFileSync(f, 'utf8') }))

  const groups: TokenGroup[] = []
  for (const t of raw) {
    const resolved = resolve(t.value, byName)
    const needle = `var(${t.name})`
    const hits = bodies
      .map(({ file, body }) => ({ file, n: body.split(needle).length - 1 }))
      .filter((h) => h.n > 0)
      .sort((a, b) => b.n - a.n)
    const token: Token = {
      name: t.name,
      value: t.value,
      resolved,
      kind: kindOf(t.name, resolved),
      uses: hits.reduce((n, h) => n + h.n, 0),
      usedIn: hits.map((h) => h.file),
    }

    const group = groups.find((g) => g.title === t.title)
    if (group) group.tokens.push(token)
    else groups.push({ title: t.title, tokens: [token] })
  }

  const all = groups.flatMap((g) => g.tokens)
  return { groups, unused: all.filter((t) => t.uses === 0), total: all.length }
}

/**
 * The brand type scale, assembled into one specimen per step so the four
 * properties that travel together (size, line-height, weight, letter-spacing)
 * are read together rather than as four separate token rows.
 */
export type TypeStep = {
  step: string
  size: string
  lineHeight?: string
  weight?: string
  letterSpacing?: string
}

export function typeScale(groups: TokenGroup[]): TypeStep[] {
  const all = groups.flatMap((g) => g.tokens)
  const at = (name: string) => all.find((t) => t.name === name)?.resolved

  const steps = all
    .map((t) => t.name.match(/^--brand-text-size-(\d+)$/)?.[1])
    .filter((s): s is string => Boolean(s))

  return steps.map((step) => ({
    step,
    size: at(`--brand-text-size-${step}`) ?? '',
    lineHeight: at(`--brand-text-lineHeight-${step}`),
    weight: at(`--brand-heading-weight-${step}`),
    letterSpacing: at(`--brand-text-letterSpacing-${step}`),
  }))
}

/* ----------------------------------------------------------------- contrast */

type Rgb = { r: number; g: number; b: number; a: number }

function parseColor(value: string): Rgb | null {
  const hex = value.trim().match(/^#([0-9a-fA-F]{3,8})$/)
  if (hex) {
    let h = hex[1]
    if (h.length === 3) h = h.split('').map((c) => c + c).join('')
    if (h.length !== 6 && h.length !== 8) return null
    return {
      r: parseInt(h.slice(0, 2), 16),
      g: parseInt(h.slice(2, 4), 16),
      b: parseInt(h.slice(4, 6), 16),
      a: h.length === 8 ? parseInt(h.slice(6, 8), 16) / 255 : 1,
    }
  }
  const fn = value.trim().match(/^rgba?\(([^)]+)\)$/)
  if (!fn) return null
  const p = fn[1].split(',').map((x) => Number(x.trim()))
  if (p.length < 3 || p.slice(0, 3).some((x) => !Number.isFinite(x))) return null
  return { r: p[0], g: p[1], b: p[2], a: p.length > 3 && Number.isFinite(p[3]) ? p[3] : 1 }
}

/** Alpha colours are the norm here, so composite before measuring, not after. */
function over(fg: Rgb, bg: Rgb): Rgb {
  return {
    r: fg.r * fg.a + bg.r * (1 - fg.a),
    g: fg.g * fg.a + bg.g * (1 - fg.a),
    b: fg.b * fg.a + bg.b * (1 - fg.a),
    a: 1,
  }
}

function luminance({ r, g, b }: Rgb): number {
  const f = (c: number) => {
    const s = c / 255
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
  }
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
}

export type ContrastRow = {
  fg: string
  bg: string
  ratio: number
  /** WCAG 2.1 AA: 4.5:1 for body text, 3:1 for large text and UI edges. */
  passesBody: boolean
  passesLarge: boolean
}

/**
 * Every foreground token measured against every surface it is actually painted
 * on. Reported rather than enforced, because below-4.5 is legitimate for some
 * pairs — a button fill is not text, and a display-only accent needs 3:1 — so
 * the call site holds the judgement. What it cannot do is make it without the
 * numbers, which is what this supplies.
 */
export function contrastReport(groups: TokenGroup[]): ContrastRow[] {
  const all = groups.flatMap((g) => g.tokens)
  const colour = (name: string) => {
    const t = all.find((x) => x.name === name)
    return t ? parseColor(t.resolved) : null
  }

  const surfaces = ['--color-canvas-default', '--color-canvas-subtle', '--color-canvas-inset']
  const foregrounds = all
    .filter((t) => t.kind === 'color' && /^--color-(fg|accent)-/.test(t.name))
    .map((t) => t.name)

  const rows: ContrastRow[] = []
  for (const bg of surfaces) {
    const bgc = colour(bg)
    if (!bgc) continue
    for (const fg of foregrounds) {
      const fgc = colour(fg)
      if (!fgc) continue
      const a = luminance(over(fgc, bgc))
      const b = luminance(bgc)
      const ratio = (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05)
      rows.push({
        fg,
        bg,
        ratio: Math.round(ratio * 100) / 100,
        passesBody: ratio >= 4.5,
        passesLarge: ratio >= 3,
      })
    }
  }
  return rows
}

/** `cubic-bezier(a, b, c, d)` → the four control numbers, for drawing the curve. */
export function bezierPoints(value: string): [number, number, number, number] | null {
  const m = value.match(/cubic-bezier\(([^)]+)\)/)
  if (!m) return null
  const n = m[1].split(',').map((p) => Number(p.trim()))
  return n.length === 4 && n.every((x) => Number.isFinite(x))
    ? [n[0], n[1], n[2], n[3]]
    : null
}

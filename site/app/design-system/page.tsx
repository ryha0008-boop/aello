import type { Metadata } from 'next'
import Nav from '@/components/Nav'
import Footer from '@/components/Footer'
import {
  BREAKPOINTS,
  bezierPoints,
  contrastReport,
  designSystem,
  typeScale,
  type Token,
} from '@/lib/design-system'
import styles from './design-system.module.css'

export const metadata: Metadata = {
  title: 'aello — design system',
  description:
    'Every design token this site is built from — colour, type, spacing, radii, motion and elevation — read straight out of app/globals.css.',
}

/** A colour needs a chip; everything else gets a specimen suited to its kind. */
function Specimen({ token }: { token: Token }) {
  const { kind, name, resolved } = token

  if (kind === 'color') {
    return <span className={styles.swatch} style={{ background: `var(${name})` }} aria-hidden="true" />
  }

  if (kind === 'font') {
    return (
      <span className={styles.fontSpecimen} style={{ fontFamily: `var(${name})` }}>
        Handgloves 0123
      </span>
    )
  }

  if (kind === 'size') {
    return (
      <span className={styles.bar} aria-hidden="true">
        <i style={{ width: `min(100%, var(${name}))` }} />
      </span>
    )
  }

  if (kind === 'shadow') {
    return <span className={styles.shadowChip} style={{ boxShadow: `var(${name})` }} aria-hidden="true" />
  }

  if (kind === 'easing') {
    const p = bezierPoints(resolved)
    if (!p) return <span className={styles.plain}>{resolved}</span>
    const [x1, y1, x2, y2] = p
    // Curve drawn in a 100x100 box, y flipped so "up" reads as progress.
    return (
      <svg className={styles.curve} viewBox="-10 -20 120 140" aria-hidden="true">
        <line x1="0" y1="100" x2="100" y2="0" className={styles.curveGuide} />
        <path
          d={`M 0 100 C ${x1 * 100} ${100 - y1 * 100}, ${x2 * 100} ${100 - y2 * 100}, 100 0`}
          className={styles.curveLine}
        />
      </svg>
    )
  }

  if (kind === 'duration') {
    return (
      <span className={styles.bar} aria-hidden="true">
        <i className={styles.durationFill} style={{ animationDuration: `var(${name})` }} />
      </span>
    )
  }

  return <span className={styles.plain}>{resolved}</span>
}

function TokenRow({ token }: { token: Token }) {
  return (
    <li className={styles.row} data-unused={token.uses === 0 ? 'true' : undefined}>
      <span className={styles.specimen}>
        <Specimen token={token} />
      </span>
      <span className={styles.rowText}>
        <span className={styles.rowHead}>
          <code className={styles.tokenName}>{token.name}</code>
          <span className={styles.tokenValue}>{token.value}</span>
          {token.value !== token.resolved && (
            <span className={styles.tokenResolved}>= {token.resolved}</span>
          )}
        </span>
        {token.usedIn.length > 0 && (
          <span className={styles.usedIn}>
            {token.usedIn.slice(0, 5).join(' · ')}
            {token.usedIn.length > 5 && ` · +${token.usedIn.length - 5} more`}
          </span>
        )}
      </span>
      <span className={styles.uses} title={`${token.uses} reference(s), excluding this page`}>
        {token.uses === 0 ? 'unused' : `${token.uses}×`}
      </span>
    </li>
  )
}

export default function DesignSystemPage() {
  const { groups, unused, total } = designSystem()
  const steps = typeScale(groups)

  // Read live out of globals.css rather than restated in the prose below. The
  // first draft hardcoded "0.6s" and was wrong within the hour, which is the
  // whole reason this page is generated in the first place.
  const byName = new Map(groups.flatMap((g) => g.tokens).map((t) => [t.name, t.resolved]))
  const value = (name: string) => byName.get(name) ?? '?'
  const seconds = (v: string) =>
    v.endsWith('ms') ? parseFloat(v) / 1000 : parseFloat(v) || 0
  // Worst first — the pairs that need a decision are the ones worth seeing.
  const contrast = contrastReport(groups).sort((a, b) => a.ratio - b.ratio)

  return (
    <>
      <Nav />

      <main>
        <section className={styles.head}>
          <div className="container">
            <p className="eyebrow">Design system</p>
            <h1 className={styles.title}>Every token this site is built from</h1>
            <p className={styles.lead}>
              {total} tokens, read out of <code>site/app/globals.css</code> when this page is built.
              There is no second copy — change a value there and this page, the landing page and the
              docs all move together.
            </p>
            <p className={styles.lead}>
              The <em>structure</em> — token names, the type scale, spacing, radii, motion — comes
              from a captured GitHub design system. The <em>colours</em> do not: they are retuned to
              orange on near-black, one warm accent family over neutral greys. Nothing is pure
              black; it flattens every border and shadow drawn on top of it.
            </p>

            <div className={styles.rules}>
              <h2 className={styles.rulesTitle}>Two rules, enforced by the build</h2>
              <ol className={styles.rulesList}>
                <li>
                  <strong>No literal colour outside <code>globals.css</code>.</strong> Every colour
                  comes from a token. A hex or <code>rgba()</code> in any component stylesheet fails{' '}
                  <code>npm run build</code> with the file and line.
                </li>
                <li>
                  <strong>Only the breakpoints listed below.</strong> Custom properties do not work
                  inside <code>@media</code>, so the literals are repeated per file — which is
                  precisely why an invented one is caught at build time instead of quietly opening a
                  dead zone.
                </li>
              </ol>
              <p className={styles.rulesNote}>
                Both live in <code>site/lib/design-system.ts</code>. They throw rather than warn, so
                a violation cannot reach the deployed page.
              </p>
            </div>
          </div>
        </section>

        <section className="section">
          <div className="container">
            <h2 className="sectionHeading">Type scale</h2>
            <p className="sectionLead">
              Each step carries its own line-height, weight and letter-spacing. They are set
              together or not at all — taking a size without its line-height is how headings end up
              looking almost right.
            </p>

            <ul className={styles.typeList}>
              {steps.map((s) => (
                <li key={s.step} className={styles.typeRow}>
                  <p
                    className={styles.typeSpecimen}
                    style={{
                      fontSize: `var(--brand-text-size-${s.step})`,
                      lineHeight: s.lineHeight ?? undefined,
                      fontWeight: s.weight ? Number(s.weight) : undefined,
                      letterSpacing: s.letterSpacing ?? undefined,
                    }}
                  >
                    Many agents. One repo.
                  </p>
                  <dl className={styles.typeMeta}>
                    <div>
                      <dt>step</dt>
                      <dd>{s.step}</dd>
                    </div>
                    <div>
                      <dt>size</dt>
                      <dd>{s.size}</dd>
                    </div>
                    {s.lineHeight && (
                      <div>
                        <dt>line-height</dt>
                        <dd>{s.lineHeight}</dd>
                      </div>
                    )}
                    {s.weight && (
                      <div>
                        <dt>weight</dt>
                        <dd>{s.weight}</dd>
                      </div>
                    )}
                    {s.letterSpacing && (
                      <div>
                        <dt>tracking</dt>
                        <dd>{s.letterSpacing}</dd>
                      </div>
                    )}
                  </dl>
                </li>
              ))}
            </ul>
          </div>
        </section>

        {groups.map((group) => (
          <section className="section" key={group.title}>
            <div className="container">
              <h2 className="sectionHeading">{group.title}</h2>

              {group.title === 'Motion' && (
                <>
                  <p className="sectionLead">
                    Most of these drive <strong>hover and focus transitions</strong> — buttons, nav
                    links, cards, the copy button. Only three pieces of motion happen without you
                    pointing at something: sections fade and rise as they scroll in
                    (<code>duration-default</code>, {value('--brand-animation-duration-default')}),
                    the voice waveform pulses continuously (<code>easing-default</code>, 1.4s), and
                    the hero demo cross-fades its panes when you pick a blueprint
                    (<code>duration-fast</code>, {value('--brand-animation-duration-fast')}).
                  </p>
                  <p className="sectionLead">
                    A duration here is not what you perceive.{' '}
                    <code>easing-default</code> is an expo-out that finishes{' '}
                    <strong>90% of the movement in the first third</strong> of its duration — so the
                    {/* One expression: JSX turns a newline between `{…}` and the next
                        text node into a space, which put a gap before the comma. */}
                    {`scroll reveal reads as roughly ${Math.round(
                      seconds(value('--brand-animation-duration-default')) * 0.33 * 1000
                    )}ms, not ${value('--brand-animation-duration-default')}.`}{' '}
                    Lengthen the duration when motion feels too quick; the curve is doing most of
                    the work.
                  </p>
                  <p className={styles.reducedNote}>
                    Your browser reports <code>prefers-reduced-motion: reduce</code>. The site
                    honours it: the scroll reveal, the waveform and every transition are switched
                    off, and the bars below are frozen. That is the setting working, not motion
                    missing. On Windows it is Settings → Accessibility → Visual effects →
                    Animation effects.
                  </p>
                </>
              )}

              <ul className={styles.tokenList}>
                {group.tokens.map((t) => (
                  <TokenRow key={t.name} token={t} />
                ))}
              </ul>
            </div>
          </section>
        ))}

        <section className="section">
          <div className="container">
            <h2 className="sectionHeading">Contrast</h2>
            <p className="sectionLead">
              Every foreground and accent token against each surface it is painted on, composited
              through its alpha first. WCAG AA wants 4.5:1 for body text and 3:1 for large text.
              Reported rather than enforced: a pair can sit below 4.5 legitimately — a button fill
              is not text, and an accent used only at display sizes needs 3:1 — so the judgement is
              per call site. Read the failures, don&apos;t automate them away.
            </p>
            <ul className={styles.contrastList}>
              {contrast.map((c) => (
                <li
                  key={`${c.fg}|${c.bg}`}
                  className={styles.contrastRow}
                  data-level={c.passesBody ? 'aa' : c.passesLarge ? 'large' : 'fail'}
                >
                  <span
                    className={styles.contrastChip}
                    style={{ background: `var(${c.bg})`, color: `var(${c.fg})` }}
                  >
                    Ag
                  </span>
                  <code className={styles.contrastPair}>
                    {c.fg} <span className={styles.contrastOn}>on</span> {c.bg}
                  </code>
                  <span className={styles.contrastRatio}>{c.ratio.toFixed(2)}:1</span>
                  <span className={styles.contrastVerdict}>
                    {c.passesBody ? 'AA' : c.passesLarge ? 'large only' : 'fails'}
                  </span>
                </li>
              ))}
            </ul>
          </div>
        </section>

        <section className="section">
          <div className="container">
            <h2 className="sectionHeading">Breakpoints</h2>
            <p className="sectionLead">
              Not tokens, and they cannot be: CSS custom properties are not usable inside a media
              query, so each stylesheet repeats the literal. The build checks them against this
              list.
            </p>
            <ul className={styles.breakpointList}>
              {BREAKPOINTS.map((b) => (
                <li key={b.value} className={styles.breakpoint}>
                  <code className={styles.breakpointValue}>{b.value}</code>
                  <span className={styles.breakpointPx}>{b.px}px · {b.label}</span>
                  <p className={styles.breakpointNote}>{b.note}</p>
                </li>
              ))}
            </ul>
          </div>
        </section>

        <section className="section">
          <div className="container">
            <h2 className="sectionHeading">Inert tokens</h2>
            {unused.length === 0 ? (
              <p className="sectionLead">
                None — every token in <code>globals.css</code> is referenced somewhere.
              </p>
            ) : (
              <>
                <p className={styles.inertLead}>
                  {unused.length} of {total} tokens have no <code>var()</code> reference anywhere in
                  the site. <strong>Changing one of these changes nothing on the page.</strong> They
                  are not a bug and not a cleanup job: the token set is carried over from the
                  captured system wholesale, so it holds slots this site has not needed yet. The
                  list is here so an edit that appears to do nothing has an explanation.
                </p>
                <ul className={styles.tokenList}>
                  {unused.map((t) => (
                    <TokenRow key={t.name} token={t} />
                  ))}
                </ul>
              </>
            )}
          </div>
        </section>
      </main>

      <Footer />
    </>
  )
}

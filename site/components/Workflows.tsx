import Link from 'next/link'
import Reveal from './Reveal'
import { getDoc } from '@/lib/docs'
import styles from './Workflows.module.css'

/**
 * Which walkthroughs to feature, keyed by the **exact `##` heading** in
 * `docs/workflows.md`. Anchors are resolved from the rendered doc at build time
 * rather than written out here — a hardcoded `#some-anchor` keeps linking to
 * nothing when a heading is reworded, and looks perfectly fine until someone
 * clicks it. Rename a heading and this build fails instead.
 */
const FEATURED: { heading: string; label?: string; body: string }[] = [
  {
    heading: 'Your first environment',
    body: 'Login, one blueprint, one project — and what the first run actually writes.',
  },
  {
    heading: 'Two agents in one repo',
    body: 'The case aello exists for: one maintainer, several contributors, one git history that says who did what.',
  },
  {
    heading: 'The session loop: work, checkpoint, hand off',
    label: 'The session loop',
    body: 'Work, /handoff before you clear, /sync to checkpoint — and the next session boots mid-thought.',
  },
  {
    heading: 'One env, two machines',
    body: 'Take an agent’s memory, skills and resume note to another machine — Windows or Linux — and bring them back.',
  },
  {
    heading: 'Telling another agent something',
    body: 'Leave a note in another environment’s inbox, including one in a different repo.',
  },
  {
    heading: 'Updating an already-placed env',
    label: 'Updating a placed env',
    body: 'Placement is idempotent, so the fix is almost always: run it again.',
  },
  {
    heading: 'Renaming a blueprint',
    body: 'What moves, what doesn’t, and why the other projects need the same command.',
  },
]

function resolve() {
  const headings = getDoc('workflows').headings
  return FEATURED.map((f) => {
    const match = headings.find((h) => h.text === f.heading)
    if (!match) {
      throw new Error(
        `Workflows.tsx: no "## ${f.heading}" heading in docs/workflows.md. ` +
          `Update the heading here, or drop it from FEATURED. Available: ` +
          headings
            .filter((h) => h.depth === 2)
            .map((h) => h.text)
            .join(' · '),
      )
    }
    return { ...f, anchor: match.id, title: f.label ?? f.heading }
  })
}

export default function Workflows() {
  const WORKFLOWS = resolve()

  return (
    <section className="section" id="workflows">
      <div className="container">
        <Reveal>
          <p className="eyebrow">Workflows</p>
          <h2 className="sectionHeading">Written down, start to finish</h2>
          <p className="sectionLead">
            Every workflow is documented as a walkthrough — with the failure modes, not just the
            happy path. The same pages ship inside the binary: <code>aello docs workflows</code>.
          </p>
        </Reveal>

        <ul className={styles.grid}>
          {WORKFLOWS.map((w, i) => (
            <Reveal key={w.anchor} delay={i * 50} className={styles.slot}>
              <li className={styles.item}>
                <Link href={`/docs/workflows/#${w.anchor}`} className={styles.link}>
                  <span className={styles.title}>{w.title}</span>
                  <span className={styles.body}>{w.body}</span>
                </Link>
              </li>
            </Reveal>
          ))}
        </ul>

        <Reveal>
          <p className={styles.more}>
            <Link href="/docs/">Read all the docs →</Link>
          </p>
        </Reveal>
      </div>
    </section>
  )
}

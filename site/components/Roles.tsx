import Reveal from './Reveal'
import styles from './Roles.module.css'

const ROLES = [
  {
    role: 'maintainer',
    owns: 'The project CLAUDE.md, CHANGELOG.md, docs/, README.md — and git',
    sync: 'Reconciles memory and all four docs against the code, then commits and pushes. One per repo.',
    accent: true,
  },
  {
    role: 'contributor',
    owns: 'Its own code, and its own CHANGELOG entry',
    sync: 'Commits and pushes with an Env: trailer. Its /sync never mentions the README or docs/ at all.',
  },
  {
    role: 'standalone',
    owns: 'Nothing outside its own session',
    sync: 'No /sync skill is seeded, and nothing is scaffolded in the project.',
  },
]

export default function Roles() {
  return (
    <section className="section" id="roles">
      <div className="container">
        <Reveal>
          <p className="eyebrow">Roles</p>
          <h2 className="sectionHeading">One maintainer per repo. Everyone else just commits.</h2>
          <p className="sectionLead">
            A blueprint&apos;s role decides what it scaffolds and what its{' '}
            <code className={styles.inlineCode}>/sync</code> skill is even told about. A contributor
            has no instructions for the README or <code className={styles.inlineCode}>docs/</code>{' '}
            in its skill file — so it can&apos;t drift the docs while the maintainer isn&apos;t
            looking.
          </p>
        </Reveal>

        <ul className={styles.grid}>
          {ROLES.map((r, i) => (
            <Reveal key={r.role} delay={i * 60} className={styles.slot}>
              <li className={styles.card} data-accent={r.accent ? 'true' : undefined}>
                <code className={styles.flag}>--role {r.role}</code>
                <p className={styles.scaffolds}>{r.owns}</p>
                <p className={styles.sync}>{r.sync}</p>
              </li>
            </Reveal>
          ))}
        </ul>
      </div>
    </section>
  )
}

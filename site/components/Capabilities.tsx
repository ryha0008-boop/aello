import Reveal from './Reveal'
import styles from './Capabilities.module.css'

const CAPABILITIES = [
  {
    flag: '--github',
    scaffolds:
      'A .gitignore entry, .gitattributes, a patch-bump workflow, and a tracked mirror of the env',
    sync: 'Repo health, commit and rebase-before-push, and an Env: trailer on every commit.',
  },
  {
    flag: '--project-md',
    scaffolds: 'A project CLAUDE.md at the repo root',
    sync: 'Reconciles it against what the code actually does.',
  },
  {
    flag: '--changelog',
    scaffolds: 'CHANGELOG.md',
    sync: 'Adds an entry for every user-facing change.',
  },
  {
    flag: '--docs',
    scaffolds: 'docs/',
    sync: 'Keeps reference docs in step with behaviour.',
  },
  {
    flag: '--readme',
    scaffolds: 'README.md',
    sync: 'Keeps install steps and the command list current.',
  },
  {
    flag: '--voice',
    scaffolds: 'A Stop hook and the TL;DR section in the persona',
    sync: 'Nothing — voice maintains no files, so it adds no /sync step.',
    accent: true,
  },
]

export default function Capabilities() {
  return (
    <section className="section" id="capabilities">
      <div className="container">
        <Reveal>
          <p className="eyebrow">Capabilities</p>
          <h2 className="sectionHeading">Pick what an agent is allowed to maintain</h2>
          <p className="sectionLead">
            Each capability scaffolds its file if it&apos;s missing, and writes one section of a{' '}
            <code className={styles.inlineCode}>/sync</code> skill generated for that blueprint
            alone. An agent without <code className={styles.inlineCode}>--github</code> gets no git
            instructions at all.
          </p>
        </Reveal>

        <ul className={styles.grid}>
          {CAPABILITIES.map((cap, i) => (
            <Reveal key={cap.flag} delay={i * 60} className={styles.slot}>
              <li className={styles.card} data-accent={cap.accent ? 'true' : undefined}>
                <code className={styles.flag}>{cap.flag}</code>
                <p className={styles.scaffolds}>{cap.scaffolds}</p>
                <p className={styles.sync}>{cap.sync}</p>
              </li>
            </Reveal>
          ))}
        </ul>
      </div>
    </section>
  )
}

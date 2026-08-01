import Reveal from './Reveal'
import styles from './Steps.module.css'

/** These are ordered on purpose: you can't run a blueprint you haven't added. */
const STEPS = [
  {
    command: 'aello login',
    title: 'Store one token',
    body: 'Runs claude setup-token once and keeps the result. The token does not rotate, so any number of concurrent envs can share it without racing each other.',
  },
  {
    command: 'aello add coder --model opus --github',
    title: 'Define a blueprint',
    body: 'A name, a model, a persona, and the files this agent is allowed to maintain. Blueprints are reusable — the same one drops into any project.',
  },
  {
    command: 'aello run coder',
    title: 'Place it in a project',
    body: 'Creates .claude-env-coder/ and launches Claude inside it. Scaffolds only the files you enabled, and only the ones that are missing.',
  },
]

export default function Steps() {
  return (
    <section className="section" id="how">
      <div className="container">
        <Reveal>
          <p className="eyebrow">Three commands</p>
          <h2 className="sectionHeading">From nothing to a working agent</h2>
        </Reveal>

        <ol className={styles.list}>
          {STEPS.map((step, i) => (
            <Reveal key={step.command} delay={i * 80}>
              <li className={styles.step}>
                <span className={styles.index}>{String(i + 1).padStart(2, '0')}</span>
                <div className={styles.body}>
                  <code className={styles.command}>{step.command}</code>
                  <h3 className={styles.title}>{step.title}</h3>
                  <p className={styles.text}>{step.body}</p>
                </div>
              </li>
            </Reveal>
          ))}
        </ol>
      </div>
    </section>
  )
}

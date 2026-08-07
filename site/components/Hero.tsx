import CopyLine from './CopyLine'
import EnvDemo from './EnvDemo'
import Reveal from './Reveal'
import styles from './Hero.module.css'

const INSTALL =
  'curl -fsSL https://raw.githubusercontent.com/ryha0008-boop/aello/main/install.sh | sh'

export default function Hero() {
  return (
    <section className={styles.hero} id="top">
      <div className={`container ${styles.inner}`}>
        <Reveal>
          <p className="eyebrow">Isolated agent environments</p>
          <h1 className={styles.title}>Many agents. One repo. No collisions.</h1>
          <p className={styles.lead}>
            aello drops each agent into its own config dir — persona, skills, hooks and history kept
            apart — so several can work the same project without overwriting each other. Like a
            venv, for agents. Claude Code by default, or the Cline CLI on its own key.
          </p>
        </Reveal>

        <Reveal delay={80}>
          <div className={styles.actions}>
            <a className={styles.primary} href="#install">
              Install aello
            </a>
            <a className={styles.secondary} href="#how">
              See how it works
            </a>
          </div>

          <div className={styles.installLine}>
            <CopyLine command={INSTALL} />
            <p className={styles.platforms}>
              Linux · macOS · Windows. Or build it with <code>cargo install</code>.
            </p>
          </div>
        </Reveal>

        <Reveal delay={160} className={styles.demoSlot}>
          <EnvDemo />
        </Reveal>
      </div>
    </section>
  )
}

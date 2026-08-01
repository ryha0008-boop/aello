import CopyLine from './CopyLine'
import Reveal from './Reveal'
import styles from './Install.module.css'

const REPO = 'https://github.com/ryha0008-boop/aello'

export default function Install() {
  return (
    <section className="section" id="install">
      <div className="container">
        <Reveal>
          <p className="eyebrow">Install</p>
          <h2 className="sectionHeading">Get aello</h2>
          <p className="sectionLead">
            A single binary with no runtime to install. You&apos;ll also need Claude Code on your
            PATH, and Python 3 for the transcript hooks.
          </p>
        </Reveal>

        <div className={styles.grid}>
          <Reveal className={styles.slot}>
            <div className={styles.block}>
              <h3 className={styles.platform}>Linux &amp; macOS</h3>
              <CopyLine command="curl -fsSL https://raw.githubusercontent.com/ryha0008-boop/aello/main/install.sh | sh" />
              <p className={styles.note}>
                Installs to <code>~/.local/bin</code> and clears the macOS quarantine flag. Override
                the target with <code>AELLO_BIN_DIR</code>.
              </p>
            </div>
          </Reveal>

          <Reveal delay={80} className={styles.slot}>
            <div className={styles.block}>
              <h3 className={styles.platform}>Windows</h3>
              <p className={styles.note}>
                Download{' '}
                <a href={`${REPO}/releases/download/latest/aello-x86_64-windows.exe`}>
                  aello-x86_64-windows.exe
                </a>
                , rename it to <code>aello.exe</code>, and put it on your PATH. The binary is
                unsigned, so SmartScreen asks once — choose <em>More info → Run anyway</em>, or
                check it against the release&apos;s <code>SHA256SUMS</code>.
              </p>
            </div>
          </Reveal>

          <Reveal delay={160} className={styles.slot}>
            <div className={styles.block}>
              <h3 className={styles.platform}>From source</h3>
              <CopyLine command="cargo install --git https://github.com/ryha0008-boop/aello" />
              <p className={styles.note}>
                Needs a Rust toolchain. <code>~/.cargo/bin</code> stays writable, so{' '}
                <code>aello update</code> works from a source install too.
              </p>
            </div>
          </Reveal>
        </div>

        <Reveal delay={240}>
          <p className={styles.after}>
            Then run <code>aello init</code> for the guided first-run wizard, or just{' '}
            <code>aello</code> for the full-screen TUI.
          </p>
        </Reveal>
      </div>
    </section>
  )
}

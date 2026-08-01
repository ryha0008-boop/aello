import Logo from './Logo'
import styles from './Footer.module.css'

const REPO = 'https://github.com/ryha0008-boop/aello'

export default function Footer() {
  return (
    <footer className={styles.footer}>
      <div className={`container ${styles.inner}`}>
        <div>
          <a href="#top" className={styles.brand}>
            <Logo size={18} />
            aello
          </a>
          <p className={styles.tagline}>Isolated Claude Code environments.</p>
        </div>

        <nav className={styles.links} aria-label="Footer">
          <a href={`${REPO}#readme`}>Documentation</a>
          <a href={`${REPO}/releases`}>Releases</a>
          <a href={`${REPO}/blob/main/CONTRIBUTING.md`}>Contributing</a>
          <a href={`${REPO}/labels/good%20first%20issue`}>Good first issues</a>
        </nav>
      </div>

      <div className={`container ${styles.legal}`}>
        <p>Dual-licensed under MIT or Apache-2.0.</p>
        <p>Not affiliated with Anthropic.</p>
      </div>
    </footer>
  )
}

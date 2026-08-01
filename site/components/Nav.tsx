import Logo from './Logo'
import styles from './Nav.module.css'

const REPO = 'https://github.com/ryha0008-boop/aello'

export default function Nav() {
  return (
    <header className={styles.nav}>
      <div className={`container ${styles.inner}`}>
        <a href="#top" className={styles.brand}>
          <Logo />
          aello
        </a>

        <nav className={styles.links} aria-label="Main">
          <a href="#how">How it works</a>
          <a href="#capabilities">Capabilities</a>
          <a href="#voice">Voice</a>
          <a href={`${REPO}#readme`}>Docs</a>
        </nav>

        <a className={styles.cta} href="#install">
          Install
        </a>
      </div>
    </header>
  )
}

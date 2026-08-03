import Link from 'next/link'
import Logo from './Logo'
import styles from './Nav.module.css'

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
          <a href="#roles">Roles</a>
          <a href="#workflows">Workflows</a>
          <Link href="/docs/">Docs</Link>
        </nav>

        <a className={styles.cta} href="#install">
          Install
        </a>
      </div>
    </header>
  )
}

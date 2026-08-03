import Link from 'next/link'
import Logo from '@/components/Logo'
import { slugs, getDoc } from '@/lib/docs'
import styles from './docs.module.css'

const REPO = 'https://github.com/ryha0008-boop/aello'

export default function DocsLayout({ children }: { children: React.ReactNode }) {
  const nav = slugs().map((s) => {
    const { title } = getDoc(s)
    return { slug: s, title }
  })

  return (
    <div className={styles.shell}>
      <header className={styles.bar}>
        <div className={styles.barInner}>
          <Link href="/" className={styles.brand}>
            <Logo />
            aello
          </Link>
          <span className={styles.crumb}>docs</span>
          <a className={styles.repo} href={REPO}>
            GitHub
          </a>
        </div>
      </header>

      <div className={styles.body}>
        {/* Sticky column on a wide screen; a horizontal scroll strip on a
            phone, so it needs no toggle and therefore no JavaScript. */}
        <div className={styles.sideWrap}>
          <nav className={styles.side} aria-label="Documentation">
            <Link href="/docs/" className={styles.sideLink}>
              Overview
            </Link>
            {nav.map((d) => (
              <Link key={d.slug} href={`/docs/${d.slug}/`} className={styles.sideLink}>
                {d.title}
              </Link>
            ))}
          </nav>
        </div>

        <main className={styles.main}>{children}</main>
      </div>
    </div>
  )
}

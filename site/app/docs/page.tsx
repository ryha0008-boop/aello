import type { Metadata } from 'next'
import Link from 'next/link'
import { allDocs } from '@/lib/docs'
import styles from './docs.module.css'

export const metadata: Metadata = {
  title: 'Docs — aello',
  description:
    'Reference documentation for aello: concepts, roles, workflows, skills, voice, and troubleshooting.',
}

export default function DocsIndex() {
  const docs = allDocs()

  return (
    <article className={styles.index}>
      <p className={styles.eyebrow}>Documentation</p>
      <h1 className={styles.indexTitle}>Everything aello does, and why it does it that way</h1>
      <p className={styles.lede}>
        These are the same pages that ship inside the binary. Run <code>aello docs</code> to list
        them in a terminal, <code>aello docs workflows</code> to print one, or press{' '}
        <code>?</code> in the TUI to read them there.
      </p>

      <div className={styles.cards}>
        {docs.map((d) => (
          <Link key={d.slug} href={`/docs/${d.slug}/`} className={styles.card}>
            <span className={styles.cardTitle}>{d.title}</span>
            <span className={styles.cardBlurb}>{d.blurb}</span>
            <span className={styles.cardSlug}>aello docs {d.slug}</span>
          </Link>
        ))}
      </div>
    </article>
  )
}

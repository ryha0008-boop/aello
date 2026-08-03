import type { Metadata } from 'next'
import { getDoc, slugs } from '@/lib/docs'
import styles from '../docs.module.css'

type Params = { params: Promise<{ slug: string }> }

export function generateStaticParams() {
  return slugs().map((slug) => ({ slug }))
}

export async function generateMetadata({ params }: Params): Promise<Metadata> {
  const { slug } = await params
  const doc = getDoc(slug)
  return { title: `${doc.title} — aello docs`, description: doc.blurb }
}

export default async function DocPage({ params }: Params) {
  const { slug } = await params
  const doc = getDoc(slug)
  const toc = doc.headings.filter((h) => h.depth === 2)

  return (
    <div className={styles.page}>
      <article className={styles.prose} dangerouslySetInnerHTML={{ __html: doc.html }} />

      {toc.length > 2 && (
        <aside className={styles.toc} aria-label="On this page">
          <p className={styles.tocTitle}>On this page</p>
          <ul>
            {toc.map((h) => (
              <li key={h.id}>
                <a href={`#${h.id}`} dangerouslySetInnerHTML={{ __html: h.text }} />
              </li>
            ))}
          </ul>
          <p className={styles.tocFoot}>
            <code>aello docs {doc.slug}</code>
          </p>
        </aside>
      )}
    </div>
  )
}

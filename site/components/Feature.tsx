import type { ReactNode } from 'react'
import Reveal from './Reveal'
import styles from './Feature.module.css'

type Props = {
  id?: string
  eyebrow: string
  title: string
  children: ReactNode
  visual: ReactNode
  /** Puts the visual on the left, for alternating rows. */
  reversed?: boolean
}

export default function Feature({ id, eyebrow, title, children, visual, reversed }: Props) {
  return (
    <section className="section" id={id}>
      <div className={`container ${styles.row}`} data-reversed={reversed ? 'true' : undefined}>
        <Reveal className={styles.prose}>
          <p className="eyebrow">{eyebrow}</p>
          <h2 className="sectionHeading">{title}</h2>
          <div className={styles.text}>{children}</div>
        </Reveal>

        <Reveal delay={80} className={styles.visual}>
          {visual}
        </Reveal>
      </div>
    </section>
  )
}

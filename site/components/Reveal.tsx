'use client'

import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react'

type Props = {
  children: ReactNode
  /** Stagger, in ms, applied as the CSS transition delay. */
  delay?: number
  className?: string
}

/**
 * Reveals its children on first scroll into view. The hidden state sits behind a
 * `scripting: enabled` media query in globals.css, so without JavaScript — where
 * nothing would ever set data-visible — the content simply renders.
 */
export default function Reveal({ children, delay = 0, className }: Props) {
  const ref = useRef<HTMLDivElement>(null)
  const [visible, setVisible] = useState(false)

  useEffect(() => {
    const el = ref.current
    if (!el) return

    const observer = new IntersectionObserver(
      ([entry]) => {
        if (!entry.isIntersecting) return
        setVisible(true)
        observer.disconnect()
      },
      { rootMargin: '0px 0px -10% 0px' }
    )

    observer.observe(el)
    return () => observer.disconnect()
  }, [])

  return (
    <div
      ref={ref}
      className={className ? `reveal ${className}` : 'reveal'}
      data-visible={visible}
      style={delay ? ({ '--reveal-delay': `${delay}ms` } as CSSProperties) : undefined}
    >
      {children}
    </div>
  )
}

'use client'

import { useEffect, useRef, useState } from 'react'
import styles from './CopyLine.module.css'

type Props = {
  command: string
  /** Shown as the shell prompt; omit for a bare line. */
  prompt?: string
}

export default function CopyLine({ command, prompt = '$' }: Props) {
  const [copied, setCopied] = useState(false)
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  useEffect(() => () => clearTimeout(timer.current), [])

  async function copy() {
    try {
      await navigator.clipboard.writeText(command)
    } catch {
      return
    }
    setCopied(true)
    clearTimeout(timer.current)
    timer.current = setTimeout(() => setCopied(false), 2000)
  }

  return (
    <div className={styles.line}>
      <code className={styles.command}>
        <span className={styles.prompt}>{prompt}</span>
        {command}
      </code>
      <button type="button" className={styles.copy} onClick={copy}>
        {copied ? 'Copied' : 'Copy'}
      </button>
      {/* The label swap is the only feedback that the copy worked, and a screen
          reader never announced it — a button's own text changing is not a live
          region. */}
      <span role="status" aria-live="polite" className={styles.srOnly}>
        {copied ? 'Copied to clipboard' : ''}
      </span>
    </div>
  )
}

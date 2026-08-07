import styles from './Visuals.module.css'

/** A note one environment left in another's inbox at the repo root. */
export function NoteVisual() {
  return (
    <figure className={styles.card}>
      <figcaption className={styles.filename}>frontend.NOTE.md</figcaption>
      <div className={styles.note}>
        <p className={styles.noteFrom}>from api — 2026-08-01</p>
        <p className={styles.noteHeading}>What I was doing</p>
        <p className={styles.noteLine}>
          Moving session tokens onto the new cookie helper in <code>auth/session.ts</code>.
        </p>
        <p className={styles.noteHeading}>The problem</p>
        <p className={styles.noteLine}>
          The login form still posts the old field name, so every request 401s after my change.
        </p>
        <p className={styles.noteHeading}>What you need to fix</p>
        <p className={styles.noteLine}>
          Rename the field to <code>session_token</code> in the form component — that side is yours.
        </p>
      </div>
    </figure>
  )
}

/**
 * The two env dirs side by side. Claude Code is on top because it is the
 * default and what everything is built around; Cline is the opt-in second.
 */
export function AgentsVisual() {
  return (
    <figure className={styles.card}>
      <figcaption className={styles.filename}>.claude-env-api/</figcaption>
      <div className={styles.note}>
        <p className={styles.noteHeading}>Claude Code — the default</p>
        <p className={styles.noteLine}>
          Isolated by <code>CLAUDE_CONFIG_DIR</code>, on one shared subscription token. The voice,
          the transcript archive and the per-turn response rules all ride on its hooks.
        </p>
      </div>

      <div className={styles.mute}>
        <code>.cline-env-runner/</code>
        <span className={styles.muteNote}>isolated by --config / --data-dir instead</span>
      </div>

      <div className={styles.note}>
        <p className={styles.noteHeading}>Cline — its own key</p>
        <p className={styles.noteLine}>
          A metered provider key, never inferred from the Claude login. Same persona, same four
          commands, same memory — delivered as rules Cline re-sends every request, since it has no
          hook to inject into.
        </p>
      </div>
    </figure>
  )
}

/** The spoken line, and the switch that stops it. */
export function VoiceVisual() {
  return (
    <figure className={styles.card}>
      <figcaption className={styles.filename}>Stop hook</figcaption>
      <div className={styles.voice}>
        <p className={styles.spoken}>
          <span className={styles.spokenLabel}>TL;DR:</span> Merged launch-prep into main and
          pushed. CI is building the release now.
        </p>
        <p className={styles.meta}>
          <span className={styles.wave} aria-hidden="true">
            <i />
            <i />
            <i />
            <i />
            <i />
          </span>
          spoken · this session leased the voice “negras”
        </p>
      </div>
      <div className={styles.mute}>
        <code>$ aello voice mute</code>
        <span className={styles.muteNote}>silences every env, from any directory</span>
      </div>
    </figure>
  )
}

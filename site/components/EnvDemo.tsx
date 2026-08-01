'use client'

import { useState, type CSSProperties } from 'react'
import styles from './EnvDemo.module.css'

type Blueprint = {
  name: string
  hue: string
  model: string
  owns: string
}

const BLUEPRINTS: Blueprint[] = [
  { name: 'coder', hue: 'var(--hue-coder)', model: 'opus', owns: 'writes the code' },
  { name: 'docs', hue: 'var(--hue-docs)', model: 'sonnet', owns: 'keeps the docs true' },
  { name: 'ops', hue: 'var(--hue-ops)', model: 'sonnet', owns: 'owns the pipeline' },
]

/** What `aello run` puts inside an env dir. */
const ENV_CONTENTS = ['CLAUDE.md', 'settings.json', 'hooks/', 'skills/sync/']

const COMMITS = [
  { env: 'coder', sha: 'a3f19c2', message: 'feat: parse the manifest header', when: '3m' },
  { env: 'docs', sha: '7b02de4', message: 'docs: document the /sync skill', when: '52m' },
  { env: 'ops', sha: 'c1d8a05', message: 'ci: cache the cargo registry', when: '2h' },
  { env: 'coder', sha: '5e4470b', message: 'fix: reject empty blueprint names', when: '4h' },
]

const OTHER_PATHS = ['claude-internal/', 'src/', 'CLAUDE.md', 'README.md']

export default function EnvDemo() {
  const [selected, setSelected] = useState<string | null>(null)

  const active = BLUEPRINTS.find((b) => b.name === selected)
  const dimmed = (name: string) => selected !== null && selected !== name

  return (
    <figure className={styles.demo}>
      <div className={styles.chips} role="group" aria-label="Blueprints in this repo">
        {BLUEPRINTS.map((bp) => (
          <button
            key={bp.name}
            type="button"
            className={styles.chip}
            style={{ '--hue': bp.hue } as CSSProperties}
            aria-pressed={selected === bp.name}
            onClick={() => setSelected(selected === bp.name ? null : bp.name)}
          >
            <span className={styles.dot} />
            {bp.name}
          </button>
        ))}
      </div>

      <div className={styles.panes}>
        <div className={styles.pane}>
          <p className={styles.paneLabel}>~/my-project</p>
          <ul className={styles.tree}>
            {BLUEPRINTS.map((bp) => (
              <li key={bp.name}>
                <span
                  className={styles.envRow}
                  style={{ '--hue': bp.hue } as CSSProperties}
                  data-dim={dimmed(bp.name)}
                >
                  <span className={styles.glyph}>├─</span>
                  <span className={styles.dot} />
                  .claude-env-{bp.name}/
                </span>

                {selected === bp.name && (
                  <ul className={styles.subtree}>
                    {ENV_CONTENTS.map((file, i) => (
                      <li key={file} className={styles.subRow}>
                        <span className={styles.glyph}>
                          {i === ENV_CONTENTS.length - 1 ? '│  └─' : '│  ├─'}
                        </span>
                        {file}
                      </li>
                    ))}
                  </ul>
                )}
              </li>
            ))}

            {OTHER_PATHS.map((path, i) => (
              <li key={path} className={styles.plainRow}>
                <span className={styles.glyph}>{i === OTHER_PATHS.length - 1 ? '└─' : '├─'}</span>
                {path}
              </li>
            ))}
          </ul>
        </div>

        <div className={styles.pane}>
          <p className={styles.paneLabel}>git log --format=&quot;%h %an %s&quot;</p>
          <ul className={styles.log}>
            {COMMITS.map((commit) => {
              const bp = BLUEPRINTS.find((b) => b.name === commit.env)
              return (
                <li
                  key={commit.sha}
                  className={styles.commit}
                  style={{ '--hue': bp?.hue } as CSSProperties}
                  data-dim={dimmed(commit.env)}
                >
                  <span className={styles.sha}>{commit.sha}</span>
                  <span className={styles.author}>
                    <span className={styles.dot} />
                    {commit.env}
                  </span>
                  <span className={styles.message}>{commit.message}</span>
                  <span className={styles.when}>{commit.when}</span>
                </li>
              )
            })}
          </ul>
        </div>
      </div>

      <figcaption className={styles.caption}>
        {active ? (
          <>
            <strong className={styles.captionStrong}>{active.name}</strong> runs {active.model} from
            its own <code>CLAUDE_CONFIG_DIR</code> and {active.owns}. Its commits are authored as{' '}
            <code>
              {active.name} &lt;{active.name}@aello.local&gt;
            </code>
            .
          </>
        ) : (
          <>Pick a blueprint to see what it keeps to itself — and what it signed.</>
        )}
      </figcaption>
    </figure>
  )
}

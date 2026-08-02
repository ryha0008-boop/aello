import type { Metadata } from 'next'
import { Mona_Sans, Hubot_Sans } from 'next/font/google'
import './globals.css'

// Mona Sans is the brand face; Hubot Sans is the system's alternate display face
// (--brand-fontStack-sansSerifAlt), used here for headings only.
const monaSans = Mona_Sans({
  subsets: ['latin'],
  axes: ['wdth'],
  display: 'swap',
  variable: '--font-mona',
})

const hubotSans = Hubot_Sans({
  subsets: ['latin'],
  axes: ['wdth'],
  display: 'swap',
  variable: '--font-hubot',
})

export const metadata: Metadata = {
  // The site's own origin — relative og:image/canonical URLs resolve against
  // this, and pointing it at the repo made every one of them resolve to a
  // github.com path that does not exist.
  metadataBase: new URL('https://ryha0008-boop.github.io/aello/'),
  title: 'aello — isolated Claude Code environments',
  description:
    'aello gives every Claude Code agent its own config dir, persona, and skills, placed into any project with one command. Like a venv, for agents.',
  openGraph: {
    title: 'aello — isolated Claude Code environments',
    description:
      'Many agents, one repo, no collisions. Isolated Claude Code environments with shared login and per-agent git attribution.',
    type: 'website',
  },
}

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <html lang="en" className={`${monaSans.variable} ${hubotSans.variable}`}>
      <body>{children}</body>
    </html>
  )
}

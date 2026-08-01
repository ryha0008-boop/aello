import Nav from '@/components/Nav'
import Hero from '@/components/Hero'
import Steps from '@/components/Steps'
import Capabilities from '@/components/Capabilities'
import Feature from '@/components/Feature'
import { NoteVisual, VoiceVisual } from '@/components/Visuals'
import Install from '@/components/Install'
import Footer from '@/components/Footer'

export default function Home() {
  return (
    <>
      <Nav />

      <main>
        <Hero />
        <Steps />
        <Capabilities />

        <Feature
          id="skills"
          eyebrow="Universal skills"
          title="Agents that leave each other notes"
          visual={<NoteVisual />}
          reversed
        >
          <p>
            Every blueprint gets <code>/handoff</code> and <code>/note</code>, whatever else you
            enabled. <code>/handoff</code> writes a self-contained resume note before you clear a
            session, so the next one starts mid-thought instead of re-reading the diff.
          </p>
          <p>
            <code>/note</code> is for the other agent. When the env working on the API breaks
            something the frontend env owns, it writes to that env&apos;s inbox at the repo root.
            The target reads it on its next run, fixes its side, and deletes it.
          </p>
        </Feature>

        <Feature
          id="voice"
          eyebrow="The voice capability"
          title="Hear the summary, not the wall of text"
          visual={<VoiceVisual />}
        >
          <p>
            With <code>--voice</code>, an env speaks the trailing <code>TL;DR:</code> line of each
            response through a neural voice — that line and nothing else. You find out a long run
            finished without sitting and watching it.
          </p>
          <p>
            Concurrent sessions lease different voices, playback is serialised across the machine so
            two envs never talk over each other, and one command stops all of them at once.
          </p>
        </Feature>

        <Install />
      </main>

      <Footer />
    </>
  )
}

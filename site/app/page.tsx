import Nav from '@/components/Nav'
import Hero from '@/components/Hero'
import Steps from '@/components/Steps'
import Roles from '@/components/Roles'
import Workflows from '@/components/Workflows'
import Feature from '@/components/Feature'
import { AgentsVisual, NoteVisual, VoiceVisual } from '@/components/Visuals'
import Install from '@/components/Install'
import Footer from '@/components/Footer'

export default function Home() {
  return (
    <>
      <Nav />

      <main>
        <Hero />
        <Steps />
        <Roles />

        <Feature id="agents" eyebrow="Two agents" title="Claude Code, or the Cline CLI" visual={<AgentsVisual />}>
          <p>
            A blueprint drives one CLI, chosen at <code>add</code> time and fixed. Claude Code is
            the default and what everything here is built around; <code>--agent cline</code> gets
            you the other. They share nothing but the project directory — separate env dirs,
            separate logins, separate everything.
          </p>
          <p>
            A Cline env is <strong>metered</strong>: it runs on your own provider key and every turn
            costs money per token, where a Claude env costs nothing beyond the subscription. Its env
            dir is gitignored unconditionally, because that key sits in plaintext inside it.
          </p>
        </Feature>

        <Feature
          id="skills"
          eyebrow="Universal skills"
          title="Agents that leave each other notes"
          visual={<NoteVisual />}
          reversed
        >
          <p>
            Every blueprint gets <code>/handoff</code> and <code>/note</code>, whatever its
            role. <code>/handoff</code> writes a self-contained resume note before you clear a
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
          eyebrow="The voice"
          title="Hear the summary, not the wall of text"
          visual={<VoiceVisual />}
        >
          <p>
            Every Claude env speaks the trailing <code>TL;DR:</code> line of each response through a
            neural voice — that line and nothing else. There is nothing to switch on. You find out
            a long run finished without sitting and watching it. (A Cline env is silent: it fires no
            end-of-response hook to speak from.)
          </p>
          <p>
            Concurrent sessions lease different voices, playback is serialised across the machine so
            two envs never talk over each other, and one command stops all of them at once.
          </p>
        </Feature>

        <Workflows />

        <Install />
      </main>

      <Footer />
    </>
  )
}

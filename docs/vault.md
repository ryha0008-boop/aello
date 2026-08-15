# Secrets: `.aello-env` and the vault

A project that needs an API key declares its **names** in a committed
`.aello-env`. The values come from an external secret store, which launches
aello rather than the other way round. **aello never resolves a secret and never
holds one.**

## The file

One bare variable name per line. Blank lines and `#` comments are ignored.

```
# what this project needs at runtime
OPENROUTER_API_KEY
DATABASE_URL
```

**Commit it.** With bare names it holds nothing secret, and it is how a second
machine — or a fresh clone — learns what the project needs without anyone
carrying a value across.

**A line containing `=` is an error, not a value.** This is the point of the
format rather than a style rule: a `KEY=value` file eventually has a real key
typed into it, sitting in a project directory. Bare names make that structurally
impossible. A malformed name (`OPEN-ROUTER`, `$KEY`, `2FA`) is also an error —
never a silent skip, because a file that looks configured while injecting
nothing is the failure this whole path exists to avoid.

## How a value actually arrives

The secret store is the **outer** process:

```
vault.ps1 run -NoCapture … -- aello run <blueprint>
```

**With `aello vault <path>` set you don't type that** — `aello run` re-runs
itself inside the store, asking for this project's declared names plus whichever
of its own credentials have left `config.toml`. One command, same result:

```
$ aello run MyEnv
[vault] injected OPENROUTER_API_KEY, AELLO_OAUTH_TOKEN (values hidden)
…
```

It only happens when there is something to fetch, so a project that declares
nothing — which is nearly all of them — launches exactly as before. A guard
variable on the child stops the second aello wrapping again; it lives on the
process, not on disk, so there is nothing to go stale.

**Declared names are fetched even when they are already set**, because the store
is the authority. That is not theoretical: on the machine this was built on, a
User-scope `OPENROUTER_API_KEY` sat in the registry with a *different* value from
the stored one, satisfied the declaration check, and a launch carried the wrong
key while reporting success.

Two limits, both loud rather than silent:

- **A launch passing `--` extras cannot be wrapped**, and aello refuses it with
  the manual command rather than launching without the values. A bare `--`
  anywhere in a `powershell -File` argument list is eaten by PowerShell's
  parameter binder, and every argument after it fails to bind — measured three
  ways, `--%` included, which does not help.
- **`-NoCapture` turns the store's output masking off.** It has to: a redirected
  stdout is a pipe, not a console, so a full-screen TUI child gets no terminal.
  Fine for a terminal you are looking at; wrong for a run you redirect to a file.

Everything aello spawns inherits its environment. That is measured, not assumed:
a variable set on aello's process reaches the `claude` child unchanged, because
`std::process::Command` inherits the parent environment and aello removes only
three specific credential names.

So aello adds no injection mechanism. It does three things injection alone leaves
broken.

### 1. It refuses to launch with a declared secret missing

```
$ aello run MyEnv
error: …/.aello-env declares OPENROUTER_API_KEY which is not set.
Launch through the vault so the value never passes through aello:
  vault.ps1 run -NoCapture … -- aello run <blueprint>
```

Without this you get an agent that works until it first needs the key, and the
measured response to *that* is the user pasting the key into the chat — which is
how one OpenRouter key ended up in twelve transcript records. Failing at second
zero is cheaper than a leak.

A **present-but-empty** variable counts as missing. A variable set to nothing
reads as configured everywhere and injects nothing, which is worse than absent
because it silences the check written to catch it.

### 2. It stops secrets leaking sideways between projects

Agents run `aello` from inside an aello env routinely. Without a declaration,
project A's keys would ride into project B's session while B's own `.aello-env`
asks for none.

Each launch sets `AELLO_DECLARED` to its own list. A nested `aello run` reads it
and strips every inherited name its project does not declare. aello never needs
to know what the store holds — the marker is self-describing.

### 3. It lets aello's own credentials leave `config.toml`

`config.toml` stores the Claude subscription token and the Cline provider key in
plaintext. Both can come from the store instead:

| Variable | Replaces |
| --- | --- |
| `AELLO_OAUTH_TOKEN` | `oauth_token` |
| `AELLO_CLINE_API_KEY` | `[cline].api_key` |

When set, each wins over the config file, so the config value can simply be
deleted.

#### Point aello at the store, and `aello login` does the move for you

```
$ aello vault C:\path\to\vault.ps1
Vault set. `aello login` will store credentials there instead of config.toml.
```

With that set, both logins hand the credential to the store over **stdin** and
then remove the plaintext copy from `config.toml`:

```
$ aello login
Running 'claude setup-token' — complete the login in your browser...
<token received — hidden from stdout>
Stored AELLO_OAUTH_TOKEN  (fingerprint 81c8b503)
Stored the token in the vault as AELLO_OAUTH_TOKEN.
Removed the plaintext copy from config.toml.
Launch through the vault so it reaches the env:
  vault.ps1 run AELLO_OAUTH_TOKEN -NoCapture -- aello run <blueprint>
```

`aello vault` with no argument shows the current setting and whether the script
is still reachable; `--clear` forgets it and logins go back to `config.toml`.

**Writing is not resolving.** The rule that aello never reads a secret out of the
store still holds — there is no verb here that gets one back. `login` already
holds the plaintext for a moment, because it just captured it from `claude
setup-token` or from a prompt; the only question is where it goes next, and a
pipe into the store beats a write into a file that keeps it forever. The value
goes over stdin and never as an argument, because arguments are visible in
process listings and shell history.

Three things this deliberately refuses rather than papers over:

- **A configured-but-missing script is an error, not a fallback.** Degrading to
  "no vault" would send the next `aello login` down the `config.toml` path and
  write a fresh plaintext copy of the credential you moved out — silently, and
  with the exact opposite of the intended effect.
- **A multi-line value is refused.** The store reads one line, so it would be
  saved truncated: a credential that is silently half a credential.
- **A store that exits non-zero fails the login.** By then `config.toml` has not
  been touched, so nothing is lost — but reporting success would leave the
  credential in neither place.

The setting is **per machine**, because `config.toml` is. A Linux box leaves it
unset and keeps the `config.toml` fallback; nothing detects a store, because a
detector is a cache that goes stale the moment the checkout moves and it would
make one repo behave differently on two machines.

⚠️ **The move is one-way.** Once the plaintext is out of `config.toml`, a plain
`aello run` has no credential — the store is the *outer* process, so every
launch has to go through it. That is the intended end state, not a bug, but it
changes the command you type every day.

⚠️ **Not `CLAUDE_CODE_OAUTH_TOKEN`.** That name is stripped from every child by
`scrub_inherited_credentials`, so a store-supplied one is deleted before it can
be used — measured: with the token removed from `config.toml` the child saw an
empty value and every env fell back to an interactive login. Weakening the scrub
is not the fix; it exists so an agent running `aello` inside an env cannot
authenticate as whoever owns the ambient variable.

⚠️ **With no vault configured, `aello login` writes a plaintext copy back.**
`login` and `edit` serialize the whole config to disk. If the store is already
supplying a credential, both commands warn before saving. They warn rather than
refuse, because a login is also how you replace a credential you have lost.
`aello vault <path>` is what turns that warning into the store doing the work.

## What this does not do

- **No masking.** A value in the session's environment can still be printed by
  anything that prints an environment. aello does not pattern-match tool calls
  to prevent that: across 83 findings in this machine's transcript scan,
  `printenv` / `echo $VAR` / `Get-ChildItem Env:` appear **zero** times, while
  the routes that do fire are config-file reads, keys inlined into `curl`, and
  values written into prose — none of which a command-text pattern catches.
- **No rotation, no expiry, no reload.** A session's environment is fixed when it
  spawns. A key added afterwards needs a relaunch — or, for a one-off, run the
  single command that needs it through the store directly, which keeps the value
  out of the session entirely.

## Testing it end to end

In order. Each step is checkable without printing a secret.

**1. Point aello at the store.** `aello vault <path-to-vault.ps1>`, then `aello
vault` on its own — it should say `Reachable`.

**2. Log in, and watch where the credential goes.** Both logins go *into aello*;
aello writes the store. You never run `vault.ps1 set` by hand for these.

- Claude: `aello login`. The browser flow is unchanged — `claude setup-token`
  opens it, you sign in with the subscription, and it mints a long-lived token.
  aello then prints `Stored the token in the vault as AELLO_OAUTH_TOKEN` and
  `Removed the plaintext copy from config.toml`.
- Cline: `aello login --agent cline`. Paste the provider key at the hidden
  prompt. Provider, model and base URL stay in `config.toml`; only the key moves.

`vault.ps1 list` should now show both names with a changed `Updated` time. A
**blank** key at the Cline prompt means "keep what is stored" — aello cannot read
it back, so it leaves it alone.

**3. Confirm `config.toml` no longer holds them.** `oauth_token` and
`[cline].api_key` should be gone from `%APPDATA%\aello\config\config.toml`. The
TUI's footer should read `AUTH: VAULT ✓`, not `AUTH: NONE ✗`.

**4. Launch, without typing a wrapper.** A plain `aello run <blueprint>` should
print a `[vault] injected …` banner and then start normally. That banner *is* the
proof aello wrapped itself; no banner means it did not, and the session is
running on whatever `config.toml` still had.

**5. Prove the right value arrived.** Compare fingerprints, never values — see
below. A launch that succeeds while fingerprints *disagree* means something other
than the store supplied it, which is the failure worth telling apart from a
clean one.

**A project secret** is the same path with one extra step: put its bare name in a
committed `.aello-env` at the project root, store it under the identical name,
and launch normally. Name equality is the whole contract.

## Verifying a secret arrived without leaking it

Printing the value to prove you have it defeats the purpose. Compare
fingerprints instead: the store's `list` shows the first four bytes of the
SHA-256 of the plaintext, and the same digest computed from the environment
variable should match. Eight hex characters prove identity and reveal nothing.

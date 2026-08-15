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

⚠️ **Not `CLAUDE_CODE_OAUTH_TOKEN`.** That name is stripped from every child by
`scrub_inherited_credentials`, so a store-supplied one is deleted before it can
be used — measured: with the token removed from `config.toml` the child saw an
empty value and every env fell back to an interactive login. Weakening the scrub
is not the fix; it exists so an agent running `aello` inside an env cannot
authenticate as whoever owns the ambient variable.

⚠️ **`aello login` writes a plaintext copy back.** `login` and `edit` serialize
the whole config to disk. If the store is already supplying a credential, both
commands warn before saving. They warn rather than refuse, because a login is
also how you replace a credential you have lost.

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

## Verifying a secret arrived without leaking it

Printing the value to prove you have it defeats the purpose. Compare
fingerprints instead: the store's `list` shows the first four bytes of the
SHA-256 of the plaintext, and the same digest computed from the environment
variable should match. Eight hex characters prove identity and reveal nothing.

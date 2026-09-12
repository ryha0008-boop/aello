#!/bin/sh
# aello-pre-commit v1 — refuse to commit key material.
#
# Seeded by aello into every project a `github` blueprint is placed in. The
# reason it exists here rather than in any one repo: aello's `/sync` mirrors an
# env's memory, persona and handoff into the tracked `claude-internal/<Env>/`
# folder and stages it by path, with no check of what is in it. Memory files are
# exactly where a session writes down a password it just used, and one of these
# repos is public.
#
# It is deliberately NARROW. It blocks two things and nothing else:
#
#   1. Credentials that spend money — provider API keys, OAuth tokens.
#   2. Passwords and key material — SSH keys, .env, certificate bundles.
#
# It says nothing about IP addresses, hostnames, machine paths or domains. That
# is a decision, not an oversight: a hook that cries wolf is bypassed with
# --no-verify and then protects nothing at all.
#
# Enabled per clone with `git config core.hooksPath .githooks`. That setting
# does NOT travel with a pull, so a fresh clone silently has no guard — aello
# re-runs it on every placement, which is what heals it.
#
# If this blocks you, move the secret out of the commit. Never pass --no-verify.

fail=0
note() { printf '\n  BLOCKED: %s\n' "$1"; fail=1; }

# Staged, added/copied/modified only. -z + tr keeps paths with spaces intact.
staged=$(git diff --cached --name-only --diff-filter=ACM -z | tr '\0' '\n')
[ -z "$staged" ] && exit 0

# ---- filenames that are fatal by existing -----------------------------------
# .env.example / .sample / .template are documentation and are allowed through.
echo "$staged" | grep -Eq '(^|/)(id_rsa|id_dsa|id_ecdsa|id_ed25519)$' \
  && note "an SSH private key file is staged"
echo "$staged" | grep -Eq '(^|/)\.env(\.local|\.production|\.prod)?$' \
  && note "a real .env file is staged (.env.example is fine, this is not)"
echo "$staged" | grep -Eiq '\.(pem|pfx|p12|jks|keystore|ppk|kdbx)$' \
  && note "a key/certificate bundle is staged"
echo "$staged" | grep -Eq '(^|/)(\.netrc|_netrc|\.pgpass|\.htpasswd)$' \
  && note "a credential file is staged"

# ---- content: only what cannot be anything but a real secret ----------------
for f in $staged; do
    git cat-file -e ":$f" 2>/dev/null || continue
    # Skip binaries.
    git diff --cached --numstat -- "$f" | grep -q '^-' && continue

    blob=$(git cat-file -p ":$f" 2>/dev/null) || continue

    # ANCHORED to line start. A real armored key always sits on its own line;
    # the same string quoted inside source does not. Without the anchor this
    # hook flags itself — and a check whose first act is to block its own author
    # gets deleted, not fixed.
    if printf '%s\n' "$blob" | grep -Eq '^-----BEGIN [A-Z ]*PRIVATE KEY-----[[:space:]]*$'; then
        note "$f contains an armored PRIVATE KEY block"
    fi
    if printf '%s\n' "$blob" | grep -Eq '^PuTTY-User-Key-File'; then
        note "$f contains a PuTTY private key"
    fi

    # A port-knock sequence is a shared secret whose entire value is being
    # unknown. Matched GENERICALLY — the sequence itself must never appear in
    # this file, or the hook protecting the secret becomes the one leaking it.
    # Either order: "knock 1/2/3" and "1/2/3 port knock" both occur in prose.
    knockrx='knock.{0,40}([0-9]{3,5}[ ,/:>-]{1,2}){2,}[0-9]{3,5}|([0-9]{3,5}[ ,/:>-]{1,2}){2,}[0-9]{3,5}.{0,40}knock'
    if printf '%s\n' "$blob" | grep -Eiq "$knockrx"; then
        note "$f looks like it contains a port-knock sequence"
    fi

    # Provider keys, minus the placeholder forms docs and fixtures use.
    hit=$(printf '%s' "$blob" | grep -Eo \
        'AKIA[0-9A-Z]{16}|gh[pousr]_[A-Za-z0-9]{36,}|AIza[0-9A-Za-z_-]{35}|sk_live_[0-9A-Za-z]{20,}|xox[baprs]-[0-9A-Za-z]{8,}-[0-9A-Za-z]{8,}|sk-ant-[a-z0-9-]{20,}' \
        | grep -Eiv 'x{3,}|example|placeholder|your|dummy|fake|test|sample|redacted' \
        | head -1)
    [ -n "$hit" ] && note "$f contains what looks like a live provider API key"
done

if [ "$fail" -ne 0 ]; then
    cat <<'EOF'

  Nothing was committed.

  aello mirrors this env's memory and handoff into claude-internal/ and stages
  it by path, so a secret written down in a memory note reaches git without
  anyone deciding it should. Move it out of the commit and try again.

  Do NOT use --no-verify. If this is a false positive, fix the pattern in
  .githooks/pre-commit so the next person is protected too — but note that
  aello replaces its own copy of this file on the next placement, so a fix
  belongs upstream in aello's src/pre_commit_hook.sh.

EOF
    exit 1
fi
exit 0

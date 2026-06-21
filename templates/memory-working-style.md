---
name: working-style
description: "User's working style: go slow, verify each feature for real, don't rebuild what works, small scoped commits"
metadata:
  node_type: memory
  type: feedback
---

Go slow — one feature at a time, the user manually verifies each on BOTH Windows and the Linux VPS before moving on. Don't "go ham"/big-bang. Structure work in small, independently-compilable, independently-verifiable increments and pause for verification. Don't rebuild what works — carry proven components over untouched, target only what's broken. **Verify for real, don't claim** — when asked to test, actually run it and report concrete output. Small scoped commits; CHANGELOG entry for every user-facing change in the same commit. The user writes long stream-of-consciousness specs — extract real requirements, play them back to confirm, then build; ask when genuinely ambiguous rather than guessing.
**Why:** per-step verification catches regressions early; proven components carry no rebuild risk. **How to apply:** small phases, each verifiable; stop for sign-off between them.

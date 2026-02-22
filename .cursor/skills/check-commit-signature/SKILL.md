---
name: check-commit-signature
description: Checks whether a git commit is signed, including SSH-signed commits, without requiring local trust configuration. Use when the user asks if a commit is signed or to gate push behavior on signature presence.
---

# Check Commit Signature

## When to use

Use this skill when you need to answer:

- "Is this commit signed?"
- "Push only if commit is signed"
- "Verify commit signature status"

## Default method (signature present)

Use this command first:

```bash
git cat-file -p <rev> | grep "^gpgsig"
```

- If it matches, the commit contains a signature block (`GPG` or `SSH`).
- If it does not match, the commit is unsigned.

Recommended default revision:

```bash
git cat-file -p HEAD | grep "^gpgsig"
```

## Why this is the default

`git log --show-signature` or `%G?` can report `N` for valid SSH-signed commits when local trust is not configured (for example, missing `gpg.ssh.allowedSignersFile`).

Presence-checking `gpgsig` answers the practical question "is the commit signed?" without depending on local verifier config.

## Optional stricter verification

If trust config is known to be set up, add:

```bash
git log -1 --show-signature <rev>
```

Report this separately as "verified locally" vs "signed but not locally verifiable."

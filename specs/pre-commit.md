# Pre-Commit Hooks

| | |
|--------|----------------------------------------------|
| Version | 0.1 |
| Status | Wrenching |
| Last Updated | 2026-01-24 |

## Overview

Pre-commit hooks provide automatic validation on every commit. Agents can't sidestep them - they run automatically via git.

**Design principles:**
- **Automatic** - No manual steps, hooks run on every `git commit`
- **Fast** - Must complete in <5s or developers bypass them
- **Simple** - Shell scripts, no external tools

## Specification

### Hook Location

Hooks live in `.githooks/` (committed to repo):

```
moto/
└── .githooks/
    └── pre-commit
```

Git is configured to use this directory via `core.hooksPath`.

### Installation

| Context | How |
|---------|-----|
| **Garage (container)** | Automatic - configured in NixOS |
| **Local development** | `make install` (one-time setup) |

### Pre-Commit Hook

**`.githooks/pre-commit`:**

```bash
#!/usr/bin/env bash
set -e

# Block secrets
if git diff --cached --name-only | grep -qE '\.(pem|key)$|\.env'; then
    echo "ERROR: Sensitive files detected"
    exit 1
fi

# Rust checks (if Rust files changed)
if git diff --cached --name-only | grep -qE '\.(rs)$|Cargo\.'; then
    cargo fmt --all --check
fi

# Nix checks (if Nix files changed)
if git diff --cached --name-only | grep -qE '\.nix$' && command -v nix &>/dev/null; then
    nix flake check --no-build 2>/dev/null || {
        echo "Nix syntax error"
        exit 1
    }
fi
```

### What the Hook Checks

| Check | When | Purpose |
|-------|------|---------|
| Secret detection | Always | Block `.pem`, `.key`, `.env` files |
| `cargo fmt --check` | Rust files changed | Formatting |
| `nix flake check --no-build` | Nix files changed | Syntax validation |

### What's NOT in the Hook

- `cargo clippy` - Too slow, run in CI
- `cargo test` - Too slow, run in CI
- `nix build` - Too slow, run in CI

The hook catches obvious mistakes fast. CI does thorough validation.

### Bypassing

For emergencies:

```bash
git commit --no-verify -m "WIP: emergency"
```

Agents should NEVER use `--no-verify` unless explicitly instructed.

## References

- [git hooks documentation](https://git-scm.com/docs/githooks)
- [core.hooksPath](https://git-scm.com/docs/git-config#Documentation/git-config.txt-corehooksPath)

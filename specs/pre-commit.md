# Pre-Commit Hooks

| | |
|--------|----------------------------------------------|
| Version | 0.2 |
| Status | Ripping |
| Last Updated | 2026-02-06 |

## Overview

Git hooks that run automatically on every commit. Agents can't sidestep them.

## Specification

### Hook: `.githooks/pre-commit`

```bash
#!/usr/bin/env bash
set -e

# Block secrets
if git diff --cached --name-only | grep -qE '\.(pem|key)$|\.env'; then
    echo "ERROR: Sensitive files detected"
    echo "FIX: git reset HEAD <filename>"
    exit 1
fi

# Rust formatting and linting (if Rust files changed)
if git diff --cached --name-only | grep -qE '\.(rs)$|Cargo\.'; then
    cargo fmt --all --check || {
        echo "FIX: cargo fmt --all"
        exit 1
    }
    cargo clippy --all-targets -- -D warnings || {
        echo "FIX: cargo clippy --fix --all-targets"
        exit 1
    }
fi

# Nix syntax (if Nix files changed)
if git diff --cached --name-only | grep -qE '\.nix$' && command -v nix &>/dev/null; then
    nix flake check --no-build 2>/dev/null || {
        echo "FIX: check nix flake check output"
        exit 1
    }
fi
```

### Installation

| Context | How |
|---------|-----|
| Garage container | Automatic via NixOS config |
| Local dev | `make install` sets `core.hooksPath` |

### What's NOT in the Hook

- `cargo test` - too slow, run in CI
- `nix build` - too slow, run in CI

### Bypass

```bash
git commit --no-verify -m "message"
```

Agents: avoid `--no-verify`. If the hook fails, fix the issue instead.

## Changelog

### v0.2 (2026-02-06)
- Add `cargo clippy` to pre-commit hook (~1s overhead)

### v0.1 (2026-01-24)
- Initial spec

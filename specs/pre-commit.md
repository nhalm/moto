# Pre-Commit Hooks

| | |
|--------|----------------------------------------------|
| Version | 0.1 |
| Status | Wrenching |
| Last Updated | 2026-01-24 |

## Overview

Pre-commit hooks provide fast feedback to agents (and humans) that their changes are on track. When working in loops, agents need to know immediately if they've broken something - not after a full CI run.

**Design principles:**
- **Fast first** - Pre-commit must be fast (<10s) or agents will skip it
- **Fail early** - Catch obvious errors before they compound
- **Path-selective** - Only run checks relevant to changed files
- **Nix-aware** - Validate Nix changes don't break the container build

## Specification

### Tool: prek

We use [prek](https://prek.j178.dev/) - a fast, Rust-based pre-commit hook manager with no Python dependency.

**Installation:**
```bash
# Via cargo
cargo install prek

# Or via nix (when available in nixpkgs)
nix profile install nixpkgs#prek
```

**Usage:**
```bash
prek install          # Install hooks
prek run --all-files  # Run all hooks on all files
prek run <hook-id>    # Run specific hook
```

### Configuration

**`.pre-commit-config.yaml`:**

```yaml
# prek configuration
# Install hooks: prek install
# Run all hooks: prek run --all-files
# Run specific hook: prek run <hook-id>

default_stages: [pre-commit]

repos:
  - repo: local
    hooks:
      # === Security ===

      - id: block-secrets
        name: Block secrets
        entry: scripts/hooks/block-secrets.sh
        language: script
        types: [text]
        pass_filenames: false
        always_run: true

      # === Rust (only when Rust files change) ===

      - id: cargo-fmt
        name: Rust formatting
        entry: cargo fmt --all --check
        language: system
        files: '\.(rs)$|Cargo\.(toml|lock)$'
        pass_filenames: false

      # === Nix (only when Nix files change) ===

      - id: nix-fmt
        name: Nix formatting
        entry: nixfmt --check
        language: system
        files: '\.nix$'
        pass_filenames: true

      - id: nix-check
        name: Nix syntax check
        entry: scripts/hooks/nix-check.sh
        language: script
        files: '\.nix$|flake\.lock$'
        pass_filenames: false

  # === Pre-push hooks (thorough checks) ===

  - repo: local
    hooks:
      - id: cargo-clippy
        name: Rust lints
        entry: cargo clippy --workspace --all-targets -- -D warnings
        language: system
        files: '\.(rs)$|Cargo\.(toml|lock)$'
        pass_filenames: false
        stages: [pre-push]

      - id: cargo-test
        name: Rust tests
        entry: cargo test --workspace
        language: system
        files: '\.(rs)$|Cargo\.(toml|lock)$'
        pass_filenames: false
        stages: [pre-push]

      - id: nix-build-check
        name: Nix build validation
        entry: scripts/hooks/nix-build-check.sh
        language: script
        files: '\.nix$|flake\.lock$|^infra/'
        pass_filenames: false
        stages: [pre-push]
```

### Hook Scripts

**`scripts/hooks/block-secrets.sh`:**

```bash
#!/usr/bin/env bash
set -euo pipefail

# Check staged files for potential secrets
STAGED=$(git diff --cached --name-only)

if echo "$STAGED" | grep -qE '\.(pem|key)$'; then
    echo "ERROR: Private key files detected"
    exit 1
fi

if echo "$STAGED" | grep -qE '(^|/)\.env($|\.)'; then
    echo "ERROR: .env files detected"
    exit 1
fi

if echo "$STAGED" | grep -qE 'credentials.*\.json$'; then
    echo "ERROR: Credentials file detected"
    exit 1
fi

exit 0
```

**`scripts/hooks/nix-check.sh`:**

```bash
#!/usr/bin/env bash
set -euo pipefail

# Skip if nix not available
if ! command -v nix &>/dev/null; then
    echo "Skipping: nix not available"
    exit 0
fi

# Fast syntax check (no build)
nix flake check --no-build 2>&1 || {
    echo "Nix syntax error - run 'nix flake check' for details"
    exit 1
}
```

**`scripts/hooks/nix-build-check.sh`:**

```bash
#!/usr/bin/env bash
set -euo pipefail

# Skip if nix not available
if ! command -v nix &>/dev/null; then
    echo "Skipping: nix not available"
    exit 0
fi

# Dry-run validates evaluation without building
echo "Validating Nix build (dry-run)..."
nix build .#moto-garage --dry-run
```

### Hook Summary

| Hook ID | Stage | Files | Purpose | Time |
|---------|-------|-------|---------|------|
| `block-secrets` | pre-commit | all | Block secrets | <1s |
| `cargo-fmt` | pre-commit | `*.rs`, `Cargo.*` | Rust formatting | ~1s |
| `nix-fmt` | pre-commit | `*.nix` | Nix formatting | ~1s |
| `nix-check` | pre-commit | `*.nix`, `flake.lock` | Nix syntax | ~2s |
| `cargo-clippy` | pre-push | `*.rs`, `Cargo.*` | Rust lints | ~30s |
| `cargo-test` | pre-push | `*.rs`, `Cargo.*` | Rust tests | ~20s |
| `nix-build-check` | pre-push | `*.nix`, `infra/*` | Nix build eval | ~10s |

**Path-selective behavior:** If you only change `.nix` files, only Nix hooks run. If you only change `.rs` files, only Rust hooks run. This keeps feedback fast and relevant.

### Directory Structure

```
moto/
├── .pre-commit-config.yaml    # prek configuration
└── scripts/
    └── hooks/
        ├── block-secrets.sh
        ├── nix-check.sh
        └── nix-build-check.sh
```

### Makefile Targets

```makefile
.PHONY: install-hooks check-hooks

# Install prek hooks
install-hooks:
	@command -v prek >/dev/null || { echo "Install prek: cargo install prek"; exit 1; }
	prek install

# Run pre-commit checks manually
check-hooks:
	prek run --all-files
```

### Agent Workflow

When agents work in loops:

1. **Make changes** to code
2. **Run `prek run`** or `make check-hooks` before committing
3. **If checks fail** - fix immediately, don't accumulate errors
4. **Commit** - hooks run automatically
5. **Continue loop** with confidence changes are valid

**Key insight:** Agents should run `prek run` proactively, not just rely on git hooks. This catches errors before attempting to commit.

### Bypassing Hooks

For emergencies only:

```bash
# Skip pre-commit (use sparingly)
git commit --no-verify -m "WIP: emergency fix"

# Skip pre-push (use sparingly)
git push --no-verify
```

**Agents should NEVER use `--no-verify`** unless explicitly instructed.

### CI Integration

Hooks are a first line of defense. CI runs the full suite:

| CI Check | Equivalent Hook | Additional |
|----------|-----------------|------------|
| `cargo fmt --check` | `cargo-fmt` | - |
| `cargo clippy` | `cargo-clippy` | - |
| `cargo test` | `cargo-test` | Coverage |
| `nix flake check` | `nix-check` | Full check |
| `nix build .#moto-garage` | - | Full build |
| Container smoke tests | - | `make docker-test-moto-garage` |

### Dependencies

- `prek` - Hook manager (Rust binary)
- `cargo` - Rust toolchain
- `nix` - Nix package manager (optional, hooks skip gracefully)
- `nixfmt` - Nix formatter (optional)

---

## References

- [prek documentation](https://prek.j178.dev/) - Fast Rust-based pre-commit hook manager
- [prek GitHub](https://github.com/j178/prek) - Source and issues
- [git hooks documentation](https://git-scm.com/docs/githooks)

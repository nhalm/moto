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
- **Nix-aware** - Validate Nix changes don't break the container build
- **Layered** - Fast checks on commit, thorough checks on push

## Specification

### Hook Layers

| Hook | When | Time Budget | Purpose |
|------|------|-------------|---------|
| pre-commit | Before commit | <10s | Fast syntax/format checks |
| pre-push | Before push | <60s | Build validation |

### Pre-Commit Checks

Fast checks that run on every commit:

```bash
# 1. Rust formatting (fast, ~1s)
cargo fmt --all --check

# 2. Nix formatting (fast, ~1s)
nixfmt --check flake.nix infra/**/*.nix

# 3. Nix syntax validation (fast, ~2s)
nix flake check --no-build

# 4. Detect secrets (fast, ~1s)
# Fail if .env, credentials, or key files are staged
```

**What we DON'T run on pre-commit:**
- `cargo clippy` - Too slow (~30s+)
- `cargo test` - Too slow
- `nix build` - Way too slow (~minutes)
- Full container build - Way too slow

### Pre-Push Checks

More thorough checks before pushing to remote:

```bash
# 1. Clippy lints (thorough, ~30s)
cargo clippy --workspace --all-targets -- -D warnings

# 2. Nix build evaluation (no actual build, ~10s)
nix build .#moto-garage --dry-run

# 3. Unit tests for changed crates (targeted, ~20s)
cargo test --workspace
```

### Nix-Specific Validation

When files in `infra/` or `flake.nix` are modified:

| Check | Command | Purpose |
|-------|---------|---------|
| Syntax | `nix flake check --no-build` | Catch Nix syntax errors |
| Eval | `nix build .#moto-garage --dry-run` | Catch evaluation errors |
| Format | `nixfmt --check` | Consistent formatting |

**Full container build** (`nix build .#moto-garage`) is NOT run in hooks - it's too slow. Run manually or in CI.

### Secret Detection

Prevent accidental commits of secrets:

```bash
# Patterns to block
*.pem
*.key
.env
.env.*
credentials.json
*_secret*
*_password*
```

**Implementation:** Use `git-secrets` or simple grep patterns.

### Installation

Hooks are installed via:

```bash
# Option 1: Make target
make install-hooks

# Option 2: Manual
cp .githooks/* .git/hooks/
chmod +x .git/hooks/*
```

### Hook Scripts

**`.githooks/pre-commit`:**

```bash
#!/usr/bin/env bash
set -euo pipefail

echo "=== Pre-commit checks ==="

# Fast checks only - must complete in <10s

echo "Checking Rust formatting..."
cargo fmt --all --check || {
    echo "Run 'cargo fmt' to fix"
    exit 1
}

echo "Checking for secrets..."
if git diff --cached --name-only | grep -E '\.(pem|key)$|\.env|credentials\.json'; then
    echo "ERROR: Potential secrets detected in staged files"
    exit 1
fi

# Only check Nix if Nix files changed
if git diff --cached --name-only | grep -E '\.nix$|flake\.lock$'; then
    echo "Checking Nix syntax..."
    nix flake check --no-build 2>/dev/null || {
        echo "Nix syntax error - run 'nix flake check' for details"
        exit 1
    }
fi

echo "=== Pre-commit passed ==="
```

**`.githooks/pre-push`:**

```bash
#!/usr/bin/env bash
set -euo pipefail

echo "=== Pre-push checks ==="

echo "Running clippy..."
cargo clippy --workspace --all-targets -- -D warnings

echo "Running tests..."
cargo test --workspace

# Only validate Nix build if Nix files changed (vs remote)
REMOTE_REF=$(git rev-parse @{upstream} 2>/dev/null || echo "HEAD~10")
if git diff --name-only "$REMOTE_REF" HEAD | grep -E '\.nix$|flake\.lock$'; then
    echo "Validating Nix build (dry-run)..."
    nix build .#moto-garage --dry-run
fi

echo "=== Pre-push passed ==="
```

### Makefile Targets

```makefile
.PHONY: install-hooks check-hooks

# Install git hooks
install-hooks:
	@mkdir -p .git/hooks
	@cp .githooks/* .git/hooks/
	@chmod +x .git/hooks/*
	@echo "Git hooks installed"

# Run pre-commit checks manually
check-hooks:
	@./.githooks/pre-commit

# Run all checks (pre-commit + pre-push)
check-all:
	@./.githooks/pre-commit
	@./.githooks/pre-push
```

### Agent Workflow

When agents work in loops:

1. **Make changes** to code
2. **Run `make check-hooks`** before committing (or hooks run automatically)
3. **If checks fail** - fix immediately, don't accumulate errors
4. **Commit** - pre-commit hook validates
5. **Continue loop** with confidence changes are valid

**Key insight:** Agents should run `make check-hooks` proactively, not just rely on git hooks. This catches errors before attempting to commit.

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
| `cargo fmt --check` | pre-commit | - |
| `cargo clippy` | pre-push | - |
| `cargo test` | pre-push | Coverage |
| `nix flake check` | pre-commit (partial) | Full check |
| `nix build .#moto-garage` | - | Full build |
| Container smoke tests | - | `make docker-test-moto-garage` |

### Dependencies

- `cargo` - Rust toolchain
- `nix` - Nix package manager (optional but recommended)
- `nixfmt` - Nix formatter (optional)

Hooks gracefully skip Nix checks if `nix` is not available.

---

## Changelog

### v0.1 (2026-01-24)
- Initial specification

## Notes

- Consider `lefthook` or `husky` for more sophisticated hook management
- Could add `typos` for spell checking in comments/docs
- Consider `cargo-deny` for license/advisory checking in pre-push

## References

- [git hooks documentation](https://git-scm.com/docs/githooks)
- [pre-commit framework](https://pre-commit.com/) (Python-based, not recommended for Rust projects)

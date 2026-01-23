# jj Workflow

| | |
|--------|----------------------------------------------|
| Version | 1.0 |
| Last Updated | 2026-01-22 |

## Overview

Defines how agents use Jujutsu (jj) and GitHub CLI (gh) to manage code flow from garages to main. Agents have full autonomy over commits, pushes, and PR creation using standard tools.

## Why jj

- **Working copy is always a commit** - Agent changes are auto-tracked without explicit commits
- **Easy to squash/reorganize** - Clean up commit history before pushing
- **Automatic rebasing** - Handles diverged branches gracefully
- **Git-compatible** - Repo stays git, jj operates on it (colocated mode)

## Code Flow

```
┌─────────────────────────────────────────────────────────────────────────┐
│                              GARAGE                                      │
│                                                                          │
│  1. Agent works, commits with jj                                        │
│  2. Agent rebases onto main when ready                                  │
│  3. Agent pushes to garage/{hostname} branch                            │
│  4. Agent creates PR with gh                                            │
│                                                                          │
└──────────────────────────────┬──────────────────────────────────────────┘
                               │
                               │ git push + gh pr create
                               ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         garage/{hostname}                               │
│                                                                          │
│  - All commits preserved (no squash until merge)                        │
│  - PR targeting main                                                    │
│  - Human reviews on GitHub                                              │
│                                                                          │
└──────────────────────────────┬──────────────────────────────────────────┘
                               │
                               │ PR merged (squash merge enforced)
                               │ Branch auto-deleted by GitHub
                               ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                              main                                        │
│                                                                          │
│  - Single squash commit                                                 │
│  - Clean history                                                         │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

## Garage Identity

The garage ID is the **container hostname**. Agents discover it with:

```bash
GARAGE_ID=$(hostname)
```

This is used for the branch name: `garage/{hostname}`

## Branch Naming

| Branch | Purpose |
|--------|---------|
| `main` | Primary branch, all PRs target here |
| `garage/{hostname}` | Sync branch for a garage (e.g., `garage/abc123`) |

Branches are auto-deleted by GitHub after PR merge (requires "Automatically delete head branches" repo setting).

## Squash Merge Enforcement

Squash merges are enforced via **GitHub branch protection rules** on main:

- Require squash merging (disable merge commits and rebase merging)
- PR description becomes the squash commit message

This ensures clean history without relying on humans to click the right button.

## Agent Workflow

### 1. Commit Work

Agents should commit after each logical unit of work:

```bash
jj commit -m "Implement feature X per spec-name.md"
```

Include spec references in commit messages for traceability.

### 2. Rebase onto Main

Before pushing, rebase onto latest main:

```bash
jj git fetch
jj rebase -d main@origin
```

If conflicts occur, resolve them:

```bash
jj resolve
```

### 3. Push to Branch

Push to the garage branch:

```bash
GARAGE_ID=$(hostname)
jj git push --change @ --to "garage/${GARAGE_ID}"
```

Or to push all commits:

```bash
jj git push --branch "garage/${GARAGE_ID}"
```

### 4. Create PR

Create the PR using GitHub CLI:

```bash
GARAGE_ID=$(hostname)

gh pr create \
  --base main \
  --head "garage/${GARAGE_ID}" \
  --title "Brief description of changes" \
  --body "## Summary

Description of what was implemented.

## Changes

- Key change 1
- Key change 2

## Spec

Implements spec-name.md
"
```

### 5. Update Existing PR

If PR already exists, just push new commits:

```bash
jj git push --branch "garage/${GARAGE_ID}"
```

The PR updates automatically.

## jj Operations Reference

| Operation | Command |
|-----------|---------|
| See changes | `jj status` |
| See commit log | `jj log` |
| Create commit | `jj commit -m "message"` |
| Squash commits | `jj squash` |
| Split a commit | `jj split` |
| Rebase onto main | `jj rebase -d main@origin` |
| Undo last operation | `jj undo` |
| Resolve conflicts | `jj resolve` |
| Fetch from remote | `jj git fetch` |
| Push branch | `jj git push --branch <name>` |

## Conflict Handling

When rebase has conflicts:

1. jj will report conflicted files
2. Agent resolves conflicts in the files
3. Run `jj resolve` to mark resolved
4. Continue with push

## Git Authentication

GitHub token is available from keybox and configured in git credentials. Agents don't need to handle authentication manually.

## PR Description Guidelines

The PR description becomes the squash commit message. It should:

1. **Summarize** the overall change (first line becomes commit title)
2. **List key changes** for reviewers
3. **Reference the spec** being implemented

## Human Review

After agent creates PR:

1. Human reviews on GitHub
2. Human can enter garage (`moto garage enter`) to inspect/modify
3. Human merges PR (squash merge enforced)
4. Branch auto-deleted by GitHub

## Dependencies

- [garage-lifecycle](garage-lifecycle.md) - Garage creation and management
- [keybox](keybox.md) - GitHub token storage

## Notes

- Agents use standard tools (jj, gh) - no moto commands for sync
- Commit history preserved in PRs for review
- Squash happens at merge time (enforced by GitHub)
- One feature per garage is the recommended pattern

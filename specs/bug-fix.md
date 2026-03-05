# Bug Fix Punch List

Living punch list for cross-cutting code bugs, wiring omissions, and small
fixes that don't warrant a spec version bump. Items are grouped by owning spec.

Loop agents: when implementing a spec, check this file for items under that
spec's heading. Fix one per iteration. Delete the item from this file after
fixing and committing.

Items marked `(blocked: ...)` can't be fixed until their dependency resolves —
same convention as tracks.md.

---

## project-structure.md

(none)

## moto-cli.md

(none)

## jj-workflow.md

(none)

## pre-commit.md

(none)

## makefile.md

(none)

## testing.md

(none)

## moto-club.md

(none)

## keybox.md

(none)

## dev-container.md

(none)

## container-system.md

(none)

## local-cluster.md

(none)

## garage-isolation.md

(none)

## garage-lifecycle.md

(none)

## moto-bike.md

(none)

## supporting-services.md

(none)

## moto-wgtunnel.md

(none)

## local-dev.md

(none)

## service-deploy.md

- `scripts/generate-manifests.sh` uses single-quoted heredocs (`<< 'YAML'`), so all `parse_toml` calls are dead code — no shell variable substitution occurs. The generated manifests ignore bike.toml values: replicas hardcoded to 1 (bike.toml says 3), resources hardcoded to 50m/128Mi/500m/512Mi (bike.toml says 250m/256Mi/1/1Gi). Fix: use unquoted heredocs (`<< YAML`) and interpolate parsed values, or rewrite to use `sed`/`envsubst` with a template.

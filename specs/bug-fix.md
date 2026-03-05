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

- **Fallback `create_garage` has no collision-retry for auto-generated names.** `garages.rs:291-356` generates a name and goes straight to DB insert — a name collision returns `GARAGE_ALREADY_EXISTS` (409). Spec requires transparent retry up to 3 times with random suffix, then `INTERNAL_ERROR`.

## keybox.md

(none)

## dev-container.md

(none)

## container-system.md

- **`make registry-start` uses port 5000 instead of 5050.** `Makefile:191` runs `docker run -d -p 5000:5000` but the `REGISTRY` variable defaults to `localhost:5050` (line 117) and spec v1.1 changed the port to 5050. Fix: change `-p 5000:5000` to `-p 5050:5000` and update the echo message.

## local-cluster.md

(none)

## garage-isolation.md

(none)

## garage-lifecycle.md

(none)

## moto-bike.md

- **K8s manifest missing POD_NAME and POD_NAMESPACE injection.** `infra/k8s/moto-system/club.yaml` does not inject `POD_NAME` and `POD_NAMESPACE` via K8s downward API. Spec (line 99, 106-107) requires these for structured logging. Fix: add `valueFrom.fieldRef` entries for `metadata.name` and `metadata.namespace`.
- **K8s manifest missing RUST_LOG env var.** `infra/k8s/moto-system/club.yaml` env section does not set `RUST_LOG`. Spec (line 110) requires `RUST_LOG="info"` for log level control. Fix: add `RUST_LOG: "info"` to the env section.
- **K8s manifest missing rolling update strategy.** `infra/k8s/moto-system/club.yaml` has no `strategy` section. Spec (lines 411-416) requires `RollingUpdate` with `maxSurge: 1` and `maxUnavailable: 0`. Fix: add strategy block to deployment spec.
- **K8s manifest incomplete security context.** `infra/k8s/moto-system/club.yaml` has pod-level `runAsUser`/`runAsGroup`/`runAsNonRoot` but is missing container-level `readOnlyRootFilesystem: true`, `allowPrivilegeEscalation: false`, and `capabilities.drop: [ALL]`. Spec (lines 69-78) requires full hardening. Fix: add container-level securityContext.

## supporting-services.md

(none)

## moto-wgtunnel.md

(none)

## local-dev.md

(none)

## service-deploy.md

(none)

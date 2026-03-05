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

- **Keybox needs a `bike.toml`.** `crates/moto-keybox-server/` has no `bike.toml`. Create one with `name = "keybox"`, `replicas = 3`, `port = 8080`, `health.port = 8081`, `health.path = "/health/ready"`, and resource defaults (cpu_request="250m", cpu_limit="1", memory_request="256Mi", memory_limit="1Gi").
- **Keybox static manifest (`keybox.yaml`) missing security baseline.** `infra/k8s/moto-system/keybox.yaml` is missing: (1) POD_NAME/POD_NAMESPACE via downward API, (2) RUST_LOG="info", (3) RollingUpdate strategy with maxSurge:1/maxUnavailable:0, (4) container securityContext (readOnlyRootFilesystem, allowPrivilegeEscalation:false, capabilities drop ALL), (5) pod securityContext (runAsUser:1000, runAsGroup:1000, runAsNonRoot:true). Apply same local-dev baseline as club.yaml, or migrate keybox to use the deployment builder.

## supporting-services.md

(none)

## moto-wgtunnel.md

(none)

## local-dev.md

(none)

## service-deploy.md

(none)

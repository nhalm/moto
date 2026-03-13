# Deployment

This guide covers deploying Moto's infrastructure services to a local k3d Kubernetes cluster. For local development with `cargo run` instead of K8s, see the [getting-started](getting-started.md) guide.

## Quick Start

Deploy the full stack with one command:

```bash
# Create the k3d cluster (if it doesn't exist)
moto cluster init

# Build images, generate secrets, deploy infrastructure, verify
make deploy

# The CLI is now connected (localhost:18080 -> moto-club)
moto garage open
```

The `make deploy` target does everything: builds container images, generates credentials, applies manifests, and waits for all pods to become ready.

## What Gets Deployed

The deployment creates a `moto-system` namespace with all infrastructure services:

```
k3d cluster "moto"
└── moto-system namespace
    ├── postgres (StatefulSet)
    │   ├── moto_club database
    │   └── moto_keybox database
    ├── moto-keybox (Deployment, 3 replicas)
    ├── moto-club (Deployment, 3 replicas)
    └── moto-ai-proxy (Deployment, 3 replicas)
```

**Garages are dynamic:** When you run `moto garage open`, moto-club creates a new namespace (e.g., `moto-garage-abc123`) with the garage pod, supporting services, and NetworkPolicies. These are ephemeral and cleaned up when the garage closes.

## What Runs Where

| Component | Namespace | What it does | Replicas |
|-----------|-----------|--------------|----------|
| **postgres** | `moto-system` | Persistent database for club and keybox | 1 (StatefulSet) |
| **moto-keybox** | `moto-system` | Secrets manager, SPIFFE identity issuer | 3 |
| **moto-club** | `moto-system` | Orchestrator, garage lifecycle manager | 3 |
| **moto-ai-proxy** | `moto-system` | AI credential-injecting proxy | 3 |
| **garage pods** | `moto-garage-*` | AI dev environment (created on-demand) | 1 per garage |

All services in `moto-system` are ClusterIP — they're accessed by the CLI via `kubectl port-forward` (automatic on deploy).

## Secrets Management

All secrets are generated locally and stored in `.dev/k8s-secrets/` (gitignored). The `make deploy-secrets` target is **idempotent** — it only generates secrets if they don't already exist.

### Secret Generation

```bash
# Generate secrets (runs automatically as part of `make deploy`)
make deploy-secrets
```

This creates:

| File | Purpose | How it's generated |
|------|---------|-------------------|
| `db-password` | PostgreSQL password | `openssl rand -hex 32` |
| `service-token` | Service-to-service auth | `openssl rand -hex 32` |
| `master.key` | Keybox envelope encryption KEK | `moto-keybox init` (Ed25519) |
| `signing.key` | SPIFFE SVID signing key | `moto-keybox init` (Ed25519) |

### Kubernetes Secrets

These files are turned into K8s secrets in the `moto-system` namespace:

| Secret Name | Contents | Used By |
|-------------|----------|---------|
| `postgres-credentials` | `password` | PostgreSQL container |
| `keybox-keys` | `master.key`, `signing.key`, `service-token` | moto-keybox (mounted at `/run/secrets/keybox/`) |
| `keybox-db-credentials` | `url` (full postgres:// connection string) | moto-keybox env |
| `club-db-credentials` | `url` (full postgres:// connection string) | moto-club env |
| `keybox-service-token` | `service-token` | moto-club (mounted at `/run/secrets/club/`) |

**Why separate secrets?** Each service mounts only the secrets it needs (least-privilege). moto-club gets the service token but not the master key or signing key.

**Never commit secrets.** The `.dev/` directory is gitignored. To regenerate: `rm -rf .dev/k8s-secrets && make deploy-secrets`.

## Accessing the Cluster

### CLI Access

The `make deploy-system` target automatically starts a background `kubectl port-forward`:

```bash
localhost:18080 -> svc/moto-club:8080 (moto-system)
```

The CLI defaults to `http://localhost:18080`, so after deployment you can immediately run:

```bash
moto garage open
moto garage list
```

### Direct Service Access (for debugging)

Port-forward to any service:

```bash
# moto-keybox (API on 8080, health on 8081, metrics on 9090)
kubectl -n moto-system port-forward svc/moto-keybox 8090:8080

# moto-ai-proxy
kubectl -n moto-system port-forward svc/moto-ai-proxy 8091:8080

# postgres
kubectl -n moto-system port-forward svc/postgres 5432:5432
psql postgres://moto:<password>@localhost:5432/moto_club
```

The database password is in `.dev/k8s-secrets/db-password`.

### Inspecting Pods

```bash
# List all pods in moto-system
kubectl -n moto-system get pods

# Logs from a specific service
kubectl -n moto-system logs -l app=moto-club --tail=100 -f

# Shell into keybox
kubectl -n moto-system exec -it deployment/moto-keybox -- /bin/sh

# Check garage namespaces
kubectl get namespaces | grep moto-garage
```

## Step-by-Step Deployment

If you need to debug or run partial deploys, use the individual targets:

```bash
# 1. Ensure k3d cluster exists
moto cluster init

# 2. Build and push container images to the k3d registry
make deploy-images
# This builds: moto-garage, moto-club, moto-keybox
# Pushes to: localhost:5050 (accessible in-cluster as moto-registry:5000)

# 3. Generate and apply secrets
make deploy-secrets

# 4. Apply all manifests
make deploy-system
# Runs: kubectl apply -k infra/k8s/moto-system/

# 5. Wait for rollout and verify
make deploy-status
# Waits for all Deployments to be ready, exits non-zero if any pod fails
```

## Container Images

Images are built with Nix via `dockerTools` and pushed to the k3d cluster's built-in registry:

| Build Target | Host Push Address | In-Cluster Reference |
|--------------|-------------------|----------------------|
| `make build-garage` | `localhost:5050/moto-garage:latest` | `moto-registry:5000/moto-garage:latest` |
| `make build-club` | `localhost:5050/moto-club:latest` | `moto-registry:5000/moto-club:latest` |
| `make build-keybox` | `localhost:5050/moto-keybox:latest` | `moto-registry:5000/moto-keybox:latest` |

**Why two addresses?** The k3d registry is exposed on `localhost:5050` for the host to push images. Inside the cluster, it's available at `moto-registry:5000` for pods to pull.

After building, images are pushed with `make push-garage`, `make push-club`, `make push-keybox`. The `make deploy-images` target does all six steps in sequence.

## Database Migrations

Both moto-club and moto-keybox **run migrations automatically on startup**. You don't need to run migrations manually or use init containers.

Migrations are embedded in the binary and run via `sqlx::migrate!()` on each pod's startup. If migrations fail, the pod enters CrashLoopBackOff and logs the error.

## Production Considerations

This deployment is designed for **local development and testing**. For production use, you would change:

### Secrets Management

- **Current:** Local `.dev/k8s-secrets/` files → K8s Secrets
- **Production:** Use an external secrets manager (AWS Secrets Manager, GCP Secret Manager, HashiCorp Vault) with External Secrets Operator or a CSI driver
- Store `master.key` and `signing.key` in an HSM or KMS
- Rotate service tokens on a schedule
- Never store secrets in manifests or environment variables

### Database

- **Current:** Single StatefulSet postgres pod with ephemeral PVC
- **Production:**
  - Managed PostgreSQL (RDS, Cloud SQL, Azure Database)
  - Multi-AZ replication
  - Automated backups and point-in-time recovery
  - Connection pooling (PgBouncer)
  - Separate databases per service on separate RDS instances (not just separate DBs on one Postgres)

### TLS and Network Security

- **Current:** Plaintext HTTP inside the cluster
- **Production:**
  - TLS everywhere: mTLS between services, TLS termination at ingress
  - Use cert-manager for automated certificate issuance (Let's Encrypt or internal CA)
  - NetworkPolicies for moto-system namespace (not just garages)
  - Egress filtering: allowlist external endpoints

### High Availability

- **Current:** 3 replicas, but local k3d is single-node
- **Production:**
  - Multi-node cluster across availability zones
  - Pod anti-affinity rules (spread replicas across nodes)
  - PodDisruptionBudgets for zero-downtime updates (already configured)
  - Health checks with appropriate thresholds
  - Leader election for moto-club (already implemented via K8s leases)

### Access Control

- **Current:** ClusterRole grants moto-club full access to create/delete namespaces cluster-wide
- **Production:**
  - Scope RBAC more tightly (namespace-scoped roles where possible)
  - Use OPA or Kyverno for admission control policies
  - Enable Pod Security Standards (restricted profile for garages)
  - Audit logging for all K8s API calls

### Observability

- **Current:** Basic health endpoints, metrics exposed on port 9090
- **Production:**
  - Prometheus + Grafana stack for metrics and dashboards
  - Centralized logging (Loki, ELK, CloudWatch Logs)
  - Distributed tracing (OpenTelemetry, Jaeger)
  - Alerting (PagerDuty, Opsgenie)
  - SLOs and error budgets

### Image Security

- **Current:** Images built locally, no signing or scanning in CI
- **Production:**
  - Sign images with Cosign and verify signatures at admission time (already supported: `make sign-images`)
  - Scan images for vulnerabilities in CI (Trivy, Grype)
  - Use a private registry with access controls
  - Minimal base images, no unnecessary binaries
  - Regular patching and dependency updates

### Compliance and Audit

- **Current:** No audit trail, secrets in K8s Secrets
- **Production:**
  - Full audit logging per SOC 2 requirements (see `specs/compliance.md`)
  - Immutable audit log storage (S3, Cloud Storage)
  - Secrets never logged or exposed in error messages
  - Data encryption at rest for all PVCs
  - Compliance scans and reporting

## Troubleshooting

| Problem | Diagnosis | Solution |
|---------|-----------|----------|
| **ImagePullBackOff** | Image not in registry | Run `make deploy-images` to build and push |
| Verify registry: `curl localhost:5050/v2/_catalog` | Should list `moto-club`, `moto-keybox`, `moto-garage` |
| **Postgres pod Pending** | PVC not bound | Check PVC: `kubectl -n moto-system get pvc` |
| | k3d uses `local-path` StorageClass (should auto-provision) |
| **CrashLoopBackOff (keybox or club)** | Missing secrets or DB not ready | Check logs: `kubectl -n moto-system logs deployment/moto-keybox` |
| | Verify secrets exist: `kubectl -n moto-system get secrets` |
| **RBAC permission denied (moto-club)** | ClusterRole not applied | Verify binding: `kubectl get clusterrolebinding moto-club` |
| | Reapply: `make deploy-system` |
| **Migrations fail** | Schema version mismatch or DB corruption | Check pod logs at startup |
| | Migrations are embedded; if they fail repeatedly, inspect DB schema manually |
| **Port-forward fails** | Cluster not running or service not ready | Check cluster: `k3d cluster list` |
| | Start cluster: `k3d cluster start moto` |
| | Check service: `kubectl -n moto-system get svc` |
| **CLI can't connect** | Port-forward not running | `make deploy-system` (restarts port-forward) |
| | Manual: `kubectl -n moto-system port-forward svc/moto-club 18080:8080 &` |

### Viewing Logs

```bash
# All logs from a deployment
kubectl -n moto-system logs deployment/moto-club --tail=100

# Stream logs (follow)
kubectl -n moto-system logs -l app=moto-keybox -f

# Previous container logs (if pod restarted)
kubectl -n moto-system logs deployment/moto-club --previous
```

### Complete Teardown

To delete everything and start fresh:

```bash
# Delete the k3d cluster (including port-forward, pods, PVCs, secrets)
make dev-cluster-down

# Also remove local state
make dev-clean

# Recreate from scratch
moto cluster init
make deploy
```

## Next Steps

- **[Getting Started](getting-started.md)** — First garage walkthrough
- **[Architecture](architecture.md)** — How components connect
- **[Security](security.md)** — Threat model and isolation layers

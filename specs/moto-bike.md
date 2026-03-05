# Bike

| | |
|--------|----------------------------------------------|
| Version | 0.4 |
| Status | Ripping |
| Last Updated | 2026-03-05 |

## Overview

A **bike** is a minimal base container image. An **engine** is a compiled binary (club, vault, proxy). A **running bike** is the bike base image with one engine binary inside - deployed to K8s.

**Bikes enable 12-factor apps.** The bike provides the minimal secure runtime. The engine implements 12-factor behaviors (stateless, env config, structured logging, health endpoints, graceful shutdown).

**Key metaphor:**
- Garage = workshop (development environment)
- Bike = motorcycle chassis (minimal base image)
- Engine = what makes it go (the service binary)

```
┌─────────────────────────────────────┐
│  Running Bike (e.g., moto-club)     │
│  ┌───────────────────────────────┐  │
│  │  Engine Binary (/bin/club)    │  │
│  │  - Service logic              │  │
│  │  - Health endpoints           │  │
│  │  - Metrics                    │  │
│  │  - Structured logging         │  │
│  └───────────────────────────────┘  │
│  Bike Base (moto-bike)              │
│  - CA certs, tzdata, non-root user  │
└─────────────────────────────────────┘
```

---

## Specification

### 1. Bike Base Image

The bike is a minimal container image. As small as possible.

**Contents:**

| Include | Why |
|---------|-----|
| CA certificates | TLS connections to external services |
| Timezone data | Correct timestamps in logs |
| Non-root user (1000:1000) | Security |

**Excludes:**

| Exclude | Why |
|---------|-----|
| Shell | Not needed, attack surface |
| Package manager | Not needed, attack surface |
| Libc (if static binary) | Engines are statically compiled |
| Any tooling | This is prod, not dev |

**Size target:** <20MB (ideally <10MB)

**Base image name:** `moto-bike`

**Build:** Nix builds the base image. See [container-system.md](container-system.md).

**Security posture:**

```yaml
securityContext:
  runAsUser: 1000
  runAsGroup: 1000
  runAsNonRoot: true
  readOnlyRootFilesystem: true
  allowPrivilegeEscalation: false
  capabilities:
    drop:
      - ALL
```

---

### 2. Engine Contract

Engines are statically compiled binaries that run inside the bike. All engines must implement these 12-factor behaviors:

#### Stateless Processes

- No local filesystem state
- All state in backing services (Postgres, Redis)
- Any instance can handle any request

#### Config via Environment

Engines read all config from environment variables. Never from files (except CA certs).

**How env vars are provided:**
- K8s ConfigMaps for non-sensitive config
- K8s Secrets for sensitive values (or fetched from keybox at runtime)
- Some values injected by K8s (POD_NAME, POD_NAMESPACE via downward API)

**Common env vars (all engines):**

Engines use a `MOTO_{ENGINE}_` prefix for engine-specific config (e.g., `MOTO_CLUB_BIND_ADDR`). Some values are standard across all engines:

```bash
# Identity (injected by K8s downward API)
POD_NAME="moto-club-abc123"          # K8s pod name
POD_NAMESPACE="moto-prod"            # K8s namespace

# Runtime
RUST_LOG="info"                       # Log level
RUST_BACKTRACE="1"                    # Backtraces on panic

# Engine-specific (example for club)
MOTO_CLUB_BIND_ADDR="0.0.0.0:8080"
MOTO_CLUB_HEALTH_BIND_ADDR="0.0.0.0:8081"
MOTO_CLUB_METRICS_BIND_ADDR="0.0.0.0:9090"
MOTO_CLUB_KEYBOX_URL="http://keybox.moto-system:8080"
```

**Engine-specific config:**

| Engine | Key Config |
|--------|------------|
| `club` | `DATABASE_URL`, `REDIS_URL` |
| `vault` | `HSM_ENDPOINT`, `ENCRYPTION_KEY_ID` |
| `proxy` | `UPSTREAM_SERVICES`, `ROUTE_CONFIG` |

#### Health Endpoints

All engines expose health on port 8081:

| Endpoint | Returns 200 when |
|----------|------------------|
| `/health/live` | Process is alive (not deadlocked) |
| `/health/ready` | Ready for traffic (deps connected) |
| `/health/startup` | Initial startup complete |

#### Structured Logging

Engines emit JSON logs to stdout (logs as event streams):

```json
{
  "timestamp": "2026-01-28T10:30:00Z",
  "level": "info",
  "message": "Request handled",
  "engine": "club",
  "pod": "moto-club-abc123",
  "trace_id": "abc123",
  "duration_ms": 42,
  "status": 200
}
```

**Log levels:**

| Level | When |
|-------|------|
| `error` | Unrecoverable failures |
| `warn` | Recoverable issues |
| `info` | Requests, lifecycle events |
| `debug` | Internal state (off in prod) |

#### Metrics

Engines expose Prometheus metrics on port 9090:

```
http_requests_total{method="GET",path="/api/...",status="200"}
http_request_duration_seconds{...}
process_cpu_seconds_total
process_resident_memory_bytes
```

#### Graceful Shutdown

- Handle SIGTERM
- Stop accepting new requests
- Complete in-flight requests
- 30-second grace period
- Exit cleanly

#### Port Convention

| Port | Purpose |
|------|---------|
| 8080 | Main API (HTTP/gRPC) |
| 8081 | Health endpoints |
| 9090 | Prometheus metrics |

---

### 3. Building Running Bikes

A running bike = bike base + engine binary.

#### Dockerfile Pattern

```dockerfile
FROM moto-bike:latest
COPY --chmod=755 club /bin/club
USER 1000:1000
ENTRYPOINT ["/bin/club"]
```

No shell. No init system. Single binary.

#### bike.toml

Each main engine crate has a `bike.toml` in its crate root. Shared/tool crates don't need one - only the final deployable engines (club, vault, proxy).

**Location:** `crates/moto-club/bike.toml`

```toml
name = "club"

[deploy]
replicas = 3
port = 8080

[health]
port = 8081
path = "/health/ready"

[resources]
cpu_request = "500m"
cpu_limit = "2"
memory_request = "512Mi"
memory_limit = "2Gi"
```

**Required fields:** `name`

**Note:** No `[build]` section - Nix flakes handle the build. See Build Pipeline below.

#### bike.toml → K8s Mapping

| bike.toml | K8s Resource |
|-----------|--------------|
| `name` | Deployment name, Service name, image name (`moto-{name}`) |
| `deploy.replicas` | `spec.replicas` |
| `deploy.port` | `containerPort`, Service `targetPort` |
| `health.*` | `readinessProbe` config |
| `resources.*` | `resources.requests`, `resources.limits` |

The Service is auto-generated with the same name as the Deployment, providing DNS-based service discovery.

**Local-dev overrides:** Local K8s manifests (see [service-deploy.md](service-deploy.md)) may use lower replicas and resource values for development. bike.toml defines the production target; local-dev intentionally runs leaner.

#### Build Pipeline

Nix flakes handle the entire build. Each engine has a flake output that:
1. Builds the Rust binary
2. Combines it with the `moto-bike` base image
3. Produces the final image (e.g., `moto-club`)

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────┐
│  nix build      │───▶│   moto-club     │───▶│   Registry  │
│  .#moto-club    │    │   (final image) │    │ moto-club:  │
│                 │    │                 │    │   abc123f   │
│  - builds binary│    │ = moto-bike     │    └─────────────┘
│  - layers on    │    │ + club binary   │
│    moto-bike    │    │                 │
└─────────────────┘    └─────────────────┘
```

**Build commands:**

```bash
# Build image locally (can run in garage dev-container)
nix build .#moto-club-image

# Load into Docker
docker load < result

# Push to registry
docker push ${MOTO_REGISTRY}/moto-club:$(git rev-parse --short HEAD)
```

**CLI convenience (wraps the above):**

```bash
moto bike build           # Build image from bike.toml in cwd
moto bike build --push    # Build and push to registry
```

See [container-system.md](container-system.md) for Nix flake details.

---

### 4. Deployment

Running bikes are K8s Deployments.

#### Namespaces

Deploy to a namespace via flag (defaults to current kubectl context namespace):

```bash
moto bike deploy                        # Uses current context namespace
moto bike deploy --namespace moto-prod  # Explicit namespace
```

#### K8s Resources

Each engine deployment creates:

- **Deployment** - Manages pod replicas
- **Service** - Internal DNS and load balancing
- **HorizontalPodAutoscaler** - Auto-scaling (optional)
- **NetworkPolicy** - Restrict traffic

#### Service Discovery

K8s Service provides DNS hostname for each bike. Other services reach it via:

```
<name>.<namespace>.svc.cluster.local

# Examples:
moto-club.moto-prod.svc.cluster.local
moto-vault.moto-prod.svc.cluster.local
```

Within the same namespace, just use the short name: `moto-club`

**Auto-generated Service spec:**

```yaml
apiVersion: v1
kind: Service
metadata:
  name: moto-club
  namespace: moto-prod
spec:
  selector:
    app: moto-club
  ports:
  - name: api
    port: 8080
    targetPort: 8080
  - name: health
    port: 8081
    targetPort: 8081
  - name: metrics
    port: 9090
    targetPort: 9090
```

#### Deployment Spec

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: moto-club
  namespace: moto-prod
spec:
  replicas: 3
  selector:
    matchLabels:
      app: moto-club
  template:
    metadata:
      labels:
        app: moto-club
    spec:
      containers:
      - name: club
        image: registry/moto-club:abc123f
        ports:
        - containerPort: 8080
        - containerPort: 8081
        - containerPort: 9090
        resources:
          requests:
            cpu: "500m"
            memory: "512Mi"
          limits:
            cpu: "2"
            memory: "2Gi"
        livenessProbe:
          httpGet:
            path: /health/live
            port: 8081
          periodSeconds: 10
        readinessProbe:
          httpGet:
            path: /health/ready
            port: 8081
          periodSeconds: 5
        startupProbe:
          httpGet:
            path: /health/startup
            port: 8081
          failureThreshold: 30
          periodSeconds: 1
      securityContext:
        runAsUser: 1000
        runAsGroup: 1000
        runAsNonRoot: true
```

#### Rolling Updates

```yaml
strategy:
  type: RollingUpdate
  rollingUpdate:
    maxSurge: 1
    maxUnavailable: 0
```

Zero-downtime deployments. Rollback via `kubectl rollout undo`.

#### Autoscaling

```yaml
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
spec:
  scaleTargetRef:
    kind: Deployment
    name: moto-club
  minReplicas: 3
  maxReplicas: 10
  metrics:
  - type: Resource
    resource:
      name: cpu
      target:
        type: Utilization
        averageUtilization: 70
```

#### Network Policies

Bikes only talk to what they need:

```yaml
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: moto-club-netpol
spec:
  podSelector:
    matchLabels:
      app: moto-club
  policyTypes:
  - Ingress
  - Egress
  ingress:
  - from:
    - namespaceSelector:
        matchLabels:
          name: moto-system
    ports:
    - port: 8080
  egress:
  - to:
    - namespaceSelector:
        matchLabels:
          name: moto-system
    ports:
    - port: 5432  # postgres
    - port: 6379  # redis
```

#### Resource Defaults

| Engine | CPU Req | CPU Lim | Mem Req | Mem Lim |
|--------|---------|---------|---------|---------|
| `club` | 500m | 2 | 512Mi | 2Gi |
| `vault` | 1 | 4 | 1Gi | 4Gi |
| `proxy` | 500m | 2 | 256Mi | 1Gi |

---

### 5. CLI Commands

See [moto-cli.md](moto-cli.md) for full details.

```bash
# Build image from bike.toml
moto bike build
moto bike build --push

# Deploy to cluster
moto bike deploy
moto bike deploy --replicas 3 --wait

# List running bikes
moto bike list

# View logs
moto bike logs club
moto bike logs club -f --tail 100
```

---

## Bike vs Garage

| Aspect | Garage | Bike |
|--------|--------|------|
| Purpose | Development | Production |
| Base image | `moto-garage` (~3GB) | `moto-bike` (<20MB) |
| Contains | Full toolchain + AI | Single engine binary |
| User | root | non-root (1000:1000) |
| Lifetime | Hours to days | Weeks to months |
| Replicas | 1 per developer | Multiple (scaled) |
| Network | Restricted egress | Service mesh |

---

## Notes

- Engines are statically compiled (no libc dependency)
- Consider PodDisruptionBudget for production stability
- Service mesh (Linkerd) for mTLS between bikes (future)
- OpenTelemetry tracing via `OTEL_EXPORTER_OTLP_ENDPOINT`

## References

- [container-system.md](container-system.md) - How bike/engine images are built
- [moto-cli.md](moto-cli.md) - CLI commands for bike build/deploy
- [keybox.md](keybox.md) - How engines get secrets

---

## Changelog

### v0.5 (2026-03-05)
- Fix: bike.toml `deploy.replicas` changed from 2 to 3 for production. HPA `minReplicas` updated to match.
- Fix: env var examples updated to show actual `MOTO_{ENGINE}_` prefix pattern instead of unprefixed names. `ENGINE_NAME` removed (not used). `BIND_ADDRESS`/`BIND_PORT` replaced with `MOTO_CLUB_BIND_ADDR` combined form.
- Docs: Added local-dev override note — local K8s manifests may use lower replicas and resources than bike.toml specifies.

### v0.4
- Docs: Fix bike.toml location from `engines/moto-club/` to `crates/moto-club/`

### v0.3
- Restructured spec: bike is base image, engine is binary, running bike is both
- Added bike.toml specification and K8s mapping
- Added Engine Contract section (12-factor requirements)
- Clarified build pipeline (Nix flakes)
- Added namespace handling (--namespace flag)
- Added service discovery (K8s Service auto-generated)
- Clarified env var injection (K8s ConfigMap/Secrets)
- Status: Ready to Rip

### v0.2
- Initial specification

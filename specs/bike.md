# Bike

| | |
|--------|----------------------------------------------|
| Status | Wrenching |
| Version | 0.2 |
| Last Updated | 2026-01-20 |

## Overview

A bike is a running instance - the motorcycle on the road. While a garage is where you wrench (develop), a bike is what's ripping (in production). Engines run inside bikes.

**Key metaphor:**
- Garage = workshop (development)
- Bike = motorcycle (production)
- Engine = the service running inside (club, vault, proxy, etc.)

## Jobs to Be Done

- [x] Define bike lifecycle (build, deploy, stop)
- [x] Define bike configuration
- [x] Define relationship between garage and bike
- [x] Define how engines run inside bikes
- [x] Define scaling model (multiple bikes)
- [x] Define health checks and monitoring
- [x] Define logging and observability
- [ ] Define K8s deployment manifests
- [ ] Define Helm chart structure

## Specification

### Bike vs Garage

| Aspect | Garage | Bike |
|--------|--------|------|
| Purpose | Development | Production |
| Container | `moto-dev` (~3GB) | `moto-engine-*` (~50MB) |
| User | root | non-root (1000:1000) |
| Lifetime | Hours to days | Weeks to months |
| Replicas | 1 per developer | Multiple (scaled) |
| Contains | Full toolchain + AI | Single binary only |
| Network | Restricted egress | Service mesh |

See [container-system.md](container-system.md) for container build details.

### Lifecycle

```
┌─────────────────────────────────────────────────────────────────┐
│                       Bike Lifecycle                             │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│   Garage                    Build                    Bike        │
│  ┌───────┐               ┌─────────┐              ┌───────┐     │
│  │Wrench │──── git push ─│   CI    │─── deploy ──▶│  Rip  │     │
│  │ code  │               │  build  │              │       │     │
│  └───────┘               └─────────┘              └───────┘     │
│                               │                        │         │
│                               ▼                        ▼         │
│                        ┌─────────────┐          ┌──────────┐    │
│                        │   Registry  │          │  K8s Pod │    │
│                        │ moto-engine │          │ running  │    │
│                        └─────────────┘          └──────────┘    │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

**Stages:**

1. **Build** (CI/CD)
   - Code merged to main triggers build
   - Nix builds minimal container (see [container-system.md](container-system.md))
   - Image pushed to registry with SHA and version tags

2. **Deploy**
   - K8s deployment updated with new image tag
   - Rolling update replaces old pods
   - Health checks gate traffic

3. **Run** (Ripping)
   - Pod runs the engine binary
   - Secrets injected via keybox
   - Metrics exposed on `/metrics`

4. **Stop**
   - Graceful shutdown on SIGTERM
   - 30-second grace period
   - In-flight requests complete

### Bike Configuration

Configuration via environment variables and K8s ConfigMaps/Secrets.

**Environment variables (common to all bikes):**

```bash
# Identity
ENGINE_NAME="club"                    # Which engine this bike runs
POD_NAME="moto-club-abc123"          # K8s pod name
POD_NAMESPACE="moto-prod"            # K8s namespace

# Runtime
RUST_LOG="info"                       # Log level
RUST_BACKTRACE="1"                    # Enable backtraces

# Networking
BIND_ADDRESS="0.0.0.0"
BIND_PORT="8080"

# Secrets (from keybox, not hardcoded)
KEYBOX_URL="http://keybox.moto-system:8080"
# DATABASE_URL, API_KEYS, etc. fetched at runtime

# Observability
METRICS_PORT="9090"
HEALTH_PORT="8081"

# TLS
SSL_CERT_FILE="/etc/ssl/certs/ca-bundle.crt"
```

**Engine-specific config:**

Each engine has its own config requirements:

| Engine | Key Config |
|--------|------------|
| `club` | `DATABASE_URL`, `REDIS_URL` |
| `vault` | `HSM_ENDPOINT`, `ENCRYPTION_KEY_ID` |
| `proxy` | `UPSTREAM_SERVICES`, `ROUTE_CONFIG` |

### How Engines Run Inside Bikes

**One engine per bike.** Each bike container runs exactly one engine binary.

```
Bike Container
┌────────────────────────────────────────┐
│                                        │
│   ┌─────────────────────────────┐     │
│   │     moto-engine-club        │     │
│   │     (single binary)         │     │
│   └─────────────────────────────┘     │
│              │                         │
│              ├── :8080  (API)         │
│              ├── :8081  (health)      │
│              └── :9090  (metrics)     │
│                                        │
└────────────────────────────────────────┘
```

**Port convention:**

| Port | Purpose |
|------|---------|
| 8080 | Main API (HTTP/gRPC) |
| 8081 | Health endpoints |
| 9090 | Prometheus metrics |

**Entrypoint:**

```dockerfile
ENTRYPOINT ["/bin/moto-engine-club"]
# No shell, no init system needed for single binary
```

### Scaling Model

**Horizontal scaling via K8s:**

```yaml
# infra/k8s/bikes/club/deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: moto-club
  namespace: moto-prod
spec:
  replicas: 3                    # Multiple bikes
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
        image: ${MOTO_REGISTRY}/moto-engine-club:v1.0.0  # or localhost:5000/...
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
```

**Autoscaling:**

```yaml
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: moto-club-hpa
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: moto-club
  minReplicas: 2
  maxReplicas: 10
  metrics:
  - type: Resource
    resource:
      name: cpu
      target:
        type: Utilization
        averageUtilization: 70
```

**Scaling dimensions:**

| Engine | Scale by | Min | Max |
|--------|----------|-----|-----|
| `club` | CPU, connections | 2 | 10 |
| `vault` | Request rate | 2 | 20 |
| `proxy` | Request rate | 3 | 50 |

### Health Checks

**Three probe types:**

```yaml
livenessProbe:
  httpGet:
    path: /health/live
    port: 8081
  initialDelaySeconds: 5
  periodSeconds: 10
  failureThreshold: 3

readinessProbe:
  httpGet:
    path: /health/ready
    port: 8081
  initialDelaySeconds: 5
  periodSeconds: 5
  failureThreshold: 3

startupProbe:
  httpGet:
    path: /health/startup
    port: 8081
  initialDelaySeconds: 0
  periodSeconds: 1
  failureThreshold: 30
```

**Health endpoint contract:**

| Endpoint | Returns 200 when |
|----------|------------------|
| `/health/live` | Process is alive (not deadlocked) |
| `/health/ready` | Ready to accept traffic (deps connected) |
| `/health/startup` | Initial startup complete |

**Implementation:**

```rust
// All engines implement this trait
pub trait HealthCheck {
    async fn is_live(&self) -> bool;
    async fn is_ready(&self) -> bool;
}
```

### Logging and Observability

**Structured logging:**

All bikes emit JSON logs to stdout:

```json
{
  "timestamp": "2026-01-20T10:30:00Z",
  "level": "info",
  "message": "Request handled",
  "engine": "club",
  "pod": "moto-club-abc123",
  "trace_id": "abc123",
  "span_id": "def456",
  "duration_ms": 42,
  "status": 200
}
```

**Log levels:**

| Level | When to use |
|-------|-------------|
| `error` | Unrecoverable failures |
| `warn` | Recoverable issues, degraded state |
| `info` | Request/response, lifecycle events |
| `debug` | Internal state (off in prod) |
| `trace` | Very verbose (never in prod) |

**Metrics (Prometheus):**

All bikes expose on `:9090/metrics`:

```
# Request metrics
http_requests_total{method="GET",path="/api/v1/...",status="200"}
http_request_duration_seconds{method="GET",path="/api/v1/..."}

# Runtime metrics
process_cpu_seconds_total
process_resident_memory_bytes
process_open_fds

# Engine-specific metrics
moto_club_active_garages
moto_vault_tokens_created_total
moto_proxy_requests_proxied_total
```

**Tracing (OpenTelemetry):**

Bikes emit traces to collector:

```bash
OTEL_EXPORTER_OTLP_ENDPOINT="http://otel-collector.moto-system:4317"
```

### Deployment Strategy

**Rolling updates (default):**

```yaml
strategy:
  type: RollingUpdate
  rollingUpdate:
    maxSurge: 1
    maxUnavailable: 0
```

- One new pod starts before old pod terminates
- Zero downtime during deployments
- Rollback via `kubectl rollout undo`

**Canary deployments (optional):**

For high-risk changes, deploy to subset first:

```bash
# Deploy canary (10% traffic)
kubectl set image deployment/moto-club club=${MOTO_REGISTRY}/moto-engine-club:canary
kubectl scale deployment/moto-club-canary --replicas=1

# Monitor metrics, then promote or rollback
```

### Resource Limits

**Default limits per bike:**

| Engine | CPU Request | CPU Limit | Memory Request | Memory Limit |
|--------|-------------|-----------|----------------|--------------|
| `club` | 500m | 2 | 512Mi | 2Gi |
| `vault` | 1 | 4 | 1Gi | 4Gi |
| `proxy` | 500m | 2 | 256Mi | 1Gi |

**Rationale:**

- `club`: Moderate CPU for API handling, memory for connections
- `vault`: Higher limits for crypto operations
- `proxy`: Low memory (stateless), CPU for throughput

### Security

**Non-root execution:**

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

**Network policies:**

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
    - port: 8080  # keybox
```

**Secrets:**

- No secrets in container image
- All secrets from keybox via SPIFFE identity
- Secrets rotated without pod restart (where possible)

### Service Mesh (Future)

Bikes will integrate with service mesh for:

- mTLS between services
- Traffic management
- Observability
- Retries and circuit breaking

## CLI Commands

```bash
# List running bikes
moto bike list

# Deploy a new version
moto bike deploy club v1.2.0

# Scale bikes
moto bike scale club --replicas 5

# View bike logs
moto bike logs club

# Restart bikes (rolling)
moto bike restart club

# Get bike status
moto bike status club
```

## Notes

- Consider Argo Rollouts for advanced deployment strategies
- PodDisruptionBudget needed for production stability
- Service mesh (Linkerd or Istio) for mTLS between bikes

## References

- [container-system.md](container-system.md) - How bike containers are built
- [keybox.md](keybox.md) - How bikes get secrets
- [moto-club.md](moto-club.md) - The club engine specification

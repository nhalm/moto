# Supporting Services

| | |
|--------|----------------------------------------------|
| Version | 0.2 |
| Last Updated | 2026-02-02 |

## Overview

Defines the supporting services (Postgres, Redis) that run inside a garage to support development and testing. Services are per-garage (isolated), ephemeral (destroyed with garage), and on-demand (only provisioned when requested).

**Philosophy:** Simple dev databases, not production infrastructure. No replication, no backups, no operators - just quick, disposable instances for development.

**Dependencies:**
- [garage-isolation.md](garage-isolation.md) - Network policy allows intra-namespace traffic
- [garage-lifecycle.md](garage-lifecycle.md) - Services created during garage open
- [moto-club.md](moto-club.md) - Provisions services based on CLI flags

## Specification

### Available Services

| Service | Image | Port | Use Case |
|---------|-------|------|----------|
| PostgreSQL | `postgres:16` | 5432 | Relational database |
| Redis | `redis:7` | 6379 | Cache, queues, sessions |

### Requesting Services

Services are requested via CLI flags when opening a garage:

```bash
# No services (default)
moto garage open my-project

# With Postgres
moto garage open my-project --with-postgres

# With Redis
moto garage open my-project --with-redis

# With both
moto garage open my-project --with-postgres --with-redis
```

### PostgreSQL

#### Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: postgres
  namespace: moto-garage-{id}
  labels:
    app: postgres
    moto.dev/supporting-service: "true"
spec:
  replicas: 1
  selector:
    matchLabels:
      app: postgres
  template:
    metadata:
      labels:
        app: postgres
        moto.dev/supporting-service: "true"
    spec:
      containers:
        - name: postgres
          image: postgres:16
          ports:
            - containerPort: 5432
          env:
            - name: POSTGRES_USER
              value: dev
            - name: POSTGRES_PASSWORD
              valueFrom:
                secretKeyRef:
                  name: postgres-credentials
                  key: password
            - name: POSTGRES_DB
              value: dev
          resources:
            requests:
              cpu: 50m
              memory: 128Mi
            limits:
              cpu: 500m
              memory: 512Mi
          volumeMounts:
            - name: data
              mountPath: /var/lib/postgresql/data
          readinessProbe:
            exec:
              command: ["pg_isready", "-U", "dev"]
            initialDelaySeconds: 5
            periodSeconds: 5
      volumes:
        - name: data
          emptyDir: {}
```

#### Service

```yaml
apiVersion: v1
kind: Service
metadata:
  name: postgres
  namespace: moto-garage-{id}
spec:
  selector:
    app: postgres
  ports:
    - port: 5432
      targetPort: 5432
```

#### Credentials Secret

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: postgres-credentials
  namespace: moto-garage-{id}
type: Opaque
stringData:
  password: "{random-generated}"
  username: dev
  database: dev
  host: postgres
  port: "5432"
  url: "postgresql://dev:{password}@postgres:5432/dev"
```

### Redis

#### Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: redis
  namespace: moto-garage-{id}
  labels:
    app: redis
    moto.dev/supporting-service: "true"
spec:
  replicas: 1
  selector:
    matchLabels:
      app: redis
  template:
    metadata:
      labels:
        app: redis
        moto.dev/supporting-service: "true"
    spec:
      containers:
        - name: redis
          image: redis:7
          ports:
            - containerPort: 6379
          args: ["--requirepass", "$(REDIS_PASSWORD)"]
          env:
            - name: REDIS_PASSWORD
              valueFrom:
                secretKeyRef:
                  name: redis-credentials
                  key: password
          resources:
            requests:
              cpu: 50m
              memory: 64Mi
            limits:
              cpu: 250m
              memory: 256Mi
          volumeMounts:
            - name: data
              mountPath: /data
          readinessProbe:
            exec:
              command: ["redis-cli", "ping"]
            initialDelaySeconds: 5
            periodSeconds: 5
      volumes:
        - name: data
          emptyDir: {}
```

#### Service

```yaml
apiVersion: v1
kind: Service
metadata:
  name: redis
  namespace: moto-garage-{id}
spec:
  selector:
    app: redis
  ports:
    - port: 6379
      targetPort: 6379
```

#### Credentials Secret

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: redis-credentials
  namespace: moto-garage-{id}
type: Opaque
stringData:
  password: "{random-generated}"
  host: redis
  port: "6379"
  url: "redis://:password@redis:6379"
```

### Credential Discovery

The garage pod discovers service credentials via environment variables injected by moto-club:

```yaml
# Injected into garage pod when --with-postgres is used
env:
  - name: POSTGRES_HOST
    value: postgres
  - name: POSTGRES_PORT
    value: "5432"
  - name: POSTGRES_USER
    value: dev
  - name: POSTGRES_PASSWORD
    valueFrom:
      secretKeyRef:
        name: postgres-credentials
        key: password
  - name: POSTGRES_DB
    value: dev
  - name: DATABASE_URL
    valueFrom:
      secretKeyRef:
        name: postgres-credentials
        key: url

# Injected into garage pod when --with-redis is used
env:
  - name: REDIS_HOST
    value: redis
  - name: REDIS_PORT
    value: "6379"
  - name: REDIS_PASSWORD
    valueFrom:
      secretKeyRef:
        name: redis-credentials
        key: password
  - name: REDIS_URL
    valueFrom:
      secretKeyRef:
        name: redis-credentials
        key: url
```

### Resource Limits

Services are constrained to fit within the garage namespace quota:

| Service | CPU Request | CPU Limit | Memory Request | Memory Limit |
|---------|-------------|-----------|----------------|--------------|
| Postgres | 50m | 500m | 128Mi | 512Mi |
| Redis | 50m | 250m | 64Mi | 256Mi |
| **Total** | 100m | 750m | 192Mi | 768Mi |

This leaves the majority of namespace quota (4 CPU, 8Gi) for the garage pod.

### Lifecycle

1. **Creation:** When garage opens with `--with-postgres` or `--with-redis`, moto-club creates the Deployment, Service, and Secret before creating the garage pod.

2. **Ready check:** Garage pod should wait for services to be ready. Services have readiness probes; moto-club waits for deployments to be available before marking garage as Ready.

3. **Destruction:** When garage closes, the entire namespace is deleted, including all supporting services. No cleanup needed.

### Network Access

Services run in the same namespace as the garage pod. The garage-isolation NetworkPolicy must allow intra-namespace traffic:

```yaml
egress:
  # Allow same-namespace traffic (for supporting services)
  - to:
      - podSelector: {}   # All pods in same namespace
```

### Storage

All services use `emptyDir` volumes:
- Data is ephemeral
- Destroyed when pod restarts or garage closes
- No PersistentVolumeClaims needed

This is intentional - supporting services are for development, not data persistence. Developers should use migrations and seed data, not rely on database state.

### Future Services

Additional services can be added following the same pattern:
- **Elasticsearch** - Full-text search (future)
- **RabbitMQ** - Message queues (future)
- **MinIO** - S3-compatible storage (future)

## Changelog

### v0.2 (2026-02-02)
- Full specification written
- Per-garage deployment model (not shared)
- On-demand provisioning via CLI flags
- PostgreSQL 16 and Redis 7
- Ephemeral storage (emptyDir)
- Credential injection via environment variables

### v0.1 (2026-01-19)
- Initial placeholder

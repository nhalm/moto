# Garage Isolation

| | |
|--------|----------------------------------------------|
| Version | 0.3 |
| Last Updated | 2026-02-02 |

## Overview

Defines the isolation model for garage environments. The goal: Claude Code runs in YOLO mode inside the garage with full autonomy, but cannot escape the sandbox, access other garages, or compromise the control plane.

**Philosophy:** The container IS the sandbox. Root inside is fine because the container boundary (namespaces, cgroups, seccomp) is the real security perimeter.

**Dependencies:**
- [keybox.md](keybox.md) - Secrets access
- [moto-club.md](moto-club.md) - Pushes WireGuard config via ConfigMap
- [dev-container.md](dev-container.md) - Container image

## Specification

### Security Model

**What Garages CAN Do:**
- Full root access inside container
- Install packages (apt, nix, cargo, npm, pip)
- Modify any file in the container
- Access internet (packages, docs, external APIs)
- Access keybox for secrets (scoped to their identity)
- Access supporting services (postgres, redis in moto-system)
- Use all pre-installed dev tools
- Build and test code
- Clone repositories

**What Garages CANNOT Do:**
- Escape the container
- Access Kubernetes API
- Reach moto-club or control plane services
- Communicate with other garages
- Access host filesystem
- Access host network
- Run privileged operations (mount, load kernel modules)
- Access other namespaces (pid, network, ipc)

### Pod Security

#### Security Context

```yaml
securityContext:
  runAsUser: 0          # Root inside container
  runAsGroup: 0
  allowPrivilegeEscalation: false
  readOnlyRootFilesystem: true
  seccompProfile:
    type: RuntimeDefault
  capabilities:
    drop:
      - ALL
    add:
      - CHOWN           # Change file ownership
      - DAC_OVERRIDE    # Bypass file permission checks
      - FOWNER          # Bypass ownership checks
      - SETGID          # Set group ID
      - SETUID          # Set user ID
      - NET_BIND_SERVICE # Bind to ports < 1024
```

**Rationale:**
- `runAsUser: 0` - Root enables apt install, system config changes
- `allowPrivilegeEscalation: false` - Cannot gain additional privileges
- `readOnlyRootFilesystem: true` - Immutable base, writes go to volumes
- `capabilities: drop ALL` - Start with nothing
- Added capabilities are minimal set for normal dev work

#### Writable Volumes

```yaml
volumes:
  # Persistent - survives pod restarts
  - name: workspace
    persistentVolumeClaim:
      claimName: workspace-pvc

  # Ephemeral - destroyed with pod
  - name: tmp
    emptyDir: {}
  - name: var-tmp
    emptyDir: {}
  - name: home
    emptyDir: {}
  - name: nix
    emptyDir: {}
  - name: cargo
    emptyDir: {}

  # For apt package installation
  - name: var-lib-apt
    emptyDir: {}
  - name: var-cache-apt
    emptyDir: {}
  - name: usr-local
    emptyDir: {}

volumeMounts:
  - name: workspace
    mountPath: /workspace
  - name: tmp
    mountPath: /tmp
  - name: var-tmp
    mountPath: /var/tmp
  - name: home
    mountPath: /root
  - name: nix
    mountPath: /nix
  - name: cargo
    mountPath: /root/.cargo
  - name: var-lib-apt
    mountPath: /var/lib/apt
  - name: var-cache-apt
    mountPath: /var/cache/apt
  - name: usr-local
    mountPath: /usr/local
```

**Note:** The workspace uses a PVC so uncommitted work survives pod restarts. All other volumes are ephemeral.

#### Forbidden Pod Settings

These MUST NOT be set on garage pods:

```yaml
hostNetwork: false      # No host network access
hostPID: false          # No host PID namespace
hostIPC: false          # No host IPC namespace
privileged: false       # No privileged mode
```

#### No Service Account

Garage pods have no Kubernetes API access:

```yaml
automountServiceAccountToken: false
```

### Network Isolation

#### Network Policy

Each garage namespace gets a NetworkPolicy:

```yaml
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: garage-isolation
  namespace: moto-garage-{id}
spec:
  podSelector: {}       # Applies to all pods in namespace
  policyTypes:
    - Ingress
    - Egress

  # Deny all ingress (WireGuard tunnel bypasses at pod level)
  ingress: []

  egress:
    # Allow DNS
    - to:
        - namespaceSelector:
            matchLabels:
              kubernetes.io/metadata.name: kube-system
      ports:
        - protocol: UDP
          port: 53

    # Allow keybox
    - to:
        - namespaceSelector:
            matchLabels:
              moto.dev/type: system
          podSelector:
            matchLabels:
              app: keybox
      ports:
        - protocol: TCP
          port: 8080

    # Allow supporting services (postgres, redis)
    - to:
        - namespaceSelector:
            matchLabels:
              moto.dev/type: system
          podSelector:
            matchLabels:
              moto.dev/supporting-service: "true"
      ports:
        - protocol: TCP
          port: 5432    # postgres
        - protocol: TCP
          port: 6379    # redis

    # Allow internet (anything not in cluster)
    - to:
        - ipBlock:
            cidr: 0.0.0.0/0
            except:
              - 10.0.0.0/8         # Private (cluster internal)
              - 172.16.0.0/12      # Private (cluster internal)
              - 192.168.0.0/16     # Private (cluster internal)
              - 100.64.0.0/10      # CGNAT / WireGuard range
              - 169.254.0.0/16     # Link-local / cloud metadata
              - 127.0.0.0/8        # Loopback
```

#### Network Access Summary

| Destination | Allowed | Reason |
|-------------|---------|--------|
| Internet | Yes | Packages, docs, external APIs |
| kube-system (DNS) | Yes | Name resolution |
| keybox | Yes | Fetch secrets |
| Supporting services | Yes | Dev databases |
| moto-club | No | Config pushed via ConfigMap |
| Other garages | No | Full isolation |
| Kubernetes API | No | No service account |
| Cloud metadata (169.254.x.x) | No | Prevent credential theft |
| WireGuard range (100.64.x.x) | No | Blocked at NetworkPolicy level |

### Resource Limits

#### Pod Resources

```yaml
resources:
  requests:
    cpu: 100m
    memory: 256Mi
  limits:
    cpu: 4
    memory: 8Gi
```

#### Namespace Quota

Each garage namespace has a ResourceQuota:

```yaml
apiVersion: v1
kind: ResourceQuota
metadata:
  name: garage-quota
  namespace: moto-garage-{id}
spec:
  hard:
    requests.cpu: "4"
    requests.memory: 8Gi
    limits.cpu: "4"
    limits.memory: 8Gi
    pods: "1"
    persistentvolumeclaims: "1"
    services: "1"
```

#### LimitRange

Default limits for any pods in the namespace:

```yaml
apiVersion: v1
kind: LimitRange
metadata:
  name: garage-limits
  namespace: moto-garage-{id}
spec:
  limits:
    - type: Container
      default:
        cpu: "1"
        memory: 1Gi
      defaultRequest:
        cpu: 100m
        memory: 256Mi
      max:
        cpu: "4"
        memory: 8Gi
```

### WireGuard Configuration (Push Model)

Garage WireGuard config is pushed via ConfigMap/Secret, not pulled from moto-club.

**Key generation:** moto-club generates the WireGuard keypair when creating the garage. The private key is stored in a Secret, the public key is stored in moto-club's database for client session routing.

moto-club creates these resources when creating the garage:

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: wireguard-config
  namespace: moto-garage-{id}
data:
  address: "100.64.x.y/32"
  peers: |
    [Peer]
    PublicKey = {moto-club-pubkey}
    Endpoint = {derp-server}:443
    AllowedIPs = 100.64.0.0/10
---
apiVersion: v1
kind: Secret
metadata:
  name: wireguard-keys
  namespace: moto-garage-{id}
type: Opaque
data:
  private_key: {base64-encoded}
  public_key: {base64-encoded}
```

The garage pod mounts these and reads on startup. No API call to moto-club needed.

### Secrets Access

Garages access secrets via keybox with SVID authentication.

| Scope | Examples | Access |
|-------|----------|--------|
| Instance | Dev credentials for this garage | Yes |
| Service | Shared dev database passwords | Yes (if policy allows) |
| Global | AI API keys | No (ai-proxy fetches, not garage) |

Secrets are pull-based and on-demand. Garage only gets secrets it explicitly requests and is authorized for.

### Threat Model

**Threats Mitigated:**

| Threat | Mitigation |
|--------|------------|
| Container escape | No privileged, no host namespaces, seccomp |
| K8s API abuse | No service account token mounted |
| Lateral movement to other garages | NetworkPolicy blocks inter-garage traffic |
| Control plane access | NetworkPolicy blocks moto-club; config via ConfigMap |
| Secret exfiltration | Keybox ABAC limits scope; no global secrets |
| Resource exhaustion | ResourceQuota and LimitRange per namespace |
| Host filesystem access | No hostPath volumes |
| Cloud credential theft | NetworkPolicy blocks 169.254.0.0/16 (metadata service) |
| WireGuard network pivoting | NetworkPolicy blocks 100.64.0.0/10 |

**Accepted Risks:**

| Risk | Rationale |
|------|-----------|
| Root inside container | Container boundary is the security perimeter, not UID |
| Internet access | Required for packages, docs; monitoring can detect abuse |
| Writable volumes | Most are ephemeral; workspace PVC destroyed when garage closes |
| moto-club knows WG private key | Key is per-garage, ephemeral; moto-club already controls lifecycle |

## Changelog

### v0.3 (2026-02-02)
- Workspace volume changed to PVC (survives pod restarts)
- Added writable mounts for apt: /var/lib/apt, /var/cache/apt, /usr/local
- NetworkPolicy: block cloud metadata (169.254.0.0/16), WireGuard range (100.64.0.0/10), loopback
- ResourceQuota: pods=1 (one garage = one pod)
- Clarified moto-club generates WireGuard keypair

### v0.2 (2026-02-02)
- Full specification written
- Root access inside container (container is sandbox)
- NetworkPolicy: egress to internet, keybox, supporting-services only
- No moto-club access (push model for WG config)
- No garage-to-garage communication
- ResourceQuota and LimitRange per namespace
- Pod security context with minimal capabilities

### v0.1 (2026-01-19)
- Initial placeholder

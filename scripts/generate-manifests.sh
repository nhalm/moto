#!/usr/bin/env bash
# Generate K8s manifests for moto-system services from bike.toml files.
#
# Reads bike.toml for each engine and generates Deployment + Service YAML
# with the standard security baseline (matching the deployment builder in
# crates/moto-k8s/src/deployment.rs). Service-specific env vars, volumes,
# and RBAC come from service-deploy.md.
#
# Usage: ./scripts/generate-manifests.sh
#
# Output:
#   infra/k8s/moto-system/keybox.yaml
#   infra/k8s/moto-system/club.yaml
#   infra/k8s/moto-system/ai-proxy.yaml

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUTPUT_DIR="$REPO_ROOT/infra/k8s/moto-system"

# --- bike.toml parser ---
# Extracts a value from a bike.toml file. Handles simple key = "value" and key = number.
parse_toml() {
    local file="$1" key="$2" default="${3:-}"
    local value
    value=$(grep -E "^${key}\s*=" "$file" 2>/dev/null | head -1 | sed 's/.*=[[:space:]]*//' | tr -d '"[:space:]')
    echo "${value:-$default}"
}

# Extracts a value from a specific [section] in a bike.toml file.
parse_toml_section() {
    local file="$1" section="$2" key="$3" default="${4:-}"
    local value
    value=$(awk -v sect="[${section}]" '
        $0 == sect { found=1; next }
        /^\[/ { found=0 }
        found { print }
    ' "$file" | grep -E "^${key}\s*=" | head -1 | sed 's/.*=[[:space:]]*//' | tr -d '"[:space:]')
    echo "${value:-$default}"
}

# --- Generate keybox.yaml ---
generate_keybox() {
    local bike_toml="$REPO_ROOT/crates/moto-keybox-server/bike.toml"
    if [ ! -f "$bike_toml" ]; then
        echo "ERROR: $bike_toml not found" >&2
        exit 1
    fi

    local name
    name=$(parse_toml "$bike_toml" "name" "keybox")
    local deploy_port
    deploy_port=$(parse_toml_section "$bike_toml" "deploy" "port" "8080")
    local health_port
    health_port=$(parse_toml_section "$bike_toml" "health" "port" "8081")
    local replicas
    replicas=$(parse_toml_section "$bike_toml" "deploy" "replicas" "1")
    local cpu_request
    cpu_request=$(parse_toml_section "$bike_toml" "resources" "cpu_request" "50m")
    local cpu_limit
    cpu_limit=$(parse_toml_section "$bike_toml" "resources" "cpu_limit" "500m")
    local memory_request
    memory_request=$(parse_toml_section "$bike_toml" "resources" "memory_request" "128Mi")
    local memory_limit
    memory_limit=$(parse_toml_section "$bike_toml" "resources" "memory_limit" "512Mi")

    cat > "$OUTPUT_DIR/keybox.yaml" << YAML
# Generated from crates/moto-keybox-server/bike.toml by scripts/generate-manifests.sh
# Do not edit manually — regenerate with: make generate-manifests
---
apiVersion: v1
kind: Service
metadata:
  name: moto-${name}
  namespace: moto-system
  labels:
    app.kubernetes.io/part-of: moto
    app.kubernetes.io/component: moto-${name}
spec:
  type: ClusterIP
  selector:
    app.kubernetes.io/component: moto-${name}
  ports:
    - name: api
      port: ${deploy_port}
      targetPort: ${deploy_port}
      protocol: TCP
    - name: health
      port: ${health_port}
      targetPort: ${health_port}
      protocol: TCP
    - name: metrics
      port: 9090
      targetPort: 9090
      protocol: TCP
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: moto-${name}
  namespace: moto-system
  labels:
    app.kubernetes.io/part-of: moto
    app.kubernetes.io/component: moto-${name}
spec:
  replicas: ${replicas}
  strategy:
    type: RollingUpdate
    rollingUpdate:
      maxSurge: 1
      maxUnavailable: 0
  selector:
    matchLabels:
      app.kubernetes.io/component: moto-${name}
  template:
    metadata:
      labels:
        app.kubernetes.io/part-of: moto
        app.kubernetes.io/component: moto-${name}
    spec:
      securityContext:
        runAsUser: 1000
        runAsGroup: 1000
        runAsNonRoot: true
      containers:
        - name: moto-${name}
          image: moto-registry:5000/moto-${name}:latest
          ports:
            - containerPort: ${deploy_port}
            - containerPort: ${health_port}
            - containerPort: 9090
          env:
            - name: POD_NAME
              valueFrom:
                fieldRef:
                  fieldPath: metadata.name
            - name: POD_NAMESPACE
              valueFrom:
                fieldRef:
                  fieldPath: metadata.namespace
            - name: RUST_LOG
              value: info
            - name: RUST_BACKTRACE
              value: "1"
            - name: MOTO_KEYBOX_DATABASE_URL
              valueFrom:
                secretKeyRef:
                  name: keybox-db-credentials
                  key: url
            - name: MOTO_KEYBOX_MASTER_KEY_FILE
              value: /run/secrets/keybox/master.key
            - name: MOTO_KEYBOX_SVID_SIGNING_KEY_FILE
              value: /run/secrets/keybox/signing.key
            - name: MOTO_KEYBOX_SERVICE_TOKEN_FILE
              value: /run/secrets/keybox/service-token
          resources:
            requests:
              cpu: ${cpu_request}
              memory: ${memory_request}
            limits:
              cpu: ${cpu_limit}
              memory: ${memory_limit}
          livenessProbe:
            httpGet:
              path: /health/live
              port: ${health_port}
            periodSeconds: 10
          readinessProbe:
            httpGet:
              path: /health/ready
              port: ${health_port}
            periodSeconds: 5
          startupProbe:
            httpGet:
              path: /health/startup
              port: ${health_port}
            failureThreshold: 30
            periodSeconds: 1
          securityContext:
            readOnlyRootFilesystem: true
            allowPrivilegeEscalation: false
            capabilities:
              drop:
                - ALL
          volumeMounts:
            - name: keybox-keys
              mountPath: /run/secrets/keybox
              readOnly: true
      volumes:
        - name: keybox-keys
          secret:
            secretName: keybox-keys
YAML

    echo "Generated $OUTPUT_DIR/keybox.yaml"
}

# --- Generate club.yaml ---
generate_club() {
    local bike_toml="$REPO_ROOT/crates/moto-club/bike.toml"
    if [ ! -f "$bike_toml" ]; then
        echo "ERROR: $bike_toml not found" >&2
        exit 1
    fi

    local name
    name=$(parse_toml "$bike_toml" "name" "club")
    local deploy_port
    deploy_port=$(parse_toml_section "$bike_toml" "deploy" "port" "8080")
    local health_port
    health_port=$(parse_toml_section "$bike_toml" "health" "port" "8081")
    local replicas
    replicas=$(parse_toml_section "$bike_toml" "deploy" "replicas" "1")
    local cpu_request
    cpu_request=$(parse_toml_section "$bike_toml" "resources" "cpu_request" "50m")
    local cpu_limit
    cpu_limit=$(parse_toml_section "$bike_toml" "resources" "cpu_limit" "500m")
    local memory_request
    memory_request=$(parse_toml_section "$bike_toml" "resources" "memory_request" "128Mi")
    local memory_limit
    memory_limit=$(parse_toml_section "$bike_toml" "resources" "memory_limit" "512Mi")

    cat > "$OUTPUT_DIR/club.yaml" << YAML
# Generated from crates/moto-club/bike.toml by scripts/generate-manifests.sh
# Do not edit manually — regenerate with: make generate-manifests
---
apiVersion: v1
kind: ServiceAccount
metadata:
  name: moto-${name}
  namespace: moto-system
  labels:
    app.kubernetes.io/part-of: moto
    app.kubernetes.io/component: moto-${name}
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: moto-${name}
  labels:
    app.kubernetes.io/part-of: moto
    app.kubernetes.io/component: moto-${name}
rules:
  - apiGroups: [""]
    resources: [namespaces]
    verbs: [get, list, create, delete, patch]
  - apiGroups: [""]
    resources: [pods]
    verbs: [get, list, create, delete]
  - apiGroups: [""]
    resources: [services]
    verbs: [get, list, create, delete]
  - apiGroups: [""]
    resources: [configmaps]
    verbs: [get, list, create, delete]
  - apiGroups: [""]
    resources: [secrets]
    verbs: [get, list, create, delete]
  - apiGroups: [""]
    resources: [persistentvolumeclaims]
    verbs: [get, list, create, delete]
  - apiGroups: [apps]
    resources: [deployments]
    verbs: [get, list, create, delete]
  - apiGroups: [networking.k8s.io]
    resources: [networkpolicies]
    verbs: [get, list, create, delete]
  - apiGroups: [""]
    resources: [resourcequotas]
    verbs: [get, list, create, delete]
  - apiGroups: [""]
    resources: [limitranges]
    verbs: [get, list, create, delete]
  - apiGroups: [authentication.k8s.io]
    resources: [tokenreviews]
    verbs: [create]
  - apiGroups: [coordination.k8s.io]
    resources: [leases]
    verbs: [get, create, update]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: moto-${name}
  labels:
    app.kubernetes.io/part-of: moto
    app.kubernetes.io/component: moto-${name}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: moto-${name}
subjects:
  - kind: ServiceAccount
    name: moto-${name}
    namespace: moto-system
---
apiVersion: v1
kind: Service
metadata:
  name: moto-${name}
  namespace: moto-system
  labels:
    app.kubernetes.io/part-of: moto
    app.kubernetes.io/component: moto-${name}
spec:
  type: ClusterIP
  selector:
    app.kubernetes.io/component: moto-${name}
  ports:
    - name: api
      port: ${deploy_port}
      targetPort: ${deploy_port}
      protocol: TCP
    - name: health
      port: ${health_port}
      targetPort: ${health_port}
      protocol: TCP
    - name: metrics
      port: 9090
      targetPort: 9090
      protocol: TCP
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: moto-${name}
  namespace: moto-system
  labels:
    app.kubernetes.io/part-of: moto
    app.kubernetes.io/component: moto-${name}
spec:
  replicas: ${replicas}
  strategy:
    type: RollingUpdate
    rollingUpdate:
      maxSurge: 1
      maxUnavailable: 0
  selector:
    matchLabels:
      app.kubernetes.io/component: moto-${name}
  template:
    metadata:
      labels:
        app.kubernetes.io/part-of: moto
        app.kubernetes.io/component: moto-${name}
    spec:
      serviceAccountName: moto-${name}
      securityContext:
        runAsUser: 1000
        runAsGroup: 1000
        runAsNonRoot: true
      containers:
        - name: moto-${name}
          image: moto-registry:5000/moto-${name}:latest
          ports:
            - containerPort: ${deploy_port}
            - containerPort: ${health_port}
            - containerPort: 9090
          env:
            - name: POD_NAME
              valueFrom:
                fieldRef:
                  fieldPath: metadata.name
            - name: POD_NAMESPACE
              valueFrom:
                fieldRef:
                  fieldPath: metadata.namespace
            - name: RUST_LOG
              value: info
            - name: RUST_BACKTRACE
              value: "1"
            - name: MOTO_CLUB_DATABASE_URL
              valueFrom:
                secretKeyRef:
                  name: club-db-credentials
                  key: url
            - name: MOTO_CLUB_KEYBOX_URL
              value: http://moto-keybox.moto-system:8080
            - name: MOTO_CLUB_KEYBOX_HEALTH_URL
              value: http://moto-keybox.moto-system:8081
            - name: MOTO_CLUB_KEYBOX_SERVICE_TOKEN_FILE
              value: /run/secrets/club/service-token
            - name: MOTO_CLUB_DEV_CONTAINER_IMAGE
              value: moto-registry:5000/moto-garage:latest
            - name: MOTO_CLUB_DERP_SERVERS
              value: "[]"
          resources:
            requests:
              cpu: ${cpu_request}
              memory: ${memory_request}
            limits:
              cpu: ${cpu_limit}
              memory: ${memory_limit}
          livenessProbe:
            httpGet:
              path: /health/live
              port: ${health_port}
            periodSeconds: 10
          readinessProbe:
            httpGet:
              path: /health/ready
              port: ${health_port}
            periodSeconds: 5
          startupProbe:
            httpGet:
              path: /health/startup
              port: ${health_port}
            failureThreshold: 30
            periodSeconds: 1
          securityContext:
            readOnlyRootFilesystem: true
            allowPrivilegeEscalation: false
            capabilities:
              drop:
                - ALL
          volumeMounts:
            - name: keybox-service-token
              mountPath: /run/secrets/club
              readOnly: true
      volumes:
        - name: keybox-service-token
          secret:
            secretName: keybox-service-token
YAML

    echo "Generated $OUTPUT_DIR/club.yaml"
}

# --- Generate ai-proxy.yaml ---
generate_ai_proxy() {
    local bike_toml="$REPO_ROOT/crates/moto-ai-proxy/bike.toml"
    if [ ! -f "$bike_toml" ]; then
        echo "ERROR: $bike_toml not found" >&2
        exit 1
    fi

    local name
    name=$(parse_toml "$bike_toml" "name" "ai-proxy")
    local deploy_port
    deploy_port=$(parse_toml_section "$bike_toml" "deploy" "port" "8080")
    local health_port
    health_port=$(parse_toml_section "$bike_toml" "health" "port" "8081")
    local replicas
    replicas=$(parse_toml_section "$bike_toml" "deploy" "replicas" "1")
    local cpu_request
    cpu_request=$(parse_toml_section "$bike_toml" "resources" "cpu_request" "50m")
    local cpu_limit
    cpu_limit=$(parse_toml_section "$bike_toml" "resources" "cpu_limit" "500m")
    local memory_request
    memory_request=$(parse_toml_section "$bike_toml" "resources" "memory_request" "128Mi")
    local memory_limit
    memory_limit=$(parse_toml_section "$bike_toml" "resources" "memory_limit" "512Mi")

    cat > "$OUTPUT_DIR/ai-proxy.yaml" << YAML
# Generated from crates/moto-ai-proxy/bike.toml by scripts/generate-manifests.sh
# Do not edit manually — regenerate with: make generate-manifests
---
apiVersion: v1
kind: ServiceAccount
metadata:
  name: moto-${name}
  namespace: moto-system
  labels:
    app.kubernetes.io/part-of: moto
    app.kubernetes.io/component: moto-${name}
---
apiVersion: v1
kind: Service
metadata:
  name: moto-${name}
  namespace: moto-system
  labels:
    app.kubernetes.io/part-of: moto
    app.kubernetes.io/component: moto-${name}
spec:
  type: ClusterIP
  selector:
    app.kubernetes.io/component: moto-${name}
  ports:
    - name: api
      port: ${deploy_port}
      targetPort: ${deploy_port}
      protocol: TCP
    - name: health
      port: ${health_port}
      targetPort: ${health_port}
      protocol: TCP
    - name: metrics
      port: 9090
      targetPort: 9090
      protocol: TCP
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: moto-${name}
  namespace: moto-system
  labels:
    app.kubernetes.io/part-of: moto
    app.kubernetes.io/component: moto-${name}
spec:
  replicas: ${replicas}
  strategy:
    type: RollingUpdate
    rollingUpdate:
      maxSurge: 1
      maxUnavailable: 0
  selector:
    matchLabels:
      app.kubernetes.io/component: moto-${name}
  template:
    metadata:
      labels:
        app.kubernetes.io/part-of: moto
        app.kubernetes.io/component: moto-${name}
    spec:
      serviceAccountName: moto-${name}
      securityContext:
        runAsUser: 1000
        runAsGroup: 1000
        runAsNonRoot: true
      containers:
        - name: moto-${name}
          image: moto-registry:5000/moto-${name}:latest
          ports:
            - containerPort: ${deploy_port}
            - containerPort: ${health_port}
            - containerPort: 9090
          env:
            - name: POD_NAME
              valueFrom:
                fieldRef:
                  fieldPath: metadata.name
            - name: POD_NAMESPACE
              valueFrom:
                fieldRef:
                  fieldPath: metadata.namespace
            - name: RUST_LOG
              value: info
            - name: RUST_BACKTRACE
              value: "1"
            - name: MOTO_AI_PROXY_KEYBOX_URL
              value: http://moto-keybox.moto-system:8080
            - name: MOTO_AI_PROXY_CLUB_URL
              value: http://moto-club.moto-system:8080
            - name: MOTO_AI_PROXY_SVID_FILE
              value: /var/run/secrets/svid/svid.jwt
          resources:
            requests:
              cpu: ${cpu_request}
              memory: ${memory_request}
            limits:
              cpu: ${cpu_limit}
              memory: ${memory_limit}
          livenessProbe:
            httpGet:
              path: /health/live
              port: ${health_port}
            periodSeconds: 10
          readinessProbe:
            httpGet:
              path: /health/ready
              port: ${health_port}
            periodSeconds: 5
          startupProbe:
            httpGet:
              path: /health/startup
              port: ${health_port}
            failureThreshold: 30
            periodSeconds: 1
          securityContext:
            readOnlyRootFilesystem: true
            allowPrivilegeEscalation: false
            capabilities:
              drop:
                - ALL
          volumeMounts:
            - name: svid
              mountPath: /var/run/secrets/svid
              readOnly: true
      volumes:
        - name: svid
          secret:
            secretName: ai-proxy-svid
YAML

    echo "Generated $OUTPUT_DIR/ai-proxy.yaml"
}

# --- Main ---
generate_keybox
generate_club
generate_ai_proxy
echo "All manifests generated."

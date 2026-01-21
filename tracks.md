# Moto Implementation Log

<!-- PREPEND new entries here, right after this comment. Newest entries at top. -->

## Log

### 2026-01-21: CLI Spec v0.2 Verification

**Spec:** moto-cli.md v0.2

**Verified:**
- `--owner` flag already present on `moto garage open` (implemented in prior work)
- `-n` short flag for `--tail` already present on `moto garage logs` (implemented in prior work)
- Spec v0.2 changelog confirms these were already implemented; spec updated to match code

**Validated:** `cargo test --workspace` passes (37 unit tests + 2 doctests, 4 ignored K8s integration tests)


---

### 2026-01-21: Auto-Generated Garage Names

**Spec:** moto-cli.md v0.1

**Implemented:**
- `moto garage open` now auto-generates names (e.g., `bold-mongoose`, `quiet-falcon`)
- Removed required `name` argument from `moto garage open` (matches spec)
- Added `names.rs` module with adjective-animal name generator (46 adjectives × 75 animals = 3450 combinations)
- Added `rand` dependency to workspace for random name selection
- Updated output format to match spec: "Created garage: <name>" with Engine, TTL, and connect hint
- Updated JSON output to match spec: `name`, `engine`, `ttl_seconds`, `status` fields

**Validated:** `cargo test --workspace` passes (33 unit tests + 2 doctests, 4 ignored K8s integration tests)


---

### 2026-01-21: Garage Close Improvements

**Spec:** moto-cli.md v0.1

**Implemented:**
- Changed `moto garage close` to accept garage name instead of ID (matches spec)
- Added `--force` flag to skip confirmation prompt
- Added confirmation prompt before closing (unless --force or --json)
- Added `close_by_name()` method to `GarageClient` in `moto-garage`
- Error message now includes suggestion `Try: moto garage list` for not-found errors
- Removed `resolve_garage_id()` function (no longer needed)

**Validated:** `cargo test --workspace` passes (31 unit tests + 2 doctests, 4 ignored K8s integration tests)


---

### 2026-01-21: Garage List Output Format

**Spec:** moto-cli.md v0.1

**Implemented:**
- Updated `moto garage list` output to match spec format: NAME, STATUS, AGE, TTL, ENGINE
- JSON output now includes `age_seconds`, `ttl_remaining_seconds`, and `engine` fields
- Removed `id` and `namespace` from list output (not in spec)
- Added `format_duration()` function for human-readable durations (e.g., "2h15m", "1d3h")
- TTL shows "expired" when TTL has passed, "-" when no TTL set
- Added 5 unit tests for `format_duration()`

**Validated:** `cargo test --workspace` passes (31 unit tests + 2 doctests, 4 ignored K8s integration tests)


---

### 2026-01-21: CLI Configuration File Support

**Spec:** moto-cli.md v0.1

**Implemented:**
- Added `Config` struct for loading `$XDG_CONFIG_HOME/moto/config.toml` (falls back to `~/.config/moto/config.toml`)
- Config supports `[output].color` (auto/always/never) and `[garage].ttl` (default TTL)
- Added `toml` and `dirs` dependencies to workspace
- Config is loaded at startup and included in `GlobalFlags`
- `moto garage open --ttl` now has precedence: CLI flag → config file → hardcoded default (4h)
- Config gracefully handles missing files (returns defaults)
- Created `config.rs` module with `Config`, `OutputConfig`, `GarageConfig`, `ColorMode` types

**Validated:** `cargo test --workspace` passes (26 unit tests + 2 doctests, 4 ignored K8s integration tests)


---

### 2026-01-21: Garage Open Engine Flag

**Spec:** moto-cli.md v0.1

**Implemented:**
- Added `--engine/-e` flag to `moto garage open`
- Added `ENGINE` label constant (`moto.dev/engine`) to `Labels` in `moto-k8s`
- Added `engine` field to `GarageInfo` struct in `moto-club-types`
- Added `with_engine()` builder method to `GarageInfo`
- Updated `Labels::for_garage()` to accept optional `engine` parameter
- Updated `GarageClient::open()` to accept optional `engine` parameter
- Updated `namespace_to_garage_info()` to parse engine label
- CLI shows engine in open output (both text and JSON)

**Validated:** `cargo test --workspace` passes (22 unit tests + 2 doctests, 4 ignored K8s integration tests)


---

### 2026-01-21: Garage Open TTL Flag

**Spec:** moto-cli.md v0.1

**Implemented:**
- Added `--ttl` flag to `moto garage open` (default: 4h, max: 48h)
- Duration format: `<number><unit>` where unit is m, h, or d
- Added `parse_ttl()` function with max validation
- Added `EXPIRES_AT` label constant to `Labels` in `moto-k8s`
- Updated `Labels::for_garage()` to accept optional `expires_at` parameter
- Updated `GarageClient::open()` to accept optional `ttl_seconds` parameter
- TTL is stored as RFC 3339 timestamp in `moto.dev/expires-at` label
- Updated `namespace_to_garage_info()` to parse expires_at label
- Garage open output shows expiration time when TTL is set

**Validated:** `cargo test --workspace` passes (20 unit tests + 2 doctests, 4 ignored K8s integration tests)


---

### 2026-01-21: Garage Logs Streaming

**Spec:** moto-cli.md v0.1

**Implemented:**
- Added `--follow/-f` flag to `moto garage logs` command
- Streams logs continuously until Ctrl+C
- Added `stream_pod_logs()` method to `PodOps` trait in `moto-k8s`
- Added `LogStream` type (pinned boxed async stream of log lines)
- Added `logs_stream()` method to `GarageClient`
- Uses `AsyncBufReadExt::lines()` for line-by-line streaming from K8s pods
- Added `IoError` variant to `moto_k8s::Error` for stream I/O errors
- Error: `--json` not supported with `--follow` (streaming incompatible with JSON output)

**Validated:** `cargo test --workspace` passes (19 unit tests + 2 doctests, 4 ignored K8s integration tests)


---

### 2026-01-21: Garage Logs Command

**Spec:** moto-cli.md v0.1

**Implemented:**
- Added `moto garage logs <name>` command
- Options: `--tail/-n <lines>` (default 100), `--since <duration>` (e.g., 5m, 1h)
- Added `PodOps` trait to `moto-k8s` with `list_pods()` and `get_pod_logs()` methods
- Added `PodLogOptions` struct for configuring log retrieval
- Added `logs()` method to `GarageClient` to fetch logs from garage pods
- Duration parsing supports `s`, `m`, `h`, `d` units
- JSON output support via `--json` flag

**Validated:** `cargo test --workspace` passes (19 unit tests + 2 doctests, 4 ignored K8s integration tests)


---

### 2026-01-20: Workspace Foundation

**Spec:** project-structure.md v1.0

**Implemented:**
- Created Cargo workspace (`Cargo.toml`) with workspace dependencies
- Created `rust-toolchain.toml` pinning stable channel
- Created `.cargo/config.toml` with build settings and aliases
- Created `Makefile` with build/test/lint targets
- Created `moto-common` crate with:
  - `Error` enum and `Result` type alias
  - `Secret<T>` wrapper for sensitive data (with tests)

**Validated:** `cargo check` and `cargo test` pass (4 unit tests + 1 doctest)


---

### 2026-01-20: Club Types

**Spec:** project-structure.md v1.0

**Implemented:**
- Created `moto-club-types` crate with:
  - `GarageId` - UUID v7 newtype with `.short()` display, FromStr, serde
  - `GarageState` - Enum: `Pending`, `Running`, `Ready`, `Terminating`, `Terminated`
  - `GarageInfo` - Struct with id, name, namespace, state, created_at, expires_at, owner
  - Builder methods: `GarageInfo::new()`, `.with_owner()`, `.with_expires_at()`

**Validated:** `cargo test --workspace` passes (12 unit tests + 1 doctest)


---

### 2026-01-20: K3s Client

**Spec:** project-structure.md v1.0

**Implemented:**
- Created `moto-k3s` crate with:
  - `K3sClient` - wraps `kube::Client`, provides moto-specific operations
  - `NamespaceOps` trait - `create_namespace()`, `delete_namespace()`, `get_namespace()`, `list_namespaces()`, `namespace_exists()`
  - `Labels` - constants for moto K8s labels (`moto.dev/type`, `moto.dev/id`, `moto.dev/name`, `moto.dev/owner`)
  - Helper methods: `Labels::garage_selector()`, `Labels::for_garage()`, `Labels::for_bike()`
  - Error types: `NamespaceExists`, `NamespaceNotFound`, `NamespaceCreate`, etc.

**Validated:** `cargo test --workspace` passes (17 unit tests + 1 doctest, 2 ignored K8s integration tests)


---

### 2026-01-20: Garage Client

**Spec:** project-structure.md v1.0

**Implemented:**
- Created `moto-garage` crate with:
  - `GarageMode` - Enum: `Local` (direct K8s) or `Remote { endpoint }` (via club)
  - `GarageClient` - Methods: `list()`, `open(name)`, `close(id)`
  - Local mode implementation using `moto-k3s` for K8s operations
  - Namespace-to-GarageInfo conversion with label extraction
  - Error types: `GarageNotFound`, `GarageExists`, `K8s`, `RemoteNotImplemented`

**Validated:** `cargo test --workspace` passes (23 unit tests + 2 doctests, 4 ignored K8s integration tests)


---

### 2026-01-20: CLI Crate

**Spec:** project-structure.md v1.0

**Implemented:**
- Created `moto-cli` crate with:
  - Binary named `moto` with clap-based CLI parsing
  - Top-level command structure: `moto <command>`
  - `moto garage list` - lists all garages in table format
  - `moto garage open <name> [--owner <owner>]` - opens a new garage
  - `moto garage close <id>` - closes a garage (supports short ID prefix)
  - ID prefix resolution for close command (matches by UUID prefix)
  - Tracing-subscriber for logging with RUST_LOG env filter

**Validated:** `cargo test --workspace` passes (23 unit tests + 2 doctests, 4 ignored K8s integration tests)


---

### 2026-01-21: Rename moto-k3s to moto-k8s

**Spec:** project-structure.md v1.1

**Implemented:**
- Renamed crate `moto-k3s` → `moto-k8s` (k3s is infrastructure, not code)
- Renamed `K3sClient` → `K8sClient` throughout
- Updated workspace `Cargo.toml` dependency
- Updated `moto-garage` to use `moto-k8s` and `K8sClient`
- Updated internal field names from `k3s` to `k8s` in `GarageClient`

**Validated:** `cargo test --workspace` passes (19 unit tests + 2 doctests, 4 ignored K8s integration tests)


---

### 2026-01-21: CLI Global Flags

**Spec:** moto-cli.md v0.1

**Implemented:**
- Added global flags to CLI (`--json/-j`, `--verbose/-v`, `--quiet/-q`, `--context/-c`)
- `GlobalFlags` struct with env var support (`MOTO_JSON`)
- Verbosity levels: 0=warn, 1=info, 2=debug, 3+=trace
- JSON output for all garage commands (list, open, close)
- Quiet mode suppresses progress messages
- Error output respects JSON flag

**Validated:** `cargo test --workspace` passes (19 unit tests + 2 doctests, 4 ignored K8s integration tests)



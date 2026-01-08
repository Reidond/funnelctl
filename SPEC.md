# SPEC: Rust CLI for short-lived public dev tunnels via Tailscale Funnel (LocalAPI backend)

## 1. Purpose

Build a small Rust CLI that provides **short-lived, public HTTPS access** to a developer's **local service (e.g., `localhost:8081`)** using **Tailscale Funnel**, as an alternative to tools like ngrok.

Key constraint: **Do not shell out to the `tailscale` CLI** from the Rust project.  
Instead, implement **Backend Option B**: talk directly to **`tailscaled` LocalAPI** to read/update Serve/Funnel configuration.

The user is the **sole administrator** of their tailnet/network.

---

## 2. Scope

### In scope (MVP — Phase 1)

- A Rust CLI that:
  - Enables (and later disables) **Funnel** for a local HTTP service.
  - Supports "ngrok-like" **foreground sessions**: keep running until Ctrl-C (or TTL), then tear down.
  - Prints the **public URL** (HTTPS) that can be pasted into a third-party webhook provider.
  - Uses **foreground serve config** (via WatchIPNBus) for automatic cleanup on crash.
- Backend abstraction with **LocalAPI backend implemented first**.
- Linux and macOS support (x86_64 + aarch64).
- Shell completions (bash, zsh, fish).
- Static binaries (musl on Linux).

### Out of scope (MVP)

- Managing tailnet policy/ACLs automatically.
- Creating or modifying Tailscale admin settings via the control-plane API.
- Exposing arbitrary TCP/UDP services (initially focus on HTTP/HTTPS proxying to `127.0.0.1:<port>`).
- Long-running daemon/service installation.
- Windows support (Phase 2).
- Unix socket targets (Phase 2).
- Configuration file (Phase 2).
- Request logging/verbose mode (Phase 2).
- Telemetry (Phase 2, opt-in experiment).

---

## 3. Terminology

- **tailnet**: Your Tailscale network.
- **tailscaled**: Local Tailscale daemon.
- **LocalAPI**: HTTP API exposed by tailscaled locally (Unix socket or localhost TCP depending on platform/build).
- **Serve**: Tailscale feature that maps an HTTPS endpoint on your node to a local service.
- **Funnel**: Serve routes exposed to the **public internet** (not just tailnet).
- **ServeConfig**: tailscaled's persisted config describing Serve/Funnel listeners and routes.
- **Foreground config**: Ephemeral ServeConfig tied to a WatchIPNBus session, automatically cleaned up when session ends.

---

## 4. Product goals and UX principles

### Primary goals

1. **One command** to create a public, short-lived URL for a local port.
2. **Safe defaults** (loopback targets; random path token by default).
3. **Non-destructive**: do not unexpectedly break existing Serve/Funnel config.
4. **Auto-cleanup**: use foreground config so crashes don't leave orphaned routes.

### UX principles

- Print a single, copy-pasteable URL.
- Make teardown predictable (Ctrl-C always tears down what we created).
- Fail with actionable errors (missing permissions, HTTPS not enabled, funnel disabled, etc.).
- Colored output when TTY detected; plain when piped.
- Detailed error format: Error/Cause/Fix structure.

---

## 5. CLI interface

### Command: `funnelctl open` (alias: `o`)

Creates a Funnel route and prints the public URL.

**Examples**

```bash
funnelctl open 8081                           # Quick tunnel with random path
funnelctl open 8081 --path /webhook           # Custom path
funnelctl open 8081 --ttl 30m                 # Auto-expire after 30 minutes
funnelctl open 8081 --bind 127.0.0.1 --path /hook
funnelctl open 8081 --json                    # Machine-readable NDJSON output
funnelctl o 8081                              # Alias
```

**Flags**

| Flag | Default | Description |
|------|---------|-------------|
| `<port>` (positional) | required | Local port on loopback (target: `http://127.0.0.1:<port>`) |
| `--bind <ip>` | `127.0.0.1` | Bind IP. Allows `127.0.0.1`, `::1`, `localhost`. Non-loopback requires `--allow-non-loopback`. |
| `--path <path>` | `/funnelctl/<random>` | URL path. Auto-generated 8-char base62 token by default. |
| `--https-port <port>` | `443` | Public HTTPS port. Must be 443, 8443, or 10000. |
| `--ttl <duration>` | none | Keep tunnel up for duration, then tear down. Minimum 30 seconds. |
| `--force` | false | Allow overwriting conflicting serve routes. |
| `--json` | false | NDJSON output for scripting. |
| `--socket <path>` | auto-detect | Unix socket override (Linux/Unix). |
| `--localapi-port <port>` | none | LocalAPI TCP port (macOS/Windows). |
| `--localapi-password-file <path>` | none | File containing LocalAPI password. Must have 0600 permissions. |

**Path validation rules:**
- Must start with `/`
- No `..` segments
- No control characters (0x00-0x1F)
- Double slashes normalized (`//foo` → `/foo`)
- Trailing slash preserved
- Warning if path < 8 characters (guessable)

**Output (human)**

```
https://node.tailnet.ts.net/funnelctl/a7Xk9mPq
├─ Local:   http://127.0.0.1:8081
├─ Expires: never (Ctrl-C to stop)
└─ Press Ctrl-C to stop
```

**Output (JSON/NDJSON)**

Events emitted:

| Event | When | Fields |
|-------|------|--------|
| `started` | Tunnel created | `version`, `url`, `local_target`, `path`, `https_port`, `started_at`, `expires_at` |
| `expiring_soon` | 60s before TTL (Phase 2) | `version`, `seconds_remaining` |
| `stopped` | Tunnel torn down | `version`, `reason`, `stopped_at`, `duration_seconds` |
| `error` | Fatal error | `version`, `code`, `message`, `suggestion` |

```json
{"version":1,"event":"started","url":"https://node.tailnet.ts.net/funnelctl/a7Xk9mPq","local_target":"http://127.0.0.1:8081","path":"/funnelctl/a7Xk9mPq","https_port":443,"started_at":"2026-01-08T12:00:00Z","expires_at":null}
{"version":1,"event":"stopped","reason":"user_interrupt","stopped_at":"2026-01-08T12:30:00Z","duration_seconds":1800}
```

### Command: `funnelctl close` (alias: `c`)

Tears down the route.

- MVP: only supports `close` for a currently-running foreground session (Ctrl-C).
- Phase 2: `funnelctl close <lease-name|id>` reads stored lease state and restores previous config.

### Command: `funnelctl status` (alias: `s`)

Shows current lease(s) and/or current ServeConfig (Phase 2).

### Command: `funnelctl doctor` (alias: `doc`)

Checks prerequisites and reports all results (does not fail-fast).

**Checks performed:**

| Check | Pass | Fail |
|-------|------|------|
| tailscaled reachable | Socket exists and responds | "tailscaled not running" |
| tailscaled version | >= 1.50.0 | "tailscaled too old (got X, need 1.50.0+)" |
| LocalAPI auth (TCP mode) | Password accepted | "Invalid LocalAPI password" |
| Permissions | Can read/write ServeConfig | "Permission denied — need root or operator group" |
| HTTPS enabled | Node has HTTPS cert | "HTTPS not enabled. Run `tailscale cert`" |
| Funnel capability | Tailnet allows Funnel | "Funnel not enabled in tailnet policy" |
| DNS name available | Node has public DNS name | "Node not yet assigned DNS name" |

**Exit code**: Returns the most severe failure code based on fix-order (tailscaled unreachable = 10, highest severity).

### Command: `funnelctl completions <shell>`

Generates shell completions for bash, zsh, or fish.

```bash
funnelctl completions bash >> ~/.bashrc
funnelctl completions zsh >> ~/.zshrc
funnelctl completions fish > ~/.config/fish/completions/funnelctl.fish
```

### Help text

- `-h`: Brief help
- `--help`: Detailed help with examples

```
EXAMPLES:
    funnelctl open 8081                    # Quick tunnel with random path
    funnelctl open 8081 --path /webhook    # Custom path
    funnelctl open 8081 --ttl 30m          # Auto-expire after 30 minutes
```

---

## 6. Architecture

### 6.1 High-level module layout

```
src/
├── cmd/
│   ├── open.rs
│   ├── close.rs
│   ├── status.rs
│   ├── doctor.rs
│   └── completions.rs
├── backend/
│   ├── mod.rs          # trait definitions
│   ├── localapi/       # Option B implementation
│   └── mock/           # for tests
├── core/
│   ├── lease.rs        # lease model and persistence
│   ├── spec.rs         # high-level TunnelSpec
│   └── patch.rs        # merge/patch logic
├── net/
│   └── localapi_transport.rs  # unix socket + tcp-with-password HTTP client
├── error.rs            # typed errors, exit codes
├── dirs.rs             # XDG directory handling
└── main.rs
```

### 6.2 Backend abstraction

Define a stable internal API so future backends can be added without changing CLI semantics.

```rust
pub struct TunnelSpec {
    pub local_target: LocalTarget,   // e.g. http://127.0.0.1:8081
    pub https_port: u16,             // 443, 8443, or 10000
    pub path: String,                // /funnelctl/<token>
    pub funnel: bool,                // true for public
}

pub struct TunnelResult {
    pub url: url::Url,
    pub lease_id: String,
    pub applied_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[async_trait::async_trait]
pub trait Backend {
    async fn apply(&self, spec: &TunnelSpec) -> Result<TunnelResult, BackendError>;
    async fn remove(&self, lease_id: &str) -> Result<(), BackendError>;
    async fn status(&self) -> Result<BackendStatus, BackendError>;
}
```

### 6.3 Lease model

A **lease** represents "what we created" and "how to undo it".

Minimal lease data:

- `lease_id` (uuid or ULID)
- `created_at`, `expires_at`
- `tunnel_spec`
- `backend_kind` + backend-specific connection config (non-secret)
- `previous_state` (snapshot or patch inverse) for Phase 2

MVP uses foreground config (automatic cleanup via WatchIPNBus), avoiding need for persistent leases.

Phase 2 adds lease persistence for detached mode.

### 6.4 XDG Directory Compliance

Full XDG Base Directory Specification compliance:

| Purpose | Linux | macOS (if XDG unset) |
|---------|-------|----------------------|
| Config | `$XDG_CONFIG_HOME/funnelctl/` | `~/Library/Application Support/funnelctl/` |
| State/Leases | `$XDG_STATE_HOME/funnelctl/` | `~/Library/Application Support/funnelctl/` |
| Lock file | `$XDG_RUNTIME_DIR/funnelctl.lock` (fallback: `$XDG_STATE_HOME`) | `~/Library/Application Support/funnelctl/` |
| Cache | `$XDG_CACHE_HOME/funnelctl/` | `~/Library/Caches/funnelctl/` |

If XDG variables are set on macOS, use XDG paths.

---

## 7. LocalAPI backend (Option B)

### 7.1 Connectivity modes

LocalAPI is an HTTP API exposed by tailscaled. Implementation must support:

- **Unix socket** (Linux/Unix): HTTP over `unix://<path>`
- **Localhost TCP + password** (macOS/Windows in some modes): HTTP over `http://127.0.0.1:<port>` with Basic auth + special header.

Common socket paths:
- `/var/run/tailscale/tailscaled.sock`
- `/run/tailscale/tailscaled.sock`

Design: provide a `LocalApiTransport` abstraction:

- `UnixSocketTransport { socket_path }`
- `TcpAuthTransport { host: 127.0.0.1, port, password }`

### 7.2 Authentication & headers (TCP mode)

For TCP-with-password mode:

- Use **HTTP Basic** auth with username empty and password set.
- Add `Sec-Tailscale: localapi` header (mirrors existing clients).
- **Never log the password.**
- Prefer `--localapi-password-file` over CLI password argument.

**Password file handling:**
- Validate file has 0600 permissions at startup; refuse if too permissive.
- Error immediately if file is empty.
- Re-read file and retry once on authentication failure (handles password rotation).

### 7.3 LocalAPI endpoints used

LocalAPI is not formally documented and may change; therefore:

- Implement endpoint constants in one module.
- Implement **capability probing** by attempting endpoints and handling 404/400.

MVP endpoints:

| Purpose | Endpoint |
|---------|----------|
| Node identity/DNS name | `GET /localapi/v0/status` |
| Watch IPN bus (session ID) | `GET /localapi/v0/watch-ipn-bus` |
| Get ServeConfig | `GET /localapi/v0/serve-config` |
| Set ServeConfig | `POST /localapi/v0/serve-config` |

Use ETag header for optimistic concurrency control.

### 7.4 Version requirements

**Minimum supported version: tailscaled 1.50.0**

Version is checked at startup. Incompatible versions receive a hard error:

```
Error: tailscaled version 1.48.0 is not supported
Cause: funnelctl requires tailscaled 1.50.0 or later for foreground config support
Fix:   Upgrade tailscaled. See https://tailscale.com/download
```

### 7.5 Applying a tunnel

Algorithm (safe-by-default):

1. **Establish WatchIPNBus session**
   - Connect to `/localapi/v0/watch-ipn-bus`
   - Receive session ID for foreground config

2. **Verify target port is accessible**
   - TCP connect to target with 2s timeout
   - Fail if nothing listening

3. **Discover node public name**
   - Call `/localapi/v0/status`
   - Extract node FQDN: `<node>.<tailnet>.ts.net`

4. **Fetch current ServeConfig**
   - If empty/null, treat as empty config.
   - Preserve unknown fields (use `serde_json::Value` for round-trip safety).

5. **Validate conflicts**
   - Check if `(https_port, path)` conflicts with existing routes.
   - Conflict detection includes:
     - Exact path match (different target = conflict unless `--force`)
     - Prefix overlap: existing `/foo/` blocks our `/foo/bar`
     - Prefix overlap: our `/foo/` would capture existing `/foo/bar`
   - Identical mapping with funnel enabled = idempotent success.

6. **Compute patch**
   - Add mapping to `Foreground[session_id]`:
     - Frontend: `https://<fqdn>:<https_port><path>`
     - Backend: `http://127.0.0.1:<port>`
   - Enable Funnel for that mapping in AllowFunnel.

7. **Write updated ServeConfig**
   - Use ETag for optimistic concurrency.

8. **Return URL**

### 7.6 Removing a tunnel

For MVP foreground sessions:
- Simply close the WatchIPNBus connection.
- tailscaled automatically removes the foreground config.

For Phase 2 detached sessions:
- Use patch inverse to remove only the route we added.
- If schema differences make patching unsafe, use snapshot restore with conflict check.

---

## 8. Short-lived sessions

### TTL behavior

| Aspect | Behavior |
|--------|----------|
| Minimum TTL | 30 seconds. Error if less. |
| Short TTL warning | Warn if < 5 minutes: "Short TTL — tunnel expires quickly." |
| Duration type | **Monotonic** (actual runtime). Pauses during system sleep. |
| Expiry | Immediate teardown with message: "TTL expired (30m). Tearing down tunnel." |
| Warning before expiry | Phase 2: "Tunnel expires in 60 seconds." |

### Foreground session (MVP)

`funnelctl open` runs in the foreground:

1. Establish WatchIPNBus session (get session ID)
2. Apply config to `Foreground[session_id]`
3. Print URL
4. Wait for:
   - Ctrl-C / SIGINT / SIGTERM
   - TTL expiry (monotonic timer)
5. On exit: close WatchIPNBus (tailscaled auto-cleans), then exit

**Signal handling:**
- First Ctrl-C: graceful shutdown, remove route
- Second Ctrl-C: abort cleanup, exit immediately (risk: orphaned route, but tailscaled should still clean up foreground config)

**Cleanup failure:**
If LocalAPI is unreachable during teardown:
```
Error: Failed to tear down tunnel
Cause: LocalAPI unreachable (tailscaled may have stopped)
Fix:   Route may still exist. Run `tailscale serve off` to clean up.
```

### Detached sessions (Phase 2)

`funnelctl open --detach`:

- Use background config (not foreground)
- Persist lease to disk (`$XDG_STATE_HOME/funnelctl/leases/`)
- Exit immediately
- `funnelctl close <lease>` tears down later

Lease storage uses file locking to avoid concurrent modifications.

### Orphan recovery (Phase 2)

On startup, scan lease directory for incomplete leases (no clean exit marker). Attempt cleanup before proceeding.

Optional `doctor --cleanup-orphans`:
```bash
$ funnelctl doctor --cleanup-orphans
Found 2 orphaned leases:
  - /funnelctl/abc123 (created 2024-01-08 10:00)
  - /funnelctl/xyz789 (created 2024-01-07 15:30)
Clean up? [y/N]: y
Removed 2 orphaned routes.
```

---

## 9. Concurrency and locking

### Concurrent instance handling

| Scenario | Behavior |
|----------|----------|
| Same path, different terminals | Second fails: "Path already in use by another funnelctl session" |
| Different paths | Works fine (independent routes) |

### Lock mechanism

Local file lock with stale PID detection:

1. Lock file: `$XDG_RUNTIME_DIR/funnelctl.lock` (or `$XDG_STATE_HOME`)
2. Contains PID of holding process
3. On startup:
   - Try to acquire lock (OS-level advisory lock via `flock`)
   - If locked, check if PID is alive
   - Dead PID = stale lock, take over
   - Alive PID = "Another funnelctl instance is running (PID 12345)"
4. Lock auto-releases on process exit (including crash)

ETag used as defense-in-depth for SetServeConfig calls.

---

## 10. Security considerations

### 10.1 Funnel is public

Funnel exposes the endpoint to the entire internet. Therefore:

- Default to an **unguessable path** (`/funnelctl/<8-char-base62-token>`).
- Warn if user uses short path (< 8 chars): "Short path '/hook' is guessable. Consider a longer path or use default random path."
- Encourage webhook signature validation in the downstream app.

### 10.2 Identity headers

When serving to tailnet only (Serve), Tailscale can add identity headers to backend requests. For Funnel/public traffic, those headers are not present. Your backend must not rely on Tailscale identity for public traffic.

### 10.3 LocalAPI is privileged

LocalAPI can change Tailscale settings. Protect access by:

- Not printing secrets.
- Using least privilege where possible.
- On Linux: require root or operator permissions (documented in `doctor`).

### 10.4 Password file security

- `--localapi-password-file` must have 0600 permissions; refuse otherwise.
- Never log password content.
- Empty password file = immediate error.

### 10.5 Data at rest (Phase 2)

If persisting leases:

- Treat ServeConfig snapshots as sensitive (may include internal routes).
- Store with 0600 permissions.
- Consider optional OS keychain integration for LocalAPI password.

---

## 11. Error handling & exit codes

### Exit codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | Generic failure |
| `2` | Invalid CLI usage |
| `10` | LocalAPI unreachable / tailscaled not running |
| `11` | Permission denied (socket access / auth) |
| `12` | Funnel/serve prerequisites unmet |
| `13` | Conflicting existing config |
| `14` | Apply/remove failed (tailscaled error) |
| `15` | Target port not accessible |
| `16` | Tailscaled version too old |

### Error message format

All errors follow consistent structure with colors (when TTY detected):

```
Error: <what failed>
Cause: <why it failed>
Fix:   <how to fix it>
```

Examples:

```
Error: LocalAPI unreachable
Cause: Socket /var/run/tailscale/tailscaled.sock not found
Fix:   Is tailscaled running? Try: sudo systemctl start tailscaled

Error: Port 8081 not accessible
Cause: Connection refused
Fix:   Start your service on port 8081 before running funnelctl

Error: Path conflict
Cause: /webhook already serves http://127.0.0.1:3000
Fix:   Use a different --path or add --force to override
```

---

## 12. Observability

### Logging

- Use `tracing` for structured logs.
- **Default: silent** (errors only to stderr).
- Enable debug output via `RUST_LOG=debug` or `-v` flag.
- Debug output goes to stderr.

### Sensitive data redaction

Never log:
- LocalAPI password
- Authorization headers
- Full ServeConfig snapshots (unless `RUST_LOG=trace`)

### User feedback during session

MVP: URL + "Press Ctrl-C to stop", silent until exit.

Phase 2: `--verbose` flag for request logging:
```
[12:34:56] GET /webhook 200 OK 43ms 1.2KB
```

---

## 13. Testing strategy

### Unit tests

- Patch/merge logic given mock ServeConfig JSON.
- Duration parsing (TTL).
- Path validation.
- Conflict detection.
- Lease serialization.

### Integration tests (opt-in)

- Requires tailscaled running.
- Mark with `#[ignore]` and provide `make test-integration`.
- Run manually before release (not in CI).
- Validate:
  - open creates route
  - URL is reachable locally and (optionally) from external probe
  - teardown cleans up foreground config

### Mock backend

- Implements Backend trait in-memory to test CLI behavior without Tailscale.

### CI pipeline

```
┌─────────────┐   ┌─────────────┐
│   fmt       │   │   clippy    │
└──────┬──────┘   └──────┬──────┘
       │                 │
       └────────┬────────┘
                ▼
         ┌──────────────┐
         │    test      │
         └──────────────┘

(parallel, independent jobs)
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│ build-linux-x64 │  │ build-linux-arm │  │ build-macos-x64 │  │ build-macos-arm │
└─────────────────┘  └─────────────────┘  └─────────────────┘  └─────────────────┘
```

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`
- Build all platform binaries (static, musl for Linux)

---

## 14. Versioning and compatibility

### LocalAPI compatibility

- LocalAPI is not guaranteed stable; implement:
  - endpoint probing
  - tolerant JSON parsing (`serde_json::Value` for unknown fields)
  - preserve unknown fields on round-trip
- Minimum supported Tailscale version: **1.50.0**
- Hard error with upgrade instructions for older versions.

### funnelctl versioning

- **SemVer** (Semantic Versioning)
- Start at **0.1.0**
- Bump to 1.0.0 when Phase 1 MVP is stable

---

## 15. Distribution

### Build targets (MVP)

| Platform | Architecture |
|----------|--------------|
| Linux | x86_64 (musl, static) |
| Linux | aarch64 (musl, static) |
| macOS | x86_64 |
| macOS | aarch64 (Apple Silicon) |

Windows: Phase 2

### Distribution method

- GitHub releases with prebuilt binaries
- `install.sh` script for easy installation

```bash
curl -fsSL https://raw.githubusercontent.com/<org>/funnelctl/main/install.sh | sh
```

---

## 16. Future roadmap

### Phase 1 (MVP)

- `open` (foreground), TTL, unix socket LocalAPI
- Foreground config via WatchIPNBus (auto-cleanup)
- Conflict detection (exact + prefix)
- Port liveness check
- `doctor` command
- Shell completions
- Static binaries for Linux + macOS

### Phase 2

- Persisted leases, `close`, `status`
- Detached mode (`--detach`)
- macOS/Windows LocalAPI TCP credential support
- Windows builds
- Unix socket targets (`--target unix:/path/to/sock`)
- Configuration file
- `--verbose` request logging
- Expiry warning (60s before TTL)
- Orphan recovery (`doctor --cleanup-orphans`)
- Opt-in telemetry experiment

### Phase 3

- Additional backends (optional):
  - Control-plane API assisted configuration (if/when supported)
  - "tailscale CLI wrapper" backend (explicitly optional; not default)

### Phase 4

- Webhook-provider helpers (display expected URL patterns, quick test endpoint, etc.)

---

## 17. Port and target validation

### Target port validation

| Check | Behavior |
|-------|----------|
| Range | Must be 1-65535; error otherwise |
| Liveness | TCP connect with 2s timeout; error if connection refused |

### HTTPS port validation

Only allowed values: **443**, **8443**, **10000** (Tailscale Funnel restriction).

### Bind address validation

| Address | Allowed |
|---------|---------|
| `127.0.0.1` | Yes (default) |
| `::1` | Yes (IPv6 loopback) |
| `localhost` | Yes (resolved at startup) |
| Other | Requires `--allow-non-loopback` flag |

---

## 18. References (non-normative)

The following were useful for understanding LocalAPI patterns and Serve/Funnel behavior:

- Tailscale source code (`ipn/serve.go`, `ipn/ipnlocal/serve.go`)
- LocalAPI examples and notes (Unix socket, whois): community writeups.
- Existing Rust LocalAPI clients demonstrating Unix socket + TCP-with-password transports.
- Tailscale documentation for Serve and Funnel concepts and limitations.

### Key findings from source

**ServeConfig structure:**
```go
type ServeConfig struct {
    TCP        map[uint16]*TCPPortHandler
    Web        map[HostPort]*WebServerConfig  // "host:port" -> config
    AllowFunnel map[HostPort]bool
    Foreground map[string]*ServeConfig        // session_id -> ephemeral config
}

type WebServerConfig struct {
    Handlers map[string]*HTTPHandler  // path -> handler
}
```

**Path matching:**
- Exact path strings as map keys
- Trailing slash (`/foo/`) indicates prefix matching
- Longest prefix wins
- No wildcard support

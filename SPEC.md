# SPEC: Rust CLI for short‑lived public dev tunnels via Tailscale Funnel (LocalAPI backend)

## 1. Purpose

Build a small Rust CLI that provides **short‑lived, public HTTPS access** to a developer’s **local service (e.g., `localhost:8081`)** using **Tailscale Funnel**, as an alternative to tools like ngrok.

Key constraint: **Do not shell out to the `tailscale` CLI** from the Rust project.  
Instead, implement **Backend Option B**: talk directly to **`tailscaled` LocalAPI** to read/update Serve/Funnel configuration.

The user is the **sole administrator** of their tailnet/network.

---

## 2. Scope

### In scope (MVP)

- A Rust CLI that:
  - Enables (and later disables) **Funnel** for a local HTTP service.
  - Supports “ngrok-like” **foreground sessions**: keep running until Ctrl‑C (or TTL), then tear down.
  - Prints the **public URL** (HTTPS) that can be pasted into a third‑party webhook provider.
- Backend abstraction with **LocalAPI backend implemented first**.
- Linux/Unix socket LocalAPI support first (most deterministic); macOS/Windows support designed but may be phased.

### Out of scope (MVP)

- Managing tailnet policy/ACLs automatically.
- Creating or modifying Tailscale admin settings via the control-plane API.
- Exposing arbitrary TCP/UDP services (initially focus on HTTP/HTTPS proxying to `127.0.0.1:<port>`).
- Long‑running daemon/service installation.

---

## 3. Terminology

- **tailnet**: Your Tailscale network.
- **tailscaled**: Local Tailscale daemon.
- **LocalAPI**: HTTP API exposed by tailscaled locally (Unix socket or localhost TCP depending on platform/build).
- **Serve**: Tailscale feature that maps an HTTPS endpoint on your node to a local service.
- **Funnel**: Serve routes exposed to the **public internet** (not just tailnet).
- **ServeConfig**: tailscaled’s persisted config describing Serve/Funnel listeners and routes.

---

## 4. Product goals and UX principles

### Primary goals

1. **One command** to create a public, short‑lived URL for a local port.
2. **Safe defaults** (loopback targets; random path token by default).
3. **Non-destructive**: do not unexpectedly break existing Serve/Funnel config.

### UX principles

- Print a single, copy‑pasteable URL.
- Make teardown predictable (Ctrl‑C always tears down what we created).
- Fail with actionable errors (missing permissions, HTTPS not enabled, funnel disabled, etc.).

---

## 5. CLI interface

### Command: `funnelctl open`

Creates a Funnel route and prints the public URL.

**Examples**

- `funnelctl open 8081`
- `funnelctl open 8081 --path /webhook`
- `funnelctl open 8081 --ttl 30m`
- `funnelctl open 8081 --bind 127.0.0.1 --path /hook --foreground`
- `funnelctl open 8081 --detach --lease-name myhook` (phase 2)

**Flags**

- Positional `<port>`: local port on loopback (default target `http://127.0.0.1:<port>`).
- `--bind <ip>`: default `127.0.0.1`. (MVP should refuse non-loopback unless `--allow-non-loopback`.)
- `--path <path>`: default **auto-generated** like `/funnelctl/<random>` to reduce accidental exposure.
- `--https-port <port>`: default `443` (front door). Advanced.
- `--ttl <duration>`: keep tunnel up for duration, then tear down and exit.
- `--foreground/--no-foreground`: default **foreground** if `--ttl` absent; if TTL present, also foreground.
- `--detach`: create route and exit immediately (requires a persisted lease). Phase 2.
- `--force`: allow overwriting/conflicting serve routes.
- LocalAPI connectivity:
  - `--socket <path>`: Unix socket override (Linux/Unix).
  - `--localapi-port <port>` and `--localapi-password <value>`: for macOS/Windows sandboxed mode.
  - `--localapi-password-file <path>`: safer than passing password on CLI.
- Output:
  - `--json`: machine-readable output.

**Output (human)**

- Prints:
  - URL
  - Expiry (if TTL)
  - Local target
  - How to stop (Ctrl‑C or `funnelctl close <lease>` in phase 2)

**Output (JSON)**

```json
{
  "url": "https://node.tailnet.ts.net/funnelctl/abcd1234",
  "expires_at": "2026-01-08T12:34:56Z",
  "local_target": "http://127.0.0.1:8081",
  "https_port": 443,
  "path": "/funnelctl/abcd1234",
  "backend": "localapi"
}
```

### Command: `funnelctl close`

Tears down the route.

- MVP: only supports `close` for a currently-running foreground session (Ctrl‑C).
- Phase 2: `funnelctl close <lease-name|id>` reads stored lease state and restores previous config.

### Command: `funnelctl status`

Shows current lease(s) and/or current ServeConfig (phase 2).

### Command: `funnelctl doctor`

Checks prerequisites (tailscaled reachable, permissions, Serve/Funnel enabled, HTTPS enabled).

---

## 6. Architecture

### 6.1 High-level module layout

- `cmd/`
  - `open.rs`, `close.rs`, `status.rs`, `doctor.rs`
- `backend/`
  - `mod.rs` (trait definitions)
  - `localapi/` (Option B implementation)
  - `mock/` (for tests)
- `core/`
  - `lease.rs` (lease model and persistence)
  - `spec.rs` (high-level TunnelSpec)
  - `patch.rs` (merge/patch logic)
- `net/`
  - `localapi_transport.rs` (unix socket + tcp-with-password HTTP client)
- `error.rs`
  - typed errors, exit codes
- `main.rs`

### 6.2 Backend abstraction

Define a stable internal API so future backends can be added without changing CLI semantics.

```rust
pub struct TunnelSpec {
    pub local_target: LocalTarget,   // e.g. http://127.0.0.1:8081
    pub https_port: u16,             // usually 443
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

A **lease** represents “what we created” and “how to undo it”.

Minimal lease data:

- `lease_id` (uuid or ULID)
- `created_at`, `expires_at`
- `tunnel_spec`
- `backend_kind` + backend-specific connection config (non-secret)
- `previous_state` (snapshot or patch inverse) **encrypted or at least protected** (see security section)

MVP can avoid persistent leases by only supporting foreground sessions. Phase 2 adds persistence.

---

## 7. LocalAPI backend (Option B)

### 7.1 Connectivity modes

LocalAPI is an HTTP API exposed by tailscaled. Implementation must support:

- **Unix socket** (Linux/Unix): HTTP over `unix://<path>`
- **Localhost TCP + password** (macOS/Windows in some modes): HTTP over `http://127.0.0.1:<port>` with Basic auth + special header.

Notes from existing community implementations and examples:

- Requests commonly use host header `local-tailscaled.sock`.
- Unix socket examples often reference paths like:
  - `/var/run/tailscale/tailscaled.sock`
  - `/run/tailscale/tailscaled.sock`

Design: provide a `LocalApiTransport` abstraction:

- `UnixSocketTransport { socket_path }`
- `TcpAuthTransport { host: 127.0.0.1, port, password }`

### 7.2 Authentication & headers (TCP mode)

For TCP-with-password mode:

- Use **HTTP Basic** auth with username empty and password set.
- Add `Sec-Tailscale: localapi` header (mirrors existing clients).

Never log the password. Prefer `--localapi-password-file`.

### 7.3 LocalAPI endpoints used

LocalAPI is not formally documented and may change; therefore:

- Implement endpoint constants in one module.
- Implement **capability probing** by attempting endpoints and handling 404/400.

MVP must at minimum:

- Fetch node identity and DNS name:
  - `GET /localapi/v0/status` (returns JSON status)
- Read and write ServeConfig / Serve routes:
  - Preferred: `GET /localapi/v0/serve` (or similar) to fetch serve config
  - Preferred: `POST /localapi/v0/serve` with JSON payload to set/replace
  - Reset: `POST /localapi/v0/serve/reset` (or equivalent)

Because exact paths can differ by version, implement probes in order:

1. Try modern `/localapi/v0/serve` (+ subpaths like `/reset`)
2. Fall back to older `/localapi/v0/serve-config` style if present

### 7.4 Applying a tunnel

Algorithm (safe-by-default):

1. **Discover node public name**
   - Call `/localapi/v0/status`
   - Extract node FQDN used for HTTPS endpoints: `<node>.<tailnet>.ts.net`
2. **Fetch current ServeConfig**
   - If empty, treat as empty config.
3. **Validate conflicts**
   - If requested `(https_port, path)` already used:
     - If identical mapping and funnel already enabled: idempotent success.
     - Else: fail unless `--force`.
4. **Compute patch**
   - Add/replace mapping:
     - Frontend: `https://<fqdn>:<https_port><path>`
     - Backend: `http://127.0.0.1:<port>`
   - Enable Funnel for that mapping.
5. **Write updated ServeConfig**
6. **Return URL**

### 7.5 Removing a tunnel

Preferred removal strategies:

- **Patch inverse**: remove only the route we added.
- If we cannot reliably isolate our route (e.g., unknown schema), use snapshot restore:
  - Store full previous config in lease.
  - Restore it on teardown.

Conflict policy:

- If restoring snapshot would remove newer unrelated changes, refuse unless `--force-restore`.

MVP (foreground only) may use snapshot restore in-memory.

---

## 8. Short‑lived sessions

### Foreground session (MVP)

`funnelctl open` runs in the foreground:

- Apply config
- Print URL
- Wait for:
  - Ctrl‑C / SIGINT / SIGTERM
  - TTL expiry
- On exit: remove route (best-effort), then exit with appropriate code.

### Detached sessions (Phase 2)

`funnelctl open --detach`:

- Persist lease to disk
- Exit immediately
- `funnelctl close <lease>` tears down later

Lease storage:

- Default: `~/.config/funnelctl/leases/` (Linux), platform-appropriate directories elsewhere.
- Use file locking to avoid concurrent modifications.

---

## 9. Security considerations

### 9.1 Funnel is public

Funnel exposes the endpoint to the entire internet. Therefore:

- Default to an **unguessable path** (`/funnelctl/<token>`).
- Warn if user uses `/` or short path without `--i-know-what-im-doing`.
- Encourage webhook signature validation in the downstream app.

### 9.2 Identity headers

When serving to tailnet only (Serve), Tailscale can add identity headers to backend requests. For Funnel/public traffic, those headers are not present. Your backend must not rely on Tailscale identity for public traffic.

### 9.3 LocalAPI is privileged

LocalAPI can change Tailscale settings. Protect access by:

- Not printing secrets.
- Using least privilege where possible.
- On Linux: require root or operator permissions (documented in `doctor`).

### 9.4 Data at rest

If persisting leases:

- Treat ServeConfig snapshots as sensitive (may include internal routes).
- Store with 0600 perms.
- Consider optional OS keychain integration for LocalAPI password.

---

## 10. Error handling & exit codes

Define consistent error categories:

- `1`: generic failure
- `2`: invalid CLI usage
- `10`: LocalAPI unreachable / tailscaled not running
- `11`: permission denied (socket access / auth)
- `12`: funnel/serve prerequisites unmet
- `13`: conflicting existing config
- `14`: apply/remove failed (tailscaled error)

All errors must print:

- What failed
- Which step
- Suggested remediation

---

## 11. Observability

- Use `tracing` for structured logs.
- Default to minimal output; enable `RUST_LOG` for details.
- Never log secrets or full headers.

---

## 12. Testing strategy

### Unit tests

- Patch/merge logic given mock ServeConfig JSON.
- Duration parsing (TTL).
- Lease serialization.

### Integration tests (opt-in)

- Requires tailscaled running.
- Mark with `#[ignore]` and provide `make test-integration`.
- Validate:
  - open creates route
  - URL is reachable locally and (optionally) from external probe
  - teardown restores prior config

### Mock backend

- Implements Backend trait in-memory to test CLI behavior without Tailscale.

---

## 13. Versioning and compatibility

- LocalAPI is not guaranteed stable; implement:
  - endpoint probing
  - tolerant JSON parsing (serde `Value` where needed)
- Minimum supported Tailscale version should be stated and checked (doctor):
  - Prefer a recent stable (because Serve/Funnel behavior and LocalAPI endpoints have changed historically).

---

## 14. Future roadmap

1. Phase 1 (MVP)
   - `open` (foreground), TTL, unix socket LocalAPI
   - basic conflict detection
2. Phase 2
   - persisted leases, `close`, `status`
   - macOS/Windows LocalAPI credential support
3. Phase 3
   - Additional backends (optional):
     - Control-plane API assisted configuration (if/when supported)
     - “tailscale CLI wrapper” backend (explicitly optional; not default)
4. Phase 4
   - Webhook-provider helpers (display expected URL patterns, quick test endpoint, etc.)

---

## 15. References (non-normative)

The following were useful for understanding LocalAPI patterns and Serve/Funnel behavior:

- LocalAPI examples and notes (Unix socket, whois): community writeups.
- Existing Rust LocalAPI clients demonstrating Unix socket + TCP-with-password transports.
- Tailscale documentation for Serve and Funnel concepts and limitations.

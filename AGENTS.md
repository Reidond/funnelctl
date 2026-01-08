# AGENTS: Engineering & code quality guidelines for this repo

This file defines expectations for any human or automated agent making changes to this repository.

## 1. North-star outcome

Deliver a small, reliable CLI that:

- Creates a public HTTPS tunnel to a local port via Tailscale Funnel
- Does not invoke the `tailscale` CLI binary
- Is safe by default (unguessable path, loopback targets)
- Cleans up after itself on Ctrl‑C / TTL expiry

## 2. Project constraints

- Do not shell out to external processes to manage Tailscale (no `tailscale ...`).
- Prefer capability probing over assuming LocalAPI stability.
- Avoid platform-specific hacks unless isolated behind a transport abstraction and documented.

## 3. Repository standards

### 3.1 Rust edition and toolchain

- Use the latest stable Rust edition in `Cargo.toml` (prefer 2021 unless the project opts into 2024).
- Enforce formatting with `rustfmt`.
- Enforce linting with `clippy` (deny warnings in CI).

### 3.2 Error handling

- No `unwrap()`/`expect()` in non-test code.
- Ensure every error is processed and logged and does not case a panic.
- Use:
  - `thiserror` for typed internal errors
  - `anyhow` only at the binary boundary (`main`) to keep call sites ergonomic
- Ensure errors are actionable (include next steps).

### 3.3 Logging

- Use `tracing` (not `println!`) for internal logs.
- Default CLI output should be clean and user-focused.
- Never log:
  - LocalAPI password
  - Authorization headers
  - Full ServeConfig snapshots unless explicitly requested with a debug flag

### 3.4 Security

- Default local bind target must be loopback (`127.0.0.1` / `::1`).
- Default path must be randomly generated.
- Persisted files must be created with restrictive permissions (0600 for secrets; 0700 dirs).
- If implementing `--localapi-password`, strongly recommend `--localapi-password-file` and redact CLI help output accordingly.

### 3.5 Backward/forward compatibility

- LocalAPI endpoints may change. Implement:
  - probing
  - tolerant JSON parsing
  - clear “unsupported version” errors with guidance

## 4. Implementation approach for Option B (LocalAPI)

### 4.1 Transport abstraction

Implement a transport layer that can issue HTTP requests via:

- Unix domain socket (Linux/Unix)
- TCP with Basic auth + `Sec-Tailscale: localapi` header (macOS/Windows sandboxed)

Keep transport code in `net/` and avoid leaking it into business logic.

### 4.2 Backend boundary

All Tailscale-specific logic belongs behind `backend::Backend`.
CLI modules should only speak in terms of `TunnelSpec`, `TunnelResult`, and `Lease`.

### 4.3 Patch strategy

Prefer “patch/inverse patch” over “full snapshot restore”, but:

- If schema differences make patching unsafe, snapshot restore is acceptable provided:
  - it is only used for foreground sessions initially
  - it is guarded by explicit conflict checks in persisted lease mode

## 5. Testing requirements

### 5.1 Minimum

Every change must keep these passing:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features`
- `cargo test`

### 5.2 Integration tests

If adding integration tests requiring tailscaled:

- Mark them `#[ignore]`
- Provide a `make test-integration` (or equivalent) that documents prerequisites

## 6. Documentation requirements

- Any user-visible behavior changes must update:
  - `SPEC.md` (design intent and behavior)
  - CLI `--help` text
- Document all flags, defaults, and safety checks.

## 7. Pull request checklist (for agents)

Before submitting a change:

1. Verify commands and examples in `SPEC.md` still match actual CLI behavior.
2. Ensure no secrets are logged or stored insecurely.
3. Confirm Ctrl‑C teardown works (unit tests and/or manual test plan).
4. Add or update tests for patch logic and TTL parsing.
5. Keep diffs small and focused; separate refactors from behavior changes.

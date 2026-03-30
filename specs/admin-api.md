# Admin API Specification

This document specifies the Admin API contract for the API Gateway v1. The Admin API runs on a separate port (default `:9090`) and is NOT exposed to clients; it is only for operational and monitoring purposes.

## Overview

The Admin API provides:

1. **Health checks** — Is the gateway alive?
2. **Readiness checks** — Is the gateway ready to serve traffic?
3. **Config status** — What config is active? When was it last reloaded?
4. **Config reload trigger** — Reload configuration from disk

All responses are JSON.

---

## Endpoints

### `GET /healthz`

**Live Check:** Is the gateway process alive?

**Response:** 200 OK

```json
{
  "ok": true
}
```

**Usage:** Use this for Kubernetes liveness probes, Docker health checks, or uptime monitoring.

**Behavior:**
- Returns 200 immediately if the process is running
- No expensive checks
- Fails only if the process crashes

---

### `GET /readyz`

**Ready Check:** Is the gateway ready to serve traffic?

**Response:** 200 OK (if ready) or 503 Service Unavailable (if not ready)

```json
{
  "ok": true,
  "config_loaded": true,
  "redis_ok": true,
  "jwks_ok": true,
  "last_config_reload_unix": 1760000000
}
```

**Fields:**

| Field | Type | Description |
| :---- | :---- | :---- |
| `ok` | boolean | `true` if all checks pass; `false` if any check fails |
| `config_loaded` | boolean | `true` if gateway config has been loaded successfully |
| `redis_ok` | boolean | `true` if Redis connectivity is healthy (checked on-demand or cached) |
| `jwks_ok` | boolean | `true` if at least one JWKS provider is reachable (best-effort check) |
| `last_config_reload_unix` | integer | Unix timestamp of the last successful config reload |

**Status Code Logic:**
- **200 OK:** `ok: true` (all checks pass)
- **503 Service Unavailable:** `ok: false` (one or more checks fail)

**Usage:** Use this for Kubernetes readiness probes, load balancer health checks, or to decide whether to route traffic to this gateway instance.

**Readiness Criteria:**
1. Config loaded at least once (required)
2. Redis is reachable (if rate limiting is enabled; can be degraded)
3. At least one JWKS provider is reachable (if auth is used; can be cached)

**Notes:**
- `redis_ok` can be `false` if Redis is unreachable, but degraded mode is active; request processing continues
- `jwks_ok` can be `false` if all JWKS endpoints are stale (cache age > 10 × TTL); protected routes return 503

---

### `GET /config/status`

**Config Status:** What config is currently active?

**Response:** 200 OK

```json
{
  "active_config_version": 1,
  "active_config_sha256": "abc123def456...",
  "last_reload_result": "success",
  "last_reload_error": null,
  "last_reload_unix": 1760000000
}
```

**Fields:**

| Field | Type | Description |
| :---- | :---- | :---- |
| `active_config_version` | integer | Config version from `configs/gateway.yaml` (top-level `version:` field) |
| `active_config_sha256` | string | SHA256 hash of the active config file (for drift detection) |
| `last_reload_result` | string | `success` or `validation_error` |
| `last_reload_error` | string or null | Error message if `last_reload_result: validation_error`; `null` if success |
| `last_reload_unix` | integer | Unix timestamp of the last reload attempt |

**Usage:** Detect config drift, verify active config, troubleshoot reload failures.

**Example (Failure):**

```json
{
  "active_config_version": 1,
  "active_config_sha256": "old_hash...",
  "last_reload_result": "validation_error",
  "last_reload_error": "route 'api' specifies non-existent upstream service 'backend2'",
  "last_reload_unix": 1760000000
}
```

---

### `POST /config/reload`

**Trigger Config Reload:** Reload `configs/gateway.yaml`, validate it, and swap to the new config if valid.

**Request:** No body required.

```
POST /config/reload
```

**Response:** 200 OK (if reload succeeds) or 400 Bad Request (if validation fails)

**Success Response (200 OK):**

```json
{
  "ok": true,
  "message": "config reloaded successfully",
  "previous_sha256": "old_hash...",
  "new_sha256": "new_hash...",
  "reload_timestamp": 1760000000
}
```

**Failure Response (400 Bad Request):**

```json
{
  "ok": false,
  "message": "config validation failed",
  "error": "route 'api' specifies non-existent upstream service 'backend2'",
  "reload_timestamp": 1760000000
}
```

**Behavior:**

1. Read `configs/gateway.yaml` from disk
2. Parse YAML
3. Validate against schema (see [../specs/config-schema.md](../specs/config-schema.md))
4. If validation passes:
   - Generate new Envoy config from the gateway config
   - Swap to the new config (atomically in memory)
   - Restart Envoy container or signal the data plane (implementation-dependent)
   - Return 200 OK with new config hash
5. If validation fails:
   - Keep the old config active
   - Return 400 with error details
   - Log the error

**In-Flight Request Handling:**

- **During swap:** Existing requests continue using the old config
- **New requests:** Use the new config immediately after swap
- **No request drops:** The swap is atomic; no requests are dropped
- **Envoy restart:** May cause brief (< 1 second) connection drains; clients should retry

**Usage:**

```bash
# Validate a new config
curl -X POST http://localhost:9090/config/reload

# Check result
curl http://localhost:9090/config/status
```

---

## Error Responses

All error responses include:

| Field | Type | Description |
| :---- | :---- | :---- |
| `ok` | boolean | `false` (indicates error) |
| `message` | string | Human-readable error summary |
| `error` | string | Detailed error message (if available) |

### Possible Error Codes

| Error | Status | Reason |
| :---- | :---- | :---- |
| `config_not_found` | 400 | `configs/gateway.yaml` does not exist |
| `config_parse_error` | 400 | YAML parsing failed (invalid syntax) |
| `validation_error` | 400 | Config validation failed (schema mismatch, missing fields, invalid values) |
| `io_error` | 500 | Unexpected I/O error reading/writing config |
| `internal_error` | 500 | Unexpected internal error |

---

## Authentication & Security

**Important:** The Admin API is NOT authenticated by default.

- **Deployment Model:** Admin port (`:9090`) must NOT be exposed to the internet
- **Network Policy:** Restrict access to admin port to trusted operators only (firewall, network policies, Kubernetes NetworkPolicy)
- **v1 Scope:** Authentication for admin API is out of scope for v1

---

## Monitoring & Alerting

Recommended metrics:

| Metric | Alert Threshold | Action |
| :---- | :---- | :---- |
| `readyz` returns 503 | Any occurrence | Investigate config, Redis, or JWKS availability |
| `/config/reload` returns 400 | Any occurrence | Review error message; fix config and retry |
| `last_config_reload_unix` is stale | > 1 day old (no reload in 24h) | Normal (unless you expect frequent changes) |
| `last_reload_result: validation_error` | Persistent | Fix config and retry reload |

---

## Testing Checklist

- [ ] `/healthz` returns 200 with `{"ok":true}` when process is running
- [ ] `/readyz` returns 200 when config is loaded and Redis is reachable
- [ ] `/readyz` returns 503 when config is invalid or Redis is unreachable (if required)
- [ ] `/config/status` returns current config version and SHA256
- [ ] `/config/status` returns error details if last reload failed
- [ ] `POST /config/reload` validates config before swapping
- [ ] `POST /config/reload` returns 400 with error details on validation failure
- [ ] After successful reload, new config is active (new routes work, old routes may fail)
- [ ] In-flight requests complete normally during config swap (no drops)
- [ ] Config reload is atomic (no partial updates)
- [ ] Admin API is NOT accessible from the public gateway port

---

## Further Reading

- [base-case-implementation-spec.md](base-case-implementation-spec.md) — Admin API overview
- [config-schema.md](config-schema.md) — Configuration validation rules
- [../../docs/operations/](../../docs/operations/) — Operational runbooks

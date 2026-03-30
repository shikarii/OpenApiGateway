# Configuration Schema Specification

This document defines the canonical YAML config schema for the API Gateway v1. Generator agents must produce configs conforming to this exact schema.

## Config File Location

`configs/gateway.yaml` is the source-of-truth configuration.

## Root-Level Schema

```yaml
version: 1

gateway:
  # Gateway server configuration
  listen_address: "0.0.0.0:8080"
  admin_address: "0.0.0.0:9090"
  request_timeout_ms: 15000
  idle_timeout_ms: 60000
  max_request_body_bytes: 10485760
  trust_forwarded_headers: false

auth:
  providers:
    - name: "main"
      issuer: "https://auth.example.local/"
      audience: "api-gateway"
      jwks_uri: "http://host.docker.internal:7001/.well-known/jwks.json"
      cache_ttl_seconds: 300
      clock_skew_seconds: 30

rate_limits:
  redis_address: "redis:6379"
  redis_db: 0
  redis_key_prefix: "rl"
  default_timeout_ms: 50
  fail_open: false
  survivability_mode:
    enabled: true
    fallback_capacity: 20
    fallback_refill_rate_per_sec: 5

routes:
  - name: "public-echo"
    hostnames: ["localhost"]
    path_prefix: "/public"
    methods: ["GET", "POST"]
    auth_required: false
    rate_limit:
      bucket_capacity: 50
      refill_rate_per_sec: 10
      key_by: "ip"
    upstream:
      service: "echo-public"
      request_timeout_ms: 5000
      retries: 1

services:
  - name: "echo-public"
    endpoints:
      - "echo-backend:8081"
    health_check:
      path: "/healthz"
      interval_ms: 2000
      timeout_ms: 500

observability:
  access_log_json: true
  prometheus_enabled: true
  tracing:
    enabled: false
    otlp_endpoint: ""
    sample_rate: 0.0
```

## Detailed Field Specifications

### gateway

| Field | Type | Required | Default | Description |
| :---- | :---- | :---- | :---- | :---- |
| `listen_address` | string | yes | — | Address and port the gateway listens on (e.g., `0.0.0.0:8080`) |
| `admin_address` | string | yes | — | Address and port the admin API listens on (e.g., `0.0.0.0:9090`) |
| `request_timeout_ms` | integer | yes | 15000 | Global timeout for client requests (milliseconds) |
| `idle_timeout_ms` | integer | yes | 60000 | Timeout for idle connections (milliseconds) |
| `max_request_body_bytes` | integer | yes | 10485760 | Maximum allowed request body size (10 MB default) |
| `trust_forwarded_headers` | boolean | yes | false | Whether to trust X-Forwarded-* headers from clients |

### auth.providers[]

| Field | Type | Required | Default | Description |
| :---- | :---- | :---- | :---- | :---- |
| `name` | string | yes | — | Unique name for this auth provider (referenced by routes) |
| `issuer` | string | yes | — | Expected JWT `iss` claim (must match exactly) |
| `audience` | string | yes | — | Expected JWT `aud` claim |
| `jwks_uri` | string | yes | — | URL to fetch JWKS from (must be reachable from gateway) |
| `cache_ttl_seconds` | integer | yes | 300 | How long to cache JWKS before refreshing |
| `clock_skew_seconds` | integer | yes | 30 | Clock skew tolerance for `nbf` and `exp` validation |

### rate_limits

| Field | Type | Required | Default | Description |
| :---- | :---- | :---- | :---- | :---- |
| `redis_address` | string | yes | — | Redis address:port |
| `redis_db` | integer | yes | 0 | Redis database number |
| `redis_key_prefix` | string | yes | "rl" | Prefix for all rate limit keys in Redis |
| `default_timeout_ms` | integer | yes | 50 | Timeout for Redis operations (milliseconds) |
| `fail_open` | boolean | yes | false | If true, allow requests when rate limiter unavailable |
| `survivability_mode.enabled` | boolean | yes | true | Enable local fallback when Redis is unavailable |
| `survivability_mode.fallback_capacity` | integer | yes | 20 | Tokens in fallback in-memory bucket |
| `survivability_mode.fallback_refill_rate_per_sec` | number | yes | 5 | Tokens/sec in fallback bucket |

### routes[]

| Field | Type | Required | Default | Description |
| :---- | :---- | :---- | :---- | :---- |
| `name` | string | yes | — | Unique route name (used in logs and metrics) |
| `hostnames` | string[] | yes | — | List of hostnames this route matches (exact match) |
| `path_prefix` | string | yes | — | Path prefix to match (longest match wins when multiple routes match) |
| `methods` | string[] | yes | — | Allowed HTTP methods (GET, POST, PUT, DELETE, PATCH, HEAD) |
| `auth_required` | boolean | yes | false | Whether this route requires JWT authentication |
| `auth_provider` | string | if auth_required=true | — | Name of auth provider to use |
| `required_scopes` | string[] | no | — | If present, JWT token must contain all these scopes |
| `rate_limit.bucket_capacity` | integer | yes | — | Maximum tokens in bucket |
| `rate_limit.refill_rate_per_sec` | number | yes | — | Tokens refilled per second |
| `rate_limit.key_by` | string | yes | — | Dimension: `ip` or `sub` (JWT subject) |
| `upstream.service` | string | yes | — | Name of upstream service (must exist in services[]) |
| `upstream.request_timeout_ms` | integer | yes | 5000 | Timeout for upstream requests |
| `upstream.retries` | integer | yes | 1 | Number of retries for idempotent methods |

### services[]

| Field | Type | Required | Default | Description |
| :---- | :---- | :---- | :---- | :---- |
| `name` | string | yes | — | Unique service name (referenced by routes) |
| `endpoints` | string[] | yes | — | List of upstream addresses (host:port) |
| `health_check.path` | string | yes | — | Health check endpoint (must start with `/`) |
| `health_check.interval_ms` | integer | yes | 2000 | Interval between health checks (milliseconds) |
| `health_check.timeout_ms` | integer | yes | 500 | Timeout for each health check |

### observability

| Field | Type | Required | Default | Description |
| :---- | :---- | :---- | :---- | :---- |
| `access_log_json` | boolean | yes | true | Format access logs as JSON (vs plain text) |
| `prometheus_enabled` | boolean | yes | true | Expose /metrics endpoint |
| `tracing.enabled` | boolean | yes | false | Enable distributed tracing |
| `tracing.otlp_endpoint` | string | if tracing.enabled | — | OpenTelemetry collector endpoint (http://localhost:4317) |
| `tracing.sample_rate` | number | if tracing.enabled | 0.0 | Trace sampling rate (0.0-1.0) |

## Validation Rules

The configuration loader must enforce these rules:

1. **Uniqueness**
   - All route names must be unique
   - All service names must be unique
   - All auth provider names must be unique

2. **Referential Integrity**
   - Every route's `upstream.service` must exist in `services[]`
   - Every route with `auth_required: true` must have `auth_provider` pointing to an existing provider

3. **Type & Value Constraints**
   - `bucket_capacity >= 1`
   - `refill_rate_per_sec > 0`
   - `path_prefix` must start with `/`
   - `health_check.path` must start with `/`
   - `methods` must be valid HTTP methods: GET, POST, PUT, DELETE, PATCH, HEAD
   - `jwks_uri` must be a valid URL
   - `redis_address` must be host:port format
   - `cache_ttl_seconds > 0`
   - `clock_skew_seconds >= 0`
   - `tracing.sample_rate` must be 0.0-1.0

4. **Conditional Requirements**
   - If `auth_required: true`, then `auth_provider` must be specified
   - If `required_scopes` is present, it must be a non-empty list
   - If `tracing.enabled: true`, then `otlp_endpoint` must be specified

5. **No Deprecated Fields**
   - agents must not invent new fields
   - Gateway must reject config with unknown fields

## Example: Complete Valid Config

```yaml
version: 1

gateway:
  listen_address: "0.0.0.0:8080"
  admin_address: "0.0.0.0:9090"
  request_timeout_ms: 15000
  idle_timeout_ms: 60000
  max_request_body_bytes: 10485760
  trust_forwarded_headers: false

auth:
  providers:
    - name: "main"
      issuer: "https://auth.example.local/"
      audience: "api-gateway"
      jwks_uri: "http://localhost:7001/.well-known/jwks.json"
      cache_ttl_seconds: 300
      clock_skew_seconds: 30

rate_limits:
  redis_address: "redis:6379"
  redis_db: 0
  redis_key_prefix: "rl"
  default_timeout_ms: 50
  fail_open: false
  survivability_mode:
    enabled: true
    fallback_capacity: 20
    fallback_refill_rate_per_sec: 5

routes:
  - name: "public-api"
    hostnames: ["api.example.com"]
    path_prefix: "/public"
    methods: ["GET", "POST"]
    auth_required: false
    rate_limit:
      bucket_capacity: 100
      refill_rate_per_sec: 10
      key_by: "ip"
    upstream:
      service: "backend"
      request_timeout_ms: 5000
      retries: 1

  - name: "protected-api"
    hostnames: ["api.example.com"]
    path_prefix: "/protected"
    methods: ["GET", "POST"]
    auth_required: true
    auth_provider: "main"
    required_scopes: ["api.read"]
    rate_limit:
      bucket_capacity: 50
      refill_rate_per_sec: 5
      key_by: "sub"
    upstream:
      service: "backend"
      request_timeout_ms: 5000
      retries: 1

services:
  - name: "backend"
    endpoints:
      - "backend-01:8080"
      - "backend-02:8080"
    health_check:
      path: "/healthz"
      interval_ms: 2000
      timeout_ms: 500

observability:
  access_log_json: true
  prometheus_enabled: true
  tracing:
    enabled: false
    otlp_endpoint: ""
    sample_rate: 0.0
```

## Further Reading

- [auth.md](auth.md) — Detailed JWT validation rules
- [rate-limiting.md](rate-limiting.md) — Detailed rate limiter behavior
- [observability.md](observability.md) — Metrics and logging schemas

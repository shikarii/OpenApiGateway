# Observability Specification

This document specifies the metrics, logging, and tracing contracts for the API Gateway v1.

## Three Pillars: Logs, Metrics, Traces

### Access Logs (JSON)

- **What:** Every request, structured JSON to stdout
- **When:** After response is fully streamed to client
- **Cardinality:** One line per request (unbounded volume, bounded cardinality)

### Prometheus Metrics

- **What:** Counters, histograms for request patterns
- **When:** Scraped on-demand (every 15-30 seconds typical)
- **Cardinality:** Strictly bounded (no raw paths, user IDs)

### Distributed Traces (Optional)

- **What:** Request flow across services
- **When:** Sampled requests sent to OpenTelemetry collector
- **Cardinality:** Sampled at configured rate (0-100%)

---

## Access Logs

### Schema

Every request produces exactly one JSON line on stdout:

```json
{
  "ts":"2026-03-29T23:59:59.000Z",
  "request_id":"2c3c7b8f",
  "remote_addr":"172.18.0.1",
  "host":"localhost",
  "method":"GET",
  "path":"/private/data",
  "route":"private-echo",
  "status":200,
  "duration_ms":14,
  "bytes_in":123,
  "bytes_out":456,
  "auth_subject":"user-123",
  "rate_limit_mode":"redis",
  "upstream_service":"echo-private",
  "upstream_addr":"echo-backend:8081"
}
```

### Field Reference

| Field | Type | Required | Description |
| :---- | :---- | :---- | :---- |
| `ts` | string (RFC3339) | yes | Request timestamp (UTC, millisecond precision) |
| `request_id` | string | yes | Unique request ID (short hash or UUID) |
| `remote_addr` | string | yes | Client IP address |
| `host` | string | yes | HTTP `Host` header value |
| `method` | string | yes | HTTP method (GET, POST, etc.) |
| `path` | string | yes | Request path (e.g., `/private/data`) |
| `route` | string | yes | Matched route name from config (e.g., `private-echo`) |
| `status` | integer | yes | HTTP status code (200, 401, 429, 503, etc.) |
| `duration_ms` | integer | yes | Total time from request in to response complete (milliseconds) |
| `bytes_in` | integer | yes | Request body size (0 if no body) |
| `bytes_out` | integer | yes | Response body size |
| `auth_subject` | string or null | yes | JWT `sub` claim if authenticated; `null` if unauth |
| `rate_limit_mode` | string | yes | `redis` or `degraded-local` or `none` |
| `upstream_service` | string | yes | Name of upstream service from config |
| `upstream_addr` | string | yes | Actual upstream address used (e.g., `host:port`) |

### Constraints

- **No raw paths in labels** — All requests use the matched route name
- **No user identifiers in separate fields** — Only the JWT `sub` (if authenticated)
- **No PII** — Do not log query parameters, headers, or request bodies

### Unmatched (404) Requests

If no route matches:

```json
{
  "ts":"2026-03-29T23:59:59.000Z",
  "request_id":"2c3c7b8f",
  "remote_addr":"172.18.0.1",
  "host":"localhost",
  "method":"GET",
  "path":"/nonexistent",
  "route":"",
  "status":404,
  "duration_ms":1,
  "bytes_in":0,
  "bytes_out":14,
  "auth_subject":null,
  "rate_limit_mode":"none",
  "upstream_service":"",
  "upstream_addr":""
}
```

---

## Prometheus Metrics

All metrics are tagged with safe, bounded label cardinality.

### Counter Metrics

#### `gateway_http_requests_total`

Total HTTP requests processed.

```
gateway_http_requests_total{route="private-api", method="GET", status_class="200"} 1250
gateway_http_requests_total{route="public-api", method="POST", status_class="400"} 3
gateway_http_requests_total{route="", method="GET", status_class="404"} 15
```

Labels:
- `route` (string) — Route name, or empty string if 404
- `method` (string) — HTTP method (GET, POST, etc.)
- `status_class` (string) — Status code bucket (1xx, 2xx, 3xx, 4xx, 5xx)

#### `gateway_http_request_duration_ms`

Request duration in milliseconds (histogram).

```
gateway_http_request_duration_ms_bucket{route="private-api",le="10"} 120
gateway_http_request_duration_ms_bucket{route="private-api",le="50"} 245
gateway_http_request_duration_ms_bucket{route="private-api",le="100"} 289
gateway_http_request_duration_ms_bucket{route="private-api",le="+Inf"} 300
gateway_http_request_duration_ms_sum{route="private-api"} 8900
gateway_http_request_duration_ms_count{route="private-api"} 300
```

Labels:
- `route` (string) — Route name

#### `gateway_auth_failures_total`

Authentication failures.

```
gateway_auth_failures_total{route="private-api", reason="invalid_signature"} 2
gateway_auth_failures_total{route="private-api", reason="token_expired"} 5
gateway_auth_failures_total{route="private-api", reason="missing_token"} 8
```

Labels:
- `route` (string) — Route name
- `reason` (string) — `invalid_signature`, `token_expired`, `missing_token`, `insufficient_scopes`, etc.

#### `gateway_rate_limit_allowed_total`

Requests allowed by rate limiter.

```
gateway_rate_limit_allowed_total{route="private-api"} 950
gateway_rate_limit_allowed_total{route="public-api"} 5000
```

Labels:
- `route` (string) — Route name

#### `gateway_rate_limit_denied_total`

Requests denied by rate limiter.

```
gateway_rate_limit_denied_total{route="private-api"} 50
gateway_rate_limit_denied_total{route="public-api"} 100
```

Labels:
- `route` (string) — Route name

#### `gateway_rate_limit_degraded_total`

Rate limiting requests served from degraded in-memory fallback.

```
gateway_rate_limit_degraded_total{route="private-api"} 120
```

Labels:
- `route` (string) — Route name

#### `gateway_upstream_failures_total`

Upstream service failures (connection errors, timeouts, 5xx responses).

```
gateway_upstream_failures_total{route="private-api", service="backend", reason="connection_timeout"} 3
gateway_upstream_failures_total{route="private-api", service="backend", reason="upstream_5xx"} 1
```

Labels:
- `route` (string) — Route name
- `service` (string) — Upstream service name
- `reason` (string) — `connection_timeout`, `read_timeout`, `upstream_5xx`, etc.

#### `gateway_config_reload_total`

Configuration reload attempts.

```
gateway_config_reload_total{result="success"} 5
gateway_config_reload_total{result="validation_error"} 1
```

Labels:
- `result` (string) — `success` or `validation_error`

### Gauge Metrics

#### `gateway_inflight_requests`

Current in-flight HTTP requests.

```
gateway_inflight_requests 42
```

No labels.

### Histogram Metrics

For histograms, Prometheus automatically creates `_bucket`, `_sum`, and `_count` series. No additional setup required.

---

## Label Cardinality Rules

**Allowed dynamic labels:**
- `route` — Safe because routes are defined in config (finite)
- `method` — Safe (fixed HTTP methods)
- `status_class` — Safe (5 classes: 1xx-5xx)
- `service` — Safe (services are defined in config)
- `reason` — Safe (bounded set of defined failure reasons)

**Forbidden labels:**
- ~~Raw request path~~ — Would explode cardinality
- ~~User ID~~ — Would explode cardinality
- ~~JWT `sub` claim~~ — Would explode cardinality
- ~~IP address~~ — Would explode cardinality

---

## Distributed Tracing (Optional)

If `observability.tracing.enabled: true`:

### Trace Propagation

Every request:

1. Generate or extract `trace_id` from `traceparent` header (W3C Trace Context)
2. Create a root span for the gateway request
3. Inject `traceparent` header into upstream request
4. Inject `x-request-id` header (same value as trace_id or separate)

### Sample Rate

- **`tracing.sample_rate: 0.0`** — No sampling (0%)
- **`tracing.sample_rate: 1.0`** — 100% sampling (all requests)
- **`tracing.sample_rate: 0.01`** — 1% sampling

### Span Attributes

Each gateway span should include:

```
trace_id: "2c3c7b8f-....."
span_id: "a1b2c3d4"
parent_span_id: (if not root)

attributes:
  http.method: "GET"
  http.url: "/private/data"
  http.status_code: 200
  http.request_content_length: 123
  http.response_content_length: 456
  route: "private-api"
  upstream_service: "backend"
  upstream_addr: "backend-01:8080"
  auth_subject: "user-123"  (if authenticated)
  duration_ms: 14

events:
  - name: "auth_check", timestamp: t1
  - name: "rate_limit_check", timestamp: t2
  - name: "upstream_request_start", timestamp: t3
  - name: "upstream_response_received", timestamp: t4
```

### Collector Configuration

If tracing is enabled, the gateway connects to an OpenTelemetry collector:

```yaml
observability:
  tracing:
    enabled: true
    otlp_endpoint: "http://otel-collector:4317"  # gRPC endpoint
    sample_rate: 0.05  # 5%
```

---

## Metrics Endpoint

The gateway exposes Prometheus metrics at:

```
GET http://localhost:9090/metrics
```

(Admin address, not gateway address)

Response format is Prometheus text exposition format (standard).

---

## Observability Best Practices

1. **Avoid Fires:** Do not emit unbounded-cardinality metrics (e.g., per-path metrics)
2. **Be Complete:** Every important event (auth failure, rate limit, etc.) has a metric
3. **Be Observable:** Metrics, logs, and traces should correlate via `request_id`
4. **Sample Wisely:** Tracing at high sample rates is expensive; use 1-5% for production
5. **Alert on Patterns:** Alert on metric trends, not just raw counts

---

## Testing Checklist

- [ ] Every request produces exactly one JSON access log line
- [ ] All required fields in access log are populated
- [ ] Prometheus metrics are exposed at /metrics (admin port)
- [ ] Metrics have bounded cardinality (no path explosion)
- [ ] `route` label is always from config (never raw path)
- [ ] Auth failures increment `gateway_auth_failures_total`
- [ ] Rate limit denials increment `gateway_rate_limit_denied_total`
- [ ] Upstream failures increment `gateway_upstream_failures_total`
- [ ] Tracing (if enabled) exports spans to OTLP collector
- [ ] `request_id` is injected into all requests and logs
- [ ] `traceparent` header is passed through to upstream services

## Further Reading

- [base-case-implementation-spec.md](base-case-implementation-spec.md) — Observability section
- [config-schema.md](config-schema.md) — Observability config
- [auth.md](auth.md) — Auth failure reasons
- [rate-limiting.md](rate-limiting.md) — Rate limit metrics

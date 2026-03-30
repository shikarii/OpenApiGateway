# Rate Limiting Specification

This document specifies the exact rate-limiting contract for the API Gateway v1. The implementation uses a Token Bucket algorithm with Redis + Lua for atomicity and a local in-memory fallback for survivability.

## Algorithm: Token Bucket

The gateway uses the Token Bucket algorithm for rate limiting:

- **Tokens** are issued at a constant `refill_rate_per_sec`
- **Capacity** is fixed at `bucket_capacity`
- A request is **allowed** if it can deduct `1` token from the bucket
- A request is **denied** if insufficient tokens remain
- **Tokens accumulate** up to the capacity
- **Idle buckets** expire from Redis after period of inactivity

### Why Token Bucket?

- Accommodates bursty real-world traffic (REST, WebSocket)
- Memory efficient (2 values per bucket: tokens + last_refill_timestamp)
- Permits controlled bursts up to capacity
- Easy to tune per route

## Redis-Based Token Bucket (Primary)

### Redis Key Format

```
rl:{route_name}:{dimension}:{dimension_value}
```

Examples:

- `rl:public-api:ip:192.168.1.20` — Rate limit for a route by IP
- `rl:private-api:sub:user-123` — Rate limit for a route by JWT subject

### Redis Hash Fields

Each bucket is stored as a Redis Hash with these fields:

| Field | Type | Description |
| :---- | :---- | :---- |
| `tokens` | float | Current token count in bucket |
| `last_refill_ms` | integer | Last refill timestamp (milliseconds since epoch) |

### Lua Script Execution

The token bucket logic is encapsulated in a Redis Lua script to ensure atomic execution (no race conditions). This script runs once per request on the Redis server.

**Script Input (ARGV):**

```
ARGV[1] = now_ms                (current time in milliseconds)
ARGV[2] = capacity              (bucket capacity)
ARGV[3] = refill_rate_per_sec   (float)
ARGV[4] = requested_tokens      (typically 1)
ARGV[5] = ttl_seconds           (key expiration time)
```

**Script Output:**

```json
{
  "allowed": 1,
  "remaining_tokens": 12,
  "retry_after_ms": 0
}
```

or (if denied):

```json
{
  "allowed": 0,
  "remaining_tokens": 0,
  "retry_after_ms": 180
}
```

### Lua Script Logic

Pseudocode:

```
1. Retrieve current tokens and last_refill_ms from hash (or defaults: tokens=capacity, last_refill_ms=now_ms)
2. Elapsed_seconds = (now_ms - last_refill_ms) / 1000.0
3. Refilled_tokens = refill_rate_per_sec * elapsed_seconds
4. New_tokens = min(tokens + refilled_tokens, capacity)
5. If new_tokens >= requested_tokens:
     a. new_tokens -= requested_tokens
     b. Set hash fields: tokens = new_tokens, last_refill_ms = now_ms
     c. Set key TTL = ttl_seconds
     d. Return {allowed: 1, remaining_tokens: new_tokens, retry_after_ms: 0}
6. Else:
     a. Do NOT modify hash
     b. Calculate retry_after_ms based on time to next token refill
     c. Return {allowed: 0, remaining_tokens: 0, retry_after_ms: retry_after_ms}
```

### TTL Calculation

Idle rate-limit buckets should expire from Redis to conserve memory:

```
ttl_seconds = ceil((capacity / refill_rate_per_sec) * 2)
```

Example: Capacity=50, refill_rate=10/sec → TTL = ceil((50/10)*2) = 10 seconds.

This gives a 2× buffer so that completely empty buckets (rare) don't expire immediately.

## Rate-Limit Dimensions

A route's rate-limit is keyed by one of these dimensions:

| Dimension | Key | Example |
| :---- | :---- | :---- |
| `ip` | Client IP address | `rl:api:ip:192.168.1.20` |
| `sub` | JWT subject claim | `rl:api:sub:user-123` |

For unprotected routes (no auth required), use `key_by: ip`.
For protected routes, use `key_by: sub` to rate-limit per authenticated user.

## Redis Unavailable (Graceful Degradation)

If Redis is unreachable (timeout, connection error, etc.):

1. **If `survivability_mode.enabled: true`:**
   - Fall back to a **local in-memory token bucket** for the same request
   - Use the configuration's `survivability_mode.fallback_capacity` and `survivability_mode.fallback_refill_rate_per_sec`
   - Emit response header: `x-rate-limit-mode: degraded-local`
   - Increment metric: `gateway_rate_limit_degraded_total{route}`

2. **If `survivability_mode.enabled: false` and `fail_open: true`:**
   - Allow the request (no rate-limiting)
   - Emit warning log

3. **If `survivability_mode.enabled: false` and `fail_open: false`:**
   - Reject the request with 503 Service Unavailable
   - Body: `{"error":"rate_limiter_unavailable"}`

### Degraded Mode Semantics

In degraded mode, each gateway instance maintains its own independent in-memory token bucket. This means:

- **No global fairness:** Multiple gateway replicas do not share token state
- **Per-instance limits:** Each replica independently enforces its fallback capacity
- **Temporary override:** Rate limits become per-replica instead of global
- **Expected behavior:** If Redis partition heals, rate limit enforcement resumes at the global level

**This is intentional:** Degraded mode prioritizes availability over strict fairness.

## Per-Route Configuration

In `configs/gateway.yaml`:

```yaml
routes:
  - name: "api"
    rate_limit:
      bucket_capacity: 50
      refill_rate_per_sec: 10
      key_by: "ip"
```

All routes with `rate_limit` policy are subject to limiting.

## Response Headers

On each request, the gateway may emit rate-limit headers:

```
x-rate-limit-limit: 50              (bucket capacity)
x-rate-limit-remaining: 12          (tokens left after this request)
x-rate-limit-reset: 1760000000      (Unix timestamp when next token refills)
x-rate-limit-mode: redis            (or "degraded-local")
retry-after: 180                    (only if denied: seconds to wait)
```

## Status Codes

| Condition | Status | Body |
| :---- | :---- | :---- |
| Request allowed | 200-399 | (normal upstream response) |
| Rate limit exceeded | 429 | `{"error":"rate_limit_exceeded"}` |
| Rate limiter unavailable (fail_open=false) | 503 | `{"error":"rate_limiter_unavailable"}` |

## Error Scenarios

| Scenario | Behavior |
| :---- | :---- |
| Route has no `rate_limit` policy | No rate limiting applied; request proceeds normally |
| Redis timeout (default 50ms) | Trigger degraded mode or fail based on config |
| Key doesn't exist in Redis | Initialize new bucket with full capacity |
| Concurrent requests race condition | Lua atomicity prevents this; no race possible |
| Dimension value is empty (e.g., unknown IP) | Use empty string as dimension value (e.g., `rl:api:ip:`) |

## Testing Checklist

- [ ] Requests within bucket capacity are allowed
- [ ] Requests exceeding capacity are denied with 429
- [ ] Tokens refill at the configured rate
- [ ] Bucket capacity is respected as a hard ceiling
- [ ] TTL-based expiration cleans up idle buckets from Redis
- [ ] Unknown `kid` in JWT causes fallback to degraded local bucket (if enabled)
- [ ] `retry-after` header accurately estimates time until next token available
- [ ] Headers `x-rate-limit-*` are present on every request
- [ ] Degraded mode does NOT synchronize state across replicas
- [ ] Switching from degraded back to Redis is seamless
- [ ] `key_by: ip` correctly extracts client IP
- [ ] `key_by: sub` correctly uses JWT subject claim
- [ ] Empty dimension values are handled gracefully

## Further Reading

- [config-schema.md](config-schema.md) — Rate limit configuration
- [auth.md](auth.md) — JWT subject claim for `key_by: sub`
- [observability.md](observability.md) — Rate limit metrics
- [base-case-implementation-spec.md](base-case-implementation-spec.md) — Failure modes

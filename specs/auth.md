# Authentication (JWT/JWKS) Specification

This document specifies the exact JWT validation contract for the API Gateway v1.

## Overview

The gateway accepts JWTs as the single source of authentication for protected routes. JWTs are validated using public keys fetched from a JWKS endpoint, enabling completely stateless authentication without external database lookups on the hot path.

## Token Source

Only the `Authorization` header is accepted in v1:

```
GET /protected/data HTTP/1.1
Authorization: Bearer eyJhbGciOiJSUzI1NiIsImtpZCI6ImtleTEifQ...
```

- **Cookie-based auth is not supported in v1**
- **Multiple auth schemes are not supported in v1**
- **Malformed authorization headers result in 401 Unauthorized**

## Full Validation Rules

A JWT is valid **only if all** of these conditions hold:

1. **Token Format**
   - Must be in format: `Bearer <jwt>`
   - JWT must be well-formed (three base64 segments separated by dots)
   - If malformed, return 401

2. **Header Claims**
   - `alg` must be exactly `RS256` in v1 (no HS256, no ECDSA, etc.)
   - `kid` must be present and must exist in current JWKS
   - If `kid` is unknown and a refresh is triggered, retry once after JWKS fetch
   - If `kid` still unknown after refresh, return 401

3. **Signature Verification**
   - Token signature must verify against the JWKS key identified by `kid`
   - If verification fails, return 401

4. **Issuer & Audience**
   - `iss` claim must match config's `auth.providers[].issuer` exactly (string comparison, case-sensitive)
   - `aud` must be an array (or string convertible to single-element array) and must contain the config's `auth.providers[].audience`
   - If either fails, return 401

5. **Time Windows** (with configured `clock_skew_seconds`)
   - Current UTC time must be >= `nbf` claim - `clock_skew_seconds`
   - Current UTC time must be < `exp` claim + `clock_skew_seconds`
   - If either fails, return 401

6. **Scopes (if route requires them)**
   - If the route defines `required_scopes: [scope1, scope2]`, the JWT must contain a `scope` or `scp` claim
   - The claim must be either:
     - A space-separated string: `"scope1 scope2 scope3"`
     - A JSON array: `["scope1", "scope2", "scope3"]`
   - The token must contain **all** required scopes (intersection check)
   - If any required scope is missing, return 403 Forbidden

## JWKS Caching & Refresh

The gateway caches JWKS responses locally to avoid hammering the external identity provider on every request.

### Cache Behavior

1. **On Startup:** Fetch JWKS immediately and cache it
2. **During Operation:**
   - Return cached JWKS for all lookups
   - Every `cache_ttl_seconds`, refresh JWKS in background (non-blocking)
   - If a request arrives with an unknown `kid`:
     - Immediately trigger a refresh (non-blocking if already in-flight)
     - Fall through to using old cache
3. **Cache Stale Threshold:** 10 × `cache_ttl_seconds`
   - If refresh fails repeatedly and cache age exceeds this threshold:
     - **Protected routes** return 503 Service Unavailable with body `{"error":"auth_provider_unavailable"}`
     - **Unprotected routes** continue normally
4. **Refresh Failure Handling:**
   - Log the error but do not block ongoing requests
   - Continue using last known-good cache

### Example Timeline

```
t=0s:     JWKS fetch succeeds. Cache TTL = 300s.
t=100s:   Request with unknown kid arrives.
          - Issue refresh request (non-blocking)
          - Use old cache for current request
          - If old cache has the kid, validation succeeds
          - If old cache missing the kid, try refresh result
t=300s:   Background refresh triggers (scheduled)
t=3000s:  Cache age = 3000s. If refresh has been failing for 2700s+
          (i.e., 9 × 300s), and next protected request arrives:
          - Return 503 auth_provider_unavailable
```

## Extracted Identity

After a JWT passes all validation checks, the gateway normalizes the identity to this canonical structure (used for rate-limit keying and header injection):

```json
{
  "sub": "user-123",
  "iss": "https://auth.example.local/",
  "aud": ["api-gateway"],
  "scopes": ["api.read", "api.write"],
  "exp_unix": 1760000000
}
```

Notes:
- `sub` (subject) is the user identifier. **If missing from JWT, validation fails (return 401).**
- `aud` is normalized to a list internally
- `scopes` is derived from `scope` or `scp` claim (space-delimited string or array), or empty list if not present
- `exp_unix` is the `exp` claim (Unix seconds)

## Forwarded Headers

If authentication succeeds, the gateway injects these headers into the upstream request:

```
x-auth-sub: user-123
x-auth-iss: https://auth.example.local/
x-auth-scopes: api.read,api.write
x-request-id: <uuid>
traceparent: <w3c trace context>    # Only if tracing enabled
```

**Do NOT forward the raw JWT or the raw decoded claims blob to upstream services.**

## Rate-Limit Keying

Protected routes with `rate_limit.key_by: "sub"` rate-limit by the JWT's `sub` claim. The rate-limit key in Redis is:

```
rl:{route_name}:sub:{sub_claim_value}
```

E.g., `rl:private-echo:sub:user-123`.

## Error Responses

| Condition | Status | Body |
| :---- | :---- | :---- |
| No Authorization header | 401 | `{"error":"missing_token"}` |
| Malformed Bearer token | 401 | `{"error":"invalid_token_format"}` |
| Token parsing fails (JWT malformed) | 401 | `{"error":"invalid_token"}` |
| `alg` is not RS256 | 401 | `{"error":"unsupported_algorithm"}` |
| `kid` unknown and refresh fails | 401 | `{"error":"unknown_key_id"}` |
| Signature verification fails | 401 | `{"error":"invalid_signature"}` |
| `iss` mismatch | 401 | `{"error":"invalid_issuer"}` |
| `aud` mismatch | 401 | `{"error":"invalid_audience"}` |
| Token expired (`exp` check failed) | 401 | `{"error":"token_expired"}` |
| Token not yet valid (`nbf` check failed) | 401 | `{"error":"token_not_yet_valid"}` |
| Required scope missing | 403 | `{"error":"insufficient_scopes"}` |
| JWKS provider unavailable (cache stale) | 503 | `{"error":"auth_provider_unavailable"}` |
| `sub` claim missing | 401 | `{"error":"missing_subject"}` |

All error responses include a `content-type: application/json` header.

## Testing Checklist

- [ ] Valid RS256 JWT with matching iss, aud, nbf, exp passes validation
- [ ] Token with wrong `alg` (HS256) is rejected with 401
- [ ] Token with unknown `kid` triggers refresh; if refresh succeeds and key exists, validation passes
- [ ] Token with unknown `kid` after refresh fails with 401
- [ ] Expired token is rejected with 401
- [ ] Token not yet valid (nbf in future) is rejected with 401
- [ ] Token with wrong `iss` is rejected with 401
- [ ] Token with wrong `aud` is rejected with 401
- [ ] Token missing required scope is rejected with 403
- [ ] Valid token injects all required headers into upstream request
- [ ] JWKS cache TTL refresh works correctly
- [ ] JWKS cache stale threshold (10×TTL) returns 503 for protected routes
- [ ] `sub` claim in token is used for rate-limit keying when `key_by: sub`

## Further Reading

- [config-schema.md](config-schema.md) — Auth provider configuration
- [rate-limiting.md](rate-limiting.md) — Rate limit keying with `key_by: sub`
- [observability.md](observability.md) — Metrics for auth failures

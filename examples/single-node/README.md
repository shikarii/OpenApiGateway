# Single-Node Deployment Example

Complete, runnable single-machine deployment of the API Gateway for local development.

## Architecture

```
[Client]
    |
    v  (port 80)
[Envoy Data Plane]
    |
    +---> [Echo Backend] (port 8081)
    |
[Gateway Manager]
    |
    +---> [Redis] (port 6379)
    +---> [JWKS Server] (port 7001)
    +---> [Prometheus] (port 9091)
```

Envoy handles HTTP traffic on port 80. The gateway manager generates Envoy config and
exposes admin endpoints on port 9090.

## Services

| Service | Image | Host Port | Purpose |
|---------|-------|-----------|---------|
| envoy | envoyproxy/envoy:v1.31 | 80 | HTTP data plane |
| gateway-manager | (built from source) | 9090 | Config + admin API |
| redis | redis:7-alpine | 6379 | Rate limiting store |
| echo-backend | ealen/echo-server | 8081 | Demo upstream |
| fake-jwks | nginx:alpine | 7001 | Static JWKS endpoint |
| prometheus | prom/prometheus | 9091 | Metrics dashboard |

## Setup

### Step 1: Start Services

```bash
docker compose -f examples/single-node/docker-compose.yml up -d --build
```

### Step 2: Test the Gateway

**Public route (through Envoy):**

```bash
curl http://localhost/public/echo -d '{"msg":"hello"}'
```

**Admin health check:**

```bash
curl http://localhost:9090/healthz
curl http://localhost:9090/readyz
curl http://localhost:9090/metrics
```

### Step 3: Generate a Dev JWT

```bash
pip install PyJWT cryptography
python3 scripts/gen-jwt-dev.py --sub user-1 --scopes "api.read"
```

> **Note:** Auth enforcement through Envoy is not yet active (requires ext_authz
> filter wiring). The JWT generator is provided for future use and testing the
> gateway manager's token validation directly.

### Step 4: Run Smoke Tests

```bash
bash scripts/smoke-test.sh
```

## Tear Down

```bash
docker compose -f examples/single-node/docker-compose.yml down -v
```

## Scale Considerations

- **Single gateway instance** on one machine
- **Single Redis instance** (no clustering)
- **Static upstream endpoints** in config
- **Suitable for:** 1-2 services, < 100 req/sec, local development

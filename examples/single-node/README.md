# Single-Node Deployment Example

This example shows a minimal single-machine deployment of the API Gateway v1 for local development or small teams.

## Architecture

```
[Client]
    |
    v
[Gateway (Port 8080)]
    |
    +---> [Redis] (Port 6379)
    +---> [Backend] (Port 8081)
    +---> [JWKS Server] (Port 7001)
```

## Setup

### Step 1: Copy Example Config

```bash
cp examples/single-node/gateway-single-node.yaml configs/gateway.yaml
```

### Step 2: Start Services

```bash
docker-compose -f examples/single-node/docker-compose.yml up -d
```

This starts:
- `redis:7` — Rate limiting store
- `gateway-manager` — Management and config loader
- `envoy` — Data plane proxy
- `echo-backend` — Demo upstream service
- `fake-jwks` — Development JWKS server

### Step 3: Generate a Dev JWT

```bash
python3 scripts/gen-jwt-dev.py --sub user-1 --scopes api.read > /tmp/token.jwt
```

### Step 4: Test the Gateway

**Unprotected route:**

```bash
curl http://localhost:8080/public/echo -d '{"msg":"hello"}'
```

**Protected route (with JWT):**

```bash
TOKEN=$(cat /tmp/token.jwt)
curl -H "Authorization: Bearer $TOKEN" http://localhost:8080/private/echo -d '{"msg":"hello"}'
```

## Scale Considerations

- **Single gateway instance** on one machine
- **Single Redis instance** (no clustering)
- **Static upstream endpoints** in config
- **Suitable for:** 1-2 services, < 100 req/sec, local development

## Next Steps

- Add more routes to `gateway.yaml`
- Configure health checks via `/readyz`
- Monitor metrics at `http://localhost:9090/metrics`
- See [../../deployments/docker-compose/](../../deployments/docker-compose/) for production multi-node setups

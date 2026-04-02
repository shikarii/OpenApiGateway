# Production Multi-Node Deployment

Docker Compose deployment with multiple gateway instances, Redis replication,
Envoy load balancing, and Prometheus + Grafana monitoring.

## Architecture

```
                    [Clients]
                       |
                       v
               [Envoy LB :80]          (front-door load balancer)
                  /        \
                 v          v
          [Envoy-1]    [Envoy-2]       (data plane instances)
              |              |
              v              v
        [Gateway-1]    [Gateway-2]     (control plane instances)
              \            /
               v          v
          [Redis Primary :6379]        (shared rate limit state)
                  |
                  v
          [Redis Replica :6380]        (read replica for HA)

  [Prometheus :9092] --> scrapes all gateway instances
  [Grafana :3000]    --> dashboards from Prometheus
```

## Services

| Service | Image | Host Port | Purpose |
|---------|-------|-----------|---------|
| envoy-lb | envoyproxy/envoy:v1.31 | 80 | Front-door load balancer |
| envoy-1 | envoyproxy/envoy:v1.31 | — | Data plane for gateway-1 |
| envoy-2 | envoyproxy/envoy:v1.31 | — | Data plane for gateway-2 |
| gateway-1 | (built from source) | 9090 | Control plane instance 1 |
| gateway-2 | (built from source) | 9091 | Control plane instance 2 |
| redis-primary | redis:7-alpine | 6379 | Rate limit state (primary) |
| redis-replica | redis:7-alpine | 6380 | Rate limit state (replica) |
| echo-backend | ealen/echo-server | — | Demo upstream service |
| fake-jwks | nginx:alpine | — | Static JWKS endpoint |
| prometheus | prom/prometheus | 9092 | Metrics collection |
| grafana | grafana/grafana | 3000 | Metrics dashboards |

## Quick Start

```bash
# Start the full stack
docker compose -f deployments/docker-compose/docker-compose.yml up -d --build

# Verify all services
docker compose -f deployments/docker-compose/docker-compose.yml ps

# Test through the load balancer
curl http://localhost/public/echo -d '{"msg":"hello"}'

# View Grafana dashboards
open http://localhost:3000  # admin/admin
```

## Configuration

### Gateway Config

Edit `deployments/docker-compose/configs/gateway-multi-node.yaml`:

- `trust_forwarded_headers: true` — enables `X-Forwarded-For` from the LB
- `redis_address: "redis-primary:6379"` — shared rate limit state
- `retries: 2` — higher retry count for production

### Load Balancer Config

Edit `deployments/docker-compose/configs/envoy-lb.yaml`:

- Round-robin across `envoy-1:8080` and `envoy-2:8080`
- Active health checking every 5 seconds
- 15 second request timeout

### Redis

- **Primary**: read/write for rate limiting
- **Replica**: asynchronous replication with `--replicaof`
- AOF persistence enabled (`--appendonly yes`)
- No auth configured by default (add `--requirepass` for production)

## Operational Tasks

### Reload Config Across All Instances

```bash
# Update the gateway config file, then reload each instance:
curl -X POST http://localhost:9090/config/reload   # gateway-1
curl -X POST http://localhost:9091/config/reload   # gateway-2

# Verify both instances loaded the same config version:
curl -s http://localhost:9090/config/status | python3 -m json.tool
curl -s http://localhost:9091/config/status | python3 -m json.tool
```

Both instances should report the same `active_config_sha256`.

### Check Health

```bash
# Individual instance health
curl http://localhost:9090/healthz   # gateway-1
curl http://localhost:9091/healthz   # gateway-2

# Readiness (includes Redis connectivity)
curl -s http://localhost:9090/readyz | python3 -m json.tool
curl -s http://localhost:9091/readyz | python3 -m json.tool
```

### View Metrics

```bash
# Raw Prometheus metrics from each instance
curl http://localhost:9090/metrics   # gateway-1
curl http://localhost:9091/metrics   # gateway-2

# Prometheus targets (should show both gateways as UP)
open http://localhost:9092/targets

# Grafana dashboard
open http://localhost:3000
```

### Scale Gateway Instances

To add a third gateway instance:

1. Add `gateway-3`, `envoy-3`, and `envoy_config_3` sections to `docker-compose.yml`
2. Add `envoy-3:8080` to the `envoy-lb.yaml` cluster endpoints
3. Add `gateway-3:9090` to `prometheus-multi.yml` scrape targets
4. Restart: `docker compose up -d --build`

### Redis Failover

If the primary fails, the replica has a recent copy of rate limit state.
To promote the replica:

```bash
docker exec redis-replica redis-cli REPLICAOF NO ONE
```

Then update `gateway-multi-node.yaml` to point to `redis-replica:6379` and
reload both gateway instances.

### Tear Down

```bash
docker compose -f deployments/docker-compose/docker-compose.yml down -v
```

## Monitoring

### Prometheus Alert Rules

Alerts are defined in `deployments/prometheus/alert_rules.yml`:

| Alert | Condition | Severity |
|-------|-----------|----------|
| HighErrorRate | >5% of requests returning 5xx for 2m | warning |
| HighRateLimitDenials | >10 denials/sec for 5m | warning |
| RateLimiterDegraded | Any degraded-mode requests for 1m | critical |
| UpstreamFailures | >1 failure/sec for 3m | warning |
| HighAuthFailures | >5 failures/sec for 5m | warning |
| ConfigReloadFailed | Any failed reload in 10m | warning |
| GatewayInstanceDown | Instance unreachable for 1m | critical |

### Grafana Dashboard

The pre-configured dashboard at `http://localhost:3000` includes:

- **Request Rate** — total req/s by status class (2xx, 4xx, 5xx)
- **In-Flight Requests** — per-instance concurrency
- **Request Duration** — p50/p90/p99 latency percentiles
- **Error Rate** — 5xx percentage with thresholds
- **Rate Limiting** — allowed/denied/degraded rates
- **Auth Failures** — by failure reason
- **Upstream Failures** — by service and failure type
- **Request Rate by Route** — per-route traffic distribution

### Key Metrics to Watch

| Metric | What to Watch |
|--------|--------------|
| `gateway_http_requests_total{status_class="5xx"}` | Should be near zero |
| `gateway_rate_limit_degraded_total` | Any increase means Redis may be down |
| `gateway_inflight_requests` | Sustained high values indicate saturation |
| `gateway_config_reload_total{result="validation_error"}` | Should be zero |

## Troubleshooting

### Gateway not receiving traffic

1. Check Envoy LB health: `docker logs envoy-lb`
2. Verify backend Envoy configs exist: `docker exec envoy-1 ls /etc/envoy/`
3. Check gateway-manager logs: `docker logs gateway-1`

### Rate limiting not working

1. Verify Redis connectivity: `curl -s localhost:9090/readyz | python3 -m json.tool`
2. Check if `redis_ok: false` — Redis may be down or unreachable
3. If `degraded`, the in-memory fallback is active (per-instance, not shared)

### Config reload fails

1. Check the error: `curl -s -X POST localhost:9090/config/reload | python3 -m json.tool`
2. Common causes: YAML syntax error, unknown fields, validation failures
3. The previous valid config remains active on failure

## Further Reading

- [../../examples/single-node/](../../examples/single-node/) — Single-node dev setup
- [../../specs/config-schema.md](../../specs/config-schema.md) — Configuration reference
- [../../specs/observability.md](../../specs/observability.md) — Metrics and logging spec
- [../../specs/admin-api.md](../../specs/admin-api.md) — Admin API reference

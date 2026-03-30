# Docker Compose Deployments

This directory contains production-ready Docker Compose configurations for various deployment scenarios.

## Single-Node (Development)

See [../examples/single-node/](../examples/single-node/) for a complete development setup.

## Production Multi-Node

For production deployments with multiple gateway instances, use:

```yaml
version: '3.9'

services:
  redis-primary:
    image: redis:7-alpine
    container_name: redis-primary
    ports:
      - "6379:6379"
    command: redis-server --requirepass changeme
    volumes:
      - redis_primary_data:/data
    networks:
      - api-gateway

  redis-replica:
    image: redis:7-alpine
    container_name: redis-replica
    ports:
      - "6380:6379"
    command: redis-server --slaveof redis-primary 6379 --requirepass changeme --masterauth changeme
    depends_on:
      - redis-primary
    volumes:
      - redis_replica_data:/data
    networks:
      - api-gateway

  gateway-1:
    image: api-gateway:latest
    container_name: gateway-1
    ports:
      - "8080:8080"
      - "9090:9090"
    environment:
      REDIS_URL: "redis://:changeme@redis-primary:6379/0"
      LISTEN_ADDR: "0.0.0.0:8080"
      ADMIN_ADDR: "0.0.0.0:9090"
    volumes:
      - ./configs/gateway.yaml:/app/configs/gateway.yaml
    depends_on:
      - redis-primary
    networks:
      - api-gateway

  gateway-2:
    image: api-gateway:latest
    container_name: gateway-2
    ports:
      - "8081:8080"
      - "9091:9090"
    environment:
      REDIS_URL: "redis://:changeme@redis-primary:6379/0"
      LISTEN_ADDR: "0.0.0.0:8080"
      ADMIN_ADDR: "0.0.0.0:9090"
    volumes:
      - ./configs/gateway.yaml:/app/configs/gateway.yaml
    depends_on:
      - redis-primary
    networks:
      - api-gateway

  envoy-lb:
    image: envoyproxy/envoy:v1.31-latest
    container_name: envoy-lb
    ports:
      - "80:10000"
      - "443:10001"
    volumes:
      - ./configs/envoy-lb.yaml:/etc/envoy/envoy.yaml
    command: /usr/local/bin/envoy -c /etc/envoy/envoy.yaml
    networks:
      - api-gateway

  prometheus:
    image: prom/prometheus:latest
    container_name: prometheus
    ports:
      - "9092:9090"
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml
      - prometheus_data:/prometheus
    command:
      - '--config.file=/etc/prometheus/prometheus.yml'
      - '--storage.tsdb.path=/prometheus'
    networks:
      - api-gateway

volumes:
  redis_primary_data:
  redis_replica_data:
  prometheus_data:

networks:
  api-gateway:
    driver: bridge
```

## Key Features

- **Redis Primary/Replica** for high availability
- **Multiple Gateway Instances** for horizontal scaling
- **Envoy Load Balancer** for external traffic distribution
- **Prometheus** for monitoring
- **Shared Config Volume** for atomic, coordinated updates

## Environment Variables

| Variable | Default | Description |
| :---- | :---- | :---- |
| `REDIS_URL` | `redis://redis:6379/0` | Redis connection string |
| `LISTEN_ADDR` | `0.0.0.0:8080` | Gateway listen address |
| `ADMIN_ADDR` | `0.0.0.0:9090` | Admin API listen address |
| `GATEWAY_CONFIG_PATH` | `/app/configs/gateway.yaml` | Path to config file |
| `LOG_LEVEL` | `info` | Log level (debug, info, warn, error) |

## Operational Tasks

### Reload Config Across All Instances

```bash
# Validate the new config
curl -X POST http://localhost:9090/config/reload
curl -X POST http://localhost:9091/config/reload

# Check status
curl http://localhost:9090/config/status
curl http://localhost:9091/config/status
```

### Scale Gateway Instances

```bash
docker-compose up -d --scale gateway=5
```

(Ensure unique admin ports in docker-compose.yml)

### Monitor Metrics

```bash
# View Prometheus targets
open http://localhost:9092/targets

# Query gateway metrics
curl -s 'http://localhost:9092/api/v1/query?query=gateway_http_requests_total'
```

## Further Reading

- [../../specs/config-schema.md](../../specs/config-schema.md) — Configuration
- [../../examples/single-node/](../../examples/single-node/) — Development setup
- [../redis/](../redis/) — Redis configuration and persistence
- [../prometheus/](../prometheus/) — Prometheus monitoring

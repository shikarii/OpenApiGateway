# Scripts Directory

This directory contains low-ceremony helper scripts for development and testing.

## Available Scripts

### `gen-jwt-dev.py`

Generate JWT tokens for local development and testing.

**Usage:**

```bash
# Generate a token for user-1 with api.read scope
python3 scripts/gen-jwt-dev.py --sub user-1 --scopes api.read

# With multiple scopes
python3 scripts/gen-jwt-dev.py --sub user-2 --scopes "api.read api.write"

# Show decoded token
python3 scripts/gen-jwt-dev.py --sub user-1 --decode

# Set expiration to 1 hour
python3 scripts/gen-jwt-dev.py --sub user-1 --hours 1
```

**Options:**

- `--sub` — JWT subject (user ID). Default: `test-user`
- `--scopes` — Space-separated scopes. Default: none
- `--issuer` — JWT issuer URL. Default: `https://dev.example.local/`
- `--audience` — JWT audience. Default: `api-gateway`
- `--hours` — Token expiration. Default: 24 hours
- `--decode` — Print decoded token (for inspection)

**Output:** Raw JWT token (suitable for piping to curl or environment variables)

### `smoke-test.sh`

Basic smoke tests for gateway functionality.

**Usage:**

```bash
# Run against localhost (default)
./scripts/smoke-test.sh

# Run against remote gateway
GATEWAY_URL=https://api.example.com ./scripts/smoke-test.sh
```

**Tests:**
- Public route without auth
- Protected route without auth (should fail)
- Protected route with valid JWT
- Admin /healthz endpoint
- Admin /readyz endpoint
- Admin /metrics endpoint

**Requirements:**
- `curl` — HTTP client
- `python3` — For JWT generation
- Gateway running and accessible

## Contributing

When adding new scripts:

1. Keep them practical and low-ceremony
2. Add a header comment explaining purpose
3. Include usage examples
4. Add this README entry
5. No important logic should live only in scripts — they're helpers, not core

## Further Reading

- [../deployments/docker-compose/](../deployments/docker-compose/) — Deployment automation
- [../examples/](../examples/) — Example configurations
- [../specs/](../specs/) — Specification details

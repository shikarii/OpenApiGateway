# Scripts Directory

Helper scripts for development and testing.

## Available Scripts

### `gen-jwt-dev.py`

Generate JWT tokens for local development and testing.

**Requirements:** `pip install PyJWT cryptography`

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

# Use a specific private key file
python3 scripts/gen-jwt-dev.py --key-file /path/to/private.pem --sub user-1
```

**Options:**

- `--sub` — JWT subject (user ID). Default: `test-user`
- `--scopes` — Space-separated scopes. Default: none
- `--issuer` — JWT issuer URL. Default: `https://dev.example.local/`
- `--audience` — JWT audience. Default: `api-gateway`
- `--hours` — Token expiration. Default: 24 hours
- `--key-file` — RSA private key PEM file. Default: `examples/single-node/jwks/private.pem`
- `--decode` — Print decoded token (for inspection)

**Output:** Raw JWT token (suitable for piping to curl or environment variables)

### `smoke-test.sh`

Smoke tests for the single-node example deployment.

**Usage:**

```bash
# Run against localhost (default: Envoy on port 80, admin on port 9090)
bash scripts/smoke-test.sh

# Override URLs
ENVOY_URL=http://localhost:8080 ADMIN_URL=http://localhost:9090 bash scripts/smoke-test.sh
```

**Tests:**
- Public route through Envoy (200)
- Unknown route (404)
- Auth tests (skipped -- needs ext_authz filter in Envoy)
- Admin /healthz, /readyz, /metrics, /config/status
- JWT generation (if python3 + PyJWT available)

**Requirements:**
- `curl` — HTTP client
- `python3` + `PyJWT` + `cryptography` — For JWT generation (optional)
- Single-node stack running via Docker Compose

## Further Reading

- [../examples/](../examples/) — Example configurations
- [../specs/](../specs/) — Specification details

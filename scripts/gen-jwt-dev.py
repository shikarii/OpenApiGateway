#!/usr/bin/env python3
"""
Generate a development JWT token for local testing.

Usage:
    python3 gen-jwt-dev.py --sub user-1 --scopes api.read,api.write
    python3 gen-jwt-dev.py --help
"""

import json
import sys
import jwt
import argparse
from datetime import datetime, timedelta
from pathlib import Path

# Default development private key (insecure, dev only)
DEV_PRIVATE_KEY = """-----BEGIN RSA PRIVATE KEY-----
MIIEpAIBAAKCAQEA0Z3VS5JJcds6lJ3ExHGNpBq29cN2UGr0Sp5UM7c/Z6V5tOLg
XrQStkzJAa7vxNwuZp5l+sRLqVZFv0yLxMYAP+cL0gC5Jq0eKxDxMvN0eHjYZdKR
GYKMvN8jqZrU7hXCKNkKPpJGlYKyPpJmP9jzYrVqYrV5YrVqYrV5YrVqYrV5Yrv1
qYrV5YrVqYrV5YrVqYrV5YrVqYrV5YrVqYrV5YrVqYrV5YrVqYrV5YrVqYrV5YrV
qYrV5YrVqYrVQIDAQABAoIBADw9LzXMQ7NmvvvP0HxZnXjAKdkXuUx9J/5q0Fxf
qyEqK1+KQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVzQVyqKQVyqKQV
yqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqK
QVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVy
qKQVyqKQVyqKQV1DZnZQlAECgYEA8xVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyq
KQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQV
yqKQVyqKQVyqKQV1DZnZQlAECgYEA8xVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyq
KQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQV1DZnZQlAECgYEA8xVyqK
QVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQV1DZnZQlAECgYEA8xVyqKQ
VyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQV1DZnZQlAECgYEA8xVyqKQVyqKQV
yqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQV1DZnZQlAECgYEA8xVyqKQVyqKQVy
qKQVyqKQV1DZnZQlAECgYEA8xVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQVyq
KQVyqKQVyqKQVyqKQV1DZnZQlAECgYEA8xVyqKQVyqKQVyqKQVyqKQVyqKQVyqK
QVyqKQVyqKQVyqKQVyqKQVyqKQVyqKQV1DZnZQlAC8=
-----END RSA PRIVATE KEY-----"""

def generate_jwt(subject, scopes=None, expires_in_hours=24, issuer="https://dev.example.local/", audience="api-gateway"):
    """Generate a JWT token."""
    
    now = datetime.utcnow()
    payload = {
        "sub": subject,
        "iss": issuer,
        "aud": [audience],
        "iat": int(now.timestamp()),
        "nbf": int(now.timestamp()),
        "exp": int((now + timedelta(hours=expires_in_hours)).timestamp()),
    }
    
    if scopes:
        payload["scope"] = scopes if isinstance(scopes, str) else " ".join(scopes)
        payload["scp"] = scopes.split() if isinstance(scopes, str) else scopes
    
    token = jwt.encode(payload, DEV_PRIVATE_KEY, algorithm="RS256", headers={"kid": "dev-key-1"})
    return token

def main():
    parser = argparse.ArgumentParser(
        description="Generate a development JWT token for testing"
    )
    parser.add_argument("--sub", default="test-user", help="JWT subject (default: test-user)")
    parser.add_argument("--scopes", help="Space-separated scopes (e.g., 'api.read api.write')")
    parser.add_argument("--issuer", default="https://dev.example.local/", help="JWT issuer")
    parser.add_argument("--audience", default="api-gateway", help="JWT audience")
    parser.add_argument("--hours", type=int, default=24, help="Token expiration in hours")
    parser.add_argument("--decode", action="store_true", help="Print decoded token instead of raw JWT")
    
    args = parser.parse_args()
    
    try:
        token = generate_jwt(
            subject=args.sub,
            scopes=args.scopes,
            expires_in_hours=args.hours,
            issuer=args.issuer,
            audience=args.audience
        )
        
        if args.decode:
            decoded = jwt.decode(token, options={"verify_signature": False})
            print(json.dumps(decoded, indent=2))
        else:
            print(token)
    
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()

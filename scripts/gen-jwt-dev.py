#!/usr/bin/env python3
"""
Generate a development JWT token for local testing.

Usage:
    python3 gen-jwt-dev.py --sub user-1 --scopes api.read,api.write
    python3 gen-jwt-dev.py --key-file path/to/private.pem --sub user-1
    python3 gen-jwt-dev.py --help

Requires: pip install PyJWT cryptography
"""

import json
import sys
import jwt
import argparse
from datetime import datetime, timedelta
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
PROJECT_ROOT = SCRIPT_DIR.parent
DEFAULT_KEY_FILE = PROJECT_ROOT / "examples" / "single-node" / "jwks" / "private.pem"


def generate_jwt(private_key, subject, scopes=None, expires_in_hours=24,
                 issuer="https://dev.example.local/", audience="api-gateway"):
    """Generate a JWT token signed with the given RSA private key."""

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

    token = jwt.encode(payload, private_key, algorithm="RS256",
                       headers={"kid": "dev-key-1"})
    return token


def main():
    parser = argparse.ArgumentParser(
        description="Generate a development JWT token for testing"
    )
    parser.add_argument("--sub", default="test-user",
                        help="JWT subject (default: test-user)")
    parser.add_argument("--scopes",
                        help="Space-separated scopes (e.g., 'api.read api.write')")
    parser.add_argument("--issuer", default="https://dev.example.local/",
                        help="JWT issuer")
    parser.add_argument("--audience", default="api-gateway",
                        help="JWT audience")
    parser.add_argument("--hours", type=int, default=24,
                        help="Token expiration in hours")
    parser.add_argument("--key-file", type=Path, default=DEFAULT_KEY_FILE,
                        help="Path to RSA private key PEM file")
    parser.add_argument("--decode", action="store_true",
                        help="Print decoded token instead of raw JWT")

    args = parser.parse_args()

    if not args.key_file.exists():
        print(f"Error: private key not found: {args.key_file}", file=sys.stderr)
        print("Run from the project root or use --key-file to specify the key path.",
              file=sys.stderr)
        sys.exit(1)

    private_key = args.key_file.read_text()

    try:
        token = generate_jwt(
            private_key=private_key,
            subject=args.sub,
            scopes=args.scopes,
            expires_in_hours=args.hours,
            issuer=args.issuer,
            audience=args.audience,
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

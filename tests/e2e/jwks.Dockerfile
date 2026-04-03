FROM python:3.12-alpine
WORKDIR /srv/jwks
COPY tests/e2e/jwks/jwks.json /srv/jwks/jwks.json
CMD ["python", "-m", "http.server", "8000", "--bind", "0.0.0.0", "--directory", "/srv/jwks"]

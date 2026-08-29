# Getting started

From zero to a working publish/pull round-trip in minutes.

← [Docs hub](README.md)

---

## Prerequisites

- Rust toolchain (or Docker for containerized run)
- `curl` for smoke scripts

---

## Option A — Docker Compose (fastest)

```bash
git clone https://github.com/zyvorai/relay-pubsub.git
cd relay-pubsub
docker compose up --build
```

| Endpoint | URL |
|----------|-----|
| REST (HTTPS) | https://127.0.0.1:8080 |
| gRPC (TLS) | 127.0.0.1:50051 |
| Ops UI | http://127.0.0.1:3000 |

Smoke test:

```bash
bash scripts/smoke.sh    # uses curl -k for self-signed cert
```

---

## Option B — Cargo (memory backend)

No Relay required — fully self-contained:

```bash
cargo run -- --backend memory
```

Another terminal:

```bash
bash scripts/smoke.sh
```

---

## Option C — relay-events (real Relay)

Point at a running Zyvor Relay:

```bash
export RELAY_BACKEND=relay-events
export RELAY_BASE_URL=https://127.0.0.1:8443
export RELAY_AUTH_TOKEN=<your-jwt>
export RELAY_TLS_INSECURE=1
cargo run
```

Verify a publish reaches Relay:

```bash
BASE=https://127.0.0.1:8080 bash scripts/smoke-relay-events.sh
```

Full farm catalog (10 types, 5 critical Act):

```bash
BASE=https://127.0.0.1:8443 GATEWAY=https://127.0.0.1:8080 \
  bash scripts/fasal-catalog-smoke.sh
```

Deep dive → [Relay events backend](RELAY_EVENTS_BACKEND.md)

---

## Your first REST publish

Self-signed cert — use `-k`:

```bash
# Create topic
curl -k -X PUT https://127.0.0.1:8080/v1/projects/demo/topics/orders \
  -H 'content-type: application/json' \
  -d '{"labels":{"team":"demo"}}'

# Publish
curl -k -X POST https://127.0.0.1:8080/v1/projects/demo/topics/orders:publish \
  -H 'content-type: application/json' \
  -d '{"messages":[{"data":"aGVsbG8=","attributes":{"source":"curl"}}]}'

# Pull (memory backend)
curl -k -X POST https://127.0.0.1:8080/v1/projects/demo/subscriptions/orders-worker:pull \
  -H 'content-type: application/json' \
  -d '{"maxMessages":10}'
```

For **relay-events**, topic name is the event type:

```bash
curl -k -X POST https://127.0.0.1:8080/v1/projects/fasal-onprem/topics/irrigation.required:publish \
  -H 'content-type: application/json' \
  -d '{
    "messages":[{
      "data":"<base64-json>",
      "attributes":{
        "severity":"critical",
        "source":"curl",
        "idempotency_key":"demo-1"
      }
    }]
  }'
```

---

## Next steps

| Goal | Doc |
|------|-----|
| Install (systemd / k8s / GHCR) | [Installation](INSTALL.md) |
| Verify with smokes + console | [Testing](TESTING.md) |
| Deploy to production host | [Deployment](DEPLOYMENT.md) |
| Kubernetes + relay-edge stack | [Deployment § k8s stack](DEPLOYMENT.md#relay-pubsub--relay-edge-stack) |
| Action Gateway wiring | [Relay events backend](RELAY_EVENTS_BACKEND.md) |
| Architecture deep dive | [Architecture](ARCHITECTURE.md) |

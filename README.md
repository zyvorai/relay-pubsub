# relay-pubsub

**Google Cloud Pub/Sub compatibility for [Zyvor Relay](https://github.com/zyvorai/relay).**

Speak familiar Pub/Sub gRPC and REST. The gateway handles translation, TLS, metrics, and — with `RELAY_BACKEND=relay-events` — forwards every publish to Relay's real **`POST /v1/events`** API.

```text
  Google SDK / curl          relay-pubsub              Zyvor Relay
  ─────────────────          ──────────────            ───────────
  Publish "irrigation.    →   topic = event type   →   Accept
  required"                    self-signed HTTPS        Notify · Act
  Pull / StreamingPull    ←   action queue          ←   /v1/actions
```

[![Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-blue.svg)](LICENSE)

---

## Why this exists

Relay's API is **event-lifecycle shaped** — not topics and subscriptions. But your edge apps, SDKs, and ops tooling speak **Google Pub/Sub**.

relay-pubsub sits in the middle: full Publisher/Subscriber surface on the front, `RelayBackend` on the back. Production path: **`relay-events`** → Relay's shipped API. Demo path: **`memory`** → instant local round-trip.

Pair with **[relay-edge](https://github.com/zyvorai/relay-edge)** for stamped farm events and industrial simulators that publish through the same gateway.

---

## Quick start

```bash
docker compose up --build
bash scripts/smoke.sh          # curl -k, self-signed TLS
```

Or with Cargo:

```bash
cargo run -- --backend memory
```

**New here?** → [docs/GETTING_STARTED.md](docs/GETTING_STARTED.md)

---

## Documentation

| Guide | What's inside |
|-------|---------------|
| [📖 Docs hub](docs/README.md) | Index of everything |
| [🚀 Getting started](docs/GETTING_STARTED.md) | Docker, Cargo, first publish |
| [⚡ Relay events backend](docs/RELAY_EVENTS_BACKEND.md) | Production backend, catalogs, actions |
| [🚢 Deployment](docs/DEPLOYMENT.md) | systemd, k8s, lab instances |
| [🏗 Architecture](docs/ARCHITECTURE.md) | Boundaries, HA, tenant model |
| [📜 Native API (legacy)](docs/relay-native-api.md) | Invented `http` backend contract |

---

## Backends at a glance

| Backend | Relay needed? | Use case |
|---------|---------------|----------|
| `memory` | No | CI, k3s smoke, demos |
| `http` | Yes (invented API) | Legacy — prefer relay-events |
| **`relay-events`** | Yes (real API) | **Fasal, relay-edge, production** |

```bash
export RELAY_BACKEND=relay-events
export RELAY_BASE_URL=https://relay.example.com:8443
export RELAY_AUTH_TOKEN=<jwt>
export RELAY_TLS_INSECURE=1    # if Relay uses self-signed TLS
cargo run
```

---

## What's implemented

<details>
<summary><strong>Google-compatible surface</strong> (click to expand)</summary>

**gRPC:** `CreateTopic`, `Publish`, `Pull`, `StreamingPull`, `Acknowledge`, `ModifyAckDeadline`, `Seek`, subscription CRUD — package names `google.pubsub.v1.Publisher` / `Subscriber`.

**REST:** Full `/v1/projects/{project}/topics/*` and `subscriptions/*` admin + data plane.

**Semantics (memory backend):** explicit ACK, NACK via zero deadline, DLQ after max attempts, ordering keys, timestamp seek, Prometheus metrics.

</details>

---

## TLS — no reverse proxy needed

gRPC and REST are **TLS-only**. First start generates a self-signed cert at `/var/lib/relay-pubsub/tls/` (configurable). Set `PUBSUB_TLS_SAN` before first start to embed your host IP and service DNS names.

```bash
# Clients
curl -k https://localhost:8080/healthz
grpcurl -insecure localhost:50051 list
```

See [Getting started](docs/GETTING_STARTED.md) for the `PUBSUB_EMULATOR_HOST` caveat with Google's plaintext emulator mode.

---

## Deploy

| Target | Command |
|--------|---------|
| **Linux host** | `bash scripts/deploy-remote.sh <HOST> <USER> --build-local --quick` |
| **Local k3s** | `bash deploy/scripts/deploy-k3s.sh` |
| **k8s + relay-edge** | From relay-edge: `./deploy/scripts/deploy-k8s-remote.sh <HOST>` |

Verify:

```bash
BASE=https://<host>:8081 bash scripts/smoke-relay-events.sh
BASE=https://<host>:8443 GATEWAY=https://<host>:8081 bash scripts/fasal-catalog-smoke.sh
```

Full reference → [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md)

**Stack integration test:** relay-edge [TEST_RESULTS.md](https://github.com/zyvorai/relay-edge/blob/main/docs/TEST_RESULTS.md) (2026-08-28 — all PASS, includes this gateway).

---

## Part of the Zyvor stack

| Project | Role |
|---------|------|
| **[relay](https://github.com/zyvorai/relay)** | Control plane |
| **relay-pubsub** (this repo) | Pub/Sub gateway |
| **[relay-edge](https://github.com/zyvorai/relay-edge)** | Farm domain + simulators |

---

## Production boundary

This is a **complete runnable MVP**, not a claim of 100% Google Pub/Sub parity. Durable replication and tenant isolation live in Relay core. See the [compatibility roadmap](docs/ARCHITECTURE.md#compatibility-roadmap) for what's next.

---

## License

Apache-2.0 · Copyright 2026 Zyvor AI Labs

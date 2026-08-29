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
[![GHCR](https://img.shields.io/badge/GHCR-relay--pubsub-black.svg)](https://github.com/zyvorai/relay-pubsub/pkgs/container/relay-pubsub)
[![Release](https://img.shields.io/github/v/release/zyvorai/relay-pubsub.svg)](https://github.com/zyvorai/relay-pubsub/releases)

**Images:** `ghcr.io/zyvorai/relay-pubsub:0.4.0` · `ghcr.io/zyvorai/relay-pubsub-console:0.4.0`

**Current release: [v0.4.0](https://github.com/zyvorai/relay-pubsub/releases/tag/v0.4.0)** · Image: `ghcr.io/zyvorai/relay-pubsub:0.4.0`

---

## Why this exists

Relay's API is **event-lifecycle shaped** — not topics and subscriptions. But your edge apps, SDKs, and ops tooling speak **Google Pub/Sub**.

relay-pubsub sits in the middle: full Publisher/Subscriber surface on the front, `RelayBackend` on the back. Production path: **`relay-events`** → Relay's shipped API. Demo path: **`memory`** → instant local round-trip.

Pair with **[relay-edge](https://github.com/zyvorai/relay-edge)** for stamped farm events and industrial simulators that publish through the same gateway.

Runs on **edge Linux (systemd)**, **Kubernetes / k3s**, or **Docker**.

---

## Quick start

```bash
docker compose up --build
bash scripts/smoke.sh          # curl -k, self-signed TLS
```

Or pull the release image:

```bash
docker pull ghcr.io/zyvorai/relay-pubsub:0.4.0
docker run --rm -p 8080:8080 -p 50051:50051 \
  -e RELAY_BACKEND=memory ghcr.io/zyvorai/relay-pubsub:0.4.0
```

**Install all targets** → [docs/INSTALL.md](docs/INSTALL.md)  
**How to test** → [docs/TESTING.md](docs/TESTING.md)  
**New here?** → [docs/GETTING_STARTED.md](docs/GETTING_STARTED.md)

---

## Documentation

| Guide | What's inside |
|-------|---------------|
| [📖 Docs hub](docs/README.md) | Index of everything |
| [📦 Installation](docs/INSTALL.md) | Docker, GHCR, Cargo, **systemd**, **Kubernetes**, console |
| [🧪 Testing](docs/TESTING.md) | Smoke, conformance, Relay, console, release checklist |
| [🚀 Getting started](docs/GETTING_STARTED.md) | First publish / pull |
| [⚡ Relay events backend](docs/RELAY_EVENTS_BACKEND.md) | Production backend, catalogs, actions |
| [🚢 Deployment](docs/DEPLOYMENT.md) | Lab notes, systemd + k8s detail |
| [🏗 Architecture](docs/ARCHITECTURE.md) | Boundaries, HA, tenant model |
| [📝 Changelog](CHANGELOG.md) | Release notes |
| [📜 Native API (legacy)](docs/relay-native-api.md) | Invented `http` backend contract |

---

## Backends at a glance

| Backend | Relay needed? | Use case |
|---------|---------------|----------|
| `memory` | No | CI, k3s smoke, demos, offline edge |
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

## What's implemented (v0.3)

<details>
<summary><strong>Google-compatible surface</strong> (click to expand)</summary>

**gRPC:** Create/Update/Get/List/Delete topics & subscriptions, Publish, Pull, StreamingPull, Acknowledge, ModifyAckDeadline, Seek (time + snapshot), Snapshots, ModifyPushConfig, ListTopicSubscriptions, IAM subset, SchemaService.

**REST:** Matching `/v1/projects/...` admin + data plane, including pagination (`pageSize` / `pageToken`), PATCH updates, snapshots, schemas, IAM.

**Semantics (memory / relay-events local store):** explicit ACK, NACK via zero deadline, DLQ after max attempts, ordering keys, exactly-once ack leases, retry backoff, timestamp + snapshot seek, push dispatcher, optional durable JSON state (`PUBSUB_PERSIST`), Prometheus metrics.

**Ops:** `/admin/v1/inventory`, `/admin/v1/logs`, `/admin/v1/push-config`, product console (Incoming / Outgoing / Stored / Logs).

</details>

---

## TLS — no reverse proxy needed

gRPC and REST are **TLS-only**. First start generates a self-signed cert at `/var/lib/relay-pubsub/tls/` (configurable). Set `PUBSUB_TLS_SAN` before first start to embed your host IP and service DNS names.

```bash
curl -k https://localhost:8080/healthz
grpcurl -insecure localhost:50051 list
```

See [Getting started](docs/GETTING_STARTED.md) for the `PUBSUB_EMULATOR_HOST` caveat with Google's plaintext emulator mode.

---

## Deploy

| Target | Command |
|--------|---------|
| **Linux host (systemd)** | `bash scripts/deploy-remote.sh <HOST> <USER> --quick` |
| **Ops console** | `bash scripts/deploy-console-remote.sh <HOST> <USER>` |
| **Local k3s** | `bash deploy/scripts/deploy-k3s.sh` |
| **Helm** | `helm upgrade --install … deploy/helm/relay-pubsub` |
| **k8s + relay-edge** | From relay-edge: `./deploy/scripts/deploy-k8s-remote.sh <HOST>` |
| **GHCR** | `docker pull ghcr.io/zyvorai/relay-pubsub:0.4.0` |

Verify:

```bash
BASE=https://<host>:8081 bash scripts/smoke.sh
BASE=https://<host>:8081 bash scripts/conformance-smoke.sh
BASE=https://<host>:8081 bash scripts/smoke-relay-events.sh
```

Full install + test guides → [INSTALL](docs/INSTALL.md) · [TESTING](docs/TESTING.md) · [DEPLOYMENT](docs/DEPLOYMENT.md)

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

This gateway targets Google Pub/Sub compatibility through v0.3 (updates, snapshots, push, IAM subset, schemas, ordering, exactly-once leases, durable local state, inventory + logs). Multi-replica durable cursors still belong in Relay core. See the [compatibility roadmap](docs/ARCHITECTURE.md#compatibility-roadmap).

---

## License

Apache-2.0 · Copyright 2026 Zyvor AI Labs · [zyvor.dev](https://zyvor.dev)

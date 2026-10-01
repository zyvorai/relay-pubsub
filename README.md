# relay-pubsub

[![CI](https://github.com/zyvorai/relay-pubsub/actions/workflows/ci.yml/badge.svg)](https://github.com/zyvorai/relay-pubsub/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/zyvorai/relay-pubsub.svg)](https://github.com/zyvorai/relay-pubsub/releases)
[![GHCR](https://img.shields.io/badge/GHCR-relay--pubsub-black.svg)](https://github.com/zyvorai/relay-pubsub/pkgs/container/relay-pubsub)

[![Book a demo](https://img.shields.io/badge/Book_a_demo-0071e3?style=for-the-badge)](https://zyvor.dev/schedule?utm_source=github&utm_medium=relay-pubsub&utm_campaign=readme_hero)
[![30-day PoC](https://img.shields.io/badge/30--day_PoC-1d1d1f?style=for-the-badge)](https://zyvor.dev/poc?utm_source=github&utm_medium=relay-pubsub&utm_campaign=readme_hero)

![relay-pubsub — Google Pub/Sub APIs in, Relay events out](docs/social/relay-pubsub-share-card.png)

**Google Cloud Pub/Sub compatibility for [Zyvor Relay](https://github.com/zyvorai/relay).**

📖 **[Read the full docs](https://zyvorai.github.io/relay-pubsub/)** — install, testing, relay-events backend, and architecture.

Speak familiar Pub/Sub gRPC and REST. The gateway handles translation, TLS, metrics, and — with `RELAY_BACKEND=relay-events` — forwards every publish to Relay's real **`POST /v1/events`** API.

```text
  Google SDK / curl          relay-pubsub              Zyvor Relay
  ─────────────────          ──────────────            ───────────

  Publish "irrigation.    →   topic = event type   →   Accept
  required"                    self-signed HTTPS        Notify · Act
  Pull / StreamingPull    ←   action queue          ←   /v1/actions
```

**Images:** `ghcr.io/zyvorai/relay-pubsub:0.4.0` · `ghcr.io/zyvorai/relay-pubsub-console:0.4.0`  
**Current release:** [v0.4.0](https://github.com/zyvorai/relay-pubsub/releases/tag/v0.4.0)

## Contents

- [Why this exists](#why-this-exists)
- [Is this for you?](#is-this-for-you)
- [Quick start](#quick-start)
- [Backends](#backends)
- [What's implemented](#whats-implemented)
- [TLS](#tls--no-reverse-proxy-needed)
- [Deploy](#deploy)
- [Documentation](#documentation)
- [License](#license)

## Why this exists

Relay's API is **event-lifecycle shaped** — not topics and subscriptions. Edge apps, SDKs, and ops tooling speak **Google Pub/Sub**.

relay-pubsub sits in the middle: full Publisher/Subscriber surface on the front, `RelayBackend` on the back. Production path: **`relay-events`** → Relay's shipped API. Demo path: **`memory`** → instant local round-trip.

Pair with **[relay-edge](https://github.com/zyvorai/relay-edge)** for stamped farm events and industrial simulators that publish through the same gateway.

Runs on **edge Linux (systemd)**, **Kubernetes / k3s**, or **Docker**.

## Is this for you?

relay-pubsub is a narrow, open-source (Apache-2.0) **protocol compatibility gateway** — so Google Pub/Sub SDKs can talk to Zyvor Relay's event-lifecycle API. It is not a general message broker (not NATS/RabbitMQ/Kafka) and not a standalone product: the `relay-events` backend needs Relay; only `memory` works alone (demos/CI).

Use it when you want Pub/Sub-shaped client code talking to a self-hosted, offline-capable edge stack — not Google's infrastructure.

> **Maturity (honest):** v0.4.0. Google Pub/Sub compatibility through the v0.3 surface (updates, snapshots, push, IAM subset, schemas, ordering, exactly-once leases, durable local state). Multi-replica durable cursors still belong in Relay core — keep a single replica with PVC. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#compatibility-roadmap).

New here? [docs/FAQ.md](docs/FAQ.md) · troubleshooting in [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md#troubleshooting)

## Quick start

```bash
git clone https://github.com/zyvorai/relay-pubsub.git
cd relay-pubsub
docker compose up --build
make ci
bash scripts/smoke.sh          # curl -k, self-signed TLS
```

Or pull the release image:

```bash
docker pull ghcr.io/zyvorai/relay-pubsub:0.4.0
docker run --rm -p 8080:8080 -p 50051:50051 \
  -e RELAY_BACKEND=memory ghcr.io/zyvorai/relay-pubsub:0.4.0
```

**Install** → [docs/INSTALL.md](docs/INSTALL.md) · **Test** → [docs/TESTING.md](docs/TESTING.md) · **First publish** → [docs/GETTING_STARTED.md](docs/GETTING_STARTED.md)

## Backends

| Backend | Relay needed? | Use case |
|---------|---------------|----------|
| `memory` | No | CI, k3s smoke, demos, offline edge |
| `http` | Yes (invented API) | Legacy — prefer relay-events |
| **`relay-events`** | Yes (real API) | **Production, relay-edge** |
| `grpc` | Stub | Scaffold until a Relay gRPC proto lands |

```bash
export RELAY_BACKEND=relay-events
export RELAY_BASE_URL=https://relay.example.com:8443
export RELAY_AUTH_TOKEN=<jwt>
export RELAY_TLS_INSECURE=1    # if Relay uses self-signed TLS
cargo run
```

## What's implemented

<details>
<summary><strong>Google-compatible surface</strong> (click to expand)</summary>

**gRPC:** Create/Update/Get/List/Delete topics & subscriptions, Publish, Pull, StreamingPull, Acknowledge, ModifyAckDeadline, Seek (time + snapshot), Snapshots, ModifyPushConfig, ListTopicSubscriptions, IAM subset, SchemaService.

**REST:** Matching `/v1/projects/...` admin + data plane, including pagination (`pageSize` / `pageToken`), PATCH updates, snapshots, schemas, IAM.

**Semantics (memory / relay-events local store):** explicit ACK, NACK via zero deadline, DLQ after max attempts, ordering keys, exactly-once ack leases, retry backoff, timestamp + snapshot seek, push dispatcher, optional durable JSON state (`PUBSUB_PERSIST`), Prometheus metrics, attribute filters, schema-bound publish, messageId dedup, CloudEvents attribute projection.

**Ops:** `/admin/v1/inventory`, `/admin/v1/logs`, `/admin/v1/push-config`, product console (Incoming / Outgoing / Stored / Logs).

</details>

## TLS — no reverse proxy needed

gRPC and REST are **TLS-only**. First start generates a self-signed cert at `/var/lib/relay-pubsub/tls/` (configurable). Set `PUBSUB_TLS_SAN` before first start to embed your host IP and service DNS names.

```bash
curl -k https://localhost:8080/healthz
grpcurl -insecure localhost:50051 list
```

See [docs/GETTING_STARTED.md](docs/GETTING_STARTED.md) for the `PUBSUB_EMULATOR_HOST` caveat with Google's plaintext emulator mode.

## Deploy

| Target | Command |
|--------|---------|
| **Linux host (systemd)** | `make deploy-remote H=<host> U=<user>` |
| **Ops console** | `bash scripts/deploy-console-remote.sh <HOST> <USER>` |
| **Local k3s** | `bash deploy/scripts/deploy-k3s.sh` |
| **Helm** | `helm upgrade --install … deploy/helm/relay-pubsub` |
| **k8s + relay-edge** | From relay-edge: `./deploy/scripts/deploy-k8s-remote.sh <HOST>` |
| **GHCR** | `docker pull ghcr.io/zyvorai/relay-pubsub:0.4.0` |

```bash
BASE=https://<host>:8081 bash scripts/smoke.sh
BASE=https://<host>:8081 bash scripts/conformance-smoke.sh
BASE=https://<host>:8081 bash scripts/smoke-relay-events.sh
```

## Part of the Zyvor stack

| Project | Role |
|---------|------|
| **[relay](https://github.com/zyvorai/relay)** | Control plane — Accept → Notify → Ack → Act → Verify |
| **relay-pubsub** (this repo) | Pub/Sub compatibility gateway |
| **[relay-edge](https://github.com/zyvorai/relay-edge)** | Farm domain + simulators |

## Documentation

| Guide | What's inside |
|-------|---------------|
| [zyvorai.github.io/relay-pubsub](https://zyvorai.github.io/relay-pubsub/) | Product docs |
| [docs/FAQ.md](docs/FAQ.md) | Licensing, support, scope |
| [docs/INSTALL.md](docs/INSTALL.md) | Docker, GHCR, systemd, Kubernetes |
| [docs/RELAY_EVENTS_BACKEND.md](docs/RELAY_EVENTS_BACKEND.md) | Production backend |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Boundaries, HA, tenant model |
| [docs/FILTERS.md](docs/FILTERS.md) | Attribute filters, schemas, CloudEvents |
| [CHANGELOG.md](CHANGELOG.md) | Release notes |

Social assets: [docs/social/](docs/social/).

## License

Commercial subscriptions and support: see [docs/SUBSCRIPTION-MODEL.md](docs/SUBSCRIPTION-MODEL.md).

### Open source (Apache-2.0)

Licensed under the [Apache License, Version 2.0](LICENSE). Personal, lab, and commercial production use at no charge, subject to Apache-2.0 (preserve notices / NOTICE where required).

### Enterprise

Production support, SLAs, and Zyvor Enterprise products are licensed separately.
[Book a demo](https://zyvor.dev/schedule?utm_source=github&utm_medium=relay-pubsub&utm_campaign=readme_footer) · [30-day PoC](https://zyvor.dev/poc?utm_source=github&utm_medium=relay-pubsub&utm_campaign=readme_footer) · fallback: [sales@zyvor.dev](mailto:sales@zyvor.dev)

Or see [zyvor.dev](https://zyvor.dev/?utm_source=github&utm_medium=relay-pubsub&utm_campaign=readme_edition).

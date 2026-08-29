# relay-pubsub documentation

**Google Pub/Sub APIs in. Relay events out.** · **v0.3.0**

---

## Start here

| I want to… | Go to |
|------------|-------|
| **Install** (Docker, GHCR, systemd, k8s) | [Installation](INSTALL.md) |
| **Test / verify** an install | [Testing](TESTING.md) |
| Run locally in 2 minutes | [Getting started](GETTING_STARTED.md) |
| Wire up Relay's real API (`relay-events`) | [Relay events backend](RELAY_EVENTS_BACKEND.md) |
| Deploy notes (lab template) | [Deployment](DEPLOYMENT.md) |
| Understand the architecture | [Architecture](ARCHITECTURE.md) |
| Integrate with relay-edge simulators | [Integration with relay-edge](#integration-with-relay-edge) |
| **Stack test results (relay-edge repo)** | [relay-edge TEST_RESULTS](https://github.com/zyvorai/relay-edge/blob/main/docs/TEST_RESULTS.md) |
| Release history | [Changelog](../CHANGELOG.md) |
| SPDX headers on source | [License headers](LICENSE_HEADERS.md) |

---

## What this gateway does

Applications speak **Google Pub/Sub** (gRPC or REST). The gateway translates to **Zyvor Relay** through a pluggable backend.

```text
  Your app                relay-pubsub              Zyvor Relay
  ────────                ──────────────            ───────────
  Publish topic      →    RelayBackend         →    POST /v1/events
  Pull subscription  ←    (memory / http /          Action Gateway
                           relay-events)            ← POST /v1/actions
```

Three backends:

| Backend | When to use |
|---------|-------------|
| `memory` | Demos, CI, k3s smoke, offline edge — no Relay needed |
| `http` | Legacy invented topics API — rarely needed |
| **`relay-events`** | **Production** — Relay's real `/v1/events` API |

Runs as **Docker**, **systemd**, or **Kubernetes** — see [Installation](INSTALL.md).

---

## TLS built in

Both gRPC and REST listeners are **TLS-only**. No nginx required.

On first start the gateway generates a self-signed cert, persists it, and reuses it across restarts. Clients use `curl -k` or `RELAY_TLS_INSECURE=1` for outbound calls.

In Kubernetes, an `emptyDir` volume holds cert material for the pod lifetime.

---

## Event catalogs (relay-events)

Topic name = Relay event type. **40 types** pre-registered at startup:

| Catalog | Topics | Source |
|---------|--------|--------|
| Farm | 10 | Fasal accommodation |
| Edge / firewater | 18 | relay-edge firewater simulator |
| Remote edge | 6 | relay-edge remote-edge simulator |
| Fleet | 6 | relay-edge fleet simulator |

Publishing to any other topic name still works — catalog is for admin UI visibility. The product console **Generate** tab can seed the same catalogs into memory.

---

## Ops console

Product UI (Incoming / Outgoing / Stored / Configure / Logs) on a separate systemd unit — [Installation § Console](INSTALL.md#5-ops-console) and [Testing § Console](TESTING.md#7-ops-console-browser).

Admin helpers: `GET /admin/v1/inventory`, `GET /admin/v1/logs`, `POST /admin/v1/push-config`.

---

## Integration with relay-edge

[relay-edge](https://github.com/zyvorai/relay-edge) stamps farm/simulator events and publishes via this gateway:

```text
  relay-edge  ──HTTPS──▶  relay-pubsub  ──relay-events──▶  Relay
  (simulators)            (topic = type)
```

Deploy both as Kubernetes pods:

```bash
# From relay-edge repo:
RELAY_AUTH_TOKEN="$(cat /tmp/lab-relay.jwt)" \
  ./deploy/scripts/deploy-k8s-remote.sh <HOST> [USER]
```

Full event verification: relay-edge [docs/EVENT_MATRIX.md](https://github.com/zyvorai/relay-edge/blob/main/docs/EVENT_MATRIX.md)

**Latest stack test (2026-08-28):** all gates PASS — [relay-edge TEST_RESULTS.md](https://github.com/zyvorai/relay-edge/blob/main/docs/TEST_RESULTS.md) (covers pubsub health, farm Act via `/v1/actions`, and full Forge path).

---

## Scripts cheat sheet

```bash
bash scripts/smoke.sh                    # memory: publish + pull
bash scripts/conformance-smoke.sh        # pagination, snapshots, IAM, schemas, push
bash scripts/smoke-relay-events.sh       # relay-events: single publish
bash scripts/fasal-catalog-smoke.sh      # all 10 farm types + Act
bash scripts/deploy-remote.sh HOST USER  # systemd deploy
bash scripts/deploy-console-remote.sh HOST USER
bash deploy/scripts/deploy-k3s.sh        # local k3s + memory
bash deploy/scripts/ci-k3s-e2e.sh        # verify k3s deploy
bash scripts/selftest.sh                 # host binary + unit + smoke
```

Image: `ghcr.io/zyvorai/relay-pubsub:0.3.0`

---

## Related projects

- [relay](https://github.com/zyvorai/relay) — control plane
- [relay-edge](https://github.com/zyvorai/relay-edge) — domain + simulators
- [zyvor.dev](https://zyvor.dev) — Zyvor

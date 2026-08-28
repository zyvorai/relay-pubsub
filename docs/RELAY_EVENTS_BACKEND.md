# Relay events backend

The production path: Pub/Sub topic publish → Relay `POST /v1/events`, plus Action Gateway for outbound acts.

← [Docs hub](README.md) · [Getting started](GETTING_STARTED.md)

---

`--backend relay-events` (`RELAY_BACKEND=relay-events`) targets Zyvor Relay's real API. Implementation: [`src/relay_events_backend.rs`](../src/relay_events_backend.rs) + [`src/action_gateway.rs`](../src/action_gateway.rs).

## Topic = event type
- **Pre-registered catalogs** in `src/config.rs` (admin UI visibility only — publish works for any non-actions topic):

| Catalog | Count | Examples |
|---------|-------|----------|
| `FASAL_CATALOG` | 10 | `irrigation.required`, `crop.advisory`, … |
| `EDGE_CATALOG` | 18 | `firewater.tank.low`, `edge.comms.down`, `telemetry.sample`, … |
| `REMOTE_EDGE_CATALOG` | 6 | `remote-edge.link.starlink.degraded`, `remote-edge.galleon.thermal`, … |
| `FLEET_CATALOG` | 6 | `fleet.power.island`, `fleet.robot.lost`, … |

Combined via `relay_events_catalog()` (40 topics at startup).

- **Message mapping:** attribute `severity` → event severity; `source` → source; `idempotency_key` → dedupe key; `data` (JSON) → event `data`.

## Inbound: publish → Relay

REST: `POST /v1/projects/{project}/topics/{topic}:publish` → `POST {RELAY_BASE_URL}/v1/events`.

[relay-edge](https://github.com/zyvorai/relay-edge) publishes the same way via `GATEWAY_BASE_URL` when simulators have `"publish": true`.

## Outbound: Relay actions → pull/ack

`POST /v1/actions` implements Relay's Action Gateway contract. Point Relay:

```bash
RELAY_ACTION_TARGETS=farm-controller=https://<gateway-host>:<port>/v1/actions,\
firewater-controller=https://<gateway-host>:<port>/v1/actions,\
remote-edge-controller=https://<gateway-host>:<port>/v1/actions,\
fleet-controller=https://<gateway-host>:<port>/v1/actions
```

Gateway is **TLS-only**. Relay must either trust the gateway cert or set `RELAY_TLS_INSECURE=1` on **Relay's** outbound action client. Include `127.0.0.1` in `PUBSUB_TLS_SAN` when Relay calls loopback.

Actions enqueue on `farm-actions` topic (local memory backend) for Pub/Sub pull/ack consumers.

## Config

| Var | Default | Notes |
|---|---|---|
| `RELAY_BACKEND` | `memory` | Set to `relay-events` |
| `RELAY_BASE_URL` | `http://relay:9090` | e.g. `https://127.0.0.1:8443` |
| `RELAY_AUTH_TOKEN` | unset | Bearer JWT |
| `RELAY_TLS_INSECURE` | `false` | `1` for self-signed Relay TLS |
| `PUBSUB_TLS_SAN` | `localhost,relay-pubsub` | Add node IP + `127.0.0.1` for action callbacks |
| `FASAL_GCP_PROJECT` | `fasal-onprem` | Pub/Sub project segment |
| `FASAL_ACTIONS_TOPIC` / `FASAL_ACTIONS_SUBSCRIPTION` | `farm-actions` / `farm-actions-sub` | Action queue |

## Tests & smoke

```bash
cargo test --release relay_events_backend action_gateway
```

End-to-end (HTTPS, self-signed — scripts use `curl -k`):

```bash
# Single relay-events publish
BASE=https://127.0.0.1:8081 bash scripts/smoke-relay-events.sh

# Full farm catalog (10 types, 5 critical Act)
BASE=https://127.0.0.1:8443 GATEWAY=https://127.0.0.1:8081 \
  bash scripts/fasal-catalog-smoke.sh

# All four families via relay-edge (sibling repo)
BASE=https://127.0.0.1:8443 GATEWAY=https://127.0.0.1:8081 EDGE=http://127.0.0.1:18086 \
  ../relay-edge/scripts/e2e-events-matrix.sh

# Full stack + Forge (see relay-edge docs/TEST_RESULTS.md)
set -a && source ../relay-edge/config/lab-stack.env && set +a
../relay-edge/scripts/e2e-forge-stack.sh
```

## Kubernetes

Helm chart supports `relay.backend=relay-events`, TLS volume, and JWT secret. Full stack with relay-edge: see relay-edge `deploy/scripts/deploy-k8s-remote.sh` and [DEPLOYMENT.md](DEPLOYMENT.md).

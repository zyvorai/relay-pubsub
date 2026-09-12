# Deploying relay-pubsub

How to run the gateway on a laptop, Linux host, or in Kubernetes — alone or with relay-edge.

← [Docs hub](README.md) · [Getting started](GETTING_STARTED.md)

---

## Deployment reference (template)

Track your own instances in ops notes — do not commit live hostnames or IPs to git.

| Host | User | HTTP | gRPC | Backend | Notes |
|---|---|---|---|---|---|
| `<host>` | `<user>` | `8081` (HTTPS) | `50061` (gRPCS) | `relay-events` | **systemd** on host. Self-signed cert at `/var/lib/relay-pubsub/tls/`. `RELAY_BASE_URL=https://127.0.0.1:8443`, `RELAY_TLS_INSECURE=1`. Pre-registers **40** topic names (farm + edge + remote-edge + fleet catalogs). JWT must match relay-edge. |
| `<host>` | `<user>` | `8082` (HTTPS) | — | n/a | Product console + demo/tests — [Ops console + product demo](#ops-console-product-demo) |
| `<host>` | `<user>` | `8080` (HTTPS, in-cluster) | `50051` (gRPCS) | `relay-events` | **k8s** pod in namespace `relay-pubsub`. Deployed via relay-edge `deploy/scripts/deploy-k8s-remote.sh`. Reaches host Relay at `https://<host>:8443`. |

Manage **systemd** gateway:

```bash
ssh <user>@<host> systemctl status relay-pubsub
ssh <user>@<host> cat /etc/relay-pubsub/relay-pubsub.env
BASE=https://<host>:8081 bash scripts/smoke-relay-events.sh
make deploy-remote-quick H=<host> U=<user>
```

Manage **k8s** pods:

```bash
ssh <user>@<host> kubectl -n relay-pubsub get pods
ssh <user>@<host> bash ~/.deployments/k8s-edge-stack/relay-edge/deploy/scripts/k8s-e2e.sh
```

---

## Ops console + product demo

`ui/` is a Zyvor-branded product site (hero, Generate catalogs, live demo, in-browser tests, ops console) deployed via `scripts/deploy-console-remote.sh` — own systemd unit, port **8082**, self-signed HTTPS.

Same-origin fetch: Node `ui/server.mjs` proxies `/v1`, `/admin`, `/healthz`, `/readyz`, `/metrics` to the gateway so the browser only trusts one cert.

```bash
# Optional 3rd arg = SSH password (sshpass).
bash scripts/deploy-console-remote.sh <host> <user> [password] \
  --gateway https://127.0.0.1:8081 \
  --project projects/demo

curl -k https://<host>:8082/          # product site
curl -k https://<host>:8082/healthz   # proxied gateway health
# https://<host>:8082/#generate       # seed catalogs
# https://<host>:8082/#demo           # live publish → pull → ack
# https://<host>:8082/#tests          # browser smoke + conformance
# https://<host>:8082/#console        # Incoming / Outgoing / Stored / Logs
```

Console panes:

| Pane | Purpose |
|------|---------|
| Incoming | Publish into topics |
| Outgoing | Pull / live / push endpoint / ack |
| Stored | Inventory (counts, backlog, peek without consuming) |
| Configure | Create / delete topics & subscriptions |
| Logs | Live gateway process log tail |

Include the host IP in gateway `PUBSUB_TLS_SAN` (and regenerate certs) so server-side clients and the console proxy can reach the gateway by name/IP.

Full install + test docs: [INSTALL.md](INSTALL.md) · [TESTING.md](TESTING.md).

---

## Deployment targets

1. [Bare Linux host via systemd](#1-bare-linux-host-via-systemd) — `scripts/deploy-remote.sh`
2. [Kubernetes pods](#2-kubernetes-pods) — Helm chart, k3s, or **relay-edge stack deploy**
3. [relay-pubsub + relay-edge stack](#relay-pubsub-relay-edge-stack) — full integration path

Also: [Installation](INSTALL.md) (GHCR pull, Docker run) and [Testing](TESTING.md) (acceptance checklist).

Functional proof for all paths: real publish (and pull for memory backend) via `scripts/smoke.sh` or `scripts/smoke-relay-events.sh` — not `/healthz` alone.

---

## 1. Bare Linux host via systemd

### Prerequisites

- Local: `ssh`, `rsync`; for `--build-local`: Linux + Rust **or** docker (cross-build via Dockerfile).
- Target: Debian/Ubuntu or RHEL-family, SSH, passwordless `sudo` recommended.

### Deploy

```bash
bash scripts/deploy-remote.sh <host> <user> --build-local --quick
make deploy-remote-quick H=<host> U=<user>
```

Profiles: full remote build (default), `--quick`, `--build-local`, `--verify-only`, `--uninstall [--purge]`, `--fleet`. See `scripts/deploy-remote.sh --help`.

### Configuration highlights

| Variable | Default | Purpose |
|---|---|---|
| `PUBSUB_TLS_SAN` | `localhost,relay-pubsub` | **Include `127.0.0.1`** if Relay calls `https://127.0.0.1:8081/v1/actions` |
| `RELAY_BACKEND` | `memory` | Use `relay-events` for real Relay integration |
| `RELAY_BASE_URL` | `http://relay:9090` | e.g. `https://127.0.0.1:8443` on lab |
| `RELAY_TLS_INSECURE` | unset | Set `1` when Relay uses self-signed TLS |
| `RELAY_AUTH_TOKEN` | empty | JWT — must match relay-edge |

Example lab `/etc/relay-pubsub/relay-pubsub.env`:

```bash
PUBSUB_HTTP_ADDR=0.0.0.0:8081
PUBSUB_GRPC_ADDR=0.0.0.0:50061
PUBSUB_TLS_SAN=localhost,127.0.0.1,<host>,relay-pubsub
RELAY_BACKEND=relay-events
RELAY_BASE_URL=https://127.0.0.1:8443
RELAY_TLS_INSECURE=1
RELAY_AUTH_TOKEN=<jwt>
```

Regenerate TLS cert after changing `PUBSUB_TLS_SAN` (delete `/var/lib/relay-pubsub/tls/*.pem`, restart).

Full variable list: `deploy/systemd/relay-pubsub.env.example`.

### Uninstall

```bash
make deploy-remote-uninstall H=<host> U=<user>
bash scripts/deploy-remote.sh <host> <user> --uninstall [--purge]
```

### Verify (systemd)

```bash
BASE=https://<host>:8081 bash scripts/smoke.sh
BASE=https://<host>:8081 bash scripts/conformance-smoke.sh
BASE=https://<host>:8081 bash scripts/smoke-relay-events.sh
BASE=https://<host>:8443 GATEWAY=https://<host>:8081 \
  bash scripts/fasal-catalog-smoke.sh
curl -k https://<host>:8081/admin/v1/inventory?project=projects/demo
curl -k https://<host>:8081/admin/v1/logs?limit=20
```

See [Testing](TESTING.md) for the full acceptance checklist.

---

## 2. Kubernetes pods

### Helm chart (`deploy/helm/relay-pubsub/`)

Defaults updated for production-style deploy:

- `relay.backend=relay-events`
- TLS `emptyDir` volume at `/var/lib/relay-pubsub/tls/`
- `PUBSUB_TLS_SAN` via `tls.san` value
- Probes: `scheme: HTTPS`
- Secret `relay-auth-token` when backend is `http` or `relay-events`

### Local k3s (memory backend, no Relay)

```bash
bash deploy/scripts/deploy-k3s.sh
bash deploy/scripts/ci-k3s-e2e.sh
```

### relay-pubsub + relay-edge stack

Both pods use **built-in self-signed HTTPS**. Deployed together from the **relay-edge** repo:

```bash
# In relay-edge repo (sibling relay-pubsub required):
RELAY_AUTH_TOKEN="$(cat /tmp/lab-relay.jwt)" \
  ./deploy/scripts/deploy-k8s-remote.sh <HOST> [USER]
```

| Release | Namespace | Service |
|---------|-----------|---------|
| `relay-pubsub` | `relay-pubsub` | `:8080` HTTPS |
| `relay-edge` | `relay-edge` | `:18086` HTTPS |

On-cluster verify:

```bash
bash deploy/scripts/k8s-e2e.sh
```

Uses `scripts/smoke-relay-events.sh` + relay-edge firewater smoke + remote-edge/fleet publish path.

**Note:** Pods reach host Relay via `https://<node-ip>:8443` (set from SSH host in deploy script). `host.k3s.internal` is not reliable on all clusters — deploy script uses the explicit host IP.

### Manual Helm (single gateway)

```bash
kubectl create namespace relay-pubsub
kubectl -n relay-pubsub create secret generic relay-pubsub-secrets \
  --from-literal=relay-auth-token="$RELAY_AUTH_TOKEN"

helm upgrade --install relay-pubsub deploy/helm/relay-pubsub \
  -n relay-pubsub \
  --set relay.backend=relay-events \
  --set relay.baseUrl=https://<relay-host>:8443 \
  --set relay.tlsInsecure=1 \
  --set tls.san="localhost,relay-pubsub,relay-pubsub.relay-pubsub.svc.cluster.local"
```

---

## Integration with relay-edge

| Component | Role |
|-----------|------|
| relay-edge | Stamps farm/simulator events, publishes to gateway |
| relay-pubsub | Maps Pub/Sub topics → `POST /v1/events`; receives actions at `/v1/actions` |
| Relay | Policies, notify, ack, act, verify |

Event matrix (all four families): relay-edge [docs/EVENT_MATRIX.md](https://github.com/zyvorai/relay-edge/blob/main/docs/EVENT_MATRIX.md) and `scripts/e2e-events-matrix.sh`.

**Full stack verification (2026-08-28, all PASS):** relay-edge [docs/TEST_RESULTS.md](https://github.com/zyvorai/relay-edge/blob/main/docs/TEST_RESULTS.md) — health probe, event matrix, Forge Decision Record path. Use `config/lab-stack.env` from relay-edge; set `GATEWAY` to this gateway's HTTPS URL.

**Relay action targets** (for farm Act evidence):

```bash
RELAY_ACTION_TARGETS=farm-controller=https://127.0.0.1:8081/v1/actions,\
firewater-controller=https://127.0.0.1:8081/v1/actions,\
remote-edge-controller=https://127.0.0.1:8081/v1/actions,\
fleet-controller=https://127.0.0.1:8081/v1/actions
```

Relay needs `RELAY_TLS_INSECURE=1` (or trust gateway cert) when action URL is HTTPS with self-signed cert.

---

## Troubleshooting

**503 on publish (relay-events):** Check `RELAY_AUTH_TOKEN`, `RELAY_BASE_URL` reachability from gateway process/pod, and Relay health.

**401 from Relay:** JWT mismatch between edge, pubsub secret, and Relay `RELAY_JWT_SECRET`.

**Farm Act fails (`mockact_*` or circuit breaker):** Relay action target wrong, gateway TLS not trusted, or `PUBSUB_TLS_SAN` missing `127.0.0.1`.

**TLS cert errors:** Use `curl -k`; ensure `PUBSUB_TLS_SAN` includes all names/IPs clients validate against before first cert generation.

## Known limitation: multi-replica action queue

`/healthz` is liveness-only. `/readyz` probes Relay when `RELAY_BACKEND=relay-events`.

Local subscribe/ACK/action-queue state persists to disk (`PUBSUB_DATA_DIR`) but is still **per-replica**. Keep `replicaCount: 1` (or sticky sessions) until Relay owns durable cursors.

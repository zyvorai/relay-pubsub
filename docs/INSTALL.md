# Installation

Install relay-pubsub from source, Docker/GHCR, systemd, or Kubernetes.

← [Docs hub](README.md) · [Testing](TESTING.md) · [Deployment](DEPLOYMENT.md)

---

## Choose a path

| Target | When to use | Guide |
|--------|-------------|-------|
| **Docker Compose** | Laptop demo (`memory` backend) | [§ Docker](#1-docker--ghcr) |
| **Cargo** | Dev / local debug | [§ Cargo](#2-cargo-from-source) |
| **systemd** | Edge / lab Linux host | [§ systemd](#3-systemd-linux-host) |
| **Kubernetes / Helm / k3s** | Cluster or edge k8s | [§ Kubernetes](#4-kubernetes--helm--k3s) |
| **Ops console** | Product UI + day-two ops | [§ Console](#5-ops-console) |

Default listeners (override with env): **HTTPS `:8080`**, **gRPCS `:50051`**. Lab systemd often uses **`:8081` / `:50061`**.

---

## 1. Docker / GHCR

### Compose (build locally)

```bash
git clone https://github.com/zyvorai/relay-pubsub.git
cd relay-pubsub
docker compose up --build
curl -k https://127.0.0.1:8080/healthz
bash scripts/smoke.sh
```

### Pull from GHCR

Images: `ghcr.io/zyvorai/relay-pubsub`

| Tag | Meaning |
|-----|---------|
| `latest` | Latest `main` build |
| `0.3.0` / `v0.3.0` | Release (semver from git tag) |
| `sha-<commit>` | Exact CI build |

```bash
docker pull ghcr.io/zyvorai/relay-pubsub:0.3.0

docker run --rm -p 8080:8080 -p 50051:50051 \
  -e RELAY_BACKEND=memory \
  -e RUST_LOG=info \
  ghcr.io/zyvorai/relay-pubsub:0.3.0
```

Production-style (needs reachable Relay):

```bash
docker run --rm -p 8080:8080 -p 50051:50051 \
  -e RELAY_BACKEND=relay-events \
  -e RELAY_BASE_URL=https://relay.example:8443 \
  -e RELAY_AUTH_TOKEN="$RELAY_AUTH_TOKEN" \
  -e RELAY_TLS_INSECURE=1 \
  -e PUBSUB_TLS_SAN=localhost,127.0.0.1 \
  -e PUBSUB_PERSIST=1 \
  -v relay-pubsub-data:/var/lib/relay-pubsub \
  ghcr.io/zyvorai/relay-pubsub:0.3.0
```

Helm default image: `ghcr.io/zyvorai/relay-pubsub` (see `deploy/helm/relay-pubsub/values.yaml`).

---

## 2. Cargo from source

```bash
git clone https://github.com/zyvorai/relay-pubsub.git
cd relay-pubsub
cargo build --release --locked
./target/release/relay-pubsub --backend memory
```

Or:

```bash
cargo run -- --backend memory
```

Env file template: [`.env.example`](../.env.example).

---

## 3. systemd (Linux host)

Units live in `deploy/systemd/`:

| Unit | Role |
|------|------|
| `relay-pubsub.service` | Gateway |
| `relay-pubsub-console.service` | Product console (optional) |
| `relay-pubsub.env.example` | Env template → `/etc/relay-pubsub/relay-pubsub.env` |

### One-shot remote install

From your laptop (needs SSH + passwordless sudo on the target):

```bash
# Full remote build on the host
bash scripts/deploy-remote.sh <host> <user>

# Or quick re-sync + rebuild
bash scripts/deploy-remote.sh <host> <user> --quick

# Optional password auth (sshpass)
bash scripts/deploy-remote.sh <host> <user> --quick --password '<pass>'
```

Makefile shortcuts:

```bash
make deploy-remote H=<host> U=<user>
make deploy-remote-quick H=<host> U=<user>
make deploy-remote-verify H=<host> U=<user>
make deploy-remote-uninstall H=<host> U=<user>
```

### What the installer does

1. Rsyncs sources (or a local release binary with `--build-local`)
2. `cargo build --release` on the host (unless binary supplied)
3. Installs `/usr/local/bin/relay-pubsub`
4. Installs systemd unit + env under `/etc/relay-pubsub/`
5. Enables and starts `relay-pubsub.service`
6. Runs `scripts/selftest.sh` (health + smoke)

### Minimal env (`/etc/relay-pubsub/relay-pubsub.env`)

```bash
PUBSUB_HTTP_ADDR=0.0.0.0:8081
PUBSUB_GRPC_ADDR=0.0.0.0:50061
PUBSUB_TLS_SAN=localhost,127.0.0.1,<host>,relay-pubsub
RELAY_BACKEND=relay-events          # or memory for offline edge
RELAY_BASE_URL=https://127.0.0.1:8443
RELAY_TLS_INSECURE=1
RELAY_AUTH_TOKEN=<jwt>
PUBSUB_PERSIST=1
PUBSUB_DATA_DIR=/var/lib/relay-pubsub/data
PUBSUB_PUSH_INTERVAL_SECONDS=2
RUST_LOG=info
```

After changing `PUBSUB_TLS_SAN`, delete `/var/lib/relay-pubsub/tls/*.pem` and restart so a new cert is minted.

```bash
systemctl status relay-pubsub
journalctl -u relay-pubsub -f
curl -k https://<host>:8081/healthz
```

---

## 4. Kubernetes / Helm / k3s

### Helm chart

Chart: `deploy/helm/relay-pubsub/` (appVersion **0.3.0**).

```bash
kubectl create namespace relay-pubsub
kubectl -n relay-pubsub create secret generic relay-pubsub-secrets \
  --from-literal=relay-auth-token="$RELAY_AUTH_TOKEN"

helm upgrade --install relay-pubsub deploy/helm/relay-pubsub \
  -n relay-pubsub \
  --set image.tag=0.3.0 \
  --set relay.backend=relay-events \
  --set relay.baseUrl=https://<relay-host>:8443 \
  --set relay.tlsInsecure=1 \
  --set tls.san="localhost,relay-pubsub,relay-pubsub.relay-pubsub.svc.cluster.local"
```

Keep `replicaCount: 1` until Relay owns durable multi-replica cursors (local queue is per-pod).

### Local k3s (memory, no Relay)

```bash
bash deploy/scripts/deploy-k3s.sh
bash deploy/scripts/ci-k3s-e2e.sh
```

### Full stack with relay-edge

From the **relay-edge** repo (sibling `relay-pubsub` required):

```bash
RELAY_AUTH_TOKEN="$(cat /tmp/lab-relay.jwt)" \
  ./deploy/scripts/deploy-k8s-remote.sh <HOST> [USER]
```

| Release | Namespace | Port |
|---------|-----------|------|
| `relay-pubsub` | `relay-pubsub` | HTTPS `:8080` |
| `relay-edge` | `relay-edge` | HTTPS `:18086` |

---

## 5. Ops console

Separate systemd unit (`relay-pubsub-console`), default **HTTPS `:8082`**.

```bash
bash scripts/deploy-console-remote.sh <host> <user> [password] \
  --gateway https://127.0.0.1:8081 \
  --project projects/demo
```

The Node `ui/server.mjs` serves the Vite build and **same-origin proxies** `/v1`, `/admin`, `/healthz`, `/readyz`, `/metrics` to the gateway so the browser only trusts one self-signed cert.

| URL | Purpose |
|-----|---------|
| `https://<host>:8082/` | Product site |
| `#generate` | Seed catalogs into memory |
| `#demo` | Live publish → pull → ack |
| `#tests` | In-browser smoke + conformance |
| `#console` | Incoming / Outgoing / Stored / Configure / **Logs** |

---

## Admin HTTP surface (ops)

All under the gateway HTTPS base (self-signed → `curl -k`):

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/healthz` | Liveness |
| GET | `/readyz` | Readiness (probes Relay when `relay-events`) |
| GET | `/metrics` | Prometheus |
| GET | `/admin/v1/topics?project=…` | List topics |
| GET | `/admin/v1/subscriptions?project=…` | List subscriptions |
| GET | `/admin/v1/inventory?project=…` | Stored counts, backlog, message peeks |
| GET | `/admin/v1/logs?limit=200&after=N` | Live process log ring buffer |
| POST | `/admin/v1/publish` | Publish (plain string `data`) |
| POST | `/admin/v1/pull` | Pull |
| POST | `/admin/v1/ack` | Acknowledge |
| POST | `/admin/v1/push-config` | Set / clear push endpoint |

Google-compatible data plane remains at `/v1/projects/...` and gRPC on the TLS port.

---

## Next

- Prove the install → [Testing](TESTING.md)
- Edge / Relay wiring → [Relay events backend](RELAY_EVENTS_BACKEND.md)
- Architecture / HA → [Architecture](ARCHITECTURE.md)

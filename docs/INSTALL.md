# Installation

Install relay-pubsub from source, Docker/GHCR, systemd, or Kubernetes.

← [Docs hub](README.md) · [Testing](TESTING.md) · [Deployment](DEPLOYMENT.md)

---

## Choose a path

| Target | When to use |
|--------|-------------|
| **Docker / GHCR** | Laptop or container runtime |
| **Cargo** | Local development |
| **systemd** | Edge / lab Linux host |
| **Kubernetes / Helm** | Cluster — gateway **+ console** + optional PVC |

**Current release: v0.4.0**

- Gateway: `ghcr.io/zyvorai/relay-pubsub:0.4.0`
- Console: `ghcr.io/zyvorai/relay-pubsub-console:0.4.0`

Default ports: gateway HTTPS **8080** / gRPCS **50051**, console HTTPS **8082** (lab systemd often **8081/50061**).

---

## Docker / GHCR

```bash
docker pull ghcr.io/zyvorai/relay-pubsub:0.4.0
docker pull ghcr.io/zyvorai/relay-pubsub-console:0.4.0

docker run --rm -p 8080:8080 -p 50051:50051 \
  -e RELAY_BACKEND=memory ghcr.io/zyvorai/relay-pubsub:0.4.0

docker run --rm -p 8082:8082 \
  -e GATEWAY_UPSTREAM=https://host.docker.internal:8080 \
  -e CONSOLE_TLS_SAN=localhost,127.0.0.1 \
  ghcr.io/zyvorai/relay-pubsub-console:0.4.0
```

Or: `docker compose up --build` then `bash scripts/smoke.sh`.

---

## Cargo

```bash
cargo build --release --locked
./target/release/relay-pubsub --backend memory
```

See [`.env.example`](../.env.example).

---

## systemd

```bash
bash scripts/deploy-remote.sh <host> <user> --quick
bash scripts/deploy-console-remote.sh <host> <user> [password]
```

Units: `relay-pubsub.service`, `relay-pubsub-console.service`. Details: [DEPLOYMENT.md](DEPLOYMENT.md).

---

## Kubernetes / Helm (gateway + console)

```bash
kubectl create namespace relay-pubsub
kubectl -n relay-pubsub create secret generic relay-pubsub-secrets \
  --from-literal=relay-auth-token="$RELAY_AUTH_TOKEN"

helm upgrade --install relay-pubsub deploy/helm/relay-pubsub \
  -n relay-pubsub \
  --set image.tag=0.4.0 \
  --set console.image.tag=0.4.0 \
  --set relay.backend=relay-events \
  --set relay.baseUrl=https://<relay-host>:8443 \
  --set persist.createPvc=true \
  --set persist.size=2Gi
```

| Service | Port |
|---------|------|
| `relay-pubsub` | 8080 HTTPS, 50051 gRPCS |
| `relay-pubsub-console` | 8082 HTTPS |

`persist.createPvc=true` (default) gives single-replica restart durability. Keep `replicaCount: 1`.

k3s smoke: `bash deploy/scripts/deploy-k3s.sh && bash deploy/scripts/ci-k3s-e2e.sh`

---

## Ops console panes

Incoming · Outgoing · Stored · Configure · Logs · Generate · Demo · Tests

Admin APIs: `/admin/v1/inventory`, `/admin/v1/logs`, `/admin/v1/push-config`, plus publish/pull/ack helpers.

---

## Next

[Testing](TESTING.md) — including `scripts/client-matrix.sh` (REST + Python + Node + Go).

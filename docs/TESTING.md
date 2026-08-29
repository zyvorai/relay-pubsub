# Testing

How to verify a relay-pubsub install — local, systemd, Kubernetes, and the ops console.

← [Docs hub](README.md) · [Installation](INSTALL.md)

---

## Quick matrix

| Gate | Command | Expect |
|------|---------|--------|
| Health | `curl -k $BASE/healthz` | `{"status":"ok",...}` |
| Ready | `curl -k $BASE/readyz` | `ok` (Relay reachable if `relay-events`) |
| Memory smoke | `BASE=$BASE bash scripts/smoke.sh` | publish + pull + ack |
| Conformance | `BASE=$BASE bash scripts/conformance-smoke.sh` | pagination, snapshots, IAM, schemas, push |
| Relay publish | `BASE=$BASE bash scripts/smoke-relay-events.sh` | one event into Relay |
| Farm catalog | `BASE=$RELAY GATEWAY=$BASE bash scripts/fasal-catalog-smoke.sh` | 10 farm types + Act |
| Unit tests | `cargo test --locked` | all pass |
| Selftest (host) | `bash scripts/selftest.sh` | binary + systemd + smoke |
| Console | open `#demo` / `#tests` / `#console` | UI green |

Set `BASE` to the gateway HTTPS URL (example: `https://127.0.0.1:8080` or lab `:8081`). Always use `curl -k` for self-signed certs.

---

## 1. Prerequisites

```bash
export BASE="${BASE:-https://127.0.0.1:8080}"
# Optional bearer if RELAY_PUBSUB_AUTH_TOKEN is set:
# export TOKEN=...
# curl -k -H "authorization: Bearer $TOKEN" ...
```

Gateway must already be running ([Installation](INSTALL.md)).

---

## 2. Health and readiness

```bash
curl -k "$BASE/healthz"
curl -k "$BASE/readyz"
curl -k "$BASE/metrics" | head
```

- `/healthz` — process up (does **not** prove Relay).
- `/readyz` — with `RELAY_BACKEND=relay-events`, also GETs Relay `/healthz`.

---

## 3. Smoke (memory / local store)

Creates topic + subscription, publishes, pulls, acks:

```bash
BASE="$BASE" bash scripts/smoke.sh
```

Use this for `memory` backend and for the **local action queue** side of `relay-events`.

---

## 4. Conformance smoke (v0.3 surface)

```bash
BASE="$BASE" bash scripts/conformance-smoke.sh
```

Covers:

- Topic/subscription create + PATCH update
- Publish / pull / ack with ordering + exactly-once flags
- Snapshots + seek
- ModifyPushConfig
- IAM get/set/test subset
- Schema create + validate
- Pagination tokens on list APIs

---

## 5. Relay events path

Requires live Relay + matching JWT:

```bash
export RELAY_BACKEND=relay-events
export RELAY_BASE_URL=https://127.0.0.1:8443
export RELAY_AUTH_TOKEN=<jwt>
export RELAY_TLS_INSECURE=1

BASE="$BASE" bash scripts/smoke-relay-events.sh

BASE=https://127.0.0.1:8443 GATEWAY="$BASE" \
  bash scripts/fasal-catalog-smoke.sh
```

Full cross-family matrix (farm / edge / remote-edge / fleet): from **relay-edge**:

```bash
../relay-edge/scripts/e2e-events-matrix.sh
```

Stack results: [relay-edge TEST_RESULTS.md](https://github.com/zyvorai/relay-edge/blob/main/docs/TEST_RESULTS.md).

---

## 6. Admin inventory and logs

Non-consuming ops checks used by the console:

```bash
# What's stored (counts, backlog, recent payloads)
curl -k "$BASE/admin/v1/inventory?project=projects/demo" | jq .

# Live process logs (ring buffer)
curl -k "$BASE/admin/v1/logs?limit=50" | jq .

# Incremental tail
curl -k "$BASE/admin/v1/logs?after=10&limit=50" | jq .

# Publish via admin helper
curl -k -X POST "$BASE/admin/v1/publish" \
  -H 'content-type: application/json' \
  -d '{"topic":"projects/demo/topics/orders","data":"{\"ok\":true}"}'

# Push config
curl -k -X POST "$BASE/admin/v1/push-config" \
  -H 'content-type: application/json' \
  -d '{"subscription":"projects/demo/subscriptions/orders-worker","push_endpoint":"https://example.com/push"}'
```

---

## 7. Ops console (browser)

With console on `:8082` (proxied to gateway):

1. Open `https://<host>:8082/` (accept self-signed once). First visit shows a **Get started** checklist (Skip/Done dismisses it).
2. **Generate** — seed farm/edge/demo/test catalogs.
3. **Demo** — one-click publish → pull → ack.
4. **Tests** — in-page health / smoke / conformance runners.
5. **Console**
   - **Incoming** — publish into a topic
   - **Outgoing** — pull / live / push endpoint / ack
   - **Stored** — inventory peek (does not consume); empty state can reopen the setup guide
   - **Configure** — create/delete resources; **Show setup guide**
   - **Logs** — live gateway log tail (filter by level/text)

```bash
curl -k https://<host>:8082/healthz          # proxied
curl -k https://<host>:8082/admin/v1/logs?limit=5
```

---

## 8. Unit / crate tests

```bash
cargo test --locked
cargo test --locked relay_events_backend action_gateway
cargo test --locked memory
```

---

## 9. systemd host selftest

After `deploy-remote.sh`, or manually on the host:

```bash
bash scripts/selftest.sh
# or
make deploy-remote-verify H=<host> U=<user>
```

Checks: binary on PATH, unit active, ports listening, `/healthz` + `/readyz`, `scripts/smoke.sh`.

---

## 10. Kubernetes

```bash
# Local k3s memory deploy
bash deploy/scripts/deploy-k3s.sh
bash deploy/scripts/ci-k3s-e2e.sh

# Stack (from relay-edge checkout on the node)
bash deploy/scripts/k8s-e2e.sh
```

Port-forward example:

```bash
kubectl -n relay-pubsub port-forward svc/relay-pubsub 8080:8080
BASE=https://127.0.0.1:8080 bash scripts/smoke-relay-events.sh
```

---

## 11. Client conformance matrix (v0.4)

```bash
BASE=https://127.0.0.1:8080 bash scripts/client-matrix.sh
```

Lanes: health, inventory/logs, `smoke.sh`, `conformance-smoke.sh`, Python REST, Node REST, Go REST; optional google-cloud-pubsub gRPC (skipped if SDK/cert incompatible).

CI runs this matrix on every PR (memory gateway).

---

## 12. Release / image smoke

```bash
docker run --rm -d --name rp -p 8080:8080 -p 50051:50051 \
  -e RELAY_BACKEND=memory ghcr.io/zyvorai/relay-pubsub:0.4.0
sleep 2
BASE=https://127.0.0.1:8080 bash scripts/client-matrix.sh
docker rm -f rp
```

---

## Suggested acceptance checklist (release)

- [ ] `cargo test --locked`
- [ ] `scripts/client-matrix.sh` (0 fail)
- [ ] Browser `#demo` + `#tests` (9/9)
- [ ] systemd gateway + console
- [ ] Helm gateway + console + PVC
- [ ] GHCR `relay-pubsub:0.4.0` and `relay-pubsub-console:0.4.0`
# Zyvor Relay Pub/Sub Gateway

A **Google Cloud Pub/Sub compatibility gateway for Zyvor Relay**.

This is intentionally a separate Relay component: applications speak familiar Google Pub/Sub gRPC/REST APIs to the gateway, while the gateway delegates durable messaging to Zyvor Relay through a small `RelayBackend` interface.

```text
Google Pub/Sub SDK / REST
          |
          v
+-------------------------+
| relay-pubsub            |
| google.pubsub.v1        |
| gRPC :50051             |
| REST :8080              |
+-----------+-------------+
            |
            | RelayBackend
            v
+-------------------------+
| Zyvor Relay             |
| topics / streams        |
| subscriptions / cursors |
| ACK / retry / DLQ       |
| replay / replication    |
+-------------------------+
```

## What is implemented

### Google-compatible gRPC methods

**Publisher**
- `CreateTopic`
- `GetTopic`
- `ListTopics`
- `DeleteTopic`
- `Publish`

**Subscriber**
- `CreateSubscription`
- `GetSubscription`
- `ListSubscriptions`
- `DeleteSubscription`
- `Pull`
- `StreamingPull`
- `Acknowledge`
- `ModifyAckDeadline`
- `Seek` by timestamp

The service/package names are exactly `google.pubsub.v1.Publisher` and `google.pubsub.v1.Subscriber`, so an official Pub/Sub client in emulator/plaintext mode can address the gateway.

### Google-style REST methods

- `PUT/GET/DELETE /v1/projects/{project}/topics/{topic}`
- `GET /v1/projects/{project}/topics`
- `POST /v1/projects/{project}/topics/{topic}:publish`
- `PUT/GET/DELETE /v1/projects/{project}/subscriptions/{subscription}`
- `GET /v1/projects/{project}/subscriptions`
- `POST .../{subscription}:pull`
- `POST .../{subscription}:acknowledge`
- `POST .../{subscription}:modifyAckDeadline`
- `POST .../{subscription}:seek`

### Messaging semantics in the embedded demo backend

- subscriptions start at messages published after subscription creation
- explicit ACK
- NACK by `ModifyAckDeadline(..., 0)`
- ack-deadline expiration and redelivery
- delivery-attempt counting
- dead-letter topic after configured attempts
- timestamp replay / seek
- ordering-key preservation
- attributes/metadata
- message IDs and publish timestamps

### Operations

- `/healthz`
- `/readyz`
- `/metrics` (Prometheus)
- optional bearer auth
- Dockerfile
- Docker Compose
- SSH remote deploy + systemd unit (bare Linux host)
- Kubernetes manifest
- Helm chart + local k3s test workflow
- GitHub Actions CI (build/test + image release + k3s E2E)
- React/Vite operations console

## Backends

### 1. Memory backend — immediate demo/test

```bash
cargo run -- --backend memory
```

Nothing else is needed. This backend is intentionally ephemeral and is not a production message store.

### 2. Zyvor Relay HTTP backend — production integration

```bash
export RELAY_BACKEND=http
export RELAY_BASE_URL=http://relay.zyvor-system.svc:9090
export RELAY_AUTH_TOKEN='...'
cargo run
```

The exact Relay-native API expected by this adapter is documented in [`docs/relay-native-api.md`](docs/relay-native-api.md) — this is an **invented** contract (`/v1/topics`, `/v1/messages:publish|pull|ack`, etc.), not Relay's real, already-shipped API. Use it only if Relay's real API is later changed to match it.

### 3. Zyvor Relay events backend — targets Relay's real API today

```bash
export RELAY_BACKEND=relay-events
export RELAY_BASE_URL=https://relay.example.com
export RELAY_AUTH_TOKEN='...'
cargo run
```

Unlike the HTTP backend above, this targets Relay's real, already-shipped event-lifecycle API (`POST /v1/events`) and Action Gateway contract (`POST /v1/actions`) directly — see [`docs/RELAY_EVENTS_BACKEND.md`](docs/RELAY_EVENTS_BACKEND.md). This is the backend to use for the Fasal on-prem integration.

## Quick start with Docker Compose

```bash
docker compose up --build
```

Endpoints (gRPC and REST are TLS-only — see [TLS](#tls) below):

- gRPCS: `127.0.0.1:50051`
- REST/admin: `https://127.0.0.1:8080`
- UI: `http://127.0.0.1:3000`

Then:

```bash
bash scripts/smoke.sh
```

## Deploying to Linux

Full reference (flags, config, troubleshooting, and a record of currently-deployed instances): [`docs/DEPLOYMENT.md`](docs/DEPLOYMENT.md).

### Bare host via systemd

```bash
bash scripts/deploy-remote.sh <host> <user> --build-local --quick
# or: make deploy-remote-quick H=<host> U=<user>
```

This builds a release binary locally (cross-building via Docker if the operator's machine isn't Linux), copies it to the target Debian/Ubuntu host over SSH, and installs it as a hardened `relay-pubsub.service` systemd unit. See `scripts/deploy-remote.sh --help` for `--verify-only`, `--uninstall`, `--dry-run`, and `--fleet` (multi-host) options, and `deploy/systemd/` for the unit file and env template.

Verify a deployment:

```bash
make deploy-remote-verify H=<host> U=<user>        # runs scripts/selftest.sh remotely
BASE="https://<host>:8080" bash scripts/smoke.sh   # functional publish/pull round-trip (self-signed cert — smoke.sh uses curl -k)
```

### Kubernetes pods

`deploy/k8s/gateway.yaml` (plain manifest) and `deploy/helm/relay-pubsub/` (Helm chart) both deploy `relay-pubsub` as a 2-replica Deployment + Service. The image is built and pushed to `ghcr.io/zyvorai/relay-pubsub` by the `release-image` GitHub Actions workflow.

To test the pod deployment end-to-end on a disposable local k3s cluster (no real cluster or Relay backend needed):

```bash
bash deploy/scripts/deploy-k3s.sh     # installs k3s, builds+imports the image, helm installs
bash deploy/scripts/ci-k3s-e2e.sh     # rollout status + scripts/smoke.sh against the Service
```

This is also what the `k3s-e2e` GitHub Actions workflow runs on PRs touching `deploy/**`.

## TLS

The gRPC and REST listeners are **TLS-only** (gRPCS/HTTPS) — the gateway terminates TLS itself, no reverse proxy required. On first start, if `PUBSUB_TLS_CERT`/`PUBSUB_TLS_KEY` don't already exist, a self-signed cert/key pair is generated and persisted there (`/var/lib/relay-pubsub/tls/{cert,key}.pem` by default) and reused on every restart. Point `PUBSUB_TLS_CERT`/`PUBSUB_TLS_KEY` at a CA-signed cert instead if you have one. `PUBSUB_TLS_SAN` (comma-separated) sets the generated cert's hostnames/IPs — only takes effect the first time a cert is generated.

Clients that don't trust the self-signed cert need to skip verification: `curl -k`, `grpcurl -insecure`, etc.

**Caveat:** because gRPC is TLS-only, Google's official client libraries in **emulator/plaintext mode** (`PUBSUB_EMULATOR_HOST=...`) can no longer reach the gateway — that mode forces a plaintext channel in the SDK itself. `examples/python_google_client.py` needs a real TLS-aware channel (e.g. `grpc.secure_channel` with the generated cert as a trusted root) instead of `PUBSUB_EMULATOR_HOST` to work against this build.

## REST example

Self-signed cert by default — add `-k` to skip curl's certificate verification (or point `--cacert` at the generated `cert.pem`).

Create a topic:

```bash
curl -k -X PUT https://localhost:8080/v1/projects/demo/topics/orders \
  -H 'content-type: application/json' \
  -d '{"labels":{"team":"payments"}}'
```

Create a subscription:

```bash
curl -k -X PUT https://localhost:8080/v1/projects/demo/subscriptions/orders-worker \
  -H 'content-type: application/json' \
  -d '{"topic":"projects/demo/topics/orders","ackDeadlineSeconds":20,"enableMessageOrdering":true}'
```

Publish:

```bash
curl -k -X POST https://localhost:8080/v1/projects/demo/topics/orders:publish \
  -H 'content-type: application/json' \
  -d '{"messages":[{"data":"aGVsbG8=","attributes":{"region":"in"},"orderingKey":"customer-17"}]}'
```

Pull:

```bash
curl -k -X POST https://localhost:8080/v1/projects/demo/subscriptions/orders-worker:pull \
  -H 'content-type: application/json' \
  -d '{"maxMessages":10}'
```

## Authentication

For local Google-client compatibility, leave `RELAY_PUBSUB_AUTH_TOKEN` unset.

To require a bearer token at the gateway:

```bash
export RELAY_PUBSUB_AUTH_TOKEN='gateway-secret'
```

For production, place identity-aware authentication in front of the gateway (OIDC/mTLS/API gateway) and map Google project names to authenticated Relay tenants. A literal `projects/foo` string must never be treated as proof of tenant identity.

## Repository layout

```text
.
├── proto/google/pubsub/v1/pubsub.proto
├── src/
│   ├── backend.rs               # vendor-neutral RelayBackend trait
│   ├── memory.rs                # runnable demo backend + tests
│   ├── http_backend.rs          # adapter into invented Relay-native API
│   ├── relay_events_backend.rs  # adapter into Relay's real API (docs/RELAY_EVENTS_BACKEND.md)
│   ├── action_gateway.rs        # POST /v1/actions receiver for relay-events
│   ├── grpc.rs                  # Google Pub/Sub gRPC compatibility
│   ├── rest.rs                  # Google REST + admin endpoints
│   ├── metrics.rs
│   └── main.rs
├── ui/                     # React/Vite Relay console
├── deploy/k8s/             # plain k8s manifest (real Relay backend)
├── deploy/helm/            # Helm chart (k8s manifest + local k3s smoke test)
├── deploy/systemd/         # systemd unit + env template for bare-host deploys
├── deploy/scripts/         # k3s install/deploy/e2e-verify scripts
├── docs/DEPLOYMENT.md      # full deploy reference + live-instance record
├── examples/
└── scripts/                # deploy-remote.sh, selftest.sh, smoke.sh, fasal-catalog-smoke.sh
```

## Important production boundary

This repository is a **complete runnable compatibility MVP**, not a claim of 100% Google Pub/Sub behavioral parity.

Before calling it drop-in production parity, add/verify:

1. exact official Google proto surface rather than the deliberately minimal wire-compatible subset;
2. snapshots and snapshot-based `Seek`;
3. push-subscription delivery worker and authenticated push;
4. Pub/Sub schema APIs and schema validation;
5. update masks / update methods and IAM APIs if customers require them;
6. full pagination tokens;
7. Google exactly-once edge semantics and durable ACK IDs across gateway failover;
8. ordering-key serialization across multiple consumers/gateway replicas;
9. quota/error/detail compatibility;
10. conformance tests against the official Pub/Sub emulator and selected Google client libraries.

Those belong in this compatibility project. Durable replication, persistence, tenant isolation and storage durability belong in **Zyvor Relay core**.

## Recommended repo relationship

```text
zyvorai/relay                 # existing/core product
zyvorai/relay-gateway         # future common protocol gateway framework
zyvorai/relay-pubsub          # this repository
```

As Kafka, NATS, MQTT, SQS/SNS or Azure Service Bus adapters are added, extract the reusable auth/tenant/metrics/backend pieces into `relay-gateway` without contaminating Relay core with vendor-specific APIs.

## License

[Apache-2.0](LICENSE) · Copyright 2026 Zyvor AI Labs

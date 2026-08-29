# Changelog

## [0.4.0] — 2026-08-29

### Added
- **Ops console in Kubernetes**: Helm `console.enabled` Deployment/Service; image `ghcr.io/zyvorai/relay-pubsub-console`.
- Console Dockerfile (`ui/Dockerfile.console`) with Node `server.mjs` same-origin proxy + auto self-signed TLS.
- Helm **PVC auto-create** (`persist.createPvc`) for single-replica restart durability.
- **Client conformance matrix**: `scripts/client-matrix.sh` (REST, conformance, Python, Node, Go lanes).
- Examples: `examples/node_rest_client.mjs`, `examples/go_rest_client`, `examples/python_google_client_tls.py`.
- CI job runs the client matrix against an in-process memory gateway.

### Changed
- Chart / images / crate version **0.4.0**.
- Release workflow builds gateway **and** console GHCR images.

### Notes
- Multi-replica shared cursors still belong in Relay core — keep `replicaCount: 1` with PVC.
- Official Google gRPC SDKs vs self-signed certs remain lab-sensitive; REST lanes are the certified path.

## [0.3.0] — 2026-08-29

### Added
- Google Pub/Sub surface expansion: Update* APIs, snapshots + seek, ModifyPushConfig, IAM subset, SchemaService, list pagination.
- Memory semantics: ordering keys, exactly-once ack leases, retry backoff, optional JSON persistence (`PUBSUB_PERSIST` / `PUBSUB_DATA_DIR`).
- Background push dispatcher (`PUBSUB_PUSH_INTERVAL_SECONDS`).
- Optional gateway auth: static bearer + OIDC JWKS (`src/auth.rs`).
- Admin APIs: `/admin/v1/inventory`, `/admin/v1/logs`, `/admin/v1/push-config`.
- `/readyz` probes Relay when using `relay-events`.
- Product console: Incoming / Outgoing / Stored / Configure / Logs; Generate catalogs; same-origin API proxy (`ui/server.mjs`).
- Conformance smoke: `scripts/conformance-smoke.sh`.
- Docs: [Installation](docs/INSTALL.md), [Testing](docs/TESTING.md).

### Changed
- Standalone `[workspace]` so nested lab deploys under `~/.deployments` do not inherit a parent Cargo workspace.
- Helm chart appVersion **0.3.0**; default image `ghcr.io/zyvorai/relay-pubsub`.

### Notes
- Multi-replica durable cursors still belong in Relay core — keep `replicaCount: 1` for the local action queue.
- GHCR: `ghcr.io/zyvorai/relay-pubsub:0.3.0` (and `latest` from `main`).

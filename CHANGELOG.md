# Changelog

## [0.3.0] — 2026-08-29

### Added
- Google Pub/Sub surface expansion: Update* APIs, snapshots + seek, ModifyPushConfig, IAM subset, SchemaService, list pagination.
- Memory semantics: ordering keys, exactly-once ack leases, retry backoff, optional JSON persistence (`PUBSUB_PERSIST` / `PUBSUB_DATA_DIR`).
- Background push dispatcher (`PUBSUB_PUSH_INTERVAL_SECONDS`).
- Optional gateway auth: static bearer + OIDC JWKS (`src/auth.rs`).
- Admin APIs: `/admin/v1/inventory` (stored / backlog / peek), `/admin/v1/logs` (process log ring buffer), `/admin/v1/push-config`.
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

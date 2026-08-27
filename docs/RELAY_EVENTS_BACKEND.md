# Relay events backend

`--backend relay-events` (`RELAY_BACKEND=relay-events`) targets Zyvor Relay's real, already-shipped API instead of the invented topics/subscriptions contract `http_backend.rs` speaks (see [`docs/relay-native-api.md`](relay-native-api.md) and [`docs/ARCHITECTURE.md`](ARCHITECTURE.md) for that older path). Implementation: [`src/relay_events_backend.rs`](../src/relay_events_backend.rs) + [`src/action_gateway.rs`](../src/action_gateway.rs).

This is the same wire contract and rationale as [zyvor/relay's `examples/fasal-pubsub-gateway`](https://github.com/zyvorai/relay) (a Go implementation of the same idea, built first) — this backend brings that same real-Relay integration to this project's more complete Google Pub/Sub protocol layer (DLQ, seek-to-time, real StreamingPull, metrics, CI, Helm/systemd/k3s deploy tooling).

## Wire contract

Relay's own API has no topics/subscriptions — it's event-lifecycle-shaped. So the contract is defined here, not reverse-engineered from Relay:

- **Topic name is the Relay event type.** Publish to a topic literally named `irrigation.required`, `disease.risk.critical`, etc. — the fixed catalog in `src/config.rs`'s `FASAL_CATALOG`, matching `docs/FASAL_ACCOMMODATION.md` #4.1/#4.2 in the zyvor/relay repo. Publishing to any topic other than the actions topic (below) forwards to Relay regardless of catalog membership — the catalog is only pre-registered at startup for admin-UI visibility.
- **Message attributes/data map onto Relay's event schema:** attribute `severity` -> event severity, attribute `source` -> event source, attribute `idempotency_key` -> event idempotency key (falls back to a hash of the message content — topic + source + severity + data — not the Pub/Sub message ID, so a retried publish of the same logical message still dedupes at Relay), message `data` (JSON) -> event `data`.

## Inbound: publish -> Relay events

Publish via gRPC (`Publisher.publish`) or REST (`POST /v1/projects/{project}/topics/{topic}:publish`) to any topic other than the actions topic. Each publish becomes `POST {RELAY_BASE_URL}/v1/events`.

## Outbound: Relay actions -> pull/ack

This backend also mounts a new route, `POST /v1/actions`, implementing Relay's Action Gateway contract (required `Idempotency-Key`, idempotent retries return the same result). Point Relay at it:

```bash
RELAY_ACTION_TARGETS=farm-controller=https://<this-host>:<PUBSUB_HTTP_ADDR-port>/v1/actions
```

This gateway's REST listener is TLS-only (see the [TLS section in the README](../README.md#tls)) — if it's using the default self-signed cert, whatever calls this URL (Relay itself) needs to skip certificate verification against it, the same way this backend's own outbound calls to Relay can via `RELAY_TLS_INSECURE` below.

Each action is enqueued as a message on the actions topic (`FASAL_ACTIONS_TOPIC`, default `farm-actions`) via the backend's normal `publish` — which for that one topic store-and-forwards locally (an inner `MemoryBackend`) instead of calling Relay, since this is the Relay -> consumer direction. The gateway returns 2xx immediately: a 2xx means "durably accepted," not "physically executed," matching the Action Gateway contract's own model — verification stays Relay's separate telemetry-probe step, unaffected by this backend. Consumers pull via `Subscriber.Pull` or `StreamingPull` and ack normally — this gets `MemoryBackend`'s DLQ-after-max-attempts and nack/redelivery for free.

## Config

| Var | Default | Notes |
|---|---|---|
| `RELAY_BACKEND` | `memory` | Set to `relay-events` |
| `RELAY_BASE_URL` | `http://relay:9090` | Relay's real REST API |
| `RELAY_AUTH_TOKEN` | unset | Sent as `?token=demo-token` (Relay demo mode) if literally `demo-token`, else `Authorization: Bearer` |
| `RELAY_TLS_INSECURE` | `false` (`1`/`true`/`yes` to enable) | Skip certificate verification on this backend's outbound HTTP client to `RELAY_BASE_URL` — needed if Relay itself is running with a self-signed/internal cert |
| `FASAL_GCP_PROJECT` | `fasal-onprem` | Project segment in Pub/Sub resource names |
| `FASAL_ACTIONS_TOPIC` / `FASAL_ACTIONS_SUBSCRIPTION` | `farm-actions` / `farm-actions-sub` | Outbound action queue, pre-created at startup |

## Open item

The exact schema Fasal's own publishing code produces is not yet confirmed against this contract — same open item as the Go gateway. `RelayEventsBackend::accept_event` is intentionally the single place that would need to change.

## Tests

```bash
cargo test --release relay_events_backend
cargo test --release action_gateway
```

`publish_all_catalog_event_types` covers all 10 `FASAL_CATALOG` entries, not just `irrigation.required`. For a real end-to-end check against a running Relay + this binary:

```bash
BASE=http://127.0.0.1:8080 GATEWAY=https://127.0.0.1:8083 ./scripts/fasal-catalog-smoke.sh
```

---
hero:
  eyebrow: ARCHITECTURE
  title: Architecture
  lead: >-
    relay-pubsub owns protocol compatibility. Relay owns the durable event
    system. Here's how the boundary holds — and where relay-edge fits.
  highlights:
    - {value: "5", label: "Authenticated request steps from external caller to a scoped Relay identity"}
    - {value: "4", label: "Compatibility milestones already shipped — v0.1 through v0.4"}
    - {value: "2+", label: "Stateless gateway replicas recommended behind an HTTP/2-capable load balancer"}
    - {value: "v0.5", label: "Next milestone — multi-replica durable cursors move into Relay core"}
---

← [Docs hub](README.md)

---

`relay-pubsub` owns protocol compatibility. `relay` owns the durable event system.

```text
+-------------------+      +--------------------+      +-------------------+
| Google SDK / REST | ---> | relay-pubsub       | ---> | Zyvor Relay       |
|                   |      |                    |      |                   |
| Publisher         |      | protocol adapter   |      | replicated log    |
| Subscriber        |      | auth/tenant map    |      | cursors / ACK     |
| StreamingPull     |      | error translation  |      | DLQ / replay      |
+-------------------+      +--------------------+      +-------------------+
```

## Take a closer look

=== "Why a backend trait"

    `RelayBackend` prevents Google-specific request types from entering Relay core. The gRPC and REST adapters translate external API objects into `TopicSpec`, `SubscriptionSpec`, `NewMessage` and `Delivery`.

    The memory implementation makes conformance testing deterministic. The HTTP implementation is the production bridge. A future native gRPC Relay backend can implement the same trait without changing the Pub/Sub surface.

=== "HA model"

    Compatibility gateways should be stateless. Run 2+ replicas behind an HTTP/2-capable load balancer. ACK IDs, cursors and exactly-once state must be durable in Relay, not process memory. The included memory backend is therefore only for tests/demos.

    Each gateway process terminates its own TLS (gRPCS/HTTPS). In Kubernetes, mount a shared TLS secret or accept per-pod self-signed certs (current Helm default uses `emptyDir` — fine for single-replica lab stacks).

    Local topic/subscription/action-queue state can be persisted to `PUBSUB_DATA_DIR/state.json` (`PUBSUB_PERSIST=1`, default on). That survives process restart on a single replica; multi-replica HA still requires sticky routing or durable state in Relay core.

    **relay-edge** follows the same pattern: optional `EDGE_TLS=1` with self-signed cert in `internal/tlsutil`, deployed alongside relay-pubsub via relay-edge `deploy/scripts/deploy-k8s-remote.sh`.

=== "Tenant model"

    External Pub/Sub resource names contain `projects/<project>`, but that string is not authorization. Production flow:

    1. authenticate caller — static bearer (`RELAY_PUBSUB_AUTH_TOKEN`) and/or OIDC JWT (`PUBSUB_OIDC_JWKS_URL` + audience/issuer);
    2. map identity to allowed projects (`PUBSUB_ALLOWED_PROJECTS`, `PUBSUB_IDENTITY_PROJECT_MAP`);
    3. authorize each resource operation against that allowlist;
    4. map the external Google-style project name to the internal Relay namespace;
    5. call Relay using a scoped service identity (`RELAY_AUTH_TOKEN`).

    mTLS / workload-identity passthrough can sit in front of the gateway; the gateway itself validates Bearer credentials today.

## Integration stack

```text
relay-edge (stamp + simulators)
       │  HTTPS, topic = event type
       ▼
relay-pubsub (relay-events backend)
       │  POST /v1/events
       ▼
Zyvor Relay (Accept → Act via POST /v1/actions → gateway)
```

See relay-edge [Event matrix](https://github.com/zyvorai/relay-edge/blob/main/docs/EVENT_MATRIX.md) for the full cross-family test gate.

## Compatibility roadmap

- ~~v0.1: core Publisher/Subscriber data path, REST, StreamingPull, time replay, DLQ~~
- ~~v0.2: official proto expansion, push dispatcher, snapshots, pagination, update APIs~~
- ~~v0.3: schema service, IAM compatibility subset, exactly-once + ordering + retry backoff, push dispatcher, admin inventory/logs, product console~~
- ~~v0.4: client conformance matrix (REST + Python/Node/Go), Helm console + PVC~~
- Unreleased (gateway): subscription attribute filters, topic schema enforcement, publish dedup, CloudEvents attribute projection — see [FILTERS.md](FILTERS.md)
- v0.5: multi-replica durable cursors in Relay core; broader official SDK TLS matrix
- Ongoing: durable ACK/cursors in Relay core (gateway persists local queue to disk today via `PUBSUB_DATA_DIR`)

# Architecture

## Boundary

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

## Why a backend trait

`RelayBackend` prevents Google-specific request types from entering Relay core. The gRPC and REST adapters translate external API objects into `TopicSpec`, `SubscriptionSpec`, `NewMessage` and `Delivery`.

The memory implementation makes conformance testing deterministic. The HTTP implementation is the production bridge. A future native gRPC Relay backend can implement the same trait without changing the Pub/Sub surface.

## HA model

Compatibility gateways should be stateless. Run 2+ replicas behind an HTTP/2-capable load balancer. ACK IDs, cursors and exactly-once state must be durable in Relay, not process memory. The included memory backend is therefore only for tests/demos.

## Tenant model

External Pub/Sub resource names contain `projects/<project>`, but that string is not authorization. Production flow should be:

1. authenticate caller (OIDC, mTLS, workload identity or trusted edge proxy);
2. map identity/credential to a Relay tenant;
3. authorize resource operation against that tenant;
4. map the external Google-style project name to the internal Relay namespace;
5. call Relay using a scoped service identity.

## Compatibility roadmap

- v0.1: core Publisher/Subscriber data path, REST, StreamingPull, time replay, DLQ
- v0.2: official full proto set, push dispatcher, snapshots, pagination, update APIs
- v0.3: schema service, IAM compatibility subset, exact-once conformance
- v0.4: multi-language Google client conformance matrix and migration tooling

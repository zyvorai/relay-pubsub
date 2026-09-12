---
hero:
  eyebrow: FAQ
  title: FAQ
---

Questions people evaluating relay-pubsub actually ask, before they've
decided to adopt it.

## Licensing & cost

**Is it really free?** Yes. Apache-2.0 — use, modify, and run it for
personal, lab, and commercial production use at no charge, subject to
preserving notices. See the README's [License](https://github.com/zyvorai/relay-pubsub#license)
section.

**What does "Enterprise" mean here?** Production support, SLAs, and
Zyvor's other commercial products are licensed separately. Contact
sales@zyvor.dev. Nothing in this repository requires it.

## Support

**What if I find a bug?** Open a GitHub issue.

**What if I find a security vulnerability?** This repo has no dedicated
`SECURITY.md` today — open an issue and flag it clearly as
security-sensitive rather than including exploit details in a public
issue.

## Scope

**Does this replace Zyvor Relay?** No — it's a thin, stateless
protocol-translation gateway in front of it. Per
[`docs/ARCHITECTURE.md`](ARCHITECTURE.md): "relay-pubsub owns protocol
compatibility. relay owns the durable event system." The production
(`relay-events`) and legacy (`http`) backends both require Relay to be
running; only the `memory` backend works standalone, for demos/CI.

**Does it work with real Google Cloud Pub/Sub tooling?** Yes — it
implements the Pub/Sub gRPC and REST surface (topics/subscriptions,
Publish, Pull, StreamingPull, Ack/ModifyAckDeadline, Seek, Snapshots, an
IAM subset, SchemaService), so existing Google SDK code can point at it
instead. It does not talk to Google's actual infrastructure — messages
route into Relay's own event system.

**Does it replace NATS/RabbitMQ/Kafka?** No — it's not a general-purpose
broker; it's a narrow compatibility shim for one specific API shape
(Pub/Sub) in front of one specific backend (Relay).

## Production readiness

**Is this production-ready?** Current release is v0.4.0. Read the
README's "Production boundary" section carefully: it "targets Google
Pub/Sub compatibility through v0.3... Multi-replica durable cursors still
belong in Relay core" — meaning HA/durability beyond a single gateway
replica isn't a property of relay-pubsub itself yet.

**Is TLS required?** Yes — the gateway is TLS-only, with a self-signed
certificate generated automatically if none is configured. See the
README's "TLS — no reverse proxy needed" section.

## Integration

**How does this relate to relay-edge?** They're designed to pair —
[relay-edge](https://github.com/zyvorai/relay-edge) generates stamped farm
events and industrial-simulator traffic that can publish through this same
gateway.

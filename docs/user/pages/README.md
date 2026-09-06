# Page-by-page guides

Each guide follows: Purpose → When to use it → How to get there → Operate from the console (UX) → Related pages.

Every route is also listed in the [complete page index](../PAGE_INDEX.md).

## Guides

| Page | What it covers |
|------|----------------|
| [Configure](guides/configure.md) | Create or delete topics/subscriptions. Show setup guide link available here. |
| [Live demo](guides/demo.md) | Publish → pull → ack against the real gateway. Generate Demo catalog first if the topic is missing. |
| [Generate](guides/generate.md) | Seed Demo or Farm catalogs into memory so Demo and Tests have topics to use. |
| [Incoming](guides/incoming.md) | Publish JSON payloads into a selected topic. |
| [Logs](guides/logs.md) | Tail gateway logs with level/text filters. |
| [Outgoing](guides/outgoing.md) | Pull deliveries, enable live receive, ack messages. |
| [Stored](guides/stored.md) | Peek topic message counts and payloads without consuming. Empty state can reopen the setup guide. |
| [Tests](guides/tests.md) | Run healthz, smoke, and conformance from the browser. |

---

8 guides. Regenerate: `node scripts/user-docs/generate-guide-index.mjs`.

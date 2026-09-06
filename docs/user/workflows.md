# Common workflows

## Purpose

1. **Publish → Relay Accept** — REST/gRPC publish maps topic → event type.
2. **Act → pull** — Relay POSTs `/v1/actions`; consumer pulls the actions subscription.
3. **Stored inventory** — Generate catalogs, then open Console → Stored.
4. **Empty Stored** — use **Show setup guide** or Generate Demo catalog.

## When to use it

- Open **Common workflows** when the job matches this screen
- Prefer the product home / Get started panel if you are unsure where to begin
- Confirm health and auth tokens if probes fail

## How to get there

- UI path: `/ui/` → **Common workflows** (or matching nav tab)
- Spotlight / in-app links when available

## Operate from the console (UX)

1. Open the relay-pubsub UI (`/ui/`) on `https://<host>:…` (see Admin basics for the default port).
2. Navigate to **Common workflows**.
3. Complete the on-screen fields / actions for this surface (1. **Publish → Relay Accept** — REST/gRPC publish maps topic → event type.
2. **Act → pull** — Relay POSTs `/v1/actions`…).
4. Use **Probe** / **Save** / **Send** (or the primary button on the page) and watch status chips.
5. **Empty / fail:** Check Admin basics env vars, JWT/`API_TOKEN`, TLS insecure for lab certs, and backend reachability.
6. **Success:** Status shows healthy / accepted; related Lab or Logs surfaces reflect the change.

Never publish lab IPs — use `<host>`.

## Related pages

- [Getting Started](../../getting-started.md)
- [Using the Dashboard](../../using-the-dashboard.md)
- [Admin basics](../../admin-basics.md)
- [Page index](../../PAGE_INDEX.md)

# Getting started — relay-pubsub

## Purpose

Full stack order: **[Relay stack day-0 onboarding](/docs/relay-stack-onboarding)**.
1. Deploy the gateway (`scripts/deploy-remote.sh`) with `RELAY_BACKEND=relay-events` and `RELAY_BASE_URL=https://<relay-host>:…`.
2. Deploy the console: `scripts/deploy-console-remote.sh <host> <user> --gateway https://127.0.0.1:8081` (upstream is loopback **on the console host** when gateway is co-located).
3. Open `https://<host>:8082/` — accept the self-signed cert.
4. Complete the **Get started** panel (or Skip): Generate → Demo → Stored.
5. Confirm status shows **Connected**.
Relay JWT is configured on the **gateway**, not in the browser.

## When to use it

- Open **Getting started — relay-pubsub** when the job matches this screen
- Prefer the product home / Get started panel if you are unsure where to begin
- Confirm health and auth tokens if probes fail

## How to get there

- UI path: `/ui/` → **Getting started — relay-pubsub** (or matching nav tab)
- Spotlight / in-app links when available

## Operate from the console (UX)

1. Open the relay-pubsub UI (`/ui/`) on `https://<host>:…` (see Admin basics for the default port).
2. Navigate to **Getting started — relay-pubsub**.
3. Complete the on-screen fields / actions for this surface (Full stack order: **[Relay stack day-0 onboarding](/docs/relay-stack-onboarding)**.
1. Deploy the gateway (`scripts/depl…).
4. Use **Probe** / **Save** / **Send** (or the primary button on the page) and watch status chips.
5. **Empty / fail:** Check Admin basics env vars, JWT/`API_TOKEN`, TLS insecure for lab certs, and backend reachability.
6. **Success:** Status shows healthy / accepted; related Lab or Logs surfaces reflect the change.

Never publish lab IPs — use `<host>`.

## Related pages

- [Getting Started](../../getting-started.md)
- [Using the Dashboard](../../using-the-dashboard.md)
- [Admin basics](../../admin-basics.md)
- [Page index](../../PAGE_INDEX.md)

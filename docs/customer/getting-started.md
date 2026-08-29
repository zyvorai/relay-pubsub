# Getting started — relay-pubsub

Full stack order: **[Relay stack day-0 onboarding](/docs/relay-stack-onboarding)**.

1. Deploy the gateway (`scripts/deploy-remote.sh`) with `RELAY_BACKEND=relay-events` and `RELAY_BASE_URL=https://<relay-host>:…`.
2. Deploy the console: `scripts/deploy-console-remote.sh <host> <user> --gateway https://127.0.0.1:8081` (upstream is loopback **on the console host** when gateway is co-located).
3. Open `https://<host>:8082/` — accept the self-signed cert.
4. Complete the **Get started** panel (or Skip): Generate → Demo → Stored.
5. Confirm status shows **Connected**.

Relay JWT is configured on the **gateway**, not in the browser.

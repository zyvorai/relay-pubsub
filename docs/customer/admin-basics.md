# Admin basics

| Variable | Purpose |
|----------|---------|
| `RELAY_BACKEND` | `relay-events` for production |
| `RELAY_BASE_URL` | Reachable Relay host (`:8443` / `:18080`) |
| `RELAY_AUTH_TOKEN` | Same JWT as edge (re-sync after Relay restart) |
| `RELAY_TLS_INSECURE` | Skip verify to self-signed Relay |
| `PUBSUB_HTTP_ADDR` | Often `0.0.0.0:8081` |
| `PUBSUB_TLS_SAN` | Include every name Relay uses for Act |
| `GATEWAY_UPSTREAM` | Console proxy target (often `https://127.0.0.1:8081` on the console host) |

## Operate from the console (UX)

1. Open this route from the nav or command palette and wait for live API data.
2. Use filters/search when present; drill into a row for detail.
3. For mutating actions: confirm role gates and impact before applying.
4. **Empty / fail:** Check service health, auth, and that required CRDs/backends for this domain are installed.
5. **Success:** Live data loads; created/updated objects appear without error toasts.


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

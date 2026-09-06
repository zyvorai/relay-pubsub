# Using the dashboard

| Section | Purpose |
|---------|---------|
| **Generate** | Seed farm / edge / fleet / demo / test catalogs |
| **Demo** | One-click publish → pull → ack |
| **Tests** | In-browser health / smoke / conformance |
| **Console → Incoming** | Publish into a topic |
| **Outgoing** | Pull / live receive / ack |
| **Stored** | Inventory peek (non-consuming) |
| **Configure** | Create/delete topics and subscriptions |
| **Logs** | Live gateway log tail |

## Operate from the console (UX)

1. Open this route from the nav or command palette and wait for live API data.
2. Use filters/search when present; drill into a row for detail.
3. For mutating actions: confirm role gates and impact before applying.
4. **Empty / fail:** Check service health, auth, and that required CRDs/backends for this domain are installed.
5. **Success:** Live data loads; created/updated objects appear without error toasts.


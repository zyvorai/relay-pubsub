---
hero:
  eyebrow: FILTERS
  title: Subscription filters, schemas, and CloudEvents
---

← [Docs hub](README.md)

relay-pubsub implements the Google Pub/Sub **attribute filter** subset, optional **topic schema enforcement**, and a **CloudEvents attribute projection**. None of these require Relay-core changes.

## Filters

Set `filter` on `CreateSubscription` (gRPC field `Subscription.filter`, REST `"filter"`).

```text
attributes.site = "pune" AND hasPrefix(attributes.device, "pump-")
```

| Expression | Matches |
|---|---|
| `attributes:site` | message has a `site` attribute |
| `NOT attributes:site` | message has no `site` attribute |
| `attributes.zone = "north"` | exact value |
| `attributes.zone != "north"` | missing or different |
| `hasPrefix(attributes.device, "pump-")` | value starts with prefix |
| `AND` / `OR` / `()` | boolean composition |

Non-matching messages are **auto-acknowledged** and never delivered (Google semantics). Filters cannot be changed after create.

REST example:

```bash
curl -k -X PUT "https://localhost:8080/v1/projects/demo/subscriptions/pune-pumps" \
  -H "content-type: application/json" \
  -d '{
    "topic": "projects/demo/topics/telemetry",
    "ackDeadlineSeconds": 30,
    "filter": "attributes.site = \"pune\" AND hasPrefix(attributes.device, \"pump-\")"
  }'
```

## Topic schema settings

Bind a schema created through SchemaService:

```bash
curl -k -X PUT "https://localhost:8080/v1/projects/demo/topics/irrigation.required" \
  -H "content-type: application/json" \
  -d '{
    "schemaSettings": {
      "schema": "projects/demo/schemas/irrigation",
      "encoding": "JSON"
    }
  }'
```

JSON encoding requires a JSON body. If the schema definition is a JSON Schema object with `required`, those keys must be present or `Publish` returns `INVALID_ARGUMENT`.

## CloudEvents

If the payload is CloudEvents 1.0 JSON (`content-type: application/cloudevents+json`, or a body with `specversion`), the gateway copies:

- `ce-id`, `ce-source`, `ce-type`, `ce-specversion`, `ce-subject`
- `relay.topic` (topic leaf name)

A subscriber can then filter `attributes.ce-type = "irrigation.required"` without learning Relay's native API.

## Publish dedup

Set `messageId` on the Pub/Sub message (or `attributes.idempotency_key`). A second publish with the same id on the same topic returns the original id and does not append another copy. Useful for flaky edge publishers.

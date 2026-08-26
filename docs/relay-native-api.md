# Relay native API contract used by `relay-pubsub`

`relay-pubsub` is a compatibility component. In production it does not own durable storage; it translates Google Pub/Sub semantics into this small Relay-native HTTP contract.

Base URL is configured with `RELAY_BASE_URL`. Authentication is `Authorization: Bearer $RELAY_AUTH_TOKEN` when configured.

## Topics

### `POST /v1/topics`
Request/response:
```json
{
  "name": "projects/acme/topics/orders",
  "labels": {"team":"payments"},
  "kms_key_name": ""
}
```

### `GET /v1/topics?project=projects/acme`
Returns an array of topic objects.

### `GET /v1/topics/by-name?name=projects/acme/topics/orders`
Returns one topic.

### `DELETE /v1/topics/by-name?name=projects/acme/topics/orders`
Returns 2xx with an empty body.

## Subscriptions

### `POST /v1/subscriptions`
```json
{
  "name": "projects/acme/subscriptions/orders-worker",
  "topic": "projects/acme/topics/orders",
  "ack_deadline_seconds": 20,
  "labels": {},
  "enable_message_ordering": true,
  "enable_exactly_once_delivery": false,
  "dead_letter": {
    "topic": "projects/acme/topics/orders-dlq",
    "max_delivery_attempts": 5
  },
  "retry": {
    "minimum_backoff_seconds": 1,
    "maximum_backoff_seconds": 60
  },
  "push_endpoint": null
}
```

### `GET /v1/subscriptions?project=projects/acme`
Returns an array of subscription objects.

### `GET /v1/subscriptions/by-name?name=...`
Returns one subscription.

### `DELETE /v1/subscriptions/by-name?name=...`
Returns 2xx.

## Data plane

### `POST /v1/messages:publish`
```json
{
  "topic": "projects/acme/topics/orders",
  "messages": [
    {
      "data": [123,34,111,114,100,101,114,34,58,49,125],
      "attributes": {"region":"in"},
      "ordering_key": "customer-17"
    }
  ]
}
```
Response:
```json
{"message_ids":["01...uuid..."]}
```

### `POST /v1/messages:pull`
```json
{"subscription":"projects/acme/subscriptions/orders-worker","max_messages":100}
```
Response:
```json
{
  "deliveries": [
    {
      "ack_id": "...",
      "delivery_attempt": 1,
      "message": {
        "id": "...",
        "data": [104,101,108,108,111],
        "attributes": {},
        "ordering_key": "",
        "published_at": "2026-08-26T12:00:00Z"
      }
    }
  ]
}
```

### `POST /v1/messages:ack`
```json
{"subscription":"...","ack_ids":["..."]}
```

### `POST /v1/messages:modify-ack-deadline`
`seconds: 0` is NACK / immediate redelivery.
```json
{"subscription":"...","ack_ids":["..."],"seconds":30}
```

### `POST /v1/subscriptions:seek`
```json
{"subscription":"...","time":"2026-08-26T12:00:00Z"}
```

## Relay implementation guidance

For a production Relay integration, make these operations idempotent where the Google API expects safe retries. Persist consumer cursors and ACK state. Replicate topic logs according to Relay's durability policy. Enforce tenant boundaries based on the `projects/<project>` prefix only after mapping the external project to an authenticated Relay tenant; never trust the string alone for authorization.

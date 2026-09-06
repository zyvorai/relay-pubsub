# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: Apache-2.0
"""Smoke test using the official google-cloud-pubsub client.

NOTE: the gateway's gRPC listener is TLS-only. PUBSUB_EMULATOR_HOST forces
the client SDK onto a plaintext channel, so it can no longer reach this
gateway — that env var only works against a real plaintext emulator/gateway.
Use scripts/smoke.sh (REST + curl -k) for a self-signed-cert-friendly smoke
test instead, or adapt this script to build a grpc.secure_channel trusting
the gateway's generated cert.
"""
from google.cloud import pubsub_v1

project = "demo"
topic_id = "orders"
subscription_id = "orders-worker"

publisher = pubsub_v1.PublisherClient()
subscriber = pubsub_v1.SubscriberClient()

topic = publisher.topic_path(project, topic_id)
subscription = subscriber.subscription_path(project, subscription_id)

try:
    publisher.create_topic(request={"name": topic})
except Exception as exc:
    if "AlreadyExists" not in type(exc).__name__ and "409" not in str(exc):
        raise

try:
    subscriber.create_subscription(request={"name": subscription, "topic": topic, "ack_deadline_seconds": 20})
except Exception as exc:
    if "AlreadyExists" not in type(exc).__name__ and "409" not in str(exc):
        raise

future = publisher.publish(topic, b'{"order":"ORD-1001"}', source="python-google-client", ordering_key="customer-17")
print("published:", future.result())

response = subscriber.pull(request={"subscription": subscription, "max_messages": 10})
for received in response.received_messages:
    print("received:", received.message.message_id, received.message.data, dict(received.message.attributes))

if response.received_messages:
    subscriber.acknowledge(request={"subscription": subscription, "ack_ids": [m.ack_id for m in response.received_messages]})
    print("acked")

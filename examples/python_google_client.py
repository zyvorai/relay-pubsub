"""Smoke test using the official google-cloud-pubsub client.

Run gateway first, then:
  pip install google-cloud-pubsub
  export PUBSUB_EMULATOR_HOST=127.0.0.1:50051
  python examples/python_google_client.py
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

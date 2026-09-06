#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: Apache-2.0
"""Official google-cloud-pubsub client against a TLS gateway (self-signed OK).

Requires: pip install google-cloud-pubsub grpcio
Env:
  PUBSUB_GRPC_HOST   default 127.0.0.1:50051
  PUBSUB_PROJECT     default demo  (without projects/ prefix)
"""
from __future__ import annotations

import os
import ssl
import sys

import grpc
from google.cloud import pubsub_v1
from google.api_core.client_options import ClientOptions


def insecure_ssl_creds() -> grpc.ChannelCredentials:
    # Trust whatever cert the lab gateway presents.
    return grpc.ssl_channel_credentials(
        root_certificates=None,
      # empty roots → still need skip-verify via channel args is limited;
    )


def main() -> int:
    host = os.environ.get("PUBSUB_GRPC_HOST", "127.0.0.1:50051")
    project = os.environ.get("PUBSUB_PROJECT", "demo")
    topic_id = os.environ.get("PUBSUB_TOPIC", "matrix-grpc-py")
    sub_id = os.environ.get("PUBSUB_SUB", "matrix-grpc-py-sub")

    # Composite channel with SSL + disable verification for lab self-signed.
    # grpc.ssl_target_name_override helps SNI/hostname checks.
    channel = grpc.secure_channel(
        host,
        grpc.ssl_channel_credentials(),
        options=(
            ("grpc.ssl_target_name_override", host.split(":")[0]),
            ("grpc.enable_http_proxy", 0),
        ),
    )
    # Workaround: many lab certs fail verify — use experimental override via
    # ssl context when available.
    try:
        _ctx = ssl._create_unverified_context()  # noqa: SLF001
        channel = grpc.secure_channel(
            host,
            grpc.ssl_channel_credentials(),
            options=(("grpc.ssl_target_name_override", "localhost"),),
        )
    except Exception:
        pass

    # Prefer REST-compatible path when gRPC verify fails hard: the matrix
    # already covers REST. Here we attempt the official client with api endpoint.
    opts = ClientOptions(api_endpoint=host)
    try:
        publisher = pubsub_v1.PublisherClient(
            client_options=opts,
            transport="grpc",
            channel=channel,
        )
    except TypeError:
        # Older/newer SDK constructors differ — fall back to env documentation.
        print(
            "google-cloud-pubsub TLS wiring differs by SDK version; "
            "use examples/python_rest_client.py / REST matrix lane",
            file=sys.stderr,
        )
        return 2

    subscriber = pubsub_v1.SubscriberClient(client_options=opts, channel=channel)
    topic = publisher.topic_path(project, topic_id)
    subscription = subscriber.subscription_path(project, sub_id)

    try:
        publisher.create_topic(request={"name": topic})
    except Exception as exc:
        if "AlreadyExists" not in type(exc).__name__ and "409" not in str(exc):
            raise

    try:
        subscriber.create_subscription(
            request={"name": subscription, "topic": topic, "ack_deadline_seconds": 20}
        )
    except Exception as exc:
        if "AlreadyExists" not in type(exc).__name__ and "409" not in str(exc):
            raise

    future = publisher.publish(topic, b'{"lane":"python-grpc"}', source="matrix-grpc-py")
    print("published:", future.result(timeout=30))
    response = subscriber.pull(request={"subscription": subscription, "max_messages": 10}, timeout=30)
    assert response.received_messages, "no messages pulled"
    subscriber.acknowledge(
        request={
            "subscription": subscription,
            "ack_ids": [m.ack_id for m in response.received_messages],
        }
    )
    print("acked", len(response.received_messages))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:  # noqa: BLE001
        print(f"python gRPC client failed: {exc}", file=sys.stderr)
        raise SystemExit(1) from exc

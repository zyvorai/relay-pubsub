#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail
# The gateway is TLS-only (HTTPS/gRPCS) with a self-signed cert by default —
# curl -k skips cert-name/CA verification. Point BASE at a real CA-signed
# endpoint and drop -k if you've configured one.
BASE=${BASE:-https://127.0.0.1:8080}
PROJECT=${PROJECT:-demo}
TOPIC="projects/$PROJECT/topics/orders"
SUB="projects/$PROJECT/subscriptions/orders-worker"

curl -k -fsS -X PUT "$BASE/v1/$TOPIC" -H 'content-type: application/json' -d '{"labels":{"env":"smoke"}}' >/dev/null || true
curl -k -fsS -X PUT "$BASE/v1/$SUB" -H 'content-type: application/json' -d "{\"topic\":\"$TOPIC\",\"ackDeadlineSeconds\":20,\"enableMessageOrdering\":true}" >/dev/null || true

DATA=$(printf '{"order":"ORD-1001"}' | base64 | tr -d '\n')
echo "publish:"
curl -k -fsS -X POST "$BASE/v1/$TOPIC:publish" -H 'content-type: application/json' -d "{\"messages\":[{\"data\":\"$DATA\",\"attributes\":{\"source\":\"curl\"},\"orderingKey\":\"user-17\"}]}"
echo

echo "pull:"
curl -k -fsS -X POST "$BASE/v1/$SUB:pull" -H 'content-type: application/json' -d '{"maxMessages":10}'
echo

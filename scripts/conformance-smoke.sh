#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs
# SPDX-License-Identifier: Apache-2.0
#
# Conformance smoke for v0.2+ surface: pagination, snapshots, seek, push config,
# update APIs, IAM, schemas. Requires a running gateway (memory backend is fine).
set -euo pipefail

BASE="${BASE:-https://localhost:8080}"
PROJECT="${PROJECT:-projects/demo}"
CURL=(curl -sk)

echo "== health =="
"${CURL[@]}" "$BASE/healthz" | grep -q ok

TOPIC="$PROJECT/topics/conformance"
SUB="$PROJECT/subscriptions/conformance-sub"
SNAP="$PROJECT/snapshots/conformance-snap"
SCHEMA="$PROJECT/schemas/conformance-schema"

echo "== cleanup =="
"${CURL[@]}" -X DELETE "$BASE/v1/$SUB" >/dev/null 2>&1 || true
"${CURL[@]}" -X DELETE "$BASE/v1/$TOPIC" >/dev/null 2>&1 || true
"${CURL[@]}" -X DELETE "$BASE/v1/$SNAP" >/dev/null 2>&1 || true
"${CURL[@]}" -X DELETE "$BASE/v1/$SCHEMA" >/dev/null 2>&1 || true

echo "== create topic/sub =="
"${CURL[@]}" -X PUT "$BASE/v1/$TOPIC" -H 'content-type: application/json' -d '{"labels":{"t":"1"}}' | grep -q "$TOPIC"
"${CURL[@]}" -X PUT "$BASE/v1/$SUB" -H 'content-type: application/json' \
  -d "{\"topic\":\"$TOPIC\",\"ackDeadlineSeconds\":30,\"enableMessageOrdering\":true,\"enableExactlyOnceDelivery\":true,\"retryPolicy\":{\"minimumBackoff\":\"0s\",\"maximumBackoff\":\"1s\"}}" \
  | grep -q "$SUB"

echo "== update topic =="
"${CURL[@]}" -X PATCH "$BASE/v1/$TOPIC?updateMask=labels" -H 'content-type: application/json' \
  -d '{"labels":{"t":"2"}}' | grep -q '"t":"2"'

echo "== publish + pull + ack =="
"${CURL[@]}" -X POST "$BASE/v1/$TOPIC:publish" -H 'content-type: application/json' \
  -d '{"messages":[{"data":"aGVsbG8=","orderingKey":"k1"}]}' | grep -q messageIds
PULL=$("${CURL[@]}" -X POST "$BASE/v1/$SUB:pull" -H 'content-type: application/json' -d '{"maxMessages":1}')
echo "$PULL" | grep -q ackId
ACK_ID=$(echo "$PULL" | sed -n 's/.*"ackId":"\([^"]*\)".*/\1/p' | head -1)
"${CURL[@]}" -X POST "$BASE/v1/$SUB:acknowledge" -H 'content-type: application/json' \
  -d "{\"ackIds\":[\"$ACK_ID\"]}" | grep -q '{}'

echo "== snapshot + seek =="
"${CURL[@]}" -X PUT "$BASE/v1/$SNAP" -H 'content-type: application/json' \
  -d "{\"subscription\":\"$SUB\",\"labels\":{}}" | grep -q "$SNAP"
"${CURL[@]}" -X POST "$BASE/v1/$SUB:seek" -H 'content-type: application/json' \
  -d "{\"snapshot\":\"$SNAP\"}" | grep -q '{}'

echo "== pagination =="
for i in 1 2 3; do
  "${CURL[@]}" -X PUT "$BASE/v1/$PROJECT/topics/page-$i" -H 'content-type: application/json' -d '{}' >/dev/null
done
PAGE=$("${CURL[@]}" "$BASE/v1/$PROJECT/topics?pageSize=2")
echo "$PAGE" | grep -q nextPageToken

echo "== IAM =="
"${CURL[@]}" -X POST "$BASE/v1/$TOPIC:getIamPolicy" -H 'content-type: application/json' -d '{}' | grep -q bindings
"${CURL[@]}" -X POST "$BASE/v1/$TOPIC:setIamPolicy" -H 'content-type: application/json' \
  -d '{"policy":{"bindings":[{"role":"roles/pubsub.publisher","members":["allAuthenticatedUsers"]}]}}' \
  | grep -q pubsub.publisher

echo "== schema =="
"${CURL[@]}" -X PUT "$BASE/v1/$SCHEMA" -H 'content-type: application/json' \
  -d '{"type":"AVRO","definition":"{\"type\":\"record\",\"name\":\"E\",\"fields\":[]}"}' \
  | grep -q "$SCHEMA"
"${CURL[@]}" -X POST "$BASE/v1/$PROJECT/schemas:validate" -H 'content-type: application/json' \
  -d "{\"schema\":{\"name\":\"$SCHEMA\",\"type\":\"AVRO\",\"definition\":\"{\\\"type\\\":\\\"record\\\",\\\"name\\\":\\\"E\\\",\\\"fields\\\":[]}\"}}" \
  | grep -q '{}'

echo "== modifyPushConfig =="
"${CURL[@]}" -X POST "$BASE/v1/$SUB:modifyPushConfig" -H 'content-type: application/json' \
  -d '{"pushConfig":{"pushEndpoint":"https://example.invalid/push"}}' | grep -q '{}'

echo "OK: conformance smoke passed against $BASE"

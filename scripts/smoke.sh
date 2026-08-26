#!/usr/bin/env bash
set -euo pipefail
BASE=${BASE:-http://127.0.0.1:8080}
PROJECT=${PROJECT:-demo}
TOPIC="projects/$PROJECT/topics/orders"
SUB="projects/$PROJECT/subscriptions/orders-worker"

curl -fsS -X PUT "$BASE/v1/$TOPIC" -H 'content-type: application/json' -d '{"labels":{"env":"smoke"}}' >/dev/null || true
curl -fsS -X PUT "$BASE/v1/$SUB" -H 'content-type: application/json' -d "{\"topic\":\"$TOPIC\",\"ackDeadlineSeconds\":20,\"enableMessageOrdering\":true}" >/dev/null || true

DATA=$(printf '{"order":"ORD-1001"}' | base64 | tr -d '\n')
echo "publish:"
curl -fsS -X POST "$BASE/v1/$TOPIC:publish" -H 'content-type: application/json' -d "{\"messages\":[{\"data\":\"$DATA\",\"attributes\":{\"source\":\"curl\"},\"orderingKey\":\"customer-17\"}]}"
echo

echo "pull:"
curl -fsS -X POST "$BASE/v1/$SUB:pull" -H 'content-type: application/json' -d '{"maxMessages":10}'
echo

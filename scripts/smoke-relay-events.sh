#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs
# SPDX-License-Identifier: Apache-2.0
# relay-events smoke: publish irrigation.required through self-signed HTTPS gateway.
set -euo pipefail
BASE=${BASE:-https://127.0.0.1:8080}
PROJECT=${PROJECT:-fasal-onprem}

curl -k -fsS "$BASE/healthz" >/dev/null
DATA=$(printf '{"zone":"A4","recommended_action":{"target":"farm-controller","command":"irrigation.start","payload":{"zone":"A4"}}}' | base64 | tr -d '\n')
KEY="k8s-smoke/$(date +%s%N)"
code=$(curl -k -sS -o /tmp/rpg-smoke.out -w '%{http_code}' -X POST \
  "$BASE/v1/projects/$PROJECT/topics/irrigation.required:publish" \
  -H 'content-type: application/json' \
  -d "{\"messages\":[{\"data\":\"$DATA\",\"attributes\":{\"severity\":\"critical\",\"source\":\"k8s-smoke\",\"idempotency_key\":\"$KEY\"}}]}")
if [[ "$code" != "200" ]]; then
  echo "publish failed: HTTP $code"
  cat /tmp/rpg-smoke.out
  exit 1
fi
echo "relay-events publish ok (HTTP $code)"
cat /tmp/rpg-smoke.out
echo

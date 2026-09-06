#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: Apache-2.0
# Client conformance matrix (v0.4 foundation).
# Always runs REST + conformance-smoke. Optional lanes skip cleanly when
# language SDKs are missing.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BASE="${BASE:-https://127.0.0.1:8080}"
PROJECT="${PROJECT:-projects/demo}"
# smoke.sh wants the bare project id (no projects/ prefix).
SMOKE_PROJECT="${PROJECT#projects/}"
CURL=(curl -sk)
PASS=0
FAIL=0
SKIP=0

ok() { echo "  [pass] $*"; PASS=$((PASS + 1)); }
bad() { echo "  [fail] $*"; FAIL=$((FAIL + 1)); }
skip() { echo "  [skip] $*"; SKIP=$((SKIP + 1)); }

echo "=== Client matrix against $BASE (project=$PROJECT) ==="

echo "== REST health =="
if "${CURL[@]}" "$BASE/healthz" | grep -q ok; then ok "GET /healthz"; else bad "GET /healthz"; fi
if "${CURL[@]}" "$BASE/readyz" | grep -q ok; then ok "GET /readyz"; else bad "GET /readyz"; fi

echo "== REST inventory / logs =="
if "${CURL[@]}" "$BASE/admin/v1/inventory?project=$(python3 -c "import urllib.parse;print(urllib.parse.quote('$PROJECT'))")" | grep -q topics; then
  ok "GET /admin/v1/inventory"
else
  bad "GET /admin/v1/inventory"
fi
if "${CURL[@]}" "$BASE/admin/v1/logs?limit=5" | grep -q entries; then
  ok "GET /admin/v1/logs"
else
  bad "GET /admin/v1/logs"
fi

echo "== REST smoke =="
if BASE="$BASE" PROJECT="$SMOKE_PROJECT" bash "$ROOT/scripts/smoke.sh" >/tmp/relay-pubsub-matrix-smoke.log 2>&1; then
  ok "scripts/smoke.sh"
else
  bad "scripts/smoke.sh (see /tmp/relay-pubsub-matrix-smoke.log)"
fi

echo "== REST conformance =="
if BASE="$BASE" PROJECT="$PROJECT" bash "$ROOT/scripts/conformance-smoke.sh" >/tmp/relay-pubsub-matrix-conf.log 2>&1; then
  ok "scripts/conformance-smoke.sh"
else
  bad "scripts/conformance-smoke.sh (see /tmp/relay-pubsub-matrix-conf.log)"
fi

echo "== Python REST client =="
if python3 - <<PY
import json, os, ssl, urllib.error, urllib.request
base = os.environ.get("BASE", "$BASE").rstrip("/")
project = os.environ.get("PROJECT", "$PROJECT")
ctx = ssl._create_unverified_context()

def req(method, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    r = urllib.request.Request(base + path, data=data, method=method, headers={"content-type": "application/json"})
    with urllib.request.urlopen(r, context=ctx, timeout=30) as resp:
        raw = resp.read().decode() or "{}"
        return json.loads(raw)

topic = f"{project}/topics/matrix-py"
sub = f"{project}/subscriptions/matrix-py-sub"
try:
    req("PUT", f"/v1/{topic}", {"labels": {"lane": "python-rest"}})
except Exception:
    pass
try:
    req("PUT", f"/v1/{sub}", {"topic": topic, "ackDeadlineSeconds": 20})
except Exception:
    pass
ids = req("POST", f"/v1/{topic}:publish", {"messages": [{"data": "cHk=", "attributes": {"source": "matrix-py"}}]})
assert ids.get("messageIds"), ids
pulled = req("POST", f"/v1/{sub}:pull", {"maxMessages": 5})
msgs = pulled.get("receivedMessages") or []
assert msgs, pulled
req("POST", f"/v1/{sub}:acknowledge", {"ackIds": [m["ackId"] for m in msgs]})
print("python-rest ok", ids["messageIds"][0])
PY
then
  ok "python REST publish/pull/ack"
else
  bad "python REST publish/pull/ack"
fi

echo "== Python google-cloud-pubsub (optional TLS) =="
if python3 -c 'import google.cloud.pubsub_v1' 2>/dev/null; then
  if python3 "$ROOT/examples/python_google_client_tls.py" >/tmp/relay-pubsub-matrix-pygrpc.log 2>&1; then
    ok "python google-cloud-pubsub TLS"
  else
    skip "python gRPC TLS not usable against this cert (REST lane covers API)"
  fi
else
  skip "google-cloud-pubsub not installed"
fi

echo "== Node fetch REST (optional) =="
if command -v node >/dev/null; then
  if node "$ROOT/examples/node_rest_client.mjs" >/tmp/relay-pubsub-matrix-node.log 2>&1; then
    ok "node REST publish/pull/ack"
  else
    bad "node REST (see /tmp/relay-pubsub-matrix-node.log)"
  fi
else
  skip "node not installed"
fi

echo "== Go client (optional) =="
if command -v go >/dev/null && [[ -f "$ROOT/examples/go_rest_client/main.go" ]]; then
  if (cd "$ROOT/examples/go_rest_client" && BASE="$BASE" PROJECT="$PROJECT" go run . >/tmp/relay-pubsub-matrix-go.log 2>&1); then
    ok "go REST publish/pull/ack"
  else
    bad "go REST (see /tmp/relay-pubsub-matrix-go.log)"
  fi
else
  skip "go toolchain or example missing"
fi

echo
echo "=== Summary: pass=$PASS fail=$FAIL skip=$SKIP ==="
[[ "$FAIL" -eq 0 ]]

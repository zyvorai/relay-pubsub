#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: Apache-2.0
# Drives the full Fasal event catalog (docs/RELAY_EVENTS_BACKEND.md,
# mirroring zyvor/relay's docs/FASAL_ACCOMMODATION.md #4.1/#4.2) through the
# relay-events backend's REST publish endpoint and Relay's real API, proving
# every catalog entry — not just irrigation.required, the only one any prior
# manual/live check exercised — routes to the correct policy, and that the 5
# critical types produce an action that reaches the gateway's new
# POST /v1/actions receiver (rpg_... provider id).
#
# Usage:
#   BASE=http://127.0.0.1:8080 GATEWAY=https://127.0.0.1:8083 \
#     ./scripts/fasal-catalog-smoke.sh
#
# Requires: Relay running at BASE, and this binary running at GATEWAY with
# RELAY_BACKEND=relay-events and RELAY_BASE_URL pointed at BASE, and Relay's
# RELAY_ACTION_TARGETS pointed at GATEWAY's /v1/actions. GATEWAY is this
# gateway's TLS-only listener — self-signed by default, hence curl -k below.
set -euo pipefail
BASE=${BASE:-http://127.0.0.1:8080}
GATEWAY=${GATEWAY:-https://127.0.0.1:8083}
PROJECT=${PROJECT:-fasal-onprem}
USER=${RELAY_DEMO_USER:-demo}
PASS=${RELAY_DEMO_PASSWORD:-demo}
FAILED=0

pass() { echo "  ✅ $1"; }
fail_soft() { echo "  ❌ $1" >&2; FAILED=$((FAILED + 1)); }

echo "== Fasal relay-events backend catalog smoke — gateway=$GATEWAY relay=$BASE =="
curl -k -fsS "$BASE/healthz" >/dev/null
curl -k -fsS "$GATEWAY/healthz" >/dev/null
LOGIN=$(curl -k -fsS -X POST "$BASE/v1/auth/login" -H 'content-type: application/json' -d "{\"username\":\"$USER\",\"password\":\"$PASS\"}")
TOKEN=$(python3 -c 'import json,sys; print(json.load(sys.stdin)["token"])' <<<"$LOGIN")
AUTH=(-H "Authorization: Bearer $TOKEN")

# event_type:command (command empty => advisory, notify-only, no action expected)
CATALOG=(
  "irrigation.required:irrigation.start"
  "soil.moisture.critical:irrigation.start"
  "fertigation.required:fertigation.start"
  "disease.risk.critical:inspection.create"
  "device.control.required:pump.start"
  "crop.advisory:"
  "weather.advisory:"
  "spray.advisory:"
  "frost.alert:"
  "pest.advisory:"
)

find_event() {
  local key=$1
  curl -k -fsS "$BASE/v1/events?limit=50" "${AUTH[@]}" | python3 -c "
import json, sys
d = json.load(sys.stdin)
for e in d['items']:
    if e['idempotency_key'] == '$key':
        print(e['id'], e.get('policy_id', ''))
        sys.exit(0)
sys.exit(1)
"
}

for entry in "${CATALOG[@]}"; do
  event_type="${entry%%:*}"
  command="${entry#*:}"
  key="fasal/catalog-smoke/$event_type/$(date +%s%N)"
  echo "[$event_type]"

  if [[ -n "$command" ]]; then
    severity="critical" # pol_critical_farm matches severities: critical, high
    data=$(printf '{"zone":"A4","recommended_action":{"target":"farm-controller","command":"%s","payload":{"zone":"A4"}}}' "$command" | base64 | tr -d '\n')
  else
    severity="info" # pol_advisory matches severities: info, medium, high — NOT critical
    data=$(printf '{"advisory":"catalog smoke"}' | base64 | tr -d '\n')
  fi

  code=$(curl -k -sS -o /dev/null -w '%{http_code}' -X POST "$GATEWAY/v1/projects/$PROJECT/topics/$event_type:publish" \
    -H 'content-type: application/json' \
    -d "{\"messages\":[{\"data\":\"$data\",\"attributes\":{\"severity\":\"$severity\",\"source\":\"catalog-smoke\",\"idempotency_key\":\"$key\"}}]}")
  if [[ "$code" != "200" ]]; then
    fail_soft "publish returned $code"
    continue
  fi

  found=""
  for _ in $(seq 1 10); do
    sleep 0.5
    found=$(find_event "$key" || true)
    [[ -n "$found" ]] && break
  done
  if [[ -z "$found" ]]; then
    fail_soft "event never appeared in Relay for $event_type"
    continue
  fi
  event_id="${found%% *}"
  policy_id="${found#* }"

  if [[ -n "$command" ]]; then
    if [[ "$policy_id" != "pol_critical_farm" ]]; then
      fail_soft "policy_id=$policy_id, want pol_critical_farm"
      continue
    fi
    curl -k -fsS -X POST "$BASE/v1/events/$event_id/ack" "${AUTH[@]}" -H 'content-type: application/json' -d '{"decision":"approve"}' >/dev/null
    action=""
    for _ in $(seq 1 20); do
      sleep 0.5
      action=$(curl -k -fsS "$BASE/v1/events/$event_id" "${AUTH[@]}" | python3 -c '
import json, sys
d = json.load(sys.stdin)
actions = d.get("actions") or []
if actions:
    a = actions[0]
    print(a.get("command", ""), a.get("state", ""), a.get("provider_id", ""))
')
      state=$(cut -d' ' -f2 <<<"$action")
      case "$state" in executed|failed) break ;; esac
    done
    read -r got_command got_state got_provider <<<"$action"
    if [[ "$got_command" != "$command" ]]; then
      fail_soft "action command=$got_command, want $command"
    elif [[ "$got_state" != "executed" ]]; then
      fail_soft "action state=$got_state, want executed"
    elif [[ "$got_provider" != rpg_* ]]; then
      fail_soft "action provider_id=$got_provider, want rpg_* (didn't reach the gateway)"
    else
      pass "critical, policy=pol_critical_farm, action=$got_command executed via $got_provider"
    fi
  else
    if [[ "$policy_id" != "pol_advisory" ]]; then
      fail_soft "policy_id=$policy_id, want pol_advisory"
      continue
    fi
    actions=$(curl -k -fsS "$BASE/v1/events/$event_id" "${AUTH[@]}" | python3 -c 'import json,sys; print(len(json.load(sys.stdin).get("actions") or []))')
    if [[ "$actions" != "0" ]]; then
      fail_soft "advisory event unexpectedly has $actions action(s)"
    else
      pass "advisory, policy=pol_advisory, notify-only (no action)"
    fi
  fi
done

if [[ "$FAILED" -gt 0 ]]; then
  echo "FAILED: $FAILED catalog entr(y/ies) failed" >&2
  exit 1
fi
echo "PASS: all ${#CATALOG[@]} Fasal catalog event types verified through relay-events backend"

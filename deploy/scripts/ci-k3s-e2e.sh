#!/usr/bin/env bash
# Verify a deploy-k3s.sh deployment: rollout status + a real publish/pull
# round-trip via scripts/smoke.sh, run against the in-cluster Service through
# a port-forward.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
NAMESPACE="${NAMESPACE:-relay-pubsub}"

export KUBECONFIG="${KUBECONFIG:-${HOME}/.kube/config}"

echo "=== ci-k3s-e2e ==="
kubectl -n "${NAMESPACE}" rollout status deployment/relay-pubsub --timeout=180s

# Try a small set of local ports for the port-forward — the host running this
# (a developer machine, CI runner, or a shared box) may already have
# something bound to any single hardcoded port, as seen when :18080 was
# already in use by an unrelated process during testing.
PORT_CANDIDATES=("${LOCAL_PORT:-18080}" 28080 38080 48080 58080)
PF_PID=""
LOCAL_PORT=""
for candidate in "${PORT_CANDIDATES[@]}"; do
    rm -f /tmp/relay-pubsub-port-forward.log
    kubectl -n "${NAMESPACE}" port-forward "svc/relay-pubsub" "${candidate}:8080" >/tmp/relay-pubsub-port-forward.log 2>&1 &
    pid=$!
    sleep 1
    if kill -0 "${pid}" 2>/dev/null && ! grep -qi "address already in use" /tmp/relay-pubsub-port-forward.log; then
        PF_PID="${pid}"
        LOCAL_PORT="${candidate}"
        break
    fi
    kill "${pid}" 2>/dev/null || true
    wait "${pid}" 2>/dev/null || true
done
if [ -z "${LOCAL_PORT}" ]; then
    echo "ERROR: could not find a free local port for port-forward (tried: ${PORT_CANDIDATES[*]})"
    exit 1
fi
echo "Using local port ${LOCAL_PORT} for port-forward"
trap 'kill "${PF_PID}" 2>/dev/null || true' EXIT

echo "Waiting for port-forward + health..."
for i in $(seq 1 30); do
    if curl -sf "http://127.0.0.1:${LOCAL_PORT}/healthz" >/dev/null 2>&1; then
        break
    fi
    if [ "${i}" -eq 30 ]; then
        echo "ERROR: health check timed out"
        kubectl -n "${NAMESPACE}" get pods
        cat /tmp/relay-pubsub-port-forward.log || true
        exit 1
    fi
    sleep 2
done

echo "Running scripts/smoke.sh against in-cluster gateway..."
BASE="http://127.0.0.1:${LOCAL_PORT}" bash "${ROOT}/scripts/smoke.sh"

echo "=== ci-k3s-e2e passed ==="

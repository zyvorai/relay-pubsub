#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs
# SPDX-License-Identifier: Apache-2.0
#
# Deploys the ui/ product console to a remote host as
# relay-pubsub-console.service — HTTPS static UI + same-origin proxy to
# the gateway (avoids browser rejecting a second self-signed cert).
set -euo pipefail

usage() {
  cat <<'EOF'
relay-pubsub-console remote deploy

Usage:
  scripts/deploy-console-remote.sh <host> <user> [password] [options]

Builds ui/ locally (npm run build) and deploys to <host> as
relay-pubsub-console.service. The Node server serves the SPA over HTTPS
and proxies /v1 /admin /healthz /readyz /metrics to the gateway.

Positional:
  password           Optional SSH password (uses sshpass when set)

Options:
  --api-base URL     VITE_API_BASE baked into the build
                     (default: empty = same-origin via console proxy)
  --gateway URL      Upstream gateway for the proxy (default: https://127.0.0.1:8081)
  --project NAME     VITE_PROJECT baked into the build (default: projects/fasal-onprem)
  --port PORT        Port for the console to listen on (default: 8082)
  --help             Show this help
EOF
}

HOST=""; USER_=""; PASS=""; API_BASE=""; GATEWAY_UPSTREAM=""; PROJECT="projects/fasal-onprem"; PORT="8082"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --api-base) API_BASE="$2"; shift 2 ;;
    --gateway) GATEWAY_UPSTREAM="$2"; shift 2 ;;
    --project) PROJECT="$2"; shift 2 ;;
    --port) PORT="$2"; shift 2 ;;
    --help) usage; exit 0 ;;
    *)
      if [[ -z "$HOST" ]]; then HOST="$1"
      elif [[ -z "$USER_" ]]; then USER_="$1"
      elif [[ -z "$PASS" && "$1" != --* ]]; then PASS="$1"
      else echo "Unexpected argument: $1" >&2; exit 1
      fi
      shift ;;
  esac
done

if [[ -z "$HOST" || -z "$USER_" ]]; then
  usage; exit 1
fi

# Empty API_BASE → browser uses same origin; console proxies to gateway.
[[ -z "$GATEWAY_UPSTREAM" ]] && GATEWAY_UPSTREAM="https://127.0.0.1:8081"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
UI_DIR="$REPO_DIR/ui"
REMOTE_APP_DIR="/opt/relay-pubsub-console"
REMOTE_TLS_DIR="/var/lib/relay-pubsub-console/tls"

SSH_OPTS=(-o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=15)

_ssh() {
  if [[ -n "$PASS" ]]; then
    command -v sshpass >/dev/null || { echo "sshpass required for password auth" >&2; exit 1; }
    SSHPASS="$PASS" sshpass -e ssh "${SSH_OPTS[@]}" "${USER_}@${HOST}" "$@"
  else
    ssh "${SSH_OPTS[@]}" "${USER_}@${HOST}" "$@"
  fi
}

_rsync() {
  if [[ -n "$PASS" ]]; then
    command -v sshpass >/dev/null || { echo "sshpass required for password auth" >&2; exit 1; }
    SSHPASS="$PASS" rsync -az --delete -e "sshpass -e ssh ${SSH_OPTS[*]}" "$@"
  else
    rsync -az --delete -e "ssh ${SSH_OPTS[*]}" "$@"
  fi
}

echo "relay-pubsub-console deploy"
echo "  target:   ${USER_}@${HOST}"
echo "  api base: ${API_BASE:-'(same origin / proxy)'}"
echo "  gateway:  ${GATEWAY_UPSTREAM}"
echo "  project:  ${PROJECT}"
echo "  port:     ${PORT}"
[[ -n "$PASS" ]] && echo "  auth:     password (sshpass)"
echo

echo "Step 1: Build console locally"
( cd "$UI_DIR" && npm ci && VITE_API_BASE="$API_BASE" VITE_PROJECT="$PROJECT" npm run build )
echo "  done"

echo "Step 2: Prepare remote directories"
_ssh REMOTE_APP_DIR="$REMOTE_APP_DIR" REMOTE_TLS_DIR="$REMOTE_TLS_DIR" DEPLOY_USER="$USER_" bash <<'REMOTE'
set -euo pipefail
SUDO=""
[ "$(id -u)" -ne 0 ] && SUDO="sudo"

if ! id relay-console &>/dev/null; then
    $SUDO useradd --system --no-create-home --shell /usr/sbin/nologin relay-console
fi

$SUDO mkdir -p "$REMOTE_APP_DIR/dist" "$REMOTE_TLS_DIR"
$SUDO chown -R "$DEPLOY_USER":"$DEPLOY_USER" "$REMOTE_APP_DIR" "$REMOTE_TLS_DIR"
REMOTE
echo "  done"

echo "Step 3: Sync build output + proxy server"
_rsync "$UI_DIR/dist/" "${USER_}@${HOST}:${REMOTE_APP_DIR}/dist/"
_rsync "$UI_DIR/server.mjs" "${USER_}@${HOST}:${REMOTE_APP_DIR}/server.mjs"
echo "  done"

echo "Step 4: Generate cert, hand off ownership"
_ssh REMOTE_APP_DIR="$REMOTE_APP_DIR" REMOTE_TLS_DIR="$REMOTE_TLS_DIR" HOST_SAN="$HOST" bash <<'REMOTE'
set -euo pipefail
SUDO=""
[ "$(id -u)" -ne 0 ] && SUDO="sudo"

if [ ! -f "$REMOTE_TLS_DIR/cert.pem" ] || [ ! -f "$REMOTE_TLS_DIR/key.pem" ]; then
    openssl req -x509 -newkey rsa:2048 -nodes -days 825 \
      -keyout "$REMOTE_TLS_DIR/key.pem" -out "$REMOTE_TLS_DIR/cert.pem" \
      -subj "/CN=relay-pubsub-console" \
      -addext "subjectAltName=DNS:localhost,IP:127.0.0.1,IP:${HOST_SAN}"
    echo "Generated self-signed cert at $REMOTE_TLS_DIR"
fi

$SUDO chown -R relay-console:relay-console "$REMOTE_APP_DIR" "$REMOTE_TLS_DIR"
REMOTE
echo "  done"

echo "Step 5: Install systemd unit"
sed \
  -e "s#{{APP_DIR}}#${REMOTE_APP_DIR}#g" \
  -e "s#{{TLS_DIR}}#${REMOTE_TLS_DIR}#g" \
  -e "s#{{PORT}}#${PORT}#g" \
  -e "s#{{GATEWAY_UPSTREAM}}#${GATEWAY_UPSTREAM}#g" \
  "$REPO_DIR/deploy/systemd/relay-pubsub-console.service" | \
  _ssh 'sudo tee /etc/systemd/system/relay-pubsub-console.service >/dev/null && sudo systemctl daemon-reload && (sudo systemctl enable --now relay-pubsub-console || sudo systemctl restart relay-pubsub-console)'
echo "  done"

echo "Step 6: Verify"
sleep 3
_ssh 'systemctl is-active --quiet relay-pubsub-console && echo "  relay-pubsub-console: active" || (echo "  relay-pubsub-console: NOT active"; sudo journalctl -u relay-pubsub-console -n 20 --no-pager)'
curl -ks --max-time 5 "https://${HOST}:${PORT}/" -o /dev/null -w "  HTTP %{http_code} from https://${HOST}:${PORT}/\n" || echo "  could not reach console externally"
curl -ks --max-time 5 "https://${HOST}:${PORT}/healthz" -w "  proxy /healthz → HTTP %{http_code}\n" || echo "  proxy /healthz failed"

echo
echo "Console: https://${HOST}:${PORT}/  (self-signed — accept once; API is same-origin via proxy)"
echo "Demo:    https://${HOST}:${PORT}/#demo"
echo "Tests:   https://${HOST}:${PORT}/#tests"

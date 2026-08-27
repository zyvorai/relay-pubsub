#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs
# SPDX-License-Identifier: Apache-2.0
#
# Deploys the ui/ ops console to a remote host as its own standalone
# systemd service (relay-pubsub-console.service) — a static HTTPS file
# server (http-server, self-signed cert) independent of relay-pubsub.service.
# Does not touch the gateway in any way.
set -euo pipefail

usage() {
  cat <<'EOF'
relay-pubsub-console remote deploy

Usage:
  scripts/deploy-console-remote.sh <host> <user> [options]

Builds ui/ locally (npm run build) and deploys the static output to <host>
as relay-pubsub-console.service — http-server serving over HTTPS with a
self-signed cert, independent of the gateway's own process/port.

Options:
  --api-base URL     VITE_API_BASE baked into the build (default: https://<host>:8081)
  --project NAME     VITE_PROJECT baked into the build (default: projects/fasal-onprem)
  --port PORT        Port for the console to listen on (default: 8082)
  --help             Show this help
EOF
}

HOST=""; USER_=""; API_BASE=""; PROJECT="projects/fasal-onprem"; PORT="8082"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --api-base) API_BASE="$2"; shift 2 ;;
    --project) PROJECT="$2"; shift 2 ;;
    --port) PORT="$2"; shift 2 ;;
    --help) usage; exit 0 ;;
    *)
      if [[ -z "$HOST" ]]; then HOST="$1"
      elif [[ -z "$USER_" ]]; then USER_="$1"
      else echo "Unexpected argument: $1" >&2; exit 1
      fi
      shift ;;
  esac
done

if [[ -z "$HOST" || -z "$USER_" ]]; then
  usage; exit 1
fi

[[ -z "$API_BASE" ]] && API_BASE="https://${HOST}:8081"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
UI_DIR="$REPO_DIR/ui"
REMOTE_APP_DIR="/opt/relay-pubsub-console"
REMOTE_TLS_DIR="/var/lib/relay-pubsub-console/tls"

echo "relay-pubsub-console deploy"
echo "  target:   ${USER_}@${HOST}"
echo "  api base: ${API_BASE}"
echo "  project:  ${PROJECT}"
echo "  port:     ${PORT}"
echo

echo "Step 1: Build console locally"
( cd "$UI_DIR" && npm ci && VITE_API_BASE="$API_BASE" VITE_PROJECT="$PROJECT" npm run build )
echo "  done"

echo "Step 2: Prepare remote directories"
ssh "${USER_}@${HOST}" REMOTE_APP_DIR="$REMOTE_APP_DIR" REMOTE_TLS_DIR="$REMOTE_TLS_DIR" DEPLOY_USER="$USER_" bash <<'REMOTE'
set -euo pipefail
SUDO=""
[ "$(id -u)" -ne 0 ] && SUDO="sudo"

if ! id relay-console &>/dev/null; then
    $SUDO useradd --system --no-create-home --shell /usr/sbin/nologin relay-console
fi

$SUDO mkdir -p "$REMOTE_APP_DIR/dist" "$REMOTE_TLS_DIR"
# Owned by the deploying user for now so rsync/openssl don't need sudo —
# handed over to relay-console after the build artifacts and cert are in place.
$SUDO chown -R "$DEPLOY_USER":"$DEPLOY_USER" "$REMOTE_APP_DIR" "$REMOTE_TLS_DIR"
REMOTE
echo "  done"

echo "Step 3: Sync build output"
rsync -az --delete "$UI_DIR/dist/" "${USER_}@${HOST}:${REMOTE_APP_DIR}/dist/"
echo "  done"

echo "Step 4: Install http-server, generate cert, hand off ownership"
ssh "${USER_}@${HOST}" REMOTE_APP_DIR="$REMOTE_APP_DIR" REMOTE_TLS_DIR="$REMOTE_TLS_DIR" bash <<'REMOTE'
set -euo pipefail
SUDO=""
[ "$(id -u)" -ne 0 ] && SUDO="sudo"

if [ ! -x "$REMOTE_APP_DIR/node_modules/.bin/http-server" ]; then
    (cd "$REMOTE_APP_DIR" && npm install http-server --no-audit --no-fund)
fi

if [ ! -f "$REMOTE_TLS_DIR/cert.pem" ] || [ ! -f "$REMOTE_TLS_DIR/key.pem" ]; then
    openssl req -x509 -newkey rsa:2048 -nodes -days 825 \
      -keyout "$REMOTE_TLS_DIR/key.pem" -out "$REMOTE_TLS_DIR/cert.pem" \
      -subj "/CN=relay-pubsub-console"
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
  "$REPO_DIR/deploy/systemd/relay-pubsub-console.service" | \
  ssh "${USER_}@${HOST}" 'sudo tee /etc/systemd/system/relay-pubsub-console.service >/dev/null && sudo systemctl daemon-reload && (sudo systemctl enable --now relay-pubsub-console || sudo systemctl restart relay-pubsub-console)'
echo "  done"

echo "Step 6: Verify"
sleep 5
ssh "${USER_}@${HOST}" 'systemctl is-active --quiet relay-pubsub-console && echo "  relay-pubsub-console: active" || echo "  relay-pubsub-console: NOT active — check: journalctl -u relay-pubsub-console"'
curl -ks --max-time 5 "https://${HOST}:${PORT}/" -o /dev/null -w "  HTTP %{http_code} from https://${HOST}:${PORT}/\n" || echo "  could not reach console externally"

echo
echo "Console: https://${HOST}:${PORT}/  (self-signed cert — accept the browser warning once)"

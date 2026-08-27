#!/usr/bin/env bash
set -euo pipefail
# ============================================================================
# selftest.sh — Post-deploy verification for relay-pubsub
# ============================================================================
# Verifies the binary, systemd unit, listening ports, and HTTP health
# endpoints, then runs scripts/smoke.sh as the real functional proof.
#
# Usage:
#   ./scripts/selftest.sh              # Run all checks
#   ./scripts/selftest.sh --quick      # Skip the functional smoke test
#
# Exit codes:
#   0 = all checks passed
#   1 = one or more checks failed
# ============================================================================

PASS=0
FAIL=0
WARN=0
QUICK=false

[[ "${1:-}" == "--quick" ]] && QUICK=true

pass() { PASS=$((PASS + 1)); echo "  [pass] $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  [fail] $1"; }
warn() { WARN=$((WARN + 1)); echo "  [warn] $1"; }
section() { echo ""; echo "=== $1 ==="; }

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Pick up the actual configured ports from the systemd env file when the
# caller hasn't already set PUBSUB_HTTP_ADDR/PUBSUB_GRPC_ADDR explicitly —
# otherwise this would silently check the built-in defaults instead of
# whatever ports the deployment was actually reconfigured to use.
if [ -z "${PUBSUB_HTTP_ADDR:-}" ] && [ -z "${PUBSUB_GRPC_ADDR:-}" ] && [ -f /etc/relay-pubsub/relay-pubsub.env ]; then
    # shellcheck disable=SC1091
    . /etc/relay-pubsub/relay-pubsub.env
fi

HTTP_ADDR="${PUBSUB_HTTP_ADDR:-0.0.0.0:8080}"
GRPC_ADDR="${PUBSUB_GRPC_ADDR:-0.0.0.0:50051}"
HTTP_PORT="${HTTP_ADDR##*:}"
GRPC_PORT="${GRPC_ADDR##*:}"

# ── Binary ───────────────────────────────────────────────────────────────────
section "Binary"

BIN=""
if command -v relay-pubsub &>/dev/null; then
    BIN="$(command -v relay-pubsub)"
    pass "relay-pubsub found: $BIN"
elif [ -x /usr/local/bin/relay-pubsub ]; then
    BIN=/usr/local/bin/relay-pubsub
    pass "relay-pubsub found: $BIN"
elif [ -x "$REPO_DIR/target/release/relay-pubsub" ]; then
    BIN="$REPO_DIR/target/release/relay-pubsub"
    pass "relay-pubsub found: $BIN (local build)"
else
    fail "relay-pubsub binary not found in PATH, /usr/local/bin, or target/release"
fi

if [ -n "$BIN" ]; then
    if ver=$("$BIN" --version 2>/dev/null); then
        pass "relay-pubsub --version: $ver"
    else
        warn "relay-pubsub --version failed (or unsupported)"
    fi
fi

# ── systemd ──────────────────────────────────────────────────────────────────
section "systemd"

if command -v systemctl &>/dev/null && systemctl list-unit-files relay-pubsub.service &>/dev/null; then
    if systemctl is-active --quiet relay-pubsub; then
        pass "relay-pubsub.service is active"
    else
        fail "relay-pubsub.service is not active (check: journalctl -u relay-pubsub)"
    fi
    if systemctl is-enabled --quiet relay-pubsub 2>/dev/null; then
        pass "relay-pubsub.service is enabled"
    else
        warn "relay-pubsub.service is not enabled (won't start on boot)"
    fi
else
    warn "no relay-pubsub.service unit found — assuming non-systemd deployment"
fi

# ── Network ──────────────────────────────────────────────────────────────────
section "Network"

if command -v ss &>/dev/null; then
    if ss -ltn 2>/dev/null | grep -q ":${HTTP_PORT}\b"; then
        pass "HTTP port ${HTTP_PORT} is listening"
    else
        fail "HTTP port ${HTTP_PORT} is not listening"
    fi
    if ss -ltn 2>/dev/null | grep -q ":${GRPC_PORT}\b"; then
        pass "gRPC port ${GRPC_PORT} is listening"
    else
        fail "gRPC port ${GRPC_PORT} is not listening"
    fi
else
    warn "ss not found — skipping port-listening checks"
fi

# ── HTTP health ──────────────────────────────────────────────────────────────
section "HTTP health"

# TLS-only, self-signed cert by default — -k skips cert verification.
BASE="https://127.0.0.1:${HTTP_PORT}"
if curl -k -fsS "${BASE}/healthz" 2>/dev/null; then
    echo ""
    pass "GET /healthz responded"
else
    fail "GET /healthz did not respond"
fi
if curl -k -fsS "${BASE}/readyz" 2>/dev/null; then
    echo ""
    pass "GET /readyz responded"
else
    fail "GET /readyz did not respond"
fi
warn "note: /healthz and /readyz do not verify backend (Relay) connectivity — known limitation, see deploy plan"

# ── Functional smoke ─────────────────────────────────────────────────────────
if ! $QUICK; then
    section "Functional smoke"
    if [ -f "$SCRIPT_DIR/smoke.sh" ]; then
        if BASE="${BASE}" bash "$SCRIPT_DIR/smoke.sh" >/dev/null 2>&1; then
            pass "scripts/smoke.sh publish+pull round-trip succeeded"
        else
            fail "scripts/smoke.sh failed — service is up but not functioning correctly"
        fi
    else
        warn "scripts/smoke.sh not found alongside selftest.sh — skipping functional check"
    fi
fi

# ── Summary ──────────────────────────────────────────────────────────────────
echo ""
echo "=== Summary ==="
echo "  Passed: $PASS  Failed: $FAIL  Warnings: $WARN"

if [[ $FAIL -gt 0 ]]; then
    echo ""
    echo "SELFTEST FAILED — fix the above errors before proceeding"
    exit 1
else
    echo ""
    echo "SELFTEST PASSED"
    exit 0
fi

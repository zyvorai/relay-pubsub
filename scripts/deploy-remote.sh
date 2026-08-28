#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs
# SPDX-License-Identifier: Apache-2.0
# ─────────────────────────────────────────────────────────────
# relay-pubsub — Remote deployment (SSH + rsync) to a systemd host
#
# Profiles:
#   default     Sync sources → install system deps → build on remote → verify
#   --quick     Rsync + remote build only (skip system dep install)
#   --quick --build-local   Rsync a pre-built binary (built locally, cross-built
#                           via Docker when the operator's machine isn't Linux)
#
# Post-deploy: remote scripts/selftest.sh (non-fatal)
# ─────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
VERSION="1.0.0"
REMOTE_DIR=""
DEPLOY_PROFILE="full"
DEPLOY_LOG="${RELAY_DEPLOY_LOG:-${HOME}/.relay-pubsub/deploy-$(date +%Y%m%d-%H%M%S).log}"

QUICK_MODE=false
UNINSTALL=false
PURGE=false
FLEET_FILE=""
KEY_AUTH=false
DRY_RUN=false
SKIP_SYNC=false
SKIP_VERIFY=false
BUILD_LOCAL=false
VERIFY_ONLY=false
PREFLIGHT_ONLY=false
VERBOSE=false
SSH_RETRIES="${RELAY_DEPLOY_SSH_RETRIES:-3}"
POSITIONAL=()

usage() {
    cat <<EOF
relay-pubsub remote deploy v${VERSION}

Usage:
  $0 <host> <user> [options]
  $0 user@host [options]
  $0 --fleet hosts.txt

Profiles:
  (default)                    Full remote build (installs build-essential/gcc, rustup if missing)
  --quick                      Rsync + cargo build on remote (skip system dep install)
  --quick --build-local        Install a locally built release binary (recommended)

Options:
  --help              Show this help
  --dry-run           Print steps without SSH/rsync/build
  --preflight-only    SSH + disk/sudo checks, then exit
  --verify-only       Run remote selftest only (no deploy)
  --skip-sync         Skip rsync (sources already on host)
  --skip-verify       Skip remote selftest
  --build-local       With --quick: install a locally built release binary
  --key               SSH key auth (clear password)
  --uninstall         Remove relay-pubsub from host
  --purge             With --uninstall: also remove /etc/relay-pubsub
  -v, --verbose       Verbose rsync

Environment:
  RELAY_DEPLOY_LOG          Log file path
  RELAY_DEPLOY_SSH_RETRIES  SSH retry count (default: 3)
  DEPLOY_DIR                Override remote staging dir (default: ~/.deployments/relay-pubsub)

Examples:
  $0 10.0.0.5 root --build-local --quick
  $0 sus@10.0.0.5 --quick
  $0 10.0.0.5 root --verify-only
  make deploy-remote-quick H=10.0.0.5 U=root

Fleet file (one host per line, key auth only — no password field):
  host user [opts]
  user@host [opts]
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)        usage; exit 0 ;;
        --quick)          QUICK_MODE=true; DEPLOY_PROFILE="quick"; shift ;;
        --uninstall)      UNINSTALL=true; shift ;;
        --purge)          PURGE=true; shift ;;
        --key)            KEY_AUTH=true; shift ;;
        --dry-run)        DRY_RUN=true; shift ;;
        --skip-sync)      SKIP_SYNC=true; shift ;;
        --skip-verify)    SKIP_VERIFY=true; shift ;;
        --build-local)    BUILD_LOCAL=true; shift ;;
        --verify-only)    VERIFY_ONLY=true; shift ;;
        --preflight-only) PREFLIGHT_ONLY=true; shift ;;
        -v|--verbose)     VERBOSE=true; shift ;;
        --fleet)
            shift
            FLEET_FILE="${1:?--fleet requires a hosts file path}"
            shift
            ;;
        *)
            POSITIONAL+=("$1")
            shift
            ;;
    esac
done

TARGET_HOST="${POSITIONAL[0]:-}"
TARGET_USER="${POSITIONAL[1]:-root}"
TARGET_PASS="${POSITIONAL[2]:-}"

if [ "$KEY_AUTH" = true ]; then
    TARGET_PASS=""
fi

if [[ -n "${TARGET_HOST}" && "${TARGET_HOST}" == *"@"* ]]; then
    TARGET_USER="${TARGET_HOST%%@*}"
    TARGET_HOST="${TARGET_HOST#*@}"
fi

_use_color() { [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; }
if _use_color; then
    C_OK=$'\033[32m'; C_FAIL=$'\033[31m'; C_INFO=$'\033[36m'; C_WARN=$'\033[33m'
    C_DIM=$'\033[2m'; C_BOLD=$'\033[1m'; C_MAG=$'\033[35m'; C_RST=$'\033[0m'
else
    C_OK= C_FAIL= C_INFO= C_WARN= C_DIM= C_BOLD= C_MAG= C_RST=
fi

_log_file() { mkdir -p "$(dirname "$DEPLOY_LOG")" 2>/dev/null || true; echo "[$(date -Iseconds)] $*" >>"$DEPLOY_LOG" 2>/dev/null || true; }
ok()   { echo "${C_OK}  [ok] $*${C_RST}"; _log_file "OK $*"; }
fail() { echo "${C_FAIL}  [fail] $*${C_RST}" >&2; _log_file "FAIL $*"; exit 1; }
info() { echo "${C_INFO}  [info] $*${C_RST}"; _log_file "INFO $*"; }
warn() { echo "${C_WARN}  [warn] $*${C_RST}"; _log_file "WARN $*"; }
dry()  { echo "${C_MAG}  [dry-run] $*${C_RST}"; _log_file "DRY $*"; }

profile_label() {
    if [ "$UNINSTALL" = true ]; then echo "uninstall"; return; fi
    if [ "$VERIFY_ONLY" = true ]; then echo "verify-only"; return; fi
    if [ "$PREFLIGHT_ONLY" = true ]; then echo "preflight"; return; fi
    echo "${DEPLOY_PROFILE}"
}

print_banner() {
    local target
    target="${TARGET_USER}@${TARGET_HOST}"
    [ -z "${TARGET_HOST}" ] && target="(fleet mode)"
    echo ""
    echo "${C_BOLD}relay-pubsub remote deploy v${VERSION}${C_RST}"
    echo "  target:  ${target}"
    echo "  profile: $(profile_label)"
    [ "$DRY_RUN" = true ] && echo "  ${C_MAG}DRY-RUN — no remote changes${C_RST}"
    [ -n "${FLEET_FILE}" ] && echo "  fleet:   ${FLEET_FILE}"
    echo ""
}

STEP_T0=0
STEP_IDX=0

step_begin() {
    STEP_IDX=$((STEP_IDX + 1))
    STEP_T0=$(date +%s)
    echo ""
    echo "${C_BOLD}${C_INFO}Step ${STEP_IDX}: $*${C_RST}"
    _log_file "STEP ${STEP_IDX}: $*"
}

step_end() {
    echo "${C_DIM}  done in $(( $(date +%s) - STEP_T0 ))s${C_RST}"
}

run_step() {
    step_begin "$1"; shift
    if [ "$DRY_RUN" = true ]; then dry "would run: $*"; step_end; return 0; fi
    "$@"; step_end
}

SSH_OPTS="-o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=15 -o ServerAliveInterval=30"
if [ -z "${TARGET_PASS}" ]; then
    SSH_OPTS+=" -o BatchMode=yes -o PreferredAuthentications=publickey"
fi

_ssh_once() {
    if [ -n "${TARGET_PASS}" ] && command -v sshpass &>/dev/null; then
        export SSHPASS="${TARGET_PASS}"
        sshpass -e ssh ${SSH_OPTS} "${TARGET_USER}@${TARGET_HOST}" "$@"
    else
        ssh ${SSH_OPTS} "${TARGET_USER}@${TARGET_HOST}" "$@"
    fi
}

_ssh() {
    local attempt=1 max="${SSH_RETRIES}"
    while [ "$attempt" -le "$max" ]; do
        if _ssh_once "$@"; then
            return 0
        fi
        attempt=$((attempt + 1))
        if [ "$attempt" -le "$max" ]; then
            local _d=$(( 2 * (attempt - 1) )); _d=$(( _d < 2 ? 2 : _d > 30 ? 30 : _d ))
            warn "SSH retry ${attempt}/${max}" && sleep "${_d}"
        fi
    done
    return 1
}

_rsync() {
    local opts="-az --delete"
    [ "$VERBOSE" = true ] && opts+=" --progress"
    if [ -n "${TARGET_PASS}" ] && command -v sshpass &>/dev/null; then
        export SSHPASS="${TARGET_PASS}"
        rsync ${opts} -e "sshpass -e ssh ${SSH_OPTS}" "$@"
    else
        rsync ${opts} -e "ssh ${SSH_OPTS}" "$@"
    fi
}

validate() {
    if [ -z "${FLEET_FILE}" ]; then
        [ -n "${TARGET_HOST}" ] || { usage; exit 1; }
    fi
    [ -f "${PROJECT_DIR}/Cargo.toml" ] || fail "Not in relay-pubsub repo: ${PROJECT_DIR}"
    if [ -n "${TARGET_PASS}" ]; then
        warn "Password auth is deprecated. Prefer: ssh-copy-id ${TARGET_USER}@${TARGET_HOST}"
        command -v sshpass &>/dev/null || fail "sshpass required for password auth (dnf/apt install sshpass)"
    fi
}

check_connectivity() {
    info "SSH -> ${TARGET_USER}@${TARGET_HOST}  log: ${DEPLOY_LOG}"
    if [ "$DRY_RUN" = true ]; then
        REMOTE_DIR="${DEPLOY_DIR:-${HOME}/.deployments/relay-pubsub}"
        return 0
    fi
    _ssh "echo ok" &>/dev/null || fail "SSH failed — try: ssh-copy-id ${TARGET_USER}@${TARGET_HOST}"
    ok "SSH connected"
    local remote_home
    remote_home=$(_ssh "echo \$HOME" 2>/dev/null | tr -d '\r')
    remote_home="${remote_home:-/home/${TARGET_USER}}"
    REMOTE_DIR="${DEPLOY_DIR:-${remote_home}/.deployments/relay-pubsub}"
    info "Remote path: ${REMOTE_DIR}"
}

preflight_remote() {
    info "Preflight on ${TARGET_HOST}..."
    if [ "$DRY_RUN" = true ]; then return 0; fi
    _ssh bash <<'REMOTE' || fail "Preflight failed"
set -e
echo "  host: $(hostname -f 2>/dev/null || hostname)"
echo "  os:   $(. /etc/os-release 2>/dev/null && echo "$PRETTY_NAME" || uname -s)"
echo "  arch: $(uname -m)"
echo "  mem:  $(free -h 2>/dev/null | awk '/^Mem:/{print $2}' || echo n/a)"
echo "  disk: $(df -h / 2>/dev/null | awk 'NR==2{print $4 " free on " $1}' || echo n/a)"
AVAIL=$(df -BG / 2>/dev/null | awk 'NR==2{gsub(/G/,"",$4); print $4}' || echo 99)
if [ "${AVAIL}" -lt 4 ] 2>/dev/null; then
    echo "  warning: less than 4G free on / — an on-host release build may fail"
fi
if [ "$(id -u)" -ne 0 ]; then
    if ! sudo -n true 2>/dev/null; then
        echo "  error: non-root user needs passwordless sudo for install/systemctl"
        exit 1
    fi
    echo "  ok: passwordless sudo"
else
    echo "  ok: running as root"
fi
command -v curl >/dev/null && echo "  ok: curl" || echo "  warning: curl missing (needed for rustup)"
REMOTE
    ok "Preflight passed"
}

build_local_artifacts() {
    step_begin "Local build (release)"
    if [ "$DRY_RUN" = true ]; then
        dry "would run: cargo build --release (or docker build --target builder as a fallback)"
        return 0
    fi
    if [ "$(uname -s)" = "Linux" ] && command -v cargo &>/dev/null; then
        (cd "${PROJECT_DIR}" && cargo build --release --locked)
    else
        info "Non-Linux build host (or no local Rust toolchain) — cross-building via Docker"
        command -v docker &>/dev/null || fail "docker is required to cross-build on a non-Linux host"
        (cd "${PROJECT_DIR}" && docker build --target builder -t relay-pubsub-builder:local -f Dockerfile .)
        docker rm -f relay-pubsub-extract &>/dev/null || true
        docker create --name relay-pubsub-extract relay-pubsub-builder:local >/dev/null
        mkdir -p "${PROJECT_DIR}/target/release"
        docker cp relay-pubsub-extract:/src/target/release/relay-pubsub "${PROJECT_DIR}/target/release/relay-pubsub"
        docker rm -f relay-pubsub-extract >/dev/null
    fi
    [ -f "${PROJECT_DIR}/target/release/relay-pubsub" ] || fail "target/release/relay-pubsub missing after build"
    ok "Local binary ready"
    step_end
}

sync_files() {
    if [ "$SKIP_SYNC" = true ]; then
        info "Skipping rsync (--skip-sync)"
        return 0
    fi
    _ssh "mkdir -p '${REMOTE_DIR}'"
    local excludes=(
        --exclude '.git'
        --exclude 'target'
        --exclude 'ui/node_modules'
        --exclude 'ui/dist'
        --exclude '*.log'
    )
    _rsync "${excludes[@]}" "${PROJECT_DIR}/" "${TARGET_USER}@${TARGET_HOST}:${REMOTE_DIR}/"
    ok "Source synced to ${REMOTE_DIR}"
}

sync_binary_only() {
    local bin="${PROJECT_DIR}/target/release/relay-pubsub"
    [ -f "$bin" ] || fail "Missing $bin — run with --build-local after building"
    _ssh "mkdir -p '${REMOTE_DIR}/bin' '${REMOTE_DIR}/deploy/systemd' '${REMOTE_DIR}/scripts'"
    _rsync "$bin" "${TARGET_USER}@${TARGET_HOST}:${REMOTE_DIR}/bin/relay-pubsub"
    _rsync "${PROJECT_DIR}/deploy/systemd/" "${TARGET_USER}@${TARGET_HOST}:${REMOTE_DIR}/deploy/systemd/"
    _rsync "${PROJECT_DIR}/scripts/selftest.sh" "${PROJECT_DIR}/scripts/smoke.sh" "${TARGET_USER}@${TARGET_HOST}:${REMOTE_DIR}/scripts/"
    ok "Release binary and deploy assets synced"
}

install_system_deps() {
    _ssh bash <<'REMOTE'
set -euo pipefail
SUDO=""
[ "$(id -u)" -ne 0 ] && SUDO="sudo"

pkg_install() {
    $SUDO "$@" || return 1
}

. /etc/os-release 2>/dev/null || true
_ID="${ID:-}" _ID_LIKE="${ID_LIKE:-}"
if [[ "$_ID" == "debian" || "$_ID" == "ubuntu" || "$_ID_LIKE" == *"debian"* || "$_ID_LIKE" == *"ubuntu"* ]]; then
    PKG=apt-get
    pkg_install apt-get update -qq
elif command -v dnf &>/dev/null; then
    PKG=dnf
elif command -v yum &>/dev/null; then
    PKG=yum
elif command -v apt-get &>/dev/null; then
    PKG=apt-get
    pkg_install apt-get update -qq
else
    echo "ERROR: unsupported package manager"
    exit 1
fi

if [ "$PKG" = "apt-get" ]; then
    # cmake is required to build aws-lc-sys (pulled in transitively by the
    # rustls-based TLS stack — axum-server/hyper-rustls/tonic's tokio-rustls).
    pkg_install apt-get install -y -qq build-essential pkg-config curl git ca-certificates cmake
elif [ "$PKG" = "dnf" ]; then
    # RHEL9/Rocky9/Alma9 ship curl-minimal by default; installing the full
    # curl package conflicts with it unless dnf may swap it out.
    pkg_install dnf install -y --allowerasing gcc make pkgconfig curl git ca-certificates cmake
else
    pkg_install "$PKG" install -y gcc make pkgconfig curl git ca-certificates cmake
fi
echo "System dependencies installed"
REMOTE
}

ensure_rust_remote() {
    _ssh bash <<'REMOTE'
set -e
if command -v cargo &>/dev/null; then
    echo "Rust: $(rustc --version 2>/dev/null || true)"
    exit 0
fi
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
echo "Rust installed: $(rustc --version)"
REMOTE
}

build_install_remote() {
    _ssh env REMOTE_STAGING="${REMOTE_DIR}" bash <<'REMOTE'
set -e
SUDO=""
[ "$(id -u)" -ne 0 ] && SUDO="sudo"
source "$HOME/.cargo/env" 2>/dev/null || true
cd "${REMOTE_STAGING}"
cargo build --release --locked
$SUDO install -m755 target/release/relay-pubsub /usr/local/bin/relay-pubsub
echo "Installed: $(relay-pubsub --version 2>/dev/null || echo ok)"
REMOTE
}

install_binary_quick() {
    _ssh env REMOTE_STAGING="${REMOTE_DIR}" bash <<'REMOTE'
set -e
SUDO=""
[ "$(id -u)" -ne 0 ] && SUDO="sudo"
$SUDO install -m755 "${REMOTE_STAGING}/bin/relay-pubsub" /usr/local/bin/relay-pubsub
echo "Installed: $(relay-pubsub --version 2>/dev/null || echo ok)"
REMOTE
}

install_systemd_unit() {
    _ssh env REMOTE_STAGING="${REMOTE_DIR}" bash <<'REMOTE'
set -e
SUDO=""
[ "$(id -u)" -ne 0 ] && SUDO="sudo"
$SUDO mkdir -p /etc/relay-pubsub
if [ ! -f /etc/relay-pubsub/relay-pubsub.env ]; then
    $SUDO cp "${REMOTE_STAGING}/deploy/systemd/relay-pubsub.env.example" /etc/relay-pubsub/relay-pubsub.env
    echo "Seeded /etc/relay-pubsub/relay-pubsub.env (edit to configure RELAY_BACKEND=http etc.)"
fi
if ! id relay-pubsub &>/dev/null; then
    $SUDO useradd --system --no-create-home --shell /usr/sbin/nologin relay-pubsub
fi
$SUDO cp "${REMOTE_STAGING}/deploy/systemd/relay-pubsub.service" /etc/systemd/system/relay-pubsub.service
$SUDO systemctl daemon-reload
if systemctl is-active --quiet relay-pubsub; then
    $SUDO systemctl restart relay-pubsub
else
    $SUDO systemctl enable --now relay-pubsub
fi
sleep 1
if systemctl is-active --quiet relay-pubsub; then
    echo "relay-pubsub: active"
else
    echo "relay-pubsub: NOT active — check: journalctl -u relay-pubsub"
    exit 1
fi
REMOTE
}

verify_remote() {
    info "Running selftest on ${TARGET_HOST}..."
    if [ "$DRY_RUN" = true ]; then
        dry "would run: bash ${REMOTE_DIR}/scripts/selftest.sh"
        return 0
    fi
    # Single SSH attempt — do not retry when selftest exits non-zero
    if _ssh_once "bash '${REMOTE_DIR}/scripts/selftest.sh'"; then
        ok "selftest passed"
    else
        warn "selftest reported failures (relay-pubsub is installed; see above)"
        return 0
    fi
}

do_uninstall() {
    _ssh env REMOTE_STAGING="${REMOTE_DIR}" PURGE="${PURGE}" bash <<'REMOTE'
set -e
SUDO=""
[ "$(id -u)" -ne 0 ] && SUDO="sudo"
$SUDO systemctl disable --now relay-pubsub 2>/dev/null || true
$SUDO rm -f /etc/systemd/system/relay-pubsub.service
$SUDO systemctl daemon-reload
$SUDO rm -f /usr/local/bin/relay-pubsub
rm -rf "${REMOTE_STAGING}"
if [ "${PURGE}" = "true" ]; then
    $SUDO rm -rf /etc/relay-pubsub
fi
echo "relay-pubsub removed"
REMOTE
    ok "Uninstalled on ${TARGET_HOST}"
}

deploy_profile_full() {
    run_step "Sync sources" sync_files
    run_step "System dependencies" install_system_deps
    run_step "Rust toolchain" ensure_rust_remote
    run_step "Build and install" build_install_remote
    run_step "systemd unit" install_systemd_unit
}

deploy_profile_quick() {
    if [ "$BUILD_LOCAL" = true ]; then
        build_local_artifacts
        run_step "Sync binary" sync_binary_only
        run_step "Install binary" install_binary_quick
    else
        run_step "Sync sources" sync_files
        run_step "Build and install" build_install_remote
    fi
    run_step "systemd unit" install_systemd_unit
}

print_deployment_summary() {
    echo ""
    echo "${C_OK}${C_BOLD}Deploy complete — ${TARGET_USER}@${TARGET_HOST}${C_RST}"
    echo "  log:    ${DEPLOY_LOG}"
    echo "  remote: ${REMOTE_DIR}"
    echo ""
    echo "  ssh ${TARGET_USER}@${TARGET_HOST}"
    echo "  ssh ${TARGET_USER}@${TARGET_HOST} systemctl status relay-pubsub"
    echo "  BASE=https://${TARGET_HOST}:8080 bash scripts/smoke.sh"
    echo "  bash ${REMOTE_DIR}/scripts/selftest.sh"
    echo ""
}

deploy_fleet() {
    local hosts_file="$1"
    [ -f "$hosts_file" ] || fail "Fleet file not found: $hosts_file"
    chmod 600 "$hosts_file" 2>/dev/null || true
    local count=0
    # Fleet lines are key-auth only (password auth is deprecated everywhere
    # else in this script too) — this keeps the field count fixed at
    # "host user [opts...]" / "user@host [opts...]" with no ambiguous
    # password slot for `read` to misassign a flag into.
    while IFS=' ' read -r host user opts; do
        [ -z "$host" ] && continue
        [[ "$host" =~ ^# ]] && continue
        count=$((count + 1))
        TARGET_HOST="$host"
        TARGET_USER="${user:-root}"
        TARGET_PASS=""
        if [[ "$host" == *"@"* ]]; then
            TARGET_USER="${host%%@*}"
            TARGET_HOST="${host#*@}"
            # With the combined user@host form, whatever `read` put in $user
            # is actually the first opts token (or empty).
            opts="${user}${opts:+ $opts}"
        fi
        STEP_IDX=0
        print_banner
        check_connectivity
        preflight_remote
        if [[ "${opts:-}" == *"--uninstall"* ]]; then
            run_step "Uninstall" do_uninstall
        elif [[ "${opts:-}" == *"--quick"* ]]; then
            QUICK_MODE=true
            BUILD_LOCAL=false
            [[ "${opts:-}" == *"--build-local"* ]] && BUILD_LOCAL=true
            deploy_profile_quick
        else
            deploy_profile_full
        fi
        [ "$SKIP_VERIFY" != true ] && verify_remote
        print_deployment_summary
    done < "$hosts_file"
    ok "Fleet complete — ${count} host(s)"
}

main() {
    print_banner
    if [ -n "${FLEET_FILE}" ]; then
        validate
        deploy_fleet "${FLEET_FILE}"
        exit 0
    fi
    validate
    check_connectivity
    preflight_remote

    if [ "$PREFLIGHT_ONLY" = true ]; then
        ok "Preflight-only complete"
        exit 0
    fi

    if [ "$UNINSTALL" = true ]; then
        run_step "Uninstall relay-pubsub" do_uninstall
        exit 0
    fi

    if [ "$VERIFY_ONLY" = true ]; then
        [ "$SKIP_VERIFY" != true ] && run_step "Verify" verify_remote
        print_deployment_summary
        exit 0
    fi

    if [ "$BUILD_LOCAL" = true ] && [ "$QUICK_MODE" != true ]; then
        warn "--build-local is intended with --quick; ignoring for full profile"
        BUILD_LOCAL=false
    fi

    case "${DEPLOY_PROFILE}" in
        quick) deploy_profile_quick ;;
        *)     deploy_profile_full ;;
    esac

    [ "$SKIP_VERIFY" != true ] && run_step "Verify (selftest)" verify_remote
    print_deployment_summary
}

main "$@"

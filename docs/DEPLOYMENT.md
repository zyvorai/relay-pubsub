# Deploying relay-pubsub

## Currently deployed instances

| Host | User | HTTP | gRPC | Backend | Notes |
|---|---|---|---|---|---|
| `212.8.248.187` | `sus` | `8081` (HTTPS) | `50061` (gRPCS) | `memory` | Non-default ports — host already runs nginx on `:8080` and a `machina-agent` process on `:50051`. Gateway terminates TLS itself (self-signed cert, generated at `/var/lib/relay-pubsub/tls/`) — no reverse proxy in front. Deployed via the full remote-build profile (`bash scripts/deploy-remote.sh 212.8.248.187 sus`), verified with `scripts/selftest.sh` (9/9 pass) and `scripts/smoke.sh` run externally against `https://212.8.248.187:8081` (self-signed cert — `curl -k`). |

To manage this instance:

```bash
ssh sus@212.8.248.187 systemctl status relay-pubsub     # check status
ssh sus@212.8.248.187 sudo systemctl restart relay-pubsub
ssh sus@212.8.248.187 cat /etc/relay-pubsub/relay-pubsub.env   # current config
BASE=https://212.8.248.187:8081 bash scripts/smoke.sh    # functional verification (self-signed cert — smoke.sh uses curl -k)
make deploy-remote-quick H=212.8.248.187 U=sus            # redeploy (rebuilds locally, rsyncs binary, restarts service — config file is preserved)
make deploy-remote-uninstall H=212.8.248.187 U=sus        # remove entirely
```

Update this table whenever a new host is deployed to or an existing one is decommissioned — it's the source of truth for "what's actually running where."

---

Two supported deployment targets:

1. [Bare Linux host via systemd](#1-bare-linux-host-via-systemd) — `scripts/deploy-remote.sh`
2. [Kubernetes pods](#2-kubernetes-pods) — `deploy/k8s/` (plain manifest) or `deploy/helm/relay-pubsub/` (Helm chart)

Both reuse `scripts/smoke.sh` (a real publish → pull round-trip over the REST API) as the functional proof that a deployment actually works, since the `/healthz`/`/readyz` endpoints are liveness-only — see [Known limitation: health checks don't check the backend](#known-limitation-healthzreadyz-dont-check-the-backend).

---

## 1. Bare Linux host via systemd

### Prerequisites

- Local machine: `ssh`, `rsync`. For the recommended `--build-local` profile: either a Linux machine with a Rust toolchain, **or** `docker` (used to cross-build a `bookworm`-compatible binary via the project's own `Dockerfile`, so a macOS/Windows operator's machine works too).
- Target host: Debian/Ubuntu or RHEL/Fedora-family, SSH access (key-based auth strongly recommended — `ssh-copy-id user@host`), passwordless `sudo` if not connecting as root.

### Deploy

```bash
bash scripts/deploy-remote.sh <host> <user> --build-local --quick
# or
make deploy-remote-quick H=<host> U=<user>
```

This:
1. Builds a release binary locally (`cargo build --release --locked` on Linux, or cross-builds via `docker build --target builder -f Dockerfile .` otherwise).
2. Copies just that binary (`rsync`) plus `deploy/systemd/` and `scripts/{selftest.sh,smoke.sh}` to `~/.deployments/relay-pubsub` on the remote host.
3. Installs it to `/usr/local/bin/relay-pubsub`.
4. Creates a dedicated `relay-pubsub` system user, seeds `/etc/relay-pubsub/relay-pubsub.env` from `deploy/systemd/relay-pubsub.env.example` **only if that file doesn't already exist** (so re-deploys never clobber a host's configured settings), installs `deploy/systemd/relay-pubsub.service`, and does `systemctl enable --now` (or `restart`, if already running).
5. Runs `scripts/selftest.sh` on the host and reports pass/fail (non-fatal — the binary is installed either way; selftest just tells you if it's actually healthy).

### Other profiles

| Command | What it does |
|---|---|
| `scripts/deploy-remote.sh <host> <user>` (no flags) | Full profile: rsyncs sources, installs `build-essential`/`gcc` + rustup on the remote host if missing, builds there with `cargo build --release --locked`. Use this if you don't want to build locally at all. |
| `--quick` (no `--build-local`) | Rsync sources + remote `cargo build`, but skip the system-dependency install step (assumes the host is already provisioned). |
| `--preflight-only` | SSH connectivity + hostname/OS/arch/mem/disk/sudo checks only. No changes made. |
| `--verify-only` | Runs `scripts/selftest.sh` on the host. No deploy. |
| `--uninstall` [`--purge`] | Stops/disables the systemd unit, removes the binary and unit file. `--purge` also removes `/etc/relay-pubsub`. |
| `--dry-run` | Prints every step it would take without touching the remote host. |
| `--fleet hosts.txt` | Repeats the chosen profile across every `host user [opts]` line in a file. |

Run `bash scripts/deploy-remote.sh --help` for the full flag list, and `make deploy-remote H=<host> U=<user> ARGS="..."` to pass arbitrary flags through Make.

### Configuration

`/etc/relay-pubsub/relay-pubsub.env` is a systemd `EnvironmentFile` (`KEY=value` per line, `#` comments). It's seeded from `deploy/systemd/relay-pubsub.env.example` on first install and never overwritten afterward — edit it directly on the host and `systemctl restart relay-pubsub` (or just re-run `deploy-remote.sh`, which restarts automatically after a successful install).

| Variable | Default | Purpose |
|---|---|---|
| `PUBSUB_GRPC_ADDR` | `0.0.0.0:50051` | gRPCS listener (Google Pub/Sub compatible API) — TLS-only |
| `PUBSUB_HTTP_ADDR` | `0.0.0.0:8080` | HTTPS/admin listener, also serves `/healthz`, `/readyz`, `/metrics` — TLS-only |
| `PUBSUB_TLS_CERT` | `/var/lib/relay-pubsub/tls/cert.pem` | Cert used by both listeners. Self-signed and generated here on first start if missing; point at a CA-signed cert instead if you have one |
| `PUBSUB_TLS_KEY` | `/var/lib/relay-pubsub/tls/key.pem` | Matching private key |
| `PUBSUB_TLS_SAN` | `localhost,relay-pubsub` | Comma-separated hostnames/IPs for the *generated* self-signed cert — only takes effect the first time a cert is generated |
| `RELAY_BACKEND` | `memory` | `memory` (self-contained demo), `http` (invented topics/subscriptions contract), or `relay-events` (Relay's real API — see [RELAY_EVENTS_BACKEND.md](RELAY_EVENTS_BACKEND.md)) |
| `RELAY_BASE_URL` | `http://relay:9090` | Only used when `RELAY_BACKEND=http` or `relay-events` |
| `RELAY_AUTH_TOKEN` | *(empty = none)* | Bearer token sent to the Relay backend when `RELAY_BACKEND=http` or `relay-events` |
| `RELAY_HTTP_TIMEOUT_SECONDS` | `15` | HTTP client timeout to the Relay backend |
| `RELAY_PUBSUB_AUTH_TOKEN` | *(empty = none)* | If set, gateway requires `Authorization: Bearer <token>` on all `/v1/*` and `/admin/*` requests |
| `FASAL_GCP_PROJECT` / `FASAL_ACTIONS_TOPIC` / `FASAL_ACTIONS_SUBSCRIPTION` | `fasal-onprem` / `farm-actions` / `farm-actions-sub` | Only used when `RELAY_BACKEND=relay-events` |

#### Port already in use on the target host?

This is common on shared/multi-purpose boxes (e.g. nginx already on `:8080`, or another service on `:50051`). Just edit the two `PUBSUB_*_ADDR` lines in `/etc/relay-pubsub/relay-pubsub.env` to free ports and restart:

```bash
ssh <user>@<host> "sudo sed -i \
  -e 's|^PUBSUB_HTTP_ADDR=.*|PUBSUB_HTTP_ADDR=0.0.0.0:8081|' \
  -e 's|^PUBSUB_GRPC_ADDR=.*|PUBSUB_GRPC_ADDR=0.0.0.0:50061|' \
  /etc/relay-pubsub/relay-pubsub.env && sudo systemctl restart relay-pubsub"
```

`scripts/selftest.sh` automatically reads the actual configured ports from this file (falling back to the defaults only if it doesn't exist), so it always checks the right ports even after a change like this.

### Verify

```bash
make deploy-remote-verify H=<host> U=<user>           # runs scripts/selftest.sh remotely
ssh <user>@<host> systemctl status relay-pubsub
BASE="https://<host>:<http-port>" bash scripts/smoke.sh   # real publish/pull round-trip, run from anywhere (self-signed cert — smoke.sh uses curl -k)
```

`selftest.sh` checks (in order): binary present + `--version` works, systemd unit active/enabled, both ports listening, `/healthz` + `/readyz` respond, and finally runs `scripts/smoke.sh` itself as the actual functional proof. It exits non-zero if anything fails.

### Uninstall

```bash
make deploy-remote-uninstall H=<host> U=<user>
# or: bash scripts/deploy-remote.sh <host> <user> --uninstall [--purge]
```

---

## 2. Kubernetes pods

Two manifests, for two different scenarios:

- **`deploy/k8s/gateway.yaml`** — plain Deployment + Service. Always assumes `RELAY_BACKEND=http` and requires a `relay-pubsub-secrets`/`relay-token` Secret to already exist in the cluster. This is the "real Relay backend" reference manifest.
- **`deploy/helm/relay-pubsub/`** — the same Deployment/Service as a parameterized Helm chart. Supports `--set relay.backend=memory` to run self-contained with no Secret required (see below) — the `RELAY_AUTH_TOKEN` env var is only templated in when `relay.backend=http`.

The image both reference is `ghcr.io/zyvorai/relay-pubsub`, built and pushed by the `.github/workflows/release-image.yml` GitHub Actions workflow (on push to `main` and on `v*` tags — no manual step needed once merged).

### Local end-to-end test (no real cluster needed)

```bash
bash deploy/scripts/deploy-k3s.sh      # installs k3s if missing, builds+imports the image, helm installs with relay.backend=memory
bash deploy/scripts/ci-k3s-e2e.sh      # kubectl rollout status + scripts/smoke.sh via port-forward
```

This is exactly what the `.github/workflows/k3s-e2e.yml` workflow runs in CI on PRs touching `deploy/**`. Useful env vars for `deploy-k3s.sh`: `NAMESPACE` (default `relay-pubsub`), `IMAGE_TAG`, `PULL_REGISTRY` (set to pull a published image instead of building locally), `SKIP_K3S_INSTALL=1` (if k3s is already installed).

### Deploying to a real cluster

```bash
helm upgrade --install relay-pubsub deploy/helm/relay-pubsub \
  -n relay-pubsub --create-namespace \
  --set image.tag=<released-tag>
  # relay.backend defaults to "http" — create the relay-pubsub-secrets/relay-token
  # Secret first, or override --set relay.backend=memory / relay.baseUrl=...
```

or apply the static manifest directly: `kubectl apply -f deploy/k8s/gateway.yaml` (after creating the `relay-pubsub-secrets` Secret it expects).

### Verify

```bash
kubectl -n relay-pubsub rollout status deployment/relay-pubsub --timeout=180s
kubectl -n relay-pubsub port-forward svc/relay-pubsub 8080:8080 &
BASE=https://127.0.0.1:8080 bash scripts/smoke.sh
```

---

## Troubleshooting

**Every request returns 401, even with `RELAY_PUBSUB_AUTH_TOKEN` "unset".** Fixed as of this deployment tooling landing (`src/main.rs`) — previously, an `EnvironmentFile`/`.env` line like `RELAY_PUBSUB_AUTH_TOKEN=` (present but empty) was parsed by `clap` as `Some("")`, a real-but-empty required token that no client could ever satisfy, rather than as unset. `main()` now filters both `RELAY_AUTH_TOKEN` and `RELAY_PUBSUB_AUTH_TOKEN` to `None` when empty. If you still see this on an older binary, either upgrade or remove the line from the env file entirely (a variable absent from the file behaves correctly on all versions).

**Client gets a TLS/cert error, or a plaintext client (e.g. `PUBSUB_EMULATOR_HOST=...` against Google's official SDKs) can't connect at all.** Both listeners are TLS-only and self-signed by default. Clients must either skip verification (`curl -k`, `grpcurl -insecure`, gRPC channel credentials built with `InsecureSkipVerify`/a custom trust root) or be pointed at a CA-signed cert via `PUBSUB_TLS_CERT`/`PUBSUB_TLS_KEY`. `PUBSUB_EMULATOR_HOST` specifically forces Google's client SDKs onto a plaintext channel, so it cannot reach this gateway at all — see the [TLS section in the README](../README.md#tls).

**`Address already in use` / crash-looping systemd unit.** The target host already has something bound to `:8080` or `:50051` (common on shared boxes). See [Port already in use](#port-already-in-use-on-the-target-host) above — check current listeners with `sudo ss -ltnp` before picking replacement ports.

## Known limitation: `/healthz`/`/readyz` don't check the backend

Both endpoints (`src/rest.rs`) return `{"status":"ok"}` unconditionally — they prove the process is alive, not that `RELAY_BACKEND=http` can actually reach the configured Relay service. A Kubernetes rollout or systemd unit can report "healthy" while the `http` backend is completely unreachable. This is why the k3s test above deliberately uses `relay.backend=memory` (self-contained) rather than trying to validate against a real Relay — and why both verification paths end with `scripts/smoke.sh`, a real publish/pull call, rather than trusting the health endpoints alone. Fixing this (a `RelayBackend::ping()` wired into `/readyz`) is tracked as a follow-up, not part of this deployment tooling.

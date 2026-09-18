# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: Apache-2.0
.PHONY: run test fmt fmt-check check build ui compose smoke conformance selftest \
	deploy-remote deploy-remote-quick deploy-remote-preflight deploy-remote-verify deploy-remote-uninstall deploy-remote-fleet \
	help ci

run: ## Run with the in-memory backend
	cargo run -- --backend memory

test: ## All tests
	cargo test --all

fmt: ## Format Rust sources
	cargo fmt --all

fmt-check: ## Fail if rustfmt would change sources
	cargo fmt --all -- --check

check: ## Clippy, all features, warnings denied
	cargo clippy --all-targets --all-features -- -D warnings

build: ## Release binary
	cargo build --release

ci: fmt-check check test build ## Local gate: rustfmt, clippy, tests, release build

help: ## Show targets
	@grep -E '^[a-zA-Z0-9_-]+:.*## ' $(MAKEFILE_LIST) | sort | awk -F':.*## ' '{printf "  \033[36m%-24s\033[0m %s\n", $$1, $$2}'

ui:
	cd ui && npm install && npm run dev

compose:
	docker compose up --build

smoke:
	bash scripts/smoke.sh

conformance:
	bash scripts/conformance-smoke.sh

selftest:
	bash scripts/selftest.sh

deploy-remote: ## Deploy: make deploy-remote H=<host> U=<user>
	@test -n "$(H)" || { echo "H is required (host)"; exit 1; }
	@test -n "$(U)" || { echo "U is required (user)"; exit 1; }
	bash scripts/deploy-remote.sh "$(H)" "$(U)" $(ARGS)

deploy-remote-quick:
	@test -n "$(H)" || { echo "H is required (host)"; exit 1; }
	@test -n "$(U)" || { echo "U is required (user)"; exit 1; }
	bash scripts/deploy-remote.sh "$(H)" "$(U)" --quick --build-local $(ARGS)

deploy-remote-preflight:
	@test -n "$(H)" || { echo "H is required (host)"; exit 1; }
	@test -n "$(U)" || { echo "U is required (user)"; exit 1; }
	bash scripts/deploy-remote.sh "$(H)" "$(U)" --preflight-only

deploy-remote-verify:
	@test -n "$(H)" || { echo "H is required (host)"; exit 1; }
	@test -n "$(U)" || { echo "U is required (user)"; exit 1; }
	bash scripts/deploy-remote.sh "$(H)" "$(U)" --verify-only

deploy-remote-uninstall:
	@test -n "$(H)" || { echo "H is required (host)"; exit 1; }
	@test -n "$(U)" || { echo "U is required (user)"; exit 1; }
	bash scripts/deploy-remote.sh "$(H)" "$(U)" --uninstall

deploy-remote-fleet:
	@test -n "$(FILE)" || { echo "FILE is required (hosts file)"; exit 1; }
	bash scripts/deploy-remote.sh --fleet "$(FILE)"

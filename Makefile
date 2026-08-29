.PHONY: run test fmt check ui compose smoke conformance selftest \
	deploy-remote deploy-remote-quick deploy-remote-preflight deploy-remote-verify deploy-remote-uninstall deploy-remote-fleet

run:
	cargo run -- --backend memory

test:
	cargo test --all

fmt:
	cargo fmt --all

check:
	cargo clippy --all-targets --all-features -- -D warnings

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

deploy-remote:
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

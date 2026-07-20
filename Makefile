# magnetic-time — common tasks. Full flag reference: CLAUDE.md / `make run ARGS=--help`.
# rustfmt/clippy are deliberate targets only; the repo never auto-formats.

BIN := target/release/magnetic-time
ARGS ?=

.PHONY: help run build check check-wasm web grad-check dump clean fmt clippy

.DEFAULT_GOAL := help

help: ## list targets
	@grep -hE '^[a-z][a-z-]*:.*##' $(MAKEFILE_LIST) | sed -E 's/:.*## /\t/' | sort

run: ## interactive clock (pass flags via ARGS="--face tide --fps")
	cargo run --release -- $(ARGS)

build: ## build the release binary
	cargo build --release

check: ## fast type-check (native)
	cargo check

check-wasm: ## type-check the browser build (must stay green)
	cargo check --target wasm32-unknown-unknown

web: ## build the wasm web component into docs/app/pkg
	./scripts/build-web.sh

grad-check: build ## verify the analytic field gradient, then exit
	$(BIN) --grad-check $(ARGS)

dump: build ## headless sample render to docs/debug/out.png
	@mkdir -p docs/debug
	$(BIN) --headless --time 10:08:30 --sim-seconds 60 --size 800 --dump docs/debug/out.png

clean: ## remove build artifacts
	cargo clean

fmt: ## rustfmt (run deliberately)
	cargo fmt

clippy: ## clippy lints (run deliberately)
	cargo clippy --release

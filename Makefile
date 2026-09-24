.DEFAULT_GOAL := help

.PHONY: help
help:
	@grep -E '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}'

# -- variables ------------------------------------------------------------------------------------

# The build target defaults to rust's host.
BUILD_TARGET ?= $(shell rustc -vV | grep host | awk '{print $$2}')
WARNINGS=RUSTDOCFLAGS="-D warnings"

# -- building -------------------------------------------------------------------------------------

.PHONY: build
build: ## By default we should build in release mode
	cargo build --release

.PHONY: build-faucet-client
build-faucet-client: ## Build triple-specific miden-faucet-client binary
	cargo build --package miden-faucet-client --target $(BUILD_TARGET) --release

# -- linting --------------------------------------------------------------------------------------

.PHONY: clippy
clippy: ## Runs Clippy with configs
	cargo clippy --locked --all-targets --all-features --workspace -- -D warnings


.PHONY: fix
fix: ## Runs Fix with configs
	cargo fix --allow-staged --allow-dirty --all-targets --all-features --workspace

.PHONY: format
format: ## Runs Format using nightly toolchain
	cargo +nightly fmt --all


.PHONY: format-check
format-check: ## Runs Format using nightly toolchain but only in check mode
	cargo +nightly fmt --all --check


.PHONY: machete
machete: ## Runs machete to find unused dependencies
	cargo machete


.PHONY: toml
toml: ## Runs Format for all TOML files
	taplo fmt


.PHONY: toml-check
toml-check: ## Runs Format for all TOML files but only in check mode
	taplo fmt --check --verbose

.PHONY: typos-check
typos-check: ## Runs spellchecker
	typos

.PHONY: lint
lint: typos-check format fix clippy toml machete ## Runs all linting tasks at once (Clippy, fixing, formatting, machete)

# --- docs ----------------------------------------------------------------------------------------

.PHONY: doc
doc: ## Generates & checks documentation
	$(WARNINGS) cargo doc --all-features --keep-going --release --locked

.PHONY: book
book: ## Builds the book & serves documentation site
	mdbook serve --open docs

# --- testing -------------------------------------------------------------------------------------

.PHONY: test
test:  ## Runs all tests
	cargo nextest run --release --all-features --workspace

# The end-to-end test runs the faucet against a real node and funding service, taken from the node
# repo's compose stack. The version follows the `miden-node-proto-build` pin, so bumping that
# dependency moves the whole network with it and nothing here needs editing.
NODE_VERSION = $(shell sed -n 's/^miden-node-proto-build *= *{ *version *= *"=\{0,1\}\([^"]*\)".*/\1/p' bin/faucet/Cargo.toml)
NODE_CHECKOUT = target/e2e/node-$(NODE_VERSION)
NODE_REGISTRY = ghcr.io/0xmiden
E2E_IMAGES = MIDEN_NODE_IMAGE=$(NODE_REGISTRY)/miden-node:v$(NODE_VERSION) \
             MIDEN_VALIDATOR_IMAGE=$(NODE_REGISTRY)/miden-validator:v$(NODE_VERSION) \
             MIDEN_NTX_BUILDER_IMAGE=$(NODE_REGISTRY)/miden-ntx-builder:v$(NODE_VERSION) \
             MIDEN_REMOTE_PROVER_IMAGE=$(NODE_REGISTRY)/miden-remote-prover:v$(NODE_VERSION) \
             MIDEN_FUNDING_SERVICE_IMAGE=$(NODE_REGISTRY)/miden-funding-service:v$(NODE_VERSION) \
             MIDEN_USDCX_GENESIS_IMAGE=$(NODE_REGISTRY)/miden-usdcx-genesis:v$(NODE_VERSION)
E2E_COMPOSE = $(E2E_IMAGES) docker compose --project-name miden-faucet-e2e -f $(NODE_CHECKOUT)/docker-compose.yml -f docker-compose.e2e.yml

$(NODE_CHECKOUT):
	git clone --depth 1 --branch v$(NODE_VERSION) https://github.com/0xMiden/node $(NODE_CHECKOUT)

.PHONY: e2e-network-up
e2e-network-up: $(NODE_CHECKOUT) ## Starts the node and funding service the end-to-end test needs
	$(E2E_COMPOSE) up --detach funding-service

.PHONY: e2e-network-down
e2e-network-down: ## Stops the end-to-end network and deletes its data
	$(E2E_COMPOSE) down --volumes --remove-orphans

.PHONY: e2e-network-logs
e2e-network-logs: ## Prints the logs of the end-to-end network
	$(E2E_COMPOSE) logs --no-color

.PHONY: test-e2e
test-e2e: e2e-network-up ## Runs the end-to-end test against a real node and funding service
	cargo test --locked -p miden-faucet --test e2e -- --ignored --nocapture; \
	    status=$$?; $(MAKE) e2e-network-down; exit $$status

# --- checking ------------------------------------------------------------------------------------

.PHONY: check
check: ## Check all targets and features for errors without code generation
	cargo check --all-features --all-targets --locked --workspace

# --- installing ----------------------------------------------------------------------------------

.PHONY: install-faucet
install-faucet: ## Installs faucet
	cargo install --path bin/faucet --locked
	cargo install --path bin/faucet-client --locked

.PHONY: check-tools
check-tools: ## Checks if development tools are installed
	@echo "Checking development tools..."
	@command -v mdbook >/dev/null 2>&1 && echo "[OK] mdbook is installed" || echo "[MISSING] mdbook is not installed (run: make install-tools)"
	@command -v typos >/dev/null 2>&1 && echo "[OK] typos is installed" || echo "[MISSING] typos is not installed (run: make install-tools)"
	@command -v cargo nextest >/dev/null 2>&1 && echo "[OK] cargo-nextest is installed" || echo "[MISSING] cargo-nextest is not installed (run: make install-tools)"
	@command -v taplo >/dev/null 2>&1 && echo "[OK] taplo is installed" || echo "[MISSING] taplo is not installed (run: make install-tools)"

.PHONY: install-tools
install-tools: ## Installs development tools required by the Makefile (mdbook, typos, nextest, taplo)
	@echo "Installing development tools..."
	cargo install mdbook --locked
	cargo install typos-cli --locked
	cargo install cargo-nextest --locked
	cargo install taplo-cli --locked
	@echo "Development tools installation complete!"

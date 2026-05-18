.DEFAULT_GOAL := help

.PHONY: help
help:
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}'

# -- variables ------------------------------------------------------------------------------------

WARNINGS=RUSTDOCFLAGS="-D warnings"

# -- linting --------------------------------------------------------------------------------------

.PHONY: clippy
clippy: ## Runs Clippy with configs
	cargo clippy --locked --all-targets --all-features

.PHONY: fix
fix: ## Runs Fix with configs
	cargo fix --allow-staged --allow-dirty --all-targets --all-features

.PHONY: build
build: ## By default we should build in release mode
	cargo build --release 

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
test: ## Runs all tests
	cargo nextest run --release --all-features

# --- checking ------------------------------------------------------------------------------------

.PHONY: check
check: ## Check all targets and features for errors without code generation
	cargo check --all-features --all-targets --locked

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

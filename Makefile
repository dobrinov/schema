# schema build helpers
CARGO_BIN := $(HOME)/.cargo/bin
# Prefer the rustup toolchain (has the wasm32 target) over e.g. Homebrew's rustc.
RUSTUP_BIN := $(shell rustup which rustc 2>/dev/null | xargs dirname 2>/dev/null)
WASM_ENV := PATH="$(RUSTUP_BIN):$(CARGO_BIN):$$PATH" DYLD_FALLBACK_LIBRARY_PATH="$(RUSTUP_BIN)/../lib" CARGO_TARGET_DIR="$(CURDIR)/target/wasm"

.PHONY: all wasm build release install test site dev clean

all: build

wasm:
	$(WASM_ENV) wasm-pack build crates/wasm --release --target no-modules --no-typescript --no-pack --out-dir ../../web/pkg

build: wasm
	SCHEMA_SKIP_WASM=1 cargo build

release: wasm
	SCHEMA_SKIP_WASM=1 cargo build --release

install: wasm
	SCHEMA_SKIP_WASM=1 cargo install --path crates/cli --force

test:
	cargo test --workspace

# Static GitHub Pages site in ./docs (landing page, playground, examples)
site: release
	./scripts/build-site.sh

# Serve the frontend from disk while hacking on web/ (no rebuild needed)
dev: build
	SCHEMA_WEB_DIR=web ./target/debug/schema examples/structure.sql

clean:
	cargo clean
	rm -rf web/pkg

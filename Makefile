# ns2bench: common tasks. `make` lists them.
# Variables: BOARD=<ssh host> BOARD_TARGET=<rust target> PORT=<ui port>

BOARD        ?=
BOARD_TARGET ?= armv7-unknown-linux-musleabihf
CARGO        := cd native && cargo   # run inside native/ so native/.cargo/config.toml (static board linking) applies
BOARD_BIN    := native/target/$(BOARD_TARGET)/release/ns2bench-board
NATIVE_DEV   := native/target/debug/ns2bench
NATIVE_REL   := native/target/release/ns2bench

.DEFAULT_GOAL := help
.PHONY: help build dev serve serve-dev test test-rust check board board-install netd \
        need-board board-status board-logs board-mode-standard board-mode-pro pico app app-dev clean \
        site release-post i18n

help: ## list targets
	@awk 'BEGIN {FS = ":.*## "} /^[a-zA-Z_-]+:.*## / {printf "  \033[1m%-20s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

# ------------------------------------------------------------------ host (the computer)
build: ## build the native engine (release) -> native/target/release/ns2bench
	$(CARGO) build --release -p ns2bench

dev: ## build the native engine (debug)
	$(CARGO) build -p ns2bench

serve: build ## run the native engine (UI at http://127.0.0.1:7630); reads ./bench.json
	$(NATIVE_REL) serve

serve-dev: dev ## run the debug build
	$(NATIVE_DEV) serve

app: ## build the desktop app (Tauri) -> native/target/release/bundle/ (needs: cargo install tauri-cli --version '^2')
	cd native/crates/app && cargo tauri build

app-dev: ## run the desktop app from source (debug)
	$(CARGO) run -p ns2bench-app

netd: ## build the offline DHCP/DNS/connectivity daemon (macOS/Linux)
	$(CARGO) build --release -p ns2bench-netd

# ------------------------------------------------------------------ checks
test: test-rust ## run every Rust test

test-rust: ## Rust tests (engine, board, network daemon and tools)
	$(CARGO) test --workspace

check: ## clippy over the Rust workspace and the board build for $(BOARD_TARGET)
	$(CARGO) clippy -q --workspace
	$(CARGO) check -q -p ns2bench-board --target $(BOARD_TARGET)

# ------------------------------------------------------------------ input board
board: ## cross-build the static NS2Bench Bridge daemon for $(BOARD_TARGET)
	@rustup target list --installed | grep -qx '$(BOARD_TARGET)' || rustup target add $(BOARD_TARGET)
	$(CARGO) build --release -p ns2bench-board --target $(BOARD_TARGET)

need-board:
	@[ -n "$(BOARD)" ] || { echo "set BOARD=<ssh host of the board>, e.g. make $(MAKECMDGOALS) BOARD=myboard"; exit 2; }

board-install: need-board board ## install/update the NS2Bench Bridge daemon + scripts on BOARD over SSH (keeps *.bak)
	sh board/install.sh $(BOARD) $(BOARD_BIN)

board-status: need-board ## daemon, gadget and USB state on BOARD
	ssh $(BOARD) '/opt/ns2bench/ns2bench-hidd.sh status; /opt/ns2bench/gadget.sh status'

board-logs: need-board ## last daemon / gadget / Pro Controller log lines on BOARD
	ssh $(BOARD) 'tail -n 20 /tmp/ns2bench-hidd.log /tmp/ns2bench-gadget.log /tmp/ns2bench-pro2.log 2>/dev/null'

board-mode-standard: ## switch the console-facing USB device to HORI pad + keyboard + mouse
	curl -fsS -XPOST http://127.0.0.1:7630/api/control -H 'content-type: application/json' -d '{"usb_mode":"standard"}' >/dev/null && echo switching

board-mode-pro: ## switch the console-facing USB device to a Switch 2 Pro Controller
	curl -fsS -XPOST http://127.0.0.1:7630/api/control -H 'content-type: application/json' -d '{"usb_mode":"pro"}' >/dev/null && echo switching

# ------------------------------------------------------------------ Raspberry Pi Pico input board
PICO_ELF := firmware/pico/target/thumbv6m-none-eabi/release/ns2bench-pico
pico: ## build the NS2Bench Bridge firmware for Pico -> firmware/pico/ns2bench-pico.uf2 (NS2BENCH_TOKEN=..., NS2BENCH_BAUD=... optional)
	@rustup target list --installed | grep -qx thumbv6m-none-eabi || rustup target add thumbv6m-none-eabi
	cd firmware/pico && cargo build --release
	$(CARGO) run --release -p ns2bench-tools -- elf2uf2 ../$(PICO_ELF) ../firmware/pico/ns2bench-pico.uf2

clean: ## remove Rust build output
	$(CARGO) clean
	cd firmware/pico && cargo clean

# ------------------------------------------------------------------ website, translations, releases
site: ## build the website into _site/ (needs: npm i --no-save marked@15.0.12)
	node scripts/site-build.mjs

i18n: ## refresh web/i18n/en.json from the UI and check every language
	node scripts/i18n.mjs && node scripts/i18n.mjs --check

release-post: ## draft X / Discord / Reddit posts for VERSION from CHANGELOG.md -> .release_posts/
	node scripts/release-posts.mjs


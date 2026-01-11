.PHONY: setup build run check

setup:
	@chmod +x scripts/install_dependencies.sh
	./scripts/install_dependencies.sh

build:
	cargo build --release

run:
	cargo run

check:
	cargo check

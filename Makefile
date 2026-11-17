.PHONY: setup build run check test package install uninstall release-patch release-minor release-major

setup:
	@chmod +x scripts/install_dependencies.sh
	./scripts/install_dependencies.sh

build:
	cargo build --release

run:
	cargo run

check:
	cargo check

test:
	cargo test --workspace

# dist/aurivon-<version>-<arch>-linux.tar.gz + .sha256
package:
	./scripts/package.sh

# per-user install into ~/.local (app launcher entry, icons, launcher wrapper, keys)
install:
	./scripts/install.sh

uninstall:
	./scripts/uninstall.sh

# bump version, changelog, tag, build, publish a GitHub release
release-patch:
	./scripts/release.sh patch
release-minor:
	./scripts/release.sh minor
release-major:
	./scripts/release.sh major

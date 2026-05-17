BOARD ?= s19-xil
MODEL ?= s19j-pro
VERSION ?= 0.1.0
DIST_DIR ?= dist
MEDIA ?= all
SD_MOUNT ?=

.PHONY: bootstrap fmt clippy test release size-budget artifact-budget run-control-plane image install-preflight verify-repro sd-test-package sd-test-verify sd-test-extract first-boot-collect clean

bootstrap:
	cargo fetch

fmt:
	cargo fmt --all

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace

release:
	cargo build --workspace --release

size-budget: release
	./scripts/check-size-budget.sh

artifact-budget:
	./scripts/check-artifact-budget.sh "$(VERSION)" "$(DIST_DIR)" "$(MODEL)"

run-control-plane:
	cargo run -p openmineros-control-plane -- --board $(BOARD) --model $(MODEL)

image:
	./scripts/build-image.sh "$(BOARD)" "$(MODEL)" "$(VERSION)" "$(DIST_DIR)" "$(MEDIA)"

install-preflight:
	./scripts/install-preflight.sh "$(BOARD)" "$(MODEL)" "$(VERSION)" "$(DIST_DIR)" "$(MEDIA)"

verify-repro:
	./scripts/verify-repro.sh "$(BOARD)" "$(MODEL)" "$(VERSION)" "$(MEDIA)"

sd-test-package:
	./scripts/prepare-sd-test.sh "$(BOARD)" "$(MODEL)" "$(VERSION)" "$(DIST_DIR)"

sd-test-verify:
	./scripts/verify-sd-test-bundle.sh "$(BOARD)" "$(MODEL)" "$(VERSION)" "$(DIST_DIR)"

sd-test-extract:
	./scripts/extract-sd-test-rootfs.sh "$(DIST_DIR)/openmineros-$(BOARD)-$(MODEL)-$(VERSION)-sd-card.img.xz" "$(SD_MOUNT)"

first-boot-collect:
	./scripts/collect-s19-first-boot.sh

clean:
	cargo clean
	rm -rf "$(DIST_DIR)"

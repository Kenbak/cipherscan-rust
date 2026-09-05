.PHONY: verify-fast verify-full

verify-fast:
	cargo fmt --all -- --check
	cargo clippy --release --all-targets -- -D warnings

verify-full: verify-fast
	bash scripts/verify-ci.sh

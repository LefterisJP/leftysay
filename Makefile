lint:
	cargo fmt -- --check
	cargo clippy -- -D warnings

test:
	cargo test

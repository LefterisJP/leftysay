lint:
	cargo fmt -- --check
	cargo clippy -- -D warnings

test:
	cargo test

PREFIX ?= /usr/local

install:
	cargo build --release
	install -d "$(DESTDIR)$(PREFIX)/bin"
	install -m 755 target/release/leftysay "$(DESTDIR)$(PREFIX)/bin/leftysay"
	install -d "$(DESTDIR)$(PREFIX)/share/leftysay/packs"
	cp -R packs/* "$(DESTDIR)$(PREFIX)/share/leftysay/packs/"

uninstall:
	rm -f "$(DESTDIR)$(PREFIX)/bin/leftysay"
	rm -rf "$(DESTDIR)$(PREFIX)/share/leftysay"

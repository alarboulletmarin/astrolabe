# Single install recipe, used by the PKGBUILD and by `sudo make install` on any distro.
PREFIX ?= /usr/local
DESTDIR ?=

all:
	cargo build --release --locked

install:
	install -Dm755 target/release/astrolabe $(DESTDIR)$(PREFIX)/bin/astrolabe
	install -Dm644 dev.andrea.Astrolabe.desktop $(DESTDIR)$(PREFIX)/share/applications/dev.andrea.Astrolabe.desktop
	install -Dm644 dev.andrea.Astrolabe.svg $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/dev.andrea.Astrolabe.svg
	install -Dm644 LICENSE $(DESTDIR)$(PREFIX)/share/licenses/astrolabe/LICENSE
	install -Dm644 assets/fonts/LICENSE-IBM-Plex.txt $(DESTDIR)$(PREFIX)/share/licenses/astrolabe/LICENSE-IBM-Plex.txt

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/astrolabe \
	  $(DESTDIR)$(PREFIX)/share/applications/dev.andrea.Astrolabe.desktop \
	  $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/dev.andrea.Astrolabe.svg
	rm -rf $(DESTDIR)$(PREFIX)/share/licenses/astrolabe

test:
	cargo test --locked

.PHONY: all install uninstall test

.PHONY: install-backend install-backend-systemd install-backend-k3s install-macos-app

# Convenience wrappers around scripts/ for people who've already cloned
# the repo — the curl one-liners in README.md calling the same scripts
# directly are the "from nothing" entry point; this isn't a second
# implementation.

install-backend:
	bash scripts/install-backend.sh

install-backend-systemd:
	SWITCHBOT_HOME_TARGET=systemd bash scripts/install-backend.sh

install-backend-k3s:
	SWITCHBOT_HOME_TARGET=k3s bash scripts/install-backend.sh

install-macos-app:
	bash scripts/install-macos-app.sh

#!/usr/bin/env bash
# Installs and starts the switchbot-home backend, either as a systemd
# service or as a k3s deployment. Builds from source — see
# docs/specs/installation-and-deployment.md for why (avoids a release
# pipeline).
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/alehrs/switchbot-home/main/scripts/install-backend.sh | bash
#
# Non-interactive use (CI, scripts): set SWITCHBOT_HOME_TARGET to
# "systemd" or "k3s" beforehand.
set -euo pipefail

REPO_URL="${SWITCHBOT_HOME_REPO_URL:-https://github.com/alehrs/switchbot-home.git}"
WORKDIR="${SWITCHBOT_HOME_WORKDIR:-$HOME/.local/share/switchbot-home}"
SRC_DIR="$WORKDIR/src"

log() { printf '==> %s\n' "$1"; }
die() {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

require_linux() {
    if [ "$(uname -s)" != "Linux" ]; then
        die "this installer targets Linux only (systemd/k3s). For macOS development, run the backend directly with 'cargo run -p backend' — see README.md."
    fi
}

choose_target() {
    if [ -n "${SWITCHBOT_HOME_TARGET:-}" ]; then
        TARGET="$SWITCHBOT_HOME_TARGET"
    elif [ -r /dev/tty ]; then
        # Running interactively, possibly piped from curl (where stdin is
        # the script itself, not the terminal) — read the prompt from the
        # controlling tty directly instead.
        read -rp "Deploy target - 'systemd' or 'k3s'? " TARGET < /dev/tty
    else
        die "set SWITCHBOT_HOME_TARGET=systemd or SWITCHBOT_HOME_TARGET=k3s and re-run (no terminal available to prompt)."
    fi

    case "$TARGET" in
        systemd|k3s) ;;
        *) die "unknown target '$TARGET' — expected 'systemd' or 'k3s'." ;;
    esac
}

detect_package_manager() {
    if command -v apt-get >/dev/null 2>&1; then
        echo apt
    elif command -v dnf >/dev/null 2>&1; then
        echo dnf
    elif command -v pacman >/dev/null 2>&1; then
        echo pacman
    else
        echo unknown
    fi
}

ensure_rust() {
    if command -v cargo >/dev/null 2>&1; then
        return
    fi
    log "Rust not found — installing via rustup"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
    # shellcheck source=/dev/null
    . "$HOME/.cargo/env"
}

ensure_c_toolchain() {
    if command -v cc >/dev/null 2>&1 || command -v gcc >/dev/null 2>&1; then
        return
    fi
    log "No C compiler found (needed to build the bundled SQLite library) — installing one"
    case "$(detect_package_manager)" in
        apt) sudo apt-get update && sudo apt-get install -y build-essential ;;
        dnf) sudo dnf install -y gcc ;;
        pacman) sudo pacman -Sy --noconfirm base-devel ;;
        *) die "couldn't detect a supported package manager (apt/dnf/pacman) — install a C compiler yourself and re-run." ;;
    esac
}

ensure_docker_and_kubectl() {
    command -v docker >/dev/null 2>&1 || die "docker is required for the k3s target — install it first (this script won't do it for you: too invasive/distro-specific)."
    command -v kubectl >/dev/null 2>&1 || die "kubectl is required for the k3s target — install it first."
    kubectl cluster-info >/dev/null 2>&1 || die "kubectl can't reach a cluster — set up k3s and your kubeconfig first (this script assumes a cluster already exists)."
}

clone_or_update_repo() {
    mkdir -p "$WORKDIR"
    if [ -d "$SRC_DIR/.git" ]; then
        log "Updating existing checkout at $SRC_DIR"
        git -C "$SRC_DIR" fetch --depth 1 origin main
        git -C "$SRC_DIR" reset --hard origin/main
    else
        log "Cloning $REPO_URL into $SRC_DIR"
        git clone --depth 1 "$REPO_URL" "$SRC_DIR"
    fi
}

install_systemd() {
    ensure_rust
    ensure_c_toolchain

    log "Building the backend (this compiles from source, can take a few minutes)"
    (cd "$SRC_DIR" && cargo build --release -p backend)

    log "Installing the binary to /usr/local/bin"
    sudo install -m 755 "$SRC_DIR/target/release/backend" /usr/local/bin/switchbot-home-backend

    log "Granting BLE access without running as root (setcap)"
    sudo setcap 'cap_net_raw,cap_net_admin+eip' /usr/local/bin/switchbot-home-backend

    if ! id -u switchbot-home >/dev/null 2>&1; then
        log "Creating the switchbot-home system user"
        sudo useradd --system --no-create-home --shell /usr/sbin/nologin switchbot-home
    fi

    sudo mkdir -p /var/lib/switchbot-home
    sudo chown switchbot-home:switchbot-home /var/lib/switchbot-home

    sudo mkdir -p /etc/switchbot-home
    if [ ! -f /etc/switchbot-home/backend.env ]; then
        log "Writing a starter env file at /etc/switchbot-home/backend.env"
        sudo tee /etc/switchbot-home/backend.env >/dev/null <<'EOF'
DATABASE_URL=sqlite:///var/lib/switchbot-home/switchbot-home.sqlite
BIND_ADDRESS=0.0.0.0:3000
# READING_INTERVAL_SECONDS=30
# RETENTION_DAYS=300
EOF
    else
        log "/etc/switchbot-home/backend.env already exists, leaving it alone"
    fi

    log "Installing the systemd unit"
    sudo install -m 644 "$SRC_DIR/deploy/systemd/switchbot-home-backend.service" \
        /etc/systemd/system/switchbot-home-backend.service
    sudo systemctl daemon-reload
    sudo systemctl enable --now switchbot-home-backend

    log "Done. Check status with: systemctl status switchbot-home-backend"
    log "Logs: journalctl -u switchbot-home-backend -f"
    log "Config: edit /etc/switchbot-home/backend.env, then 'systemctl restart switchbot-home-backend'"
}

install_k3s() {
    ensure_docker_and_kubectl

    log "Building the backend container image"
    docker build -f "$SRC_DIR/deploy/docker/Dockerfile" -t switchbot-home-backend:local "$SRC_DIR"

    log "Importing the image into k3s's containerd (no registry needed)"
    docker save switchbot-home-backend:local | sudo k3s ctr images import -

    log "Applying Kubernetes manifests"
    kubectl apply -f "$SRC_DIR/deploy/k3s/"

    log "Waiting for the rollout to finish"
    kubectl rollout status deployment/switchbot-home-backend -n switchbot-home --timeout=120s

    log "Done. Check status with: kubectl get pods -n switchbot-home"
    log "Logs: kubectl logs -n switchbot-home -l app=switchbot-home-backend -f"
    log "Config: edit deploy/k3s/10-configmap.yaml, 'kubectl apply -f deploy/k3s/', then 'kubectl rollout restart deployment/switchbot-home-backend -n switchbot-home'"
}

main() {
    require_linux
    choose_target
    clone_or_update_repo

    case "$TARGET" in
        systemd) install_systemd ;;
        k3s) install_k3s ;;
    esac
}

main "$@"

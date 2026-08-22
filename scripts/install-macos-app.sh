#!/usr/bin/env bash
# Builds and installs the SwitchBotHome menu-bar app from source.
#
# Building locally (rather than downloading a prebuilt .app) sidesteps
# Gatekeeper entirely: a .app downloaded from the internet gets a
# com.apple.quarantine attribute and, without paid Developer ID signing +
# notarization, macOS refuses to open it. A locally-built .app never gets
# that attribute. See docs/specs/installation-and-deployment.md §2.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/alehrs/switchbot-home/main/scripts/install-macos-app.sh | bash
set -euo pipefail

REPO_URL="${SWITCHBOT_HOME_REPO_URL:-https://github.com/alehrs/switchbot-home.git}"
WORKDIR="${SWITCHBOT_HOME_WORKDIR:-$HOME/.local/share/switchbot-home}"
SRC_DIR="$WORKDIR/src"
APP_NAME="SwitchBotHome.app"

log() { printf '==> %s\n' "$1"; }
die() {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

require_macos() {
    if [ "$(uname -s)" != "Darwin" ]; then
        die "this installer targets macOS only."
    fi
}

ensure_xcode_clt() {
    if xcode-select -p >/dev/null 2>&1; then
        return
    fi
    log "Xcode Command Line Tools not found — triggering the installer"
    log "This opens a one-time GUI prompt on a fresh Mac; accept it, then re-run this script."
    xcode-select --install
    exit 1
}

ensure_homebrew() {
    if command -v brew >/dev/null 2>&1; then
        return
    fi
    log "Homebrew not found — installing"
    /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
}

ensure_xcodegen() {
    if command -v xcodegen >/dev/null 2>&1; then
        return
    fi
    log "XcodeGen not found — installing via Homebrew"
    brew install xcodegen
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

build_and_install() {
    local macos_dir="$SRC_DIR/macos-app"
    local build_dir="$macos_dir/build"

    log "Generating the Xcode project"
    (cd "$macos_dir" && xcodegen generate)

    log "Building (Release configuration — this can take a minute)"
    rm -rf "$build_dir"
    (cd "$macos_dir" && xcodebuild \
        -project SwitchBotHome.xcodeproj \
        -scheme SwitchBotHome \
        -configuration Release \
        -derivedDataPath "$build_dir" \
        CODE_SIGNING_ALLOWED=NO \
        build)

    local built_app="$build_dir/Build/Products/Release/$APP_NAME"
    [ -d "$built_app" ] || die "build succeeded but $built_app wasn't found — something's off in the build layout."

    log "Installing to /Applications"
    rm -rf "/Applications/$APP_NAME"
    cp -R "$built_app" /Applications/

    rm -rf "$build_dir"
}

main() {
    require_macos
    ensure_xcode_clt
    ensure_homebrew
    ensure_xcodegen
    clone_or_update_repo
    build_and_install

    log "Installed. Launching it now — look for a thermometer icon in the menu bar."
    log "No Dock icon by design; use the 'Quit' button inside its popover to close it."
    log "If your backend isn't on http://localhost:3000, open Settings... from the popover to point it elsewhere."
    open -a "/Applications/$APP_NAME"
}

main "$@"

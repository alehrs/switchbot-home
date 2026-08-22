# Build context is the repo root (docker build -f deploy/docker/Dockerfile .),
# not this directory — the workspace Cargo.toml/Cargo.lock live at the root.
FROM rust:1-slim AS builder
WORKDIR /app
# libdbus-1-dev: contradicts what docs/specs/installation-and-deployment.md
# guessed before this was actually tried — bluer's libdbus-sys build
# script does hard-require the real system libdbus via pkg-config, it's
# not pure-Rust zbus-only. Confirmed by an actual failed build, not
# assumed: without this the build fails at
# "Package dbus-1 was not found in the pkg-config search path."
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libdbus-1-dev \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY backend ./backend
RUN cargo build --release -p backend

# debian:stable-slim (a floating tag, always the current stable release)
# rather than pinning a codename that will eventually go out of date.
FROM debian:stable-slim
# libdbus-1-3: the runtime half of the same dependency — the binary
# dynamically links libdbus, so the shared library (not just the -dev
# headers needed only at build time) has to be present here too.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libdbus-1-3 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/backend /usr/local/bin/switchbot-home-backend
ENTRYPOINT ["/usr/local/bin/switchbot-home-backend"]

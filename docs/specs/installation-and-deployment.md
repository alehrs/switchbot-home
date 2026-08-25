# Installation & deployment

Status: Implemented — scripts, systemd unit, Dockerfile (build-tested),
k3s manifests (syntax-checked, not yet applied to a real cluster), CI
workflow. k3s's BLE-in-container config is still unverified on real
cluster hardware (§7).
Last updated: 2026-08-21

## 1. Goal

One command installs and runs each piece of `switchbot-home`, for anyone
with access to the (public) repo:

```
curl -fsSL https://raw.githubusercontent.com/alehrs/switchbot-home/main/scripts/install-backend.sh | bash
curl -fsSL https://raw.githubusercontent.com/alehrs/switchbot-home/main/scripts/install-macos-app.sh | bash
```

The backend script supports two deployment targets, chosen at install
time: **systemd** (a native binary as a Linux service) or **k3s** (a
container in the user's own Kubernetes cluster). Separately, a **CI/CD
pipeline** auto-deploys the backend to the maintainer's own homelab via
k3s on every push to `main` — a different, automated path from the
one-liner, which stays for anyone else who wants to self-host their own
copy manually.

Everything **builds from source** on the target machine — no release
pipeline, no prebuilt binaries, no code-signing account. This was a
deliberate choice (see §2) over shipping prebuilt artifacts.

## 2. Distribution model and why

- **Repo is public** → a plain `curl` against `raw.githubusercontent.com`
  works for anyone, no authentication needed for the one-liners.
- **Build from source, not prebuilt binaries/apps**:
  - Backend: avoids standing up a cross-compilation/release pipeline just
    to hand out a binary. The install script installs Rust (via `rustup`)
    if missing, then `cargo build --release`s locally.
  - macOS app: avoids the Gatekeeper problem entirely. A `.app` downloaded
    from the internet gets a `com.apple.quarantine` extended attribute,
    and without a paid Apple Developer ID signing + notarization
    pipeline, macOS refuses to open it ("cannot be opened because the
    developer cannot be verified"). A `.app` **built locally** via
    `xcodebuild` never gets that attribute, so Gatekeeper never enters
    the picture — this is exactly the same "sign to run locally" trust
    Xcode already gives a local build today. Building from source sidesteps
    the entire signing/notarization problem rather than working around it.
- **Trade-off accepted**: install takes longer (a real compile, not a
  download) and needs build tooling present on the target machine (the
  scripts install what's missing: Rust for the backend, Homebrew +
  XcodeGen for the macOS app — but **Xcode Command Line Tools can't be
  silently automated**; see §5's open risk).

## 3. Repository layout additions

```
switchbot-home/
├── Makefile                      # convenience wrapper around scripts/, for people who already cloned
├── scripts/
│   ├── install-backend.sh        # the backend one-liner's target
│   └── install-macos-app.sh      # the macOS app one-liner's target
├── deploy/
│   ├── systemd/
│   │   └── switchbot-home-backend.service
│   ├── docker/
│   │   └── Dockerfile
│   └── k3s/
│       ├── 00-namespace.yaml   # numeric prefixes control `kubectl apply -f deploy/k3s/`'s
│       ├── 10-configmap.yaml   # apply order — it's alphabetical, and configmap/deployment
│       ├── 20-pvc.yaml         # would otherwise apply before the namespace exists
│       ├── 30-deployment.yaml
│       └── 40-service.yaml
└── .github/
    └── workflows/
        └── deploy-backend.yml     # CI/CD: push to main -> build + roll out on the homelab's k3s
```

The `Makefile` is a thin convenience layer (`make install-backend`,
`make install-backend-systemd`, `make install-backend-k3s`,
`make install-macos-app`) that just calls the same scripts under `scripts/`
— for someone who's already cloned the repo, not a second implementation
of the install logic. The curl one-liners remain the actual "from nothing"
entry point; the Makefile targets exist alongside them, not instead.

## 4. Backend install script (`scripts/install-backend.sh`)

1. **Refuse to run on anything but Linux**, with a clear error message.
   BLE on this backend relies on BlueZ via `btleplug`'s Linux backend;
   there's no systemd or k3s on macOS, and the macOS backend story is
   already covered separately (`cargo run -p backend` directly, for dev —
   see `README.md`). Don't pretend to support a platform combination that
   doesn't exist.
2. Ask which deployment target: **systemd** or **k3s** (interactive
   prompt by default; also accept a flag/env var, e.g.
   `SWITCHBOT_HOME_TARGET=k3s`, for non-interactive/scripted use).
3. Check prerequisites for the chosen target and install what's missing:
   - **systemd**: `rustc`/`cargo` (install via `rustup` if absent — the
     same installer used manually earlier in this project) and a C
     compiler (needed to build the bundled SQLite C library that `sqlx`'s
     `sqlite` feature already vendors — `build-essential` on
     Debian/Ubuntu, or the equivalent on the detected distro).
   - **k3s**: `docker` and `kubectl` on the machine running the installer.
     The script does **not** try to install k3s itself — that's a bigger
     commitment than installing this app, and assumes the user already
     has a cluster. Error out clearly with a link to k3s's own docs if
     `kubectl` can't reach a cluster.
4. Clone the repo (shallow, `--depth 1`) into a working directory, e.g.
   `~/.local/share/switchbot-home/src` (shared with the macOS app
   installer if both ever run on the same machine, to avoid cloning twice).
5. **systemd path**:
   - `cargo build --release -p backend`.
   - Copy the binary to `/usr/local/bin/switchbot-home-backend`.
   - `setcap 'cap_net_raw,cap_net_admin+eip' /usr/local/bin/switchbot-home-backend`
     so BLE scanning works **without running the service as root** — the
     standard non-root pattern for raw BLE/HCI access on Linux (needs
     verifying against this specific binary/btleplug's actual syscalls;
     see §7).
   - Create a dedicated system user (`useradd --system --no-create-home
     switchbot-home`) and a data directory `/var/lib/switchbot-home`
     (owned by that user) for the SQLite file.
   - Install `deploy/systemd/switchbot-home-backend.service` to
     `/etc/systemd/system/`, with `EnvironmentFile=/etc/switchbot-home/backend.env`
     (see §6 for why a separate env file, not inline `Environment=` lines).
   - Write a starter `/etc/switchbot-home/backend.env` with
     `DATABASE_URL=sqlite:///var/lib/switchbot-home/switchbot-home.sqlite`
     and `BIND_ADDRESS=0.0.0.0:3000`, commented-out lines for the optional
     `READING_INTERVAL_SECONDS`/`RETENTION_DAYS`.
   - `systemctl daemon-reload && systemctl enable --now switchbot-home-backend`.
6. **k3s path**:
   - `docker build -f deploy/docker/Dockerfile -t switchbot-home-backend:local .`
   - Import the image directly into k3s's own containerd — **no registry
     needed** for a manual single-node install:
     `docker save switchbot-home-backend:local | sudo k3s ctr images import -`.
   - `kubectl apply -f deploy/k3s/` (namespace, PVC, ConfigMap, Deployment,
     Service — see §6/§7 for the BLE-specific pod config).
7. Print a summary: where the service/pod is running, how to check its
   logs (`journalctl -u switchbot-home-backend` / `kubectl logs`), and
   where to edit env vars (§6).

## 5. macOS app install script (`scripts/install-macos-app.sh`)

1. **Refuse to run on anything but macOS**, clear error otherwise.
2. Check for Xcode Command Line Tools (`xcode-select -p`); if missing,
   trigger `xcode-select --install`. **This can't be fully automated** —
   it opens Apple's own GUI installer, which needs a manual click-through
   the very first time on a machine with no Xcode/CLT at all. Document
   this plainly rather than pretending the one-liner is 100% hands-off on
   a brand new Mac.
3. Check for Homebrew; install via the official one-liner if missing.
4. `brew install xcodegen` if missing.
5. Clone the repo (shallow) into the same shared working directory as §4.
6. `cd macos-app && xcodegen generate`.
7. `xcodebuild -project SwitchBotHome.xcodeproj -scheme SwitchBotHome -configuration Release -derivedDataPath build build`
   (Release, not the Debug config used for this project's own dev/test
   cycles; `CODE_SIGNING_ALLOWED=NO` to keep the build non-interactive —
   no keychain prompts — which is fine per §2's Gatekeeper reasoning).
8. Move the built `.app` from the DerivedData products path to
   `/Applications/SwitchBotHome.app` (replacing any existing copy).
9. `open -a SwitchBotHome` to launch it immediately, then print: where to
   find the menu bar icon, and to set the backend URL via the app's own
   Settings if it's not on `localhost:3000`.
10. Launch-at-login is **not** offered by the installer — it's explicitly
    out of scope in the app itself today (`docs/specs/macos-app.md` §11);
    nothing to wire up here until that's built.

## 6. Backend configuration after install

Both deployment targets keep configuration **external to the built
artifact**, so editing env vars never means rebuilding:

- **systemd**: edit `/etc/switchbot-home/backend.env`, then
  `systemctl restart switchbot-home-backend`.
- **k3s**: edit `deploy/k3s/10-configmap.yaml` (or `kubectl edit configmap
  switchbot-home-backend-config` directly), `kubectl apply` it again,
  then `kubectl rollout restart deployment/switchbot-home-backend` — a
  ConfigMap change alone doesn't get picked up by an already-running pod
  without a restart, and adding a tool like Reloader to auto-restart on
  ConfigMap changes isn't worth the extra moving part at this scale.

Same four backend env vars either way (`DATABASE_URL`, `BIND_ADDRESS`,
`READING_INTERVAL_SECONDS`, `RETENTION_DAYS` — see `README.md`). One
difference worth noting: the k3s ConfigMap's `BIND_ADDRESS` defaults to
`0.0.0.0:8090`, not `:3000` like the systemd env file — because
`hostNetwork: true` (§7) binds it directly on the node, and `:3000` is
already Grafana's port on the maintainer's own homelab node (confirmed by
`ss -tln` before the first real deploy). If your own k3s node already
uses `:8090` for something else, change it in the ConfigMap before
applying.

## 7. BLE from inside a container — the real risk in this design

Confirmed with the user before designing this: their k3s node has a
reachable Bluetooth adapter, and they're fine with a less "clean" pod
spec to make it work. On Linux, `btleplug` talks to BlueZ over D-Bus —
inside a container that means the pod needs:

- `hostNetwork: true` (BLE/HCI access doesn't work meaningfully through
  a network-namespaced container the normal way).
- The host's D-Bus system socket bind-mounted in. Confirmed by an actual
  error message from a real (unmounted) test run: the binary looks for it
  at `/run/dbus/system_bus_socket` specifically.
- Either `NET_ADMIN`/`NET_RAW` capabilities added in the container's
  `securityContext`, or, if that's not sufficient in practice,
  `privileged: true` as a fallback (worse, but simpler — decide based on
  what actually works during implementation, don't assume the least-
  privileged option works without trying it).

**The container build/runtime half of this is now verified; the actual
k3s/real-hardware half is not.** What's been confirmed on this machine
(Docker Desktop on macOS, `docker build`/`docker run` — not a real k3s
node, not real BLE hardware):

- The image (`deploy/docker/Dockerfile`) builds successfully and produces
  a working binary.
- Running it (no D-Bus socket mounted, as a plain `docker run` would
  have) starts the HTTP server fine and logs a clean, non-fatal error for
  the BLE side specifically: `Failed to connect to socket
  /run/dbus/system_bus_socket: No such file or directory` — confirming
  two things at once: the exact socket path to mount in the k3s manifest
  (§ above, already updated to match), and that a missing D-Bus socket
  degrades gracefully (HTTP still serves, matching the backend's existing
  "BLE failure doesn't take the API down" design) rather than crashing
  the whole process.
- `GET /devices` returned `[]` correctly against an in-memory database
  inside the container — the HTTP/SQLite stack works in this environment.

**Now actually deployed to the real k3s node, and this surfaced a new,
more fundamental gap than the D-Bus socket path.** The pod runs, the
D-Bus socket mounts correctly (no more "No such file or directory"), and
the HTTP API is reachable at the node's IP (`GET /devices` → `[]`, as
expected with no data yet) — but the BLE scanner itself failed with `The
name org.bluez was not provided by any .service files`. Investigated
directly on the host (not from inside the container): the Bluetooth
*hardware* is genuinely present and kernel-recognized (`lsusb` shows an
IMC Networks Bluetooth Radio, `/sys/class/bluetooth/hci0` exists) — but
**`bluez` (the userspace daemon that owns `org.bluez` on the system D-Bus
bus) isn't installed on the host at all** (`dpkg -l | grep bluez` returns
nothing, `systemctl status bluetooth` → "could not be found"). The
container's design (talk to the *host's* BlueZ over the mounted D-Bus
socket, rather than running its own contained `bluetoothd`) was correct
all along — it just assumed BlueZ was already running on any k3s node
used for this, which turned out not to hold on this specific host. Fix
(Debian/Ubuntu; needs root, so — same as the containerd-import root
requirement in §8 — this is done manually by the maintainer, not by
automation): `sudo apt-get install -y bluez && sudo systemctl enable
--now bluetooth`.

**With BlueZ installed, the software stack is now fully verified end to
end**: the pod's logs show `BLE scan started` with no error (the earlier
`org.bluez` error is gone), and `bluetoothctl show` confirms the adapter
is `Powered: yes` on the host. What's left is purely environmental, not a
bug: a raw 12-second `bluetoothctl scan on` on the host found **zero**
BLE devices of *any* kind, not just no SwitchBot sensor — meaning nothing
is currently broadcasting within range of the homelab's adapter at all.
The scanning pipeline (D-Bus socket → BlueZ → `btleplug` → the backend's
parser) has nothing left to blame; this is a physical range/placement
question (is a SwitchBot sensor, or a phone, or anything else BLE, ever
near wherever this homelab box physically sits?) that can only be settled
by bringing a sensor within range of it, not by anything checkable from
software.

The original "secondary unknown" about needing a system `libdbus` package
turned out to be **wrong, and is now corrected from a real build
failure, not left as an assumption**: `bluer`'s `libdbus-sys` build
script hard-requires the actual system `libdbus-1-dev` via `pkg-config`
at build time (`Package dbus-1 was not found in the pkg-config search
path` — it does not use a pure-Rust `zbus`-only path as guessed) and the
compiled binary dynamically links `libdbus-1-3` at runtime. Both are in
`deploy/docker/Dockerfile` now; without them the build fails outright
(confirmed) rather than just missing an optional feature.

## 8. CI/CD: auto-deploy to the maintainer's homelab

Separate from the manual one-liner — this is the maintainer's own
push-to-deploy automation, not something "anyone with repo access" gets.

- **Connectivity**: a **self-hosted GitHub Actions runner installed on
  the homelab** (registered against this repo). Labels come from the
  runner's own defaults — `self-hosted, Linux, X64` — matching the
  homelab's existing runners for other repos (`upvote-service`,
  `alessiocavallo.it`), neither of which registers a custom label like
  `homelab` either; `deploy-backend.yml`'s `runs-on:` matches that same
  triplet rather than inventing a new label no runner would ever have.
  The runner makes outbound-only connections to GitHub to pick up jobs —
  no inbound port needs to be opened on the home network, unlike a
  GitHub-hosted runner trying to reach in.
- Because the runner lives on the homelab already, it has direct
  `docker`/`kubectl` access to the local k3s cluster. **This does push
  through the homelab's existing local registry (`registry.local:5000`),
  not a direct containerd import** — corrected from the original design
  here after actually setting this up: `docker save ... | sudo k3s ctr
  images import -` (what the manual k3s install path in §4.6 still uses)
  needs root, and the runner's own user has no passwordless sudo on this
  host, confirmed by testing both a plain and a scoped `sudo -n` and
  getting `permission denied` on `/run/k3s/containerd/containerd.sock` for
  both. `registry.local:5000` is the homelab's own established pattern
  for this — other repos' pipelines already push there, Docker credentials
  for it were already configured on the runner's host account, and a
  registry push needs no root at all. So this pipeline needs one small
  stored-credential dependency after all (the runner's already-configured
  Docker login for `registry.local:5000`), just not a GitHub-side secret —
  it's the same host-level credential every other homelab pipeline reuses.
- `.github/workflows/deploy-backend.yml`:
  - Trigger: `on: push: branches: [main]`, path-filtered to `backend/**`
    and `deploy/**` so unrelated commits (docs, the macOS app) don't
    trigger a redeploy.
  - `runs-on: [self-hosted, Linux, X64]`.
  - `env: KUBECONFIG: /home/multivac/.kube/config` at the job level — the
    runner process is started by systemd, not an interactive shell, so it
    never picks up `~/.bashrc`'s override, and without this every
    `kubectl` call fails with `permission denied` reading the root-only
    `/etc/rancher/k3s/k3s.yaml` default. Confirmed by a real failed CI run
    (the first end-to-end trigger after the runner came online) before
    this was added.
  - Steps: checkout → `docker build` (tag both
    `registry.local:5000/switchbot-home-backend:${{ github.sha }}` and
    `:latest`) → `docker push` both tags → `kubectl apply -f` the
    namespace/ConfigMap/PVC/Service files only (**not**
    `30-deployment.yaml`, which still points at the generic
    `switchbot-home-backend:local` tag for the public installer's
    benefit — applying it as-is would reset the live Deployment to that
    unpullable tag for a few seconds, confirmed for real as an
    `ErrImagePull` + pod kill/recreate during the manual deploy that set
    this pipeline up) → `sed`-substitute the pushed
    `registry.local:5000/switchbot-home-backend:${{ github.sha }}` tag
    into `30-deployment.yaml` and `kubectl apply` that in one step, so the
    live Deployment's image is never wrong even momentarily → `kubectl
    rollout status deployment/switchbot-home-backend` (fails the workflow
    if the rollout doesn't succeed, instead of silently leaving a broken
    deployment).
- Setting up the runner itself (registering it with GitHub, running it as
  a persistent service on the homelab) follows GitHub's own runner setup
  docs — not reproduced here since it's a one-time, account-specific,
  token-based step better done directly from GitHub's UI at the time.

## 9. README additions

An "Install" section (near the top, before the existing "Backend: build,
run, test" / "macOS app: build, run, test" developer-focused sections —
those stay as-is for people working ON the code, this is for people who
just want to run it) with:

```
# Backend (Linux only — systemd or k3s, your choice at install time)
curl -fsSL https://raw.githubusercontent.com/alehrs/switchbot-home/main/scripts/install-backend.sh | bash

# macOS menu-bar app
curl -fsSL https://raw.githubusercontent.com/alehrs/switchbot-home/main/scripts/install-macos-app.sh | bash
```

Plus: what each script needs installed already vs. installs itself (§4/§5),
where to look at logs, and where the env-var config file lives per
deployment target (§6).

## 10. Open risks / to validate during implementation

- **Real k3s + real Bluetooth hardware is the one piece still unverified**
  (§7) — the container itself is now confirmed to build and run
  correctly, including the exact D-Bus socket path and the required
  `libdbus` packages (found via an actual failed build, not guessed), but
  none of that has run on an actual k3s node with a real adapter yet.
  Everything else here is standard, well-trodden territory (systemd
  units, k3s manifests, self-hosted runners).
- `setcap` for non-root BLE access under systemd (§4.5) needs the same
  kind of empirical check — a commonly-used pattern for BLE tools on
  Linux in general, not yet confirmed against this specific binary (no
  Linux systemd host was available to test against in this environment,
  only the containerized build/run path was testable).
- Xcode Command Line Tools' first-run GUI prompt (§5.2) can't be scripted
  around — accepted as a known, unavoidable manual step on a genuinely
  fresh Mac, not something to solve here.
- Exact Linux distro(s) to support for the systemd path's "install a C
  compiler" step isn't pinned down — Debian/Ubuntu (`apt`) is the most
  likely homelab target; the script should detect the package manager and
  give a clear "unsupported distro, install a C toolchain yourself"
  message rather than silently failing on anything else.

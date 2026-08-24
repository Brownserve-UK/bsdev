# bsdev

`bsdev` is a cross-platform (Windows/Linux/macOS) CLI that launches and connects to a personal,
permanently-running Arch Linux dev container (Docker). This repo holds both the **Rust launcher**
and the **container image**. Run `bsdev` from any shell and it pulls the image, starts the
container, and drops you into it over ssh; `code .` inside the container opens the folder in the
host's VSCode.

## Repo layout

- `Cargo.toml` - virtual workspace, members `["core", "cli"]`, `[workspace.package]` holds the
  shared version (the Brownserve release tooling bumps it; keep `Cargo.lock` committed).
- `core/` (crate `bsdev-core`) - all orchestration logic, unit-tested, `thiserror` errors:
  - `settings.rs` - `Settings`: constants with `BSDEV_*` env overrides (`BSDEV_IMAGE`,
    `BSDEV_CONTAINER`, `BSDEV_PORT`, `BSDEV_USER`). The home directory is always the fixed
    `bsdev-home` named volume (not overridable). `repos_dir` optionally bind-mounts a host
    directory at `~/host-repos` (unset by default) so code changes are reachable from the host,
    e.g. for running integration tests in host VMs; resolved from `BSDEV_REPOS` if set, else from
    `config.json` (see `config.rs`), persisted via `bsdev repos <path>` / `Settings::persist_repos_dir`.
    `adb_port` optionally forwards a host adb server port into the container (unset/disabled by
    default); resolved from `BSDEV_ADB_PORT` if set, else from `config.json`, persisted via
    `bsdev adb [<port>]` / `Settings::persist_adb_port`. The ssh key lives in a bsdev state dir via
    the `directories` crate (NOT `~/.ssh`).
  - `config.rs` - `Config`: a `serde`-derived struct (JSON, `serde_json`) persisted at
    `<state dir>/config.json`, holding user settings across runs (`repos_dir`, `adb_port`). Every
    field is optional so the file can grow without breaking older configs.
  - `docker.rs` - pure arg-builders (`run_args`, tested) + wrappers (`state`, `pull_image`,
    `run_container`, `ensure_authorized_key`, `remove`, `remove_volume`, ...).
  - `ssh.rs` - `ensure_keypair`, `read_pubkey`, `connect_args`/`connect` (interactive session) and
    `adb_tunnel_args` (background adb tunnel, see `adbtunnel.rs`), sharing a common `base_args` - all
    explicit (no reliance on `~/.ssh/config`; host keys are discarded via
    `UserKnownHostsFile=/dev/null` + `StrictHostKeyChecking=no`, the way `vagrant ssh` does, since the
    container's host keys change on recreate).
  - `codebridge.rs` - the reverse `code` channel (see below).
  - `adbtunnel.rs` - `start`/`stop` for a dedicated, detached background `ssh -N -R` tunnel that
    reverse-forwards the host's adb server port into the container, tracked via a PID file (see
    "adb passthrough" below).
  - `process.rs` - `Command` runners: `run`/`capture` with inherited stdio (real TTY) + friendly
    not-found errors, and `spawn_detached` (no inherited stdio, no shared process group/console -
    survives the parent exiting) used by the adb tunnel.
- `cli/` (crate `bsdev`, `[[bin]] name = "bsdev"`) - thin clap shell, `anyhow`:
  - `cli.rs` - clap types; `main.rs` - dispatch + command handlers.
  - Commands: `bsdev` (default: ensure up + connect), `up`, `down`, `status`, `rebuild`, `reset`,
    `repos` (get/persist/unset the `~/host-repos` bind-mount source directory), `adb` (get/persist/
    unset the forwarded host adb server port).
- `image/` - the container image (published to `ghcr.io/brownserve-uk/bsdev` by CI):
  - `Dockerfile` - Arch, `bsdev` user + passwordless sudo, sshd as PID 1, `/etc/bsdev-container`
    marker, ALL tooling baked in (gh, chezmoi, git, fish, Node, Rust via rustup, oh-my-posh, tenv,
    topgrade, Claude Code), and a `devcontainer.metadata` label so VSCode attaches as `bsdev`.
  - `bsdev-entrypoint.sh` - installs the host pubkey into `authorized_keys`, then `exec sshd -D`.
  - `code` - in-container `code` shim (bash; sends the path over the reverse channel).
  - `fish-cargo-path.fish` - puts `~/.cargo/bin` on PATH for fish.
- `.build/` + `.github/` - Brownserve "RustApp" scaffold (PowerShell/Invoke-Build + CI). Do not
  hand-edit the scaffold unless necessary.

## How it works

- **Connect flow** (`ensure_up` in `cli/src/main.rs`): ensure Docker -> ensure keypair -> read
  pubkey -> pull image if missing -> run/start container -> `ensure_authorized_key` (docker exec,
  idempotent, so a persisted volume or rotated key just works) -> reconcile the adb tunnel (no-op if
  disabled, and a no-op if already alive and forwarding the right port) -> start the code-bridge
  listener -> `ssh` in. `ensure_up` runs for both `bsdev up` and plain `bsdev`, so a second session
  (a new terminal/tab opened while a first is still using the tunnel) doesn't disrupt it.
- **`code .` cold-launch**: the launcher runs a host TCP listener on `127.0.0.1:9918` and the ssh
  session reverse-forwards it (`-R`) into the container. The `image/code` shim writes
  `<dir|file> <abs-path>` to that port; the host listener opens it via VSCode **Dev Containers
  attach** (`code --folder-uri vscode-remote://attached-container+<hex(container)>/<path>`) - no
  ssh config, no Remote-SSH. Requires the host's VSCode + Dev Containers extension.
- **adb passthrough** (`adbtunnel.rs`, opt-in via `bsdev adb [<port>]`): once you `code .` and close
  the terminal `bsdev` was launched from, VSCode's own integrated terminals attach to the container
  directly (`docker exec`-style), not through that ssh session - so adb forwarding can't ride the
  interactive `connect` session the way the code-bridge does. Instead `adbtunnel::start` runs a
  second, fully **detached** `ssh -N -R 127.0.0.1:<port>:127.0.0.1:<port>` (via
  `process::spawn_detached`: no inherited stdio, `DETACHED_PROCESS`/`CREATE_NEW_PROCESS_GROUP` on
  Windows, a fresh `process_group` on Unix), tracked by a PID file
  (`Settings::adb_tunnel_pid_path`, storing `<pid>:<port>` so a changed port is detected too).
  `adbtunnel::start` only kills-and-respawns when the tracked PID is missing/dead or forwarding the
  wrong port (`adbtunnel::is_alive`, shelling out to `tasklist`/`kill -0`) - restarting unconditionally
  on every `bsdev`/`bsdev up` would sever any adb command in flight through an already-good tunnel
  the moment a second session started. `down`/`rebuild`/`reset` call `adbtunnel::stop` unconditionally.
  The container's `adb` client (from `android-sdk-platform-tools`, already in the image) just connects
  to its default `127.0.0.1:5037`, which the tunnel makes real - no `ADB_SERVER_SOCKET`, no Docker
  networking (`host.docker.internal`/`host-gateway`) needed, and the host's adb server never needs to
  bind beyond loopback. Requires `adb start-server` already running on the host.
- **Provisioning is NOT in this repo.** The image is batteries-included; user-specific setup (gh
  auth + `chezmoi init --apply`) is done once inside the container by
  `bootstrap/bootstrap-bsdev.sh` in the separate `shoddyguard/portable_config` chezmoi repo.

## Building, testing, running

There is usually **no cargo on the host** (Windows dev box), so build/test in a throwaway
container. From the repo root:

```sh
# build + test (Linux target)
docker run --rm -e CARGO_TARGET_DIR=/tmp/target -v "$PWD:/w" -w /w rust:1-slim \
  sh -c "cargo build --workspace && cargo test --workspace"

# cross-build the Windows binary (host has the msvc target but no VS Build Tools / link.exe,
# so use windows-gnu), output to the repo target, then copy to ~/.local/bin
docker run --rm -e CARGO_TARGET_DIR=/w/target -v "$PWD:/w" -w /w rust:1-slim \
  sh -c "rustup target add x86_64-pc-windows-gnu && apt-get update -qq && \
         apt-get install -y -qq gcc-mingw-w64-x86-64 && \
         cargo build --release --target x86_64-pc-windows-gnu"
# -> target/x86_64-pc-windows-gnu/release/bsdev.exe  (copy over ~/.local/bin/bsdev.exe)

# build the image locally (tag as the GHCR ref so `bsdev` uses it without pulling)
docker build -t ghcr.io/brownserve-uk/bsdev:latest ./image

# authoritative scaffold gate (needs PowerShell 7 + Rust; runs the Pester binary contract)
./.build/build.ps1 -Build BuildTestAndCheck
```

The Windows binary is often locked while a `bsdev` session is running - close the session before
overwriting `~/.local/bin/bsdev.exe`.

## Conventions

- Scaffold contract: binary must be named `bsdev`; `--help`/`--version` must work with `bsdev` in
  the version output (Pester test `.build/tests/Basic.Binary.Tests.ps1`). Root `Cargo.toml` stays a
  virtual workspace with `[workspace.package] version`.
- `.editorconfig`: LF everywhere; `.rs` = 4-space, `Cargo.toml`/`.toml` = 2-space, `.ps1` = CRLF.
  The Edit tool on Windows can reintroduce CRLF, so normalize edited files to LF after editing.
- Keep `core` logic pure/testable (arg-vector builders) and `cli` thin. Errors: `thiserror` in
  `core`, `anyhow` `.context(...)` in `cli`.
- Conventional Commits PR titles and signed commits are hard requirements (see
  `.github/CONTRIBUTING.md`). Do not run `git commit`/`git push` unless explicitly asked.

## Related repos and gotchas

- `shoddyguard/portable_config` (chezmoi dotfiles) provides `bootstrap/bootstrap-bsdev.sh`, the
  shell configs that detect the container via the `/etc/bsdev-container` marker, and the `run_once`
  installer that downloads the `bsdev` release binary. It is applied INSIDE the container, and the
  container pulls it from GitHub - so chezmoi changes only take effect there once pushed and
  `chezmoi update` is run in the container.
- VSCode attach also needs `dev.containers.copyGitConfig: false` (a per-machine client setting the
  image can't set) so it uses the container's git config instead of copying the host's.
- The GHCR image build/publish CI is a separate concern; locally the image is a `docker build` of
  `image/`.

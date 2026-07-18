# bsdev

`bsdev` launches and connects to a permanently-running Arch Linux dev
container, from any shell on any OS.

> [!NOTE]
> Built for Brownserve projects and conventions, may have limited use outside of that context.

The container comes with:

- git
- GitHub CLI
- chezmoi
- fish
- oh-my-posh
- topgrade
- Node.js / npm
- Rust (via rustup)
- tenv
- Claude Code
- JDK
- Android SDK

## Install

Grab the latest release and run the installer for your platform:

```sh
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/Brownserve-UK/bsdev/main/scripts/install.sh | bash
```

```powershell
# Windows
irm https://raw.githubusercontent.com/Brownserve-UK/bsdev/main/scripts/install.ps1 | iex
```

Both scripts download the right binary for your OS/architecture from the
[latest release](https://github.com/Brownserve-UK/bsdev/releases/latest) and
put it on your PATH.

## Usage

Just run `bsdev`. On first run it creates an SSH key, pulls the image and
starts the container; every run after that just connects you. `code .` inside
the container opens the folder in your host's VSCode - no config needed, just
the [Dev Containers](https://marketplace.visualstudio.com/items?itemName=ms-vscode-remote.remote-containers)
extension installed.

```plain
Usage: bsdev [OPTIONS] [COMMAND]

Commands:
  up       Ensure the image and container are up, without connecting
  down     Stop the container (its home volume is preserved)
  status   Show image, container and home volume state
  rebuild  Pull the latest image and recreate the container (keeps the home volume)
  repos    Get or persist the host directory bind-mounted at ~/host-repos
  adb      Get or persist the host adb server port forwarded into the container

Options:
  -v, --verbose  Print each docker/ssh command as it runs
  -h, --help     Print help
  -V, --version  Print version
```

If you want code changes made inside the container reachable from the host
(e.g. to run integration tests in host VMs), point bsdev at a host directory;
it's bind-mounted at `~/host-repos` in the container. Persist it once with
`bsdev repos <path>` so you don't have to set it every run, or set `BSDEV_REPOS`
to override it for a single run (`bsdev repos --unset` clears the persisted
value, `bsdev repos` with no arguments shows it). This is optional and off by
default - a plain host directory can't hold Unix symlinks on Windows, so a
repo relying on those should live in a WSL2/Linux-backed path instead.

For Android dev, `bsdev adb [<port>]` (default 5037) forwards your host's adb
server into the container over a dedicated background ssh tunnel, so `adb`
inside the container reaches devices attached to the host - this keeps working
even in a VSCode terminal after you've closed the terminal `bsdev` was launched
from. Requires `adb start-server` already running on the host. Off by default;
`bsdev adb --unset` disables it, and `bsdev adb` with no arguments shows the
current port.

## Building

Requires the [Rust toolchain](https://rustup.rs) and [PowerShell 7+](https://github.com/PowerShell/PowerShell).

```powershell
./.build/build.ps1 -Build BuildTestAndCheck
```

See [CONTRIBUTING.md](.github/CONTRIBUTING.md) for more.

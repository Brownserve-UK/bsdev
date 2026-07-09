# bsdev

`bsdev` launches and connects to a permanently-running Arch Linux dev
container, from any shell on any OS.

> [!NOTE]
> Built for Brownserve projects and conventions, may have limited use outside of that context.

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
the container opens the folder in your host's VSCode, no manual Remote-SSH
setup required.

The container comes with:

- git, GitHub CLI (`gh`), chezmoi
- fish shell, oh-my-posh, topgrade
- Node.js / npm
- Rust (via rustup)
- tenv
- Claude Code

```
Usage: bsdev [OPTIONS] [COMMAND]

Commands:
  up       Ensure the image and container are up, without connecting
  down     Stop the container (its home volume is preserved)
  status   Show image, container and volume state
  rebuild  Pull the latest image and recreate the container (keeps the home volume)

Options:
  -v, --verbose  Print each docker/ssh command as it runs
  -h, --help     Print help
  -V, --version  Print version
```

## Building

Requires the [Rust toolchain](https://rustup.rs) and [PowerShell 7+](https://github.com/PowerShell/PowerShell).

```powershell
./.build/build.ps1 -Build BuildTestAndCheck
```

See [CONTRIBUTING.md](.github/CONTRIBUTING.md) for more.

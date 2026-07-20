use std::fs;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

use bsdev_core::docker::{self, ContainerState};
use bsdev_core::{adbtunnel, codebridge, ssh, Settings};

mod cli;
mod update;

use cli::{Cli, Command};

/// Default adb server port, used when `bsdev adb` is run with no explicit port.
const DEFAULT_ADB_PORT: u16 = 5037;

fn main() -> Result<()> {
    let args = Cli::parse();

    // Updating must not load container settings. In particular, a system-wide
    // install may need to be updated with elevated privileges, and that should
    // not read or create configuration for the elevated account.
    if let Some(Command::Update { yes }) = &args.command {
        return update::run(*yes);
    }

    let settings = Settings::load().context("Failed to determine bsdev settings")?;
    let verbose = args.verbose;

    match args.command {
        None => connect(&settings, verbose),
        Some(Command::Update { .. }) => {
            unreachable!("updates are handled before settings are loaded")
        }
        Some(Command::Up) => {
            ensure_up(&settings, verbose)?;
            println!("bsdev is up. Run `bsdev` to connect.");
            Ok(())
        }
        Some(Command::Down) => down(&settings, verbose),
        Some(Command::Status) => status(&settings),
        Some(Command::Rebuild) => rebuild(&settings, verbose),
        Some(Command::Reset { yes }) => reset(&settings, verbose, yes),
        Some(Command::Repos { path, unset }) => repos(path, unset),
        Some(Command::Adb { port, unset }) => adb(port, unset),
    }
}

/// Ensure Docker is available, the ssh keypair exists, the image is present, and
/// the container is running - creating/pulling/starting as needed.
fn ensure_up(settings: &Settings, verbose: bool) -> Result<()> {
    docker::ensure_available().context("Docker is required to run bsdev")?;
    ssh::ensure_keypair(settings, verbose).context("Failed to create the bsdev ssh key")?;
    let pubkey = ssh::read_pubkey(settings).context("Failed to read the bsdev public key")?;

    if !docker::image_present(&settings.image).context("Failed to inspect the bsdev image")? {
        println!("Pulling {} ...", settings.image);
        docker::pull_image(&settings.image, verbose)?;
    }

    match docker::state(&settings.container).context("Failed to inspect the bsdev container")? {
        ContainerState::Running => {}
        ContainerState::Stopped => {
            println!("Starting the bsdev container ...");
            docker::start(&settings.container, verbose).context("Failed to start the container")?;
        }
        ContainerState::Missing => {
            ensure_repos_dir(settings)?;
            println!("Creating the bsdev container ...");
            docker::run_container(settings, &pubkey, verbose).context("Failed to create the container")?;
        }
    }

    // Authorise our key inside the container every time: covers a persisted home
    // volume created with a previous key, or a rotated/relocated host key, so we
    // never need a recreate to reconnect.
    docker::ensure_authorized_key(settings, &pubkey, verbose)
        .context("Failed to authorize the bsdev key in the container")?;

    // Restart the background adb tunnel (see `adbtunnel::start`) so it's alive
    // for the container's whole session, independent of any connect session -
    // a no-op unless an adb port is configured.
    adbtunnel::start(settings, verbose).context("Failed to start the adb tunnel")?;
    Ok(())
}

/// Create the `BSDEV_REPOS` host directory (if configured) before the
/// container is created, so Docker doesn't create it root-owned and the bind
/// mount has somewhere to land on a fresh volume.
fn ensure_repos_dir(settings: &Settings) -> Result<()> {
    if let Some(dir) = &settings.repos_dir {
        fs::create_dir_all(dir).context("Failed to create the bsdev repos directory")?;
    }
    Ok(())
}

fn connect(settings: &Settings, verbose: bool) -> Result<()> {
    ensure_up(settings, verbose)?;
    // Start the host-side `code` bridge listener; the ssh session reverse-forwards
    // its port so `code .` inside the container opens folders in the host VSCode.
    codebridge::spawn_listener(settings);
    ssh::connect(settings, verbose).context("Failed to connect to the bsdev container")
}

fn down(settings: &Settings, verbose: bool) -> Result<()> {
    docker::ensure_available().context("Docker is required to run bsdev")?;
    adbtunnel::stop(settings, verbose);
    match docker::state(&settings.container)? {
        ContainerState::Missing => println!("No bsdev container to stop."),
        _ => {
            docker::stop(&settings.container, verbose).context("Failed to stop the container")?;
            println!("bsdev stopped.");
        }
    }
    Ok(())
}

fn rebuild(settings: &Settings, verbose: bool) -> Result<()> {
    docker::ensure_available().context("Docker is required to run bsdev")?;
    adbtunnel::stop(settings, verbose);
    println!("Pulling {} ...", settings.image);
    docker::pull_image(&settings.image, verbose)?;

    if docker::state(&settings.container)? != ContainerState::Missing {
        println!(
            "Removing the old container (the '{}' home volume is kept) ...",
            settings.volume
        );
        docker::remove(&settings.container, verbose).context("Failed to remove the container")?;
    }

    ssh::ensure_keypair(settings, verbose).context("Failed to create the bsdev ssh key")?;
    let key = ssh::read_pubkey(settings).context("Failed to read the bsdev public key")?;
    ensure_repos_dir(settings)?;
    println!("Creating the bsdev container ...");
    docker::run_container(settings, &key, verbose).context("Failed to create the container")?;
    println!("bsdev rebuilt. Run `bsdev` to connect.");
    Ok(())
}

fn status(settings: &Settings) -> Result<()> {
    if docker::ensure_available().is_err() {
        println!("docker:    NOT available");
        return Ok(());
    }
    println!("docker:    available");

    let image = if docker::image_present(&settings.image)? { "present" } else { "missing" };
    println!("image:     {} ({})", settings.image, image);

    let state = match docker::state(&settings.container)? {
        ContainerState::Running => "running",
        ContainerState::Stopped => "stopped",
        ContainerState::Missing => "missing",
    };
    println!("container: {} ({})", settings.container, state);

    let home = if docker::volume_present(&settings.volume)? { "present" } else { "missing" };
    println!("home:      {} ({})", settings.volume, home);
    if let Some(repos_dir) = &settings.repos_dir {
        println!("repos:     {}", repos_dir.display());
    }
    println!("key:       {}", settings.key_path.display());
    match settings.adb_port {
        Some(port) => println!("adb:       forwarding host port {port}"),
        None => println!("adb:       disabled"),
    }
    Ok(())
}

/// Delete the container and its home volume for a clean slate. Prompts for
/// confirmation (the home volume holds all repos/provisioning) unless `yes`
/// is set. The host keypair is left in place - it is re-authorized on the
/// next `up`. Any `BSDEV_REPOS` host directory is left untouched - it's the
/// user's own files on the host, not bsdev-managed state.
fn reset(settings: &Settings, verbose: bool, yes: bool) -> Result<()> {
    docker::ensure_available().context("Docker is required to run bsdev")?;

    let container_exists = docker::state(&settings.container)
        .context("Failed to inspect the bsdev container")?
        != ContainerState::Missing;
    let home_exists =
        docker::volume_present(&settings.volume).context("Failed to inspect the bsdev home volume")?;

    if !container_exists && !home_exists {
        println!("Nothing to reset - no bsdev container or home volume exist.");
        return Ok(());
    }

    if !yes {
        eprintln!("This permanently deletes:");
        if container_exists {
            eprintln!("  - the '{}' container", settings.container);
        }
        if home_exists {
            eprintln!(
                "  - the '{}' home volume (ALL repos, provisioning and data)",
                settings.volume
            );
        }
        if let Some(repos_dir) = &settings.repos_dir {
            eprintln!("  (the repos directory at {} is left untouched)", repos_dir.display());
        }
        if !confirm("Continue?")? {
            println!("Aborted.");
            return Ok(());
        }
    }

    adbtunnel::stop(settings, verbose);

    // Remove the container first - it's using the home volume.
    if container_exists {
        println!("Removing the '{}' container ...", settings.container);
        docker::remove(&settings.container, verbose).context("Failed to remove the container")?;
    }
    if home_exists {
        println!("Removing the '{}' home volume ...", settings.volume);
        docker::remove_volume(&settings.volume, verbose).context("Failed to remove the home volume")?;
    }
    println!("Reset complete. Run `bsdev` to start fresh.");
    Ok(())
}

/// Get or persist the `BSDEV_REPOS` host directory (see `Settings::persist_repos_dir`).
fn repos(path: Option<PathBuf>, unset: bool) -> Result<()> {
    if unset {
        Settings::clear_persisted_repos_dir().context("Failed to clear the persisted repos directory")?;
        println!("Cleared the persisted repos directory.");
        return Ok(());
    }

    if let Some(dir) = path {
        Settings::persist_repos_dir(&dir).context("Failed to persist the repos directory")?;
        println!("Persisted repos directory: {}", dir.display());
        return Ok(());
    }

    match Settings::persisted_repos_dir().context("Failed to read the persisted repos directory")? {
        Some(dir) => println!("{}", dir.display()),
        None => println!(
            "No repos directory persisted. Run `bsdev repos <path>` to set one, or set BSDEV_REPOS to override for a single run."
        ),
    }
    Ok(())
}

/// Get or persist the host adb server port forwarded into the container (see
/// `Settings::persist_adb_port`). Takes effect on the next `bsdev up`/`bsdev`,
/// which restarts the background tunnel to match.
fn adb(port: Option<u16>, unset: bool) -> Result<()> {
    if unset {
        Settings::clear_persisted_adb_port().context("Failed to clear the persisted adb port")?;
        println!("Cleared the persisted adb port. Run `bsdev up` (or `bsdev`) to stop the tunnel.");
        return Ok(());
    }

    if let Some(port) = port {
        Settings::persist_adb_port(port).context("Failed to persist the adb port")?;
        println!("Persisted adb port: {port}. Run `bsdev up` (or `bsdev`) to start forwarding it.");
        return Ok(());
    }

    match Settings::persisted_adb_port().context("Failed to read the persisted adb port")? {
        Some(port) => println!("{port}"),
        None => println!(
            "No adb port persisted. Run `bsdev adb [<port>]` to set one (defaults to {DEFAULT_ADB_PORT}), or set BSDEV_ADB_PORT to override for a single run."
        ),
    }
    Ok(())
}

/// Prompt for a y/N confirmation. Refuses (errors) when stdin is not a terminal
/// so a piped/non-interactive invocation can't silently wipe data - use `--yes`.
fn confirm(prompt: &str) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("Refusing to reset without confirmation; re-run with --yes to reset non-interactively.");
    }
    eprint!("{prompt} [y/N] ");
    std::io::stderr().flush().ok();
    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .context("Failed to read confirmation")?;
    let answer = input.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}

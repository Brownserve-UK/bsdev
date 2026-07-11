use std::io::{IsTerminal, Write};

use anyhow::{Context, Result};
use clap::Parser;

use bsdev_core::docker::{self, ContainerState};
use bsdev_core::{codebridge, ssh, Settings};

mod cli;

use cli::{Cli, Command};

fn main() -> Result<()> {
    let args = Cli::parse();
    let settings = Settings::load().context("Failed to determine bsdev settings")?;
    let verbose = args.verbose;

    match args.command {
        None => connect(&settings, verbose),
        Some(Command::Up) => {
            ensure_up(&settings, verbose)?;
            println!("bsdev is up. Run `bsdev` to connect.");
            Ok(())
        }
        Some(Command::Down) => down(&settings, verbose),
        Some(Command::Status) => status(&settings),
        Some(Command::Rebuild) => rebuild(&settings, verbose),
        Some(Command::Reset { yes }) => reset(&settings, verbose, yes),
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
            println!("Creating the bsdev container ...");
            docker::run_container(settings, &pubkey, verbose).context("Failed to create the container")?;
        }
    }

    // Authorise our key inside the container every time: covers a persisted home
    // volume created with a previous key, or a rotated/relocated host key, so we
    // never need a recreate to reconnect.
    docker::ensure_authorized_key(settings, &pubkey, verbose)
        .context("Failed to authorize the bsdev key in the container")?;
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
    println!("Pulling {} ...", settings.image);
    docker::pull_image(&settings.image, verbose)?;

    if docker::state(&settings.container)? != ContainerState::Missing {
        println!("Removing the old container (the {} volume is kept) ...", settings.volume);
        docker::remove(&settings.container, verbose).context("Failed to remove the container")?;
    }

    ssh::ensure_keypair(settings, verbose).context("Failed to create the bsdev ssh key")?;
    let key = ssh::read_pubkey(settings).context("Failed to read the bsdev public key")?;
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

    let volume = if docker::volume_present(&settings.volume)? { "present" } else { "missing" };
    println!("volume:    {} ({})", settings.volume, volume);
    println!("key:       {}", settings.key_path.display());
    Ok(())
}

/// Delete the container and its home volume for a clean slate. Prompts for
/// confirmation (the volume holds all repos/provisioning) unless `yes` is set.
/// The host keypair is left in place - it is re-authorized on the next `up`.
fn reset(settings: &Settings, verbose: bool, yes: bool) -> Result<()> {
    docker::ensure_available().context("Docker is required to run bsdev")?;

    let container_exists = docker::state(&settings.container)
        .context("Failed to inspect the bsdev container")?
        != ContainerState::Missing;
    let volume_exists =
        docker::volume_present(&settings.volume).context("Failed to inspect the bsdev volume")?;

    if !container_exists && !volume_exists {
        println!("Nothing to reset - no bsdev container or volume exist.");
        return Ok(());
    }

    if !yes {
        eprintln!("This permanently deletes:");
        if container_exists {
            eprintln!("  - the '{}' container", settings.container);
        }
        if volume_exists {
            eprintln!(
                "  - the '{}' volume (ALL repos, provisioning and data in the container's home)",
                settings.volume
            );
        }
        if !confirm("Continue?")? {
            println!("Aborted.");
            return Ok(());
        }
    }

    // Remove the container first - the volume can't be removed while it's in use.
    if container_exists {
        println!("Removing the '{}' container ...", settings.container);
        docker::remove(&settings.container, verbose).context("Failed to remove the container")?;
    }
    if volume_exists {
        println!("Removing the '{}' volume ...", settings.volume);
        docker::remove_volume(&settings.volume, verbose).context("Failed to remove the volume")?;
    }
    println!("Reset complete. Run `bsdev` to start fresh.");
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

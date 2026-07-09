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

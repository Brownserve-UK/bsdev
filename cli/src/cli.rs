use clap::{Parser, Subcommand};

/// `bsdev` - launch and connect to your personal dev container.
///
/// Running `bsdev` with no subcommand ensures the image and container are up
/// (pulling/creating as needed) and drops you into it over ssh.
#[derive(Parser, Debug)]
#[command(name = "bsdev", version, about = "Launch and connect to your bsdev dev container")]
pub struct Cli {
    /// Print each docker/ssh command as it runs.
    #[arg(long, short = 'v', global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Ensure the image and container are up, without connecting.
    Up,
    /// Stop the container (its home volume is preserved).
    Down,
    /// Show image, container and volume state.
    Status,
    /// Pull the latest image and recreate the container (keeps the home volume).
    Rebuild,
}

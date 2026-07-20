use std::path::PathBuf;

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
    /// Update bsdev to the latest published release.
    Update {
        /// Skip the confirmation prompt.
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Ensure the image and container are up, without connecting.
    Up,
    /// Stop the container (its home volume is preserved).
    Down,
    /// Show image, container and home volume state.
    Status,
    /// Pull the latest image and recreate the container (keeps the home volume).
    Rebuild,
    /// Delete the container and its home volume for a clean slate (destructive).
    Reset {
        /// Skip the confirmation prompt.
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Get or persist the host directory bind-mounted at `~/host-repos`.
    ///
    /// With no arguments, prints the currently persisted directory. Give a
    /// path to persist it (used on every future run without needing
    /// `BSDEV_REPOS` set); `BSDEV_REPOS` still overrides it for a single run.
    Repos {
        /// Host directory to persist as the repos bind-mount source.
        path: Option<PathBuf>,
        /// Clear the persisted repos directory.
        #[arg(long, conflicts_with = "path")]
        unset: bool,
    },
    /// Get or persist the host adb server port forwarded into the container.
    ///
    /// With no arguments, prints the currently persisted port (or the default,
    /// 5037, if enabling for the first time). Forwarding a port requires an
    /// `adb` server already running on the host; every `bsdev up`/`bsdev`
    /// restarts a dedicated background ssh tunnel that keeps it forwarded for
    /// as long as the container is up, independent of any connect session.
    /// `BSDEV_ADB_PORT` overrides the persisted port for a single run.
    Adb {
        /// Host adb server port to persist and forward (defaults to 5037).
        port: Option<u16>,
        /// Clear the persisted adb port (disables the tunnel).
        #[arg(long, conflicts_with = "port")]
        unset: bool,
    },
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Command};

    #[test]
    fn parses_update_command() {
        let cli = Cli::try_parse_from(["bsdev", "update"]).unwrap();

        assert!(matches!(cli.command, Some(Command::Update { yes: false })));
    }

    #[test]
    fn parses_update_yes_flag() {
        let cli = Cli::try_parse_from(["bsdev", "update", "--yes"]).unwrap();

        assert!(matches!(cli.command, Some(Command::Update { yes: true })));
    }

    #[test]
    fn rejects_unknown_update_arguments() {
        assert!(Cli::try_parse_from(["bsdev", "update", "--unknown"]).is_err());
    }
}

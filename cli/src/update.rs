use std::fs::{self, OpenOptions};
use std::io::IsTerminal;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use self_update::backends::github::Update;
use self_update::errors::Error as UpdateError;
use self_update::Status;

const OWNER: &str = "Brownserve-UK";
const REPO: &str = "bsdev";
const BINARY: &str = "bsdev";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Update the current executable from the latest published GitHub release.
pub fn run(yes: bool) -> Result<()> {
    require_confirmation_or_terminal(yes, std::io::stdin().is_terminal())?;

    let executable =
        std::env::current_exe().context("Failed to locate the running bsdev executable")?;
    ensure_install_directory_writable(&executable)?;

    let status = Update::configure()
        .repo_owner(OWNER)
        .repo_name(REPO)
        .bin_name(BINARY)
        .current_version(CURRENT_VERSION)
        .show_output(false)
        .show_download_progress(std::io::stdout().is_terminal())
        .no_confirm(yes)
        .build()
        .context("Failed to configure the bsdev updater")?
        .update()
        .map_err(map_update_error)?;

    println!("{}", status_message(&status));
    Ok(())
}

fn require_confirmation_or_terminal(yes: bool, stdin_is_terminal: bool) -> Result<()> {
    if !yes && !stdin_is_terminal {
        bail!("Refusing to update without confirmation; re-run with --yes in a non-interactive session.");
    }
    Ok(())
}

/// Probe the executable's directory before making a network request. The
/// updater replaces the executable via a sibling file, so directory
/// writability is the relevant permission on every supported platform.
fn ensure_install_directory_writable(executable: &Path) -> Result<()> {
    let install_dir = executable
        .parent()
        .context("The running bsdev executable has no parent directory")?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let probe = install_dir.join(format!(
        ".{BINARY}-update-check-{}-{nonce}",
        std::process::id()
    ));

    match OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(file) => drop(file),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            bail!(permission_message(executable));
        }
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "Failed to verify update access to {}",
                    install_dir.display()
                )
            });
        }
    }

    fs::remove_file(&probe).with_context(|| {
        format!(
            "Failed to remove update permission probe at {}",
            probe.display()
        )
    })?;
    Ok(())
}

fn map_update_error(error: UpdateError) -> anyhow::Error {
    match error {
        UpdateError::Io(ref io_error)
            if io_error.kind() == std::io::ErrorKind::PermissionDenied =>
        {
            let executable = std::env::current_exe().unwrap_or_else(|_| BINARY.into());
            anyhow::anyhow!(permission_message(&executable))
        }
        UpdateError::Network(_) | UpdateError::Ureq(_) => anyhow::anyhow!(error)
            .context("Failed to contact GitHub or download the latest bsdev release"),
        UpdateError::Release(_) => anyhow::anyhow!(error)
            .context("The latest bsdev release has no compatible asset for this platform"),
        _ => anyhow::anyhow!(error).context("Failed to install the latest bsdev release"),
    }
}

fn permission_message(executable: &Path) -> String {
    if cfg!(windows) {
        format!(
            "Cannot update {} without elevated permissions. Open a terminal as Administrator and run `bsdev update` again.",
            executable.display()
        )
    } else {
        format!(
            "Cannot update {} without elevated permissions. Re-run with `sudo bsdev update`.",
            executable.display()
        )
    }
}

fn status_message(status: &Status) -> String {
    match status {
        Status::UpToDate(version) => format!("bsdev is already up to date (v{version})."),
        Status::Updated(version) => {
            format!("Updated bsdev from v{CURRENT_VERSION} to v{version}. The next invocation will use the new version.")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_yes_when_stdin_is_not_a_terminal() {
        assert!(require_confirmation_or_terminal(false, false).is_err());
        assert!(require_confirmation_or_terminal(true, false).is_ok());
        assert!(require_confirmation_or_terminal(false, true).is_ok());
    }

    #[test]
    fn writable_directory_passes_preflight_without_leaving_a_probe() {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir
            .path()
            .join(if cfg!(windows) { "bsdev.exe" } else { "bsdev" });

        ensure_install_directory_writable(&executable).unwrap();

        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn status_messages_distinguish_current_and_updated_versions() {
        assert_eq!(
            status_message(&Status::UpToDate("1.2.3".into())),
            "bsdev is already up to date (v1.2.3)."
        );
        assert!(status_message(&Status::Updated("9.8.7".into())).contains("to v9.8.7"));
    }

    #[test]
    fn permission_message_explains_how_to_elevate() {
        let message = permission_message(Path::new("/protected/bsdev"));

        if cfg!(windows) {
            assert!(message.contains("Administrator"));
        } else {
            assert!(message.contains("sudo bsdev update"));
        }
    }
}

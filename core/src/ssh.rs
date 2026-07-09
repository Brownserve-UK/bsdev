use std::fs;

use crate::error::{BsdevError, Result};
use crate::process;
use crate::settings::Settings;

const SSH: &str = "ssh";
const SSH_KEYGEN: &str = "ssh-keygen";

/// Ensure the host keypair used to reach the container exists (creating a
/// passphraseless ed25519 key if not). This is host-side connection setup, not
/// container tooling.
pub fn ensure_keypair(settings: &Settings, verbose: bool) -> Result<()> {
    if settings.key_path.exists() {
        return Ok(());
    }
    if let Some(dir) = settings.key_path.parent() {
        fs::create_dir_all(dir)?;
    }
    let key = settings.key_path.to_string_lossy().into_owned();
    process::run(
        SSH_KEYGEN,
        ["-t", "ed25519", "-N", "", "-C", "bsdev", "-f", key.as_str()],
        verbose,
    )
}

/// Read the public key that will be authorized inside the container.
pub fn read_pubkey(settings: &Settings) -> Result<String> {
    Ok(fs::read_to_string(settings.pubkey_path())?.trim().to_string())
}

/// Build the explicit `ssh` argument vector. Deliberately self-contained (does
/// not rely on the `Host bsdev` alias in ~/.ssh/config), so the launcher works
/// even before chezmoi has deployed that config.
pub fn connect_args(settings: &Settings) -> Vec<String> {
    vec![
        "-t".to_string(),
        "-p".to_string(),
        settings.port.to_string(),
        "-i".to_string(),
        settings.key_path.to_string_lossy().into_owned(),
        "-o".to_string(),
        "IdentitiesOnly=yes".to_string(),
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
        "-o".to_string(),
        format!("UserKnownHostsFile={}", settings.known_hosts.to_string_lossy()),
        format!("{}@127.0.0.1", settings.user),
    ]
}

/// Open an interactive ssh session into the container. A non-zero exit from the
/// remote login shell (e.g. the last command the user ran failed) is not treated
/// as a launcher error; only failures to spawn ssh itself propagate.
pub fn connect(settings: &Settings, verbose: bool) -> Result<()> {
    match process::run(SSH, connect_args(settings), verbose) {
        Ok(()) | Err(BsdevError::CommandFailed { .. }) => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn settings() -> Settings {
        Settings {
            image: "img".to_string(),
            container: "bsdev".to_string(),
            volume: "bsdev-home".to_string(),
            port: 2222,
            user: "bsdev".to_string(),
            home_dir: PathBuf::from("/home/host"),
            key_path: PathBuf::from("/home/host/.ssh/bsdev"),
            known_hosts: PathBuf::from("/home/host/.ssh/known_hosts.bsdev"),
        }
    }

    #[test]
    fn connect_args_are_explicit_and_alias_free() {
        let a = settings();
        let args = connect_args(&a);
        assert!(args.contains(&"-t".to_string()));
        assert!(args.windows(2).any(|w| w[0] == "-p" && w[1] == "2222"));
        assert!(args.windows(2).any(|w| w[0] == "-i" && w[1] == "/home/host/.ssh/bsdev"));
        assert_eq!(args.last().unwrap(), "bsdev@127.0.0.1");
    }
}

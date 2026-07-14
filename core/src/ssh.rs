use std::fs;

use crate::codebridge;
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
        // Reverse-forward the code bridge port so the in-container `code` shim can
        // reach the host listener spawned by codebridge::spawn_listener.
        "-R".to_string(),
        format!("127.0.0.1:{p}:127.0.0.1:{p}", p = codebridge::CODE_PORT),
        "-p".to_string(),
        settings.port.to_string(),
        "-i".to_string(),
        settings.key_path.to_string_lossy().into_owned(),
        "-o".to_string(),
        "IdentitiesOnly=yes".to_string(),
        // The container regenerates its sshd host keys whenever it's recreated
        // (rm/rebuild), so pinning them just causes "host key changed" failures.
        // It's a local, self-created container on 127.0.0.1 that we launched and
        // injected our own key into, so - like `vagrant ssh` - don't store or
        // check host keys at all (UserKnownHostsFile=/dev/null + no strict check).
        // Nothing is ever written to the user's known_hosts.
        "-o".to_string(),
        "StrictHostKeyChecking=no".to_string(),
        "-o".to_string(),
        format!("UserKnownHostsFile={}", null_device()),
        // Quiet the resulting "Permanently added ..." warning, but keep real
        // errors visible (Vagrant uses FATAL; ERROR is a touch more debuggable).
        "-o".to_string(),
        "LogLevel=ERROR".to_string(),
        format!("{}@127.0.0.1", settings.user),
    ]
}

/// The platform's null device, used as an ssh `UserKnownHostsFile` so container
/// host keys are neither stored nor checked.
#[cfg(windows)]
fn null_device() -> &'static str {
    "NUL"
}

#[cfg(not(windows))]
fn null_device() -> &'static str {
    "/dev/null"
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
            home_dir: PathBuf::from("/state/bsdev/home"),
            port: 2222,
            user: "bsdev".to_string(),
            key_path: PathBuf::from("/state/bsdev/id_ed25519"),
        }
    }

    #[test]
    fn connect_args_are_explicit_and_alias_free() {
        let a = settings();
        let args = connect_args(&a);
        assert!(args.contains(&"-t".to_string()));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "-R" && w[1] == "127.0.0.1:9918:127.0.0.1:9918"));
        assert!(args.windows(2).any(|w| w[0] == "-p" && w[1] == "2222"));
        assert!(args.contains(&"-i".to_string()));
        // Host keys are discarded (container regenerates them on recreate).
        assert!(args.contains(&"StrictHostKeyChecking=no".to_string()));
        assert!(args
            .iter()
            .any(|a| a.starts_with("UserKnownHostsFile=") && (a.ends_with("/dev/null") || a.ends_with("NUL"))));
        assert_eq!(args.last().unwrap(), "bsdev@127.0.0.1");
    }
}

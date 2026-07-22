use crate::error::{BsdevError, Result};
use crate::process;
use crate::settings::Settings;

const DOCKER: &str = "docker";

/// Whether the named container exists and, if so, whether it is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerState {
    Missing,
    Stopped,
    Running,
}

/// Fail unless the Docker daemon is reachable.
pub fn ensure_available() -> Result<()> {
    match process::capture(DOCKER, &["version", "--format", "{{.Server.Version}}"]) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(BsdevError::DockerUnavailable(
            "the Docker daemon is not responding (is Docker running?)".to_string(),
        )),
        Err(BsdevError::CommandNotFound(_)) => Err(BsdevError::DockerUnavailable(
            "the `docker` command was not found on PATH".to_string(),
        )),
        Err(e) => Err(e),
    }
}

pub fn image_present(image: &str) -> Result<bool> {
    Ok(process::capture(DOCKER, &["image", "inspect", image])?.is_some())
}

pub fn pull_image(image: &str, verbose: bool) -> Result<()> {
    process::run(DOCKER, ["pull", image], verbose).map_err(|_| BsdevError::ImagePull(image.to_string()))
}

pub fn state(container: &str) -> Result<ContainerState> {
    match process::capture(DOCKER, &["inspect", "-f", "{{.State.Running}}", container])? {
        None => Ok(ContainerState::Missing),
        Some(s) if s == "true" => Ok(ContainerState::Running),
        Some(_) => Ok(ContainerState::Stopped),
    }
}

/// Whether the named volume exists (status/reset checks).
pub fn volume_present(volume: &str) -> Result<bool> {
    Ok(process::capture(DOCKER, &["volume", "inspect", volume])?.is_some())
}

/// The container's current `State.StartedAt` timestamp, `None` if it's missing.
/// The container ID survives a plain `restart`, but this always changes on one -
/// which is the signal we actually care about for the adb tunnel: a restart
/// kills sshd, so any tunnel dialled into the old session is dead too, even
/// though its host-side ssh process may still be sat there alive.
pub fn started_at(container: &str) -> Result<Option<String>> {
    process::capture(DOCKER, &["inspect", "-f", "{{.State.StartedAt}}", container])
}

/// Build the `docker run` argument vector. Pure so it can be unit-tested without
/// a Docker daemon. The public key is injected via an env var (read in Rust, not
/// via a shell `cat`) so this stays cross-platform.
pub fn run_args(settings: &Settings, authorized_key: &str) -> Vec<String> {
    let mut args = vec![
        "run".to_string(),
        "-d".to_string(),
        "--name".to_string(),
        settings.container.clone(),
        "--hostname".to_string(),
        settings.container.clone(),
        "--restart".to_string(),
        "unless-stopped".to_string(),
        "-p".to_string(),
        format!("127.0.0.1:{}:22", settings.port),
        "-v".to_string(),
        settings.home_mount(),
    ];
    if let Some(repos_mount) = settings.repos_mount() {
        args.push("-v".to_string());
        args.push(repos_mount);
    }
    args.push("-e".to_string());
    args.push(format!("BSDEV_AUTHORIZED_KEY={authorized_key}"));
    args.push("-e".to_string());
    args.push(format!("BSDEV_HOST_HOSTNAME={}", settings.host_hostname));
    args.push(settings.image.clone());
    args
}

pub fn run_container(settings: &Settings, authorized_key: &str, verbose: bool) -> Result<()> {
    process::run(DOCKER, run_args(settings, authorized_key), verbose)
}

pub fn start(container: &str, verbose: bool) -> Result<()> {
    process::run(DOCKER, ["start", container], verbose)
}

pub fn stop(container: &str, verbose: bool) -> Result<()> {
    process::run(DOCKER, ["stop", container], verbose)
}

pub fn remove(container: &str, verbose: bool) -> Result<()> {
    process::run(DOCKER, ["rm", "-f", container], verbose)
}

pub fn remove_volume(volume: &str, verbose: bool) -> Result<()> {
    process::run(DOCKER, ["volume", "rm", volume], verbose)
}

/// Ensure `pubkey` is present in the container user's authorized_keys. Idempotent
/// and run on every `up`, so a rotated/relocated host key or a persisted home
/// volume created with a previous key still authorises without a recreate. The
/// key is passed via an env var to avoid shell-quoting issues.
pub fn ensure_authorized_key(settings: &Settings, pubkey: &str, verbose: bool) -> Result<()> {
    let env = format!("BSDEV_PUB={pubkey}");
    let script = r#"install -d -m 700 ~/.ssh && touch ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys && { grep -qxF "$BSDEV_PUB" ~/.ssh/authorized_keys || printf '%s\n' "$BSDEV_PUB" >> ~/.ssh/authorized_keys; }"#;
    process::run(
        DOCKER,
        [
            "exec",
            "-u",
            settings.user.as_str(),
            "-e",
            env.as_str(),
            settings.container.as_str(),
            "bash",
            "-c",
            script,
        ],
        verbose,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn settings() -> Settings {
        Settings {
            image: "ghcr.io/brownserve-uk/bsdev:latest".to_string(),
            container: "bsdev".to_string(),
            volume: "bsdev-home".to_string(),
            repos_dir: None,
            port: 2222,
            user: "bsdev".to_string(),
            key_path: PathBuf::from("/state/bsdev/id_ed25519"),
            host_hostname: "my-laptop".to_string(),
            adb_port: None,
        }
    }

    fn has_pair(args: &[String], a: &str, b: &str) -> bool {
        args.windows(2).any(|w| w[0] == a && w[1] == b)
    }

    #[test]
    fn run_args_has_expected_shape() {
        let args = run_args(&settings(), "ssh-ed25519 AAAA test");
        assert_eq!(args[0], "run");
        assert!(args.contains(&"-d".to_string()));
        assert!(has_pair(&args, "--name", "bsdev"));
        assert!(has_pair(&args, "--hostname", "bsdev"));
        assert!(has_pair(&args, "--restart", "unless-stopped"));
        assert!(has_pair(&args, "-p", "127.0.0.1:2222:22"));
        assert!(has_pair(&args, "-v", "bsdev-home:/home/bsdev"));
        assert!(has_pair(&args, "-e", "BSDEV_AUTHORIZED_KEY=ssh-ed25519 AAAA test"));
        assert!(has_pair(&args, "-e", "BSDEV_HOST_HOSTNAME=my-laptop"));
        // The image is always the final positional argument.
        assert_eq!(args.last().unwrap(), "ghcr.io/brownserve-uk/bsdev:latest");
    }

    #[test]
    fn run_args_honours_a_custom_port() {
        let mut s = settings();
        s.port = 2200;
        let args = run_args(&s, "k");
        assert!(has_pair(&args, "-p", "127.0.0.1:2200:22"));
    }

    #[test]
    fn run_args_omits_repos_mount_when_unset() {
        let args = run_args(&settings(), "k");
        assert!(!args.iter().any(|a| a.contains("/host-repos")));
    }

    #[test]
    fn run_args_includes_repos_mount_when_set() {
        let mut s = settings();
        s.repos_dir = Some(PathBuf::from("/host/repos"));
        let args = run_args(&s, "k");
        assert!(has_pair(&args, "-v", "/host/repos:/home/bsdev/host-repos"));
    }
}

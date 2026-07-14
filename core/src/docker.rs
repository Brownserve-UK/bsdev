use std::fs;
use std::path::Path;

use crate::error::{BsdevError, Result};
use crate::process;
use crate::settings::Settings;

const DOCKER: &str = "docker";

/// Marker written into the host home dir once seeding completes, so an
/// interrupted seed (partial copy) is not mistaken for a valid home.
const SEED_MARKER: &str = ".bsdev-seeded";

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

/// Whether the host home dir exists and is non-empty (status/reset checks).
pub fn home_present(dir: &Path) -> Result<bool> {
    Ok(dir.exists() && fs::read_dir(dir)?.next().is_some())
}

/// Pure decision logic for whether a host home dir needs seeding, factored out
/// of `home_needs_seed` so it can be unit-tested without touching the filesystem.
fn needs_seed(exists: bool, empty: bool, marker_present: bool) -> bool {
    !exists || empty || !marker_present
}

/// True when the host home dir must be seeded: it does not exist, or it is
/// empty, or it exists but the completion marker is absent (partial seed).
pub fn home_needs_seed(dir: &Path) -> Result<bool> {
    if !dir.exists() {
        return Ok(needs_seed(false, true, false));
    }
    let empty = fs::read_dir(dir)?.next().is_none();
    let marker_present = dir.join(SEED_MARKER).exists();
    Ok(needs_seed(true, empty, marker_present))
}

/// Populate the host home dir from the image's `/home/<user>` the first time,
/// before any `docker run`, so the empty bind mount does not shadow the
/// image-baked tooling (rustup/cargo/claude/yay). Idempotent.
///
/// A named volume copies image content automatically; a bind mount does not,
/// so we do it explicitly with a throwaway (created, never started) container
/// + `docker cp`.
pub fn seed_home(settings: &Settings, verbose: bool) -> Result<()> {
    if !home_needs_seed(&settings.home_dir)? {
        return Ok(());
    }

    // A non-empty dir with no marker is a partial seed or someone else's data -
    // refuse to clobber it silently.
    if settings.home_dir.exists()
        && fs::read_dir(&settings.home_dir)?.next().is_some()
        && !settings.home_dir.join(SEED_MARKER).exists()
    {
        return Err(BsdevError::HomeSeedConflict(settings.home_dir.clone()));
    }

    // Create the dir from Rust (as the host user) BEFORE any docker run, so
    // Docker never creates it root-owned.
    fs::create_dir_all(&settings.home_dir)?;

    let seed = format!("{}-seed", settings.container);
    let _ = process::run(DOCKER, ["rm", "-f", seed.as_str()], verbose); // clear any stale seed
    process::run(DOCKER, ["create", "--name", seed.as_str(), settings.image.as_str()], verbose)?;

    let src = format!("{}:{}/.", seed, settings.container_home()); // trailing /. copies contents
    let dst = settings.home_dir.to_string_lossy().into_owned();
    let copied = process::run(DOCKER, ["cp", "-a", src.as_str(), dst.as_str()], verbose);
    let _ = process::run(DOCKER, ["rm", "-f", seed.as_str()], verbose); // always clean up

    copied?;
    fs::write(settings.home_dir.join(SEED_MARKER), b"")?;
    Ok(())
}

/// Delete the host home dir (reset). Tries a plain recursive remove first
/// (works when the files are owned by the host user, i.e. uid 1000 == 1000).
/// If that hits a permission error (container wrote files as a uid the host
/// user can't unlink), fall back to removing it from inside a container that
/// mounts the parent dir, so it deletes the whole `home` subtree as root.
pub fn remove_home(dir: &Path, image: &str, verbose: bool) -> Result<()> {
    match fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            let parent = dir.parent().ok_or(BsdevError::NoHome)?;
            let name = dir.file_name().and_then(|s| s.to_str()).ok_or(BsdevError::NoHome)?;
            let mount = format!("{}:/p", parent.display());
            let target = format!("/p/{name}");
            process::run(
                DOCKER,
                ["run", "--rm", "-v", mount.as_str(), image, "rm", "-rf", target.as_str()],
                verbose,
            )?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

/// Build the `docker run` argument vector. Pure so it can be unit-tested without
/// a Docker daemon. The public key is injected via an env var (read in Rust, not
/// via a shell `cat`) so this stays cross-platform.
pub fn run_args(settings: &Settings, authorized_key: &str) -> Vec<String> {
    vec![
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
        "-e".to_string(),
        format!("BSDEV_AUTHORIZED_KEY={authorized_key}"),
        settings.image.clone(),
    ]
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
            home_dir: PathBuf::from("/state/bsdev/home"),
            port: 2222,
            user: "bsdev".to_string(),
            key_path: PathBuf::from("/state/bsdev/id_ed25519"),
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
        assert!(has_pair(&args, "-v", "/state/bsdev/home:/home/bsdev"));
        assert!(has_pair(&args, "-e", "BSDEV_AUTHORIZED_KEY=ssh-ed25519 AAAA test"));
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
    fn needs_seed_when_dir_missing() {
        assert!(needs_seed(false, true, false));
    }

    #[test]
    fn needs_seed_when_dir_empty() {
        assert!(needs_seed(true, true, false));
    }

    #[test]
    fn needs_seed_when_marker_absent() {
        // Non-empty but no completion marker: a partial/interrupted seed.
        assert!(needs_seed(true, false, false));
    }

    #[test]
    fn does_not_need_seed_when_marker_present() {
        assert!(!needs_seed(true, false, true));
    }
}

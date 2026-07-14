use std::path::PathBuf;

use directories::ProjectDirs;

use crate::error::{BsdevError, Result};

/// Runtime configuration. Sensible constants, each overridable via a `BSDEV_*`
/// environment variable so the tool can be pointed at a different image, port,
/// etc. without rebuilding.
#[derive(Debug, Clone)]
pub struct Settings {
    /// GHCR image reference to pull and run.
    pub image: String,
    /// Container (and hostname) name.
    pub container: String,
    /// Named volume mounted at the container's home directory.
    pub volume: String,
    /// Optional host directory bind-mounted at `~/host-repos`, so code
    /// changes made in the container are reachable from the host (e.g. for
    /// running integration tests in host VMs). Only mounted when set - there
    /// is no default, since a plain host bind mount can't hold Unix symlinks
    /// on Windows (a repo with symlinks needs a WSL2/ext4 path instead).
    pub repos_dir: Option<PathBuf>,
    /// Host port forwarded to the container's sshd (published on 127.0.0.1).
    pub port: u16,
    /// Login user inside the container.
    pub user: String,
    /// Private key used to reach the container. Lives in bsdev's own state dir
    /// (not ~/.ssh) - like Vagrant keeping its key under .vagrant.
    pub key_path: PathBuf,
    /// Hostname of the machine bsdev is launched from, passed into the
    /// container so it can tell which host it's attached to.
    pub host_hostname: String,
}

impl Settings {
    /// Build settings from constants + `BSDEV_*` overrides.
    pub fn load() -> Result<Self> {
        let dirs = ProjectDirs::from("", "", "bsdev").ok_or(BsdevError::NoHome)?;
        // Machine-local state dir (e.g. ~/.local/share/bsdev, %LOCALAPPDATA%\bsdev\data).
        let state = dirs.data_local_dir().to_path_buf();
        Ok(Self {
            image: env_or("BSDEV_IMAGE", "ghcr.io/brownserve-uk/bsdev:latest"),
            container: env_or("BSDEV_CONTAINER", "bsdev"),
            volume: "bsdev-home".to_string(),
            repos_dir: std::env::var("BSDEV_REPOS").ok().map(PathBuf::from),
            port: env_or("BSDEV_PORT", "2222").parse().unwrap_or(2222),
            user: env_or("BSDEV_USER", "bsdev"),
            key_path: state.join("id_ed25519"),
            host_hostname: std::env::var("BSDEV_HOST_HOSTNAME").unwrap_or_else(|_| {
                hostname::get()
                    .map(|h| h.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| "unknown".to_string())
            }),
        })
    }

    /// The container's home directory (where the host bind mount is mounted).
    pub fn container_home(&self) -> String {
        format!("/home/{}", self.user)
    }

    /// The `-v` "source:target" spec for the home volume mount.
    pub fn home_mount(&self) -> String {
        format!("{}:{}", self.volume, self.container_home())
    }

    /// The `-v` "source:target" spec for the optional repos bind mount, if
    /// `BSDEV_REPOS` is set.
    pub fn repos_mount(&self) -> Option<String> {
        self.repos_dir
            .as_ref()
            .map(|d| format!("{}:{}/host-repos", d.display(), self.container_home()))
    }

    /// Path to the public half of `key_path` (`<key_path>.pub`).
    pub fn pubkey_path(&self) -> PathBuf {
        let mut p = self.key_path.clone();
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("id_ed25519")
            .to_string();
        p.set_file_name(format!("{name}.pub"));
        p
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

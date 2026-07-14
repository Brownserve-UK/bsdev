use thiserror::Error;

/// Errors surfaced by the core orchestration logic.
#[derive(Debug, Error)]
pub enum BsdevError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("`{0}` was not found on PATH - is it installed?")]
    CommandNotFound(String),

    #[error("`{cmd}` exited with status {code}")]
    CommandFailed { cmd: String, code: String },

    #[error("Docker is not available: {0}")]
    DockerUnavailable(String),

    #[error(
        "could not pull image `{0}` - is it published, and are you logged in to the registry (try `docker login ghcr.io`)?"
    )]
    ImagePull(String),

    #[error("could not determine your home directory")]
    NoHome,

    #[error("could not parse config file `{path}`: {source}")]
    Config { path: std::path::PathBuf, source: serde_json::Error },
}

pub type Result<T> = std::result::Result<T, BsdevError>;

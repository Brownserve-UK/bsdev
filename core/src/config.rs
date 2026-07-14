use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{BsdevError, Result};

/// Name of the JSON file (under the state dir) that persists user settings
/// across runs, so they don't need setting via env var every time.
const CONFIG_FILE: &str = "config.json";

/// Persisted bsdev settings. Every field is optional so the file can grow
/// without breaking older configs (missing keys just deserialize to `None`),
/// and an absent/empty file is equivalent to all-defaults.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Host directory bind-mounted at `~/host-repos` (see `Settings::repos_mount`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repos_dir: Option<PathBuf>,
}

impl Config {
    /// Load the config from `state_dir`, or `Config::default()` if it doesn't exist.
    pub fn load(state_dir: &Path) -> Result<Self> {
        let path = state_dir.join(CONFIG_FILE);
        match fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).map_err(|source| BsdevError::Config { path, source }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Write the config to `state_dir`, creating it if necessary.
    pub fn save(&self, state_dir: &Path) -> Result<()> {
        fs::create_dir_all(state_dir)?;
        let path = state_dir.join(CONFIG_FILE);
        let json = serde_json::to_string_pretty(self).map_err(|source| BsdevError::Config { path: path.clone(), source })?;
        fs::write(path, json)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bsdev-config-test-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn load_missing_file_is_default() {
        let dir = temp_state_dir("missing");
        assert_eq!(Config::load(&dir).unwrap(), Config::default());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = temp_state_dir("roundtrip");
        let config = Config { repos_dir: Some(PathBuf::from("/some/host/path")) };
        config.save(&dir).unwrap();
        assert_eq!(Config::load(&dir).unwrap(), config);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_tolerates_unknown_keys() {
        let dir = temp_state_dir("forward-compat");
        fs::write(dir.join(CONFIG_FILE), r#"{"repos_dir":"/x","future_setting":42}"#).unwrap();
        assert_eq!(Config::load(&dir).unwrap().repos_dir, Some(PathBuf::from("/x")));
        fs::remove_dir_all(&dir).ok();
    }
}

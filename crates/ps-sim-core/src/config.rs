//! Loading of TOML / RON data files with explicit errors.

use serde::de::DeserializeOwned;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid TOML in {path}: {source}")]
    Toml {
        path: PathBuf,
        source: Box<toml::de::Error>,
    },
    #[error("invalid RON in {path}: {source}")]
    Ron {
        path: PathBuf,
        source: Box<ron::error::SpannedError>,
    },
    #[error("invalid value in {path}: {msg}")]
    Invalid { path: PathBuf, msg: String },
}

pub fn load_toml<T: DeserializeOwned>(path: &Path) -> Result<T, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.into(),
        source,
    })?;
    toml::from_str(&text).map_err(|source| ConfigError::Toml {
        path: path.into(),
        source: Box::new(source),
    })
}

pub fn parse_toml<T: DeserializeOwned>(text: &str, name: &str) -> Result<T, ConfigError> {
    toml::from_str(text).map_err(|source| ConfigError::Toml {
        path: name.into(),
        source: Box::new(source),
    })
}

pub fn load_ron<T: DeserializeOwned>(path: &Path) -> Result<T, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.into(),
        source,
    })?;
    parse_ron(&text, &path.to_string_lossy())
}

pub fn parse_ron<T: DeserializeOwned>(text: &str, name: &str) -> Result<T, ConfigError> {
    ron::from_str(text).map_err(|source| ConfigError::Ron {
        path: name.into(),
        source: Box::new(source),
    })
}

/// Finds the data directory (containing `configs/` and `maps/`).
///
/// The data is the root of a Project-s-content checkout or release. Order:
/// `PS_CONTENT_DIR` (or the older `PROJET_S_DATA`) env var, then the executable
/// directory, its parents and the current directory, each also probed for a
/// sibling `Project-s-content/` (multi-repository development layout).
pub fn find_data_dir() -> PathBuf {
    for var in ["PS_CONTENT_DIR", "PROJET_S_DATA"] {
        if let Ok(p) = std::env::var(var)
            && !p.is_empty()
        {
            return PathBuf::from(p);
        }
    }
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        let mut d = exe.parent().map(Path::to_path_buf);
        while let Some(dir) = d {
            candidates.push(dir.clone());
            d = dir.parent().map(Path::to_path_buf);
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd);
    }
    for c in &candidates {
        for dir in [c.clone(), c.join("Project-s-content")] {
            if dir.join("configs").is_dir() && dir.join("maps").is_dir() {
                return dir;
            }
        }
    }
    PathBuf::from(".")
}

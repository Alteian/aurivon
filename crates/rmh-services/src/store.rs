use serde::{de::DeserializeOwned, Serialize};
use std::path::{Path, PathBuf};

/// A token file (mode 0600). `Debug` never prints the value.
#[derive(Clone)]
pub struct Secret {
    pub path: PathBuf,
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Secret").field("path", &self.path).finish_non_exhaustive()
    }
}

impl Secret {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// ~/.config/aurivon/<name>.json
    pub fn default_path(name: &str) -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("aurivon").join(format!("{name}.json")))
    }

    pub fn load<T: DeserializeOwned>(&self) -> Option<T> {
        serde_json::from_str(&std::fs::read_to_string(&self.path).ok()?).ok()
    }

    pub fn save<T: Serialize>(&self, value: &T) -> anyhow::Result<()> {
        write_private(&self.path, &serde_json::to_vec_pretty(value)?)
    }

    pub fn clear(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub(crate) fn write_private(path: &Path, data: &[u8]) -> anyhow::Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut f = std::fs::OpenOptions::new().create(true).write(true).truncate(true).mode(0o600).open(path)?;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        f.write_all(data)?;
    }
    #[cfg(not(unix))]
    std::fs::write(path, data)?;
    Ok(())
}

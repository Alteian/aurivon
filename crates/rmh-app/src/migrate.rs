//! One-time move of user files from the old `rustmediahub` (and short-lived
//! `lyrion`) directories to `aurivon` (config, library database, caches, logs) so nothing is lost after
//! the rename. Runs at startup before anything opens those files.

use std::path::{Path, PathBuf};

pub const APP_ID: &str = "aurivon";
/// Earlier names of the app, oldest first
pub const LEGACY_IDS: &[&str] = &["rustmediahub", "lyrion"];

/// What was moved, for logging (paths only, never file contents)
#[derive(Debug, Default)]
pub struct MigrationReport {
    pub moved: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
}

fn bases() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = [dirs::config_dir(), dirs::data_dir(), dirs::data_local_dir(), dirs::cache_dir(), dirs::state_dir()]
        .into_iter()
        .flatten()
        .collect();
    v.dedup();
    v
}

/// Move `<base>/rustmediahub/*` into `<base>/aurivon/` for every XDG base dir
pub fn migrate_legacy_dirs() -> MigrationReport {
    let mut report = MigrationReport::default();
    for base in bases() {
        for legacy in LEGACY_IDS {
            migrate_dir(&base.join(legacy), &base.join(APP_ID), &mut report);
        }
    }
    report
}

/// Move entries of `old` into `new` that don't exist there yet. The old
/// install tree (`app/`, managed by install.sh) is left for the installer.
pub fn migrate_dir(old: &Path, new: &Path, report: &mut MigrationReport) {
    let Ok(entries) = std::fs::read_dir(old) else { return };
    if std::fs::create_dir_all(new).is_err() {
        return;
    }
    for e in entries.flatten() {
        let name = e.file_name();
        if name == "app" {
            continue;
        }
        let dest = new.join(&name);
        if dest.exists() {
            report.skipped.push(e.path());
            continue;
        }
        if std::fs::rename(e.path(), &dest).is_ok() {
            report.moved.push(dest);
        } else {
            report.skipped.push(e.path());
        }
    }
    // remove the old directory once it's empty
    let _ = std::fs::remove_dir(old);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_entries_without_overwriting() {
        let t = tempfile::tempdir().unwrap();
        let old = t.path().join(LEGACY_IDS[0]);
        let new = t.path().join(APP_ID);
        std::fs::create_dir_all(old.join("images")).unwrap();
        std::fs::create_dir_all(old.join("app/versions")).unwrap();
        std::fs::write(old.join("library.db"), "db").unwrap();
        std::fs::write(old.join("config.json"), "old").unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("config.json"), "new").unwrap();
        let mut r = MigrationReport::default();
        migrate_dir(&old, &new, &mut r);
        assert_eq!(std::fs::read_to_string(new.join("library.db")).unwrap(), "db");
        assert!(new.join("images").is_dir());
        assert_eq!(std::fs::read_to_string(new.join("config.json")).unwrap(), "new");
        assert!(old.join("config.json").exists()); // kept, not overwritten
        assert!(old.join("app").exists() && !new.join("app").exists());
    }
}

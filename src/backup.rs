//! Versioned user-data backup envelope. Callers provide only non-secret data.
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const MAX_BACKUP_BYTES: usize = 8 * 1024 * 1024;
const RECOVERY_DIR: &str = ".fasttube-import-recovery";
const RECOVERY_MARKER: &str = "incomplete";
#[derive(Clone, Debug, PartialEq)]
pub struct Backup {
    pub library: serde_json::Value,
    pub queue: serde_json::Value,
    pub history: serde_json::Value,
    pub preferences: serde_json::Value,
}
pub fn encode(backup: &Backup) -> io::Result<Vec<u8>> {
    let bytes = serde_json::to_vec(
        &serde_json::json!({"format":"fasttube-backup","version":1,"library":backup.library,"queue":backup.queue,"history":backup.history,"preferences":backup.preferences}),
    )?;
    if bytes.len() > MAX_BACKUP_BYTES {
        return Err(invalid("backup exceeds 8 MiB"));
    }
    Ok(bytes)
}
pub fn decode(bytes: &[u8]) -> io::Result<Backup> {
    if bytes.len() > MAX_BACKUP_BYTES {
        return Err(invalid("backup exceeds 8 MiB"));
    }
    let root: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| invalid("invalid backup JSON"))?;
    if root.get("format").and_then(|v| v.as_str()) != Some("fasttube-backup")
        || root.get("version").and_then(|v| v.as_u64()) != Some(1)
    {
        return Err(invalid("unsupported backup format"));
    }
    for field in ["library", "queue", "history", "preferences"] {
        if !root.get(field).is_some_and(|v| v.is_object()) {
            return Err(invalid("missing backup data"));
        }
    }
    Ok(Backup {
        library: root["library"].clone(),
        queue: root["queue"].clone(),
        history: root["history"].clone(),
        preferences: root["preferences"].clone(),
    })
}
pub fn save(path: &Path, backup: &Backup) -> io::Result<()> {
    let bytes = encode(backup)?;
    crate::history::write_atomic(path, &bytes)
}

/// Snapshot of every local data file touched by a multi-file import.
///
/// The marker is written last. If the process stops before commit, the next
/// startup can restore all files to their pre-import contents.
pub struct ImportRestorePoint {
    root: PathBuf,
    paths: Vec<(String, PathBuf)>,
    completed: bool,
}

impl ImportRestorePoint {
    pub fn create(root: &Path, paths: &[(&str, &Path)]) -> io::Result<Self> {
        let directory = root.join(RECOVERY_DIR);
        if directory.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "já existe um ponto de restauração de importação",
            ));
        }
        let result = (|| -> io::Result<()> {
            fs::create_dir_all(&directory)?;
            for (name, path) in paths {
                validate_data_file(root, path)?;
                if path.exists() {
                    let bytes = fs::read(path)?;
                    crate::history::write_atomic(&directory.join(recovery_file(name)), &bytes)?;
                }
            }
            crate::history::write_atomic(&directory.join(RECOVERY_MARKER), b"fasttube-import-v1")
        })();
        if let Err(error) = result {
            let _ = fs::remove_dir_all(&directory);
            return Err(error);
        }
        Ok(Self {
            root: root.to_path_buf(),
            paths: paths
                .iter()
                .map(|(name, path)| ((*name).to_owned(), (*path).to_path_buf()))
                .collect(),
            completed: false,
        })
    }

    pub fn restore(&mut self) -> io::Result<bool> {
        if self.completed {
            return Ok(false);
        }
        let restored = restore_from(&self.root, &self.paths)?;
        self.completed = true;
        Ok(restored)
    }

    pub fn commit(mut self) -> io::Result<()> {
        let directory = self.root.join(RECOVERY_DIR);
        fs::remove_dir_all(&directory)?;
        self.completed = true;
        Ok(())
    }
}

impl Drop for ImportRestorePoint {
    fn drop(&mut self) {
        if !self.completed {
            let _ = self.restore();
        }
    }
}

/// Restores an interrupted import before normal application data is loaded.
pub fn recover_pending_import(root: &Path, paths: &[(&str, &Path)]) -> io::Result<bool> {
    let directory = root.join(RECOVERY_DIR);
    if !directory.join(RECOVERY_MARKER).is_file() {
        return Ok(false);
    }
    let owned: Vec<_> = paths
        .iter()
        .map(|(name, path)| ((*name).to_owned(), (*path).to_path_buf()))
        .collect();
    restore_from(root, &owned).map(|_| true)
}

fn restore_from(root: &Path, paths: &[(String, PathBuf)]) -> io::Result<bool> {
    let directory = root.join(RECOVERY_DIR);
    if !directory.join(RECOVERY_MARKER).is_file() {
        return Ok(false);
    }
    for (name, path) in paths {
        validate_data_file(root, path)?;
        let backup = directory.join(recovery_file(name));
        if path.exists() {
            fs::remove_file(path)?;
        }
        if backup.exists() {
            let bytes = fs::read(&backup)?;
            crate::history::write_atomic(path, &bytes)?;
        }
    }
    fs::remove_dir_all(directory)?;
    Ok(true)
}

fn validate_data_file(root: &Path, path: &Path) -> io::Result<()> {
    if path.parent() != Some(root) || path.file_name().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "arquivo de dados fora da pasta de recuperação",
        ));
    }
    Ok(())
}

fn recovery_file(name: &str) -> String {
    format!("{name}.bin")
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versioned_round_trip_and_invalid_input() {
        let b = Backup {
            library: serde_json::json!({}),
            queue: serde_json::json!({}),
            history: serde_json::json!({}),
            preferences: serde_json::json!({}),
        };
        assert_eq!(decode(&encode(&b).unwrap()).unwrap(), b);
        assert!(decode(b"{}").is_err());
        assert!(decode(&vec![0; MAX_BACKUP_BYTES + 1]).is_err());
    }

    #[test]
    fn restore_point_rolls_back_existing_and_removes_new_files() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "fasttube-restore-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let existing = root.join("library.json");
        let created = root.join("history.json");
        fs::write(&existing, b"before").unwrap();
        let refs = [
            ("library", existing.as_path()),
            ("history", created.as_path()),
        ];
        let mut point = ImportRestorePoint::create(&root, &refs).unwrap();
        fs::write(&existing, b"partial").unwrap();
        fs::write(&created, b"new").unwrap();
        assert!(point.restore().unwrap());
        assert!(!point.restore().unwrap());
        assert_eq!(fs::read(&existing).unwrap(), b"before");
        assert!(!created.exists());
        assert!(!root.join(RECOVERY_DIR).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn interrupted_import_recovers_original_file_at_startup() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "fasttube-recovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let data = root.join("queue.json");
        fs::write(&data, b"original").unwrap();
        let refs = [("queue", data.as_path())];
        let point = ImportRestorePoint::create(&root, &refs).unwrap();
        std::mem::forget(point);
        fs::write(&data, b"interrupted").unwrap();
        assert!(recover_pending_import(&root, &refs).unwrap());
        assert_eq!(fs::read(&data).unwrap(), b"original");
        assert!(!recover_pending_import(&root, &refs).unwrap());
        fs::remove_dir_all(root).unwrap();
    }
}

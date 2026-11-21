//! Atomic JSON file storage with debounced saves.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::sync::{Mutex, Notify};
use tokio::time::{sleep, Instant};

/// Load JSON from `path`. Missing file → `Ok(None)`. Corrupt → rename aside and `Ok(None)`.
pub fn load<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    match serde_json::from_slice(&bytes) {
        Ok(v) => Ok(Some(v)),
        Err(e) => {
            let broken = path.with_extension(format!(
                "broken-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
            ));
            let _ = fs::rename(path, &broken);
            eprintln!(
                "syncaidync: could not parse {}; moved to {} ({e})",
                path.display(),
                broken.display()
            );
            Ok(None)
        }
    }
}

/// Write `value` to `path` via temp file + fsync + rename.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let tmp = path.with_extension(format!("{}.tmp", crate::ids::random_hex(16)));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|e| format!("create {}: {e}", tmp.display()))?;
    let result = (|| {
        file.write_all(bytes)
            .map_err(|e| format!("write {}: {e}", tmp.display()))?;
        file.sync_all()
            .map_err(|e| format!("fsync {}: {e}", tmp.display()))?;
        drop(file);
        fs::rename(&tmp, path).map_err(|e| format!("rename {}: {e}", path.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

struct SaverInner<T> {
    path: PathBuf,
    pending: Option<T>,
    dirty_since: Option<Instant>,
}

/// Debounces writes of `T` to a JSON file (at most one write per 250 ms, plus flush on demand).
pub struct Saver<T: Serialize + Clone + Send + 'static> {
    inner: Arc<Mutex<SaverInner<T>>>,
    notify: Arc<Notify>,
}

impl<T: Serialize + Clone + Send + 'static> Saver<T> {
    pub fn new(path: PathBuf) -> Self {
        let inner = Arc::new(Mutex::new(SaverInner {
            path,
            pending: None,
            dirty_since: None,
        }));
        let notify = Arc::new(Notify::new());
        let inner_task = inner.clone();
        let notify_task = notify.clone();
        tokio::spawn(async move {
            loop {
                notify_task.notified().await;
                loop {
                    let wait = {
                        let g = inner_task.lock().await;
                        match g.dirty_since {
                            Some(since) => {
                                let elapsed = since.elapsed();
                                if elapsed >= Duration::from_millis(250) {
                                    None
                                } else {
                                    Some(Duration::from_millis(250) - elapsed)
                                }
                            }
                            None => break,
                        }
                    };
                    if let Some(d) = wait {
                        sleep(d).await;
                    } else {
                        flush_locked(&inner_task).await;
                        break;
                    }
                }
            }
        });
        Self { inner, notify }
    }

    pub async fn queue(&self, value: T) {
        {
            let mut g = self.inner.lock().await;
            g.pending = Some(value);
            if g.dirty_since.is_none() {
                g.dirty_since = Some(Instant::now());
            }
        }
        self.notify.notify_one();
    }

    pub async fn flush(&self) {
        flush_locked(&self.inner).await;
    }
}

async fn flush_locked<T: Serialize + Clone>(inner: &Mutex<SaverInner<T>>) {
    let (path, value) = {
        let mut g = inner.lock().await;
        g.dirty_since = None;
        match g.pending.take() {
            Some(v) => (g.path.clone(), v),
            None => return,
        }
    };
    match serde_json::to_vec_pretty(&value) {
        Ok(bytes) => {
            if let Err(e) = atomic_write(&path, &bytes) {
                eprintln!("syncaidync: save failed: {e}");
            }
        }
        Err(e) => eprintln!("syncaidync: encode failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct Doc {
        version: u32,
        n: u32,
    }

    #[test]
    fn load_missing_and_corrupt() {
        let dir = std::env::temp_dir().join(format!("store-{}", crate::ids::random_hex(4)));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.json");
        assert!(load::<Doc>(&path).unwrap().is_none());
        fs::write(&path, b"not-json").unwrap();
        assert!(load::<Doc>(&path).unwrap().is_none());
        assert!(dir.read_dir().unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("broken")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_round_trip() {
        let dir = std::env::temp_dir().join(format!("store-{}", crate::ids::random_hex(4)));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ok.json");
        let doc = Doc { version: 1, n: 7 };
        atomic_write(&path, &serde_json::to_vec(&doc).unwrap()).unwrap();
        let got: Doc = load(&path).unwrap().unwrap();
        assert_eq!(got, doc);
        let _ = fs::remove_dir_all(&dir);
    }
}

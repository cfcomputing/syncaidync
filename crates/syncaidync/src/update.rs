//! Daily release checks, cached in the app home.

use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use syncaidync_core::version;

const REMOTE: &str = "https://github.com/cfcomputing/syncaidync.git";
const DAY_MS: u64 = 86_400_000;

#[derive(Default)]
pub struct UpdateCache {
    record: tokio::sync::Mutex<Option<Record>>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct Record {
    checked_at: u64,
    latest: Option<String>,
}

#[derive(Serialize)]
pub struct Status {
    current: &'static str,
    latest: Option<String>,
    newer: bool,
    command: &'static str,
}

impl UpdateCache {
    pub async fn status(&self, home: &Path) -> Status {
        let latest = if std::env::var("SYNCAIDYNC_NO_UPDATE_CHECK").as_deref() == Ok("1") {
            None
        } else {
            self.latest(home).await
        };
        let current = env!("CARGO_PKG_VERSION");
        Status {
            current,
            newer: latest
                .as_deref()
                .is_some_and(|tag| version::is_newer(tag, current)),
            latest,
            command:
                "cargo install --git https://github.com/cfcomputing/syncaidync --locked syncaidync",
        }
    }

    async fn latest(&self, home: &Path) -> Option<String> {
        let path = home.join("config.json");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_millis() as u64;

        {
            let mut cache = self.record.lock().await;
            let record = cache.get_or_insert_with(|| load_record(&path));
            if record.checked_at != 0 && now.saturating_sub(record.checked_at) < DAY_MS {
                return record.latest.clone();
            }
        }

        // Fetch outside the lock so concurrent /update calls do not pile up.
        let fetched = fetch_latest().await;

        let mut cache = self.record.lock().await;
        let record = cache.get_or_insert_with(|| load_record(&path));
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_millis() as u64;
        if record.checked_at == 0 || now.saturating_sub(record.checked_at) >= DAY_MS {
            if let Some(latest) = fetched {
                record.latest = latest;
            }
            record.checked_at = now;
            save_record(&path, record);
        }
        record.latest.clone()
    }
}

fn load_record(path: &Path) -> Record {
    crate::store::load::<serde_json::Value>(path)
        .ok()
        .flatten()
        .and_then(|doc| serde_json::from_value(doc["update"].clone()).ok())
        .unwrap_or_default()
}

fn save_record(path: &Path, record: &Record) {
    let mut doc = crate::store::load::<serde_json::Value>(path)
        .ok()
        .flatten()
        .unwrap_or_else(|| serde_json::json!({ "version": 1 }));
    if !doc.is_object() {
        doc = serde_json::json!({ "version": 1 });
    }
    if let Ok(update) = serde_json::to_value(record) {
        doc["update"] = update;
        if doc.get("version").is_none() {
            doc["version"] = serde_json::json!(1);
        }
        if let Ok(data) = serde_json::to_vec_pretty(&doc) {
            let _ = crate::store::atomic_write(path, &data);
        }
    }
}

async fn fetch_latest() -> Option<Option<String>> {
    let remote = std::env::var("SYNCAIDYNC_UPDATE_REMOTE").unwrap_or_else(|_| REMOTE.into());
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new("git")
            .env_remove("SYNCAIDYNC_KEY")
            .args(["ls-remote", "--tags", "--refs", "--", &remote])
            .env("GIT_TERMINAL_PROMPT", "0")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.split_once('\t')?.1.strip_prefix("refs/tags/"))
            .filter_map(|tag| Some((version::parse(tag)?, tag)))
            .max()
            .map(|(_, tag)| tag.to_string()),
    )
}

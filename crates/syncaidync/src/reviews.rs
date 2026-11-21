//! Persist reviews.json keyed by `s:<session>` or `p:<pull>`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use syncaidync_core::{ReadPlan, Review, MAX_NOTES, MAX_NOTE_CHARS, MAX_OVERALL_CHARS};
use tokio::sync::Mutex;

use crate::store::{self, Saver};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReviewsFile {
    version: u32,
    reviews: BTreeMap<String, Review>,
}

pub struct Reviews {
    inner: Arc<Mutex<BTreeMap<String, Review>>>,
    saver: Saver<ReviewsFile>,
}

impl Reviews {
    pub fn load(home: &std::path::Path) -> Self {
        let path = home.join("reviews.json");
        let map = match store::load::<ReviewsFile>(&path) {
            Ok(Some(f)) => f.reviews,
            _ => BTreeMap::new(),
        };
        let saver = Saver::new(path);
        Self {
            inner: Arc::new(Mutex::new(map)),
            saver,
        }
    }

    pub fn session_key(id: &str) -> String {
        format!("s:{id}")
    }

    pub fn pull_key(id: &str) -> String {
        format!("p:{id}")
    }

    pub async fn get(&self, key: &str) -> Review {
        let guard = self.inner.lock().await;
        guard.get(key).cloned().unwrap_or_else(|| Review::empty(0))
    }

    pub async fn put_session(
        &self,
        session_id: &str,
        reviewed: BTreeMap<String, String>,
        notes: Vec<syncaidync_core::ReviewNote>,
        overall: String,
        now_ms: u64,
    ) -> Result<Review, String> {
        self.put_key(
            &Self::session_key(session_id),
            reviewed,
            notes,
            overall,
            now_ms,
        )
        .await
    }

    pub async fn put_pull(
        &self,
        pull_id: &str,
        reviewed: BTreeMap<String, String>,
        notes: Vec<syncaidync_core::ReviewNote>,
        overall: String,
        now_ms: u64,
    ) -> Result<Review, String> {
        self.put_key(&Self::pull_key(pull_id), reviewed, notes, overall, now_ms)
            .await
    }

    async fn put_key(
        &self,
        key: &str,
        reviewed: BTreeMap<String, String>,
        notes: Vec<syncaidync_core::ReviewNote>,
        overall: String,
        now_ms: u64,
    ) -> Result<Review, String> {
        if notes.len() > MAX_NOTES {
            return Err(format!("At most {MAX_NOTES} notes."));
        }
        for n in &notes {
            if n.text.chars().count() > MAX_NOTE_CHARS {
                return Err(format!(
                    "Each note can be at most {MAX_NOTE_CHARS} characters."
                ));
            }
        }
        if overall.chars().count() > MAX_OVERALL_CHARS {
            return Err(format!(
                "The overall note can be at most {MAX_OVERALL_CHARS} characters."
            ));
        }
        let mut guard = self.inner.lock().await;
        let plan = guard.get(key).and_then(|r| r.plan.clone());
        let review = Review {
            reviewed,
            notes,
            overall,
            plan,
            updated_at: now_ms,
        };
        guard.insert(key.to_string(), review.clone());
        let file = ReviewsFile {
            version: 1,
            reviews: guard.clone(),
        };
        drop(guard);
        self.saver.queue(file).await;
        Ok(review)
    }

    pub async fn set_plan(&self, session_id: &str, plan: ReadPlan, now_ms: u64) -> Review {
        self.set_plan_key(&Self::session_key(session_id), plan, now_ms)
            .await
    }

    pub async fn set_plan_pull(&self, pull_id: &str, plan: ReadPlan, now_ms: u64) -> Review {
        self.set_plan_key(&Self::pull_key(pull_id), plan, now_ms)
            .await
    }

    async fn set_plan_key(&self, key: &str, plan: ReadPlan, now_ms: u64) -> Review {
        let mut guard = self.inner.lock().await;
        let mut review = guard
            .get(key)
            .cloned()
            .unwrap_or_else(|| Review::empty(now_ms));
        review.plan = Some(plan);
        review.updated_at = now_ms;
        guard.insert(key.to_string(), review.clone());
        let file = ReviewsFile {
            version: 1,
            reviews: guard.clone(),
        };
        drop(guard);
        self.saver.queue(file).await;
        review
    }

    pub async fn mark_notes_sent(&self, key: &str, ids: &[String], now_ms: u64) {
        let set: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
        let mut guard = self.inner.lock().await;
        let Some(review) = guard.get_mut(key) else {
            return;
        };
        for n in &mut review.notes {
            if set.contains(n.id.as_str()) {
                n.sent = true;
            }
        }
        review.updated_at = now_ms;
        let file = ReviewsFile {
            version: 1,
            reviews: guard.clone(),
        };
        drop(guard);
        self.saver.queue(file).await;
    }

    pub async fn remove_session(&self, session_id: &str) {
        self.remove_key(&Self::session_key(session_id)).await;
    }

    pub async fn remove_pull(&self, pull_id: &str) {
        self.remove_key(&Self::pull_key(pull_id)).await;
    }

    async fn remove_key(&self, key: &str) {
        let mut guard = self.inner.lock().await;
        if guard.remove(key).is_some() {
            let file = ReviewsFile {
                version: 1,
                reviews: guard.clone(),
            };
            drop(guard);
            self.saver.queue(file).await;
        }
    }

    pub async fn flush(&self) {
        self.saver.flush().await;
    }
}

#[allow(dead_code)]
pub fn path_in(home: PathBuf) -> PathBuf {
    home.join("reviews.json")
}

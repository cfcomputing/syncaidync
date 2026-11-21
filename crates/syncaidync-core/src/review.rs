//! Review notes and reviewed marks for a ChangeSet.

use serde::{Deserialize, Serialize};

use crate::readplan::ReadPlan;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewNote {
    pub id: String,
    pub path: String,
    pub side: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<u32>,
    #[serde(default)]
    pub quote: String,
    pub text: String,
    pub sent: bool,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Review {
    #[serde(default)]
    pub reviewed: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub notes: Vec<ReviewNote>,
    #[serde(default)]
    pub overall: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<ReadPlan>,
    pub updated_at: u64,
}

impl Review {
    pub fn empty(now_ms: u64) -> Self {
        Self {
            reviewed: std::collections::BTreeMap::new(),
            notes: Vec::new(),
            overall: String::new(),
            plan: None,
            updated_at: now_ms,
        }
    }
}

pub const MAX_NOTES: usize = 500;
pub const MAX_NOTE_CHARS: usize = 4_000;
pub const MAX_OVERALL_CHARS: usize = 10_000;

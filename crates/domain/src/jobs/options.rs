use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn default_concurrency() -> u32 {
    1
}

/// Immutable options recorded with a task. No IO or protocol dependency is required.
/// Unknown fields are retained so a migration never silently drops future options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrozenJobOptions {
    pub output_root: String,
    pub conflict_policy: String,
    pub password_group: String,
    pub cache_enabled: bool,
    pub cache_input: bool,
    pub cache_root: String,
    pub cache_quota_bytes: u64,
    pub min_free_bytes: u64,
    pub expand_nested: bool,
    pub max_depth: u32,
    pub verification: String,
    pub max_entries: u64,
    pub max_expanded_bytes: u64,
    pub recursive_scan: bool,
    pub input_exclusions: Vec<String>,
    pub output_exclusions: Vec<String>,
    pub small_first: bool,
    pub theme: String,
    pub history_limit: usize,
    pub always_on_top: bool,
    pub completion_sound: bool,
    pub minimize_to_tray: bool,
    pub unwrap_single_root: bool,
    pub source_cleanup: String,
    #[serde(default = "default_concurrency")]
    pub concurrency: u32,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, serde_json::Value>,
}

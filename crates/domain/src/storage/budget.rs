use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceBudget {
    pub max_entries: u64,
    pub max_total_bytes: u64,
    pub max_written_bytes: u64,
    pub min_free_bytes: u64,
    pub decoder_memory_limit_bytes: u64,
    pub max_batch_candidates: usize,
}

impl Default for ResourceBudget {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            max_total_bytes: 100 * 1024 * 1024 * 1024,
            max_written_bytes: 100 * 1024 * 1024 * 1024,
            min_free_bytes: 1024 * 1024 * 1024,
            decoder_memory_limit_bytes: 2 * 1024 * 1024 * 1024,
            max_batch_candidates: 128,
        }
    }
}

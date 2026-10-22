//! Owns cache quota reservations independently of job and scheduler state.

use crate::{paths, route::ExtractionRoute, types::Settings, worker::WorkerClient};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use super::lock_error;

#[derive(Debug)]
pub(super) struct BudgetError {
    pub code: String,
    pub pause_batch: bool,
}

impl BudgetError {
    fn new(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            pause_batch: false,
        }
    }
}

/// A claim only needs to know whether its writer has exited. Implementations
/// must not call back into the coordinator or acquire job/snapshot locks.
pub(super) trait BudgetExitPolicy: Send + Sync {
    fn retain_if_unconfirmed(&self, job_id: &str) -> bool;
}

/// Shares the existing request registrations and receipt source, rather than
/// maintaining another copy of worker state that could become stale.
pub(super) struct RequestBudgetOwner {
    in_flight: Arc<Mutex<HashSet<String>>>,
    request_ids: Arc<Mutex<HashMap<String, String>>>,
    worker: WorkerClient,
    stop_batch: Arc<AtomicBool>,
}

impl RequestBudgetOwner {
    pub fn new(
        in_flight: Arc<Mutex<HashSet<String>>>,
        request_ids: Arc<Mutex<HashMap<String, String>>>,
        worker: WorkerClient,
        stop_batch: Arc<AtomicBool>,
    ) -> Self {
        Self {
            in_flight,
            request_ids,
            worker,
            stop_batch,
        }
    }
}

impl BudgetExitPolicy for RequestBudgetOwner {
    fn retain_if_unconfirmed(&self, job_id: &str) -> bool {
        if !self
            .in_flight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(job_id)
        {
            return false;
        }
        let ids = self
            .request_ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let unconfirmed = ids
            .get(job_id)
            .and_then(|id| self.worker.request_exit_receipt(id))
            .is_none_or(|receipt| !receipt.exit_confirmed);
        if unconfirmed {
            self.stop_batch.store(true, Ordering::Release);
        }
        unconfirmed
    }
}

pub(super) struct CacheBudgetManager {
    reservations: Arc<Mutex<HashMap<PathBuf, u64>>>,
    unknown_owners: Mutex<HashMap<PathBuf, String>>,
    exit_policy: Arc<dyn BudgetExitPolicy>,
}

impl CacheBudgetManager {
    pub fn new(exit_policy: Arc<dyn BudgetExitPolicy>) -> Self {
        Self {
            reservations: Arc::new(Mutex::new(HashMap::new())),
            unknown_owners: Mutex::new(HashMap::new()),
            exit_policy,
        }
    }

    /// Direct work uses its volume's free space. Cache work additionally reserves
    /// quota for the entire phase, including failure and cancellation paths.
    pub fn claim(
        &self,
        settings: &Settings,
        route: &ExtractionRoute,
        parent: &Path,
        estimated_needed: Option<u64>,
    ) -> Result<BudgetClaim, BudgetError> {
        let free = paths::free_bytes(parent)
            .map_err(|_| BudgetError::new("SharedResourceUnavailable"))?
            .saturating_sub(settings.min_free_bytes);
        if free == 0 {
            return Err(BudgetError {
                code: "SharedResourceUnavailable".into(),
                pause_batch: true,
            });
        }
        let (reservation, budget) = match route {
            ExtractionRoute::Direct { .. } => {
                let requested = estimated_needed
                    .unwrap_or(settings.max_expanded_bytes)
                    .min(settings.max_expanded_bytes);
                (None, requested.min(free))
            }
            ExtractionRoute::SsdCache { .. } => {
                let root = fs::canonicalize(settings.cache_root.trim())
                    .map_err(|_| BudgetError::new("SharedResourceUnavailable"))?;
                let mut ledger = self
                    .reservations
                    .lock()
                    .map_err(|_| BudgetError::new(lock_error()))?;
                // Materialized files and active reservations are both counted.
                // The visible usage projection is not a quota authority.
                let used = paths::tree_size(&root).map_err(BudgetError::new)?;
                let reserved = ledger.get(&root).copied().unwrap_or(0);
                let quota_left = settings
                    .cache_quota_bytes
                    .saturating_sub(used)
                    .saturating_sub(reserved);
                if estimated_needed.is_some_and(|needed| {
                    needed > settings.max_expanded_bytes || needed > settings.cache_quota_bytes
                }) {
                    return Err(BudgetError::new("ResourceLimit"));
                }
                // Unknown archives need the available bounded envelope after
                // other phases finish, without a guessed expansion ratio.
                if estimated_needed.is_none() && reserved > 0 {
                    return Err(BudgetError::new("CacheQuotaContention"));
                }
                let requested = estimated_needed
                    .unwrap_or_else(|| settings.max_expanded_bytes.min(free).min(quota_left));
                if requested == 0
                    || requested > settings.cache_quota_bytes
                    || requested > settings.max_expanded_bytes
                {
                    return Err(BudgetError::new("ResourceLimit"));
                }
                if quota_left < requested || free < requested {
                    return Err(BudgetError::new(if reserved > 0 {
                        "CacheQuotaContention"
                    } else {
                        "ResourceLimit"
                    }));
                }
                ledger.insert(root.clone(), reserved.saturating_add(requested));
                (Some((root, requested)), requested)
            }
        };
        Ok(BudgetClaim {
            budget,
            reservation,
            ledger: self.reservations.clone(),
            owner: None,
        })
    }

    pub fn claim_for_job(
        &self,
        settings: &Settings,
        route: &ExtractionRoute,
        parent: &Path,
        needed: Option<u64>,
        job_id: &str,
        live_cache_peer: bool,
    ) -> Result<BudgetClaim, BudgetError> {
        if matches!(route, ExtractionRoute::SsdCache { .. }) {
            let root = fs::canonicalize(settings.cache_root.trim())
                .map_err(|_| BudgetError::new("SharedResourceUnavailable"))?;
            let mut owners = self
                .unknown_owners
                .lock()
                .map_err(|_| BudgetError::new(lock_error()))?;
            if owners.get(&root).is_some_and(|owner| owner != job_id) {
                return Err(BudgetError::new("CacheQuotaContention"));
            }
            if needed.is_none() {
                // Ownership lasts until job cleanup, including a failed claim
                // and the gap between extraction phases and cache writeback.
                owners.insert(root, job_id.to_string());
            }
        }
        let mut claim = match self.claim(settings, route, parent, needed) {
            Ok(claim) => claim,
            Err(error)
                if error.code == "ResourceLimit"
                    && live_cache_peer
                    && needed.is_none_or(|bytes| {
                        bytes > 0
                            && bytes <= settings.cache_quota_bytes
                            && bytes <= settings.max_expanded_bytes
                    }) =>
            {
                return Err(BudgetError::new("CacheQuotaContention"));
            }
            Err(error) => return Err(error),
        };
        claim.owner = Some((self.exit_policy.clone(), job_id.to_string()));
        Ok(claim)
    }

    pub fn release_unknown_owner(&self, job_id: &str) {
        self.unknown_owners
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, owner| owner != job_id);
    }

    #[cfg(test)]
    pub fn reserved_bytes(&self) -> u64 {
        self.reservations.lock().unwrap().values().copied().sum()
    }
}

/// A phase-scoped claim, with no reference back to the job coordinator.
pub(super) struct BudgetClaim {
    budget: u64,
    reservation: Option<(PathBuf, u64)>,
    ledger: Arc<Mutex<HashMap<PathBuf, u64>>>,
    owner: Option<(Arc<dyn BudgetExitPolicy>, String)>,
}

impl BudgetClaim {
    pub fn budget(&self) -> u64 {
        self.budget
    }
}

impl Drop for BudgetClaim {
    fn drop(&mut self) {
        if let Some((owner, job_id)) = &self.owner
            && owner.retain_if_unconfirmed(job_id)
        {
            // Dropping a worker future only requests termination. Preserve its
            // quota while the process may still be writing cache files.
            return;
        }
        if let Some((root, amount)) = self.reservation.take()
            && let Ok(mut ledger) = self.ledger.lock()
            && let Some(total) = ledger.get_mut(&root)
        {
            *total = total.saturating_sub(amount);
            if *total == 0 {
                ledger.remove(&root);
            }
        }
    }
}

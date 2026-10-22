//! Opt-in, authenticated loopback API. The desktop host remains the only owner
//! of business state; this crate supplies transport and application grants.
pub mod command;
mod server;
pub mod store;

use std::sync::atomic::{AtomicBool, Ordering};

pub use command::{ApiCommand, FieldError, Scope};
pub use server::{ApiBackend, ApiError, BackendFuture, LocalApiServer};
pub use store::{
    ApiClient, ApiClientInput, ApiClientView, ApiSettings, ApiStatus, ApiStore, CreatedClient,
};

/// Explicit, process-local "trust every path" switch.
///
/// It is deliberately **not** read from the environment: an inherited
/// `OPENUNPACK_TRUSTED_ALL_PATHS` variable would let any parent process disable
/// every path-approval check simply by launching the app. The host must call
/// [`set_trusted_all_paths`] after parsing its own command line, and only in
/// headless mode.
static TRUSTED_ALL_PATHS: AtomicBool = AtomicBool::new(false);

/// Enables or disables the headless "trust all paths" mode for this process.
pub fn set_trusted_all_paths(enabled: bool) {
    TRUSTED_ALL_PATHS.store(enabled, Ordering::Release);
}

/// Returns whether the current process explicitly opted into trusting all paths.
pub fn is_trusted_all_paths() -> bool {
    TRUSTED_ALL_PATHS.load(Ordering::Acquire)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_switch_is_explicit_process_state() {
        set_trusted_all_paths(false);
        assert!(!is_trusted_all_paths());
        set_trusted_all_paths(true);
        assert!(is_trusted_all_paths());
        set_trusted_all_paths(false);
        assert!(!is_trusted_all_paths());
    }
}

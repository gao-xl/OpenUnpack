pub mod desktop;
pub mod error;
pub mod worker;

pub use desktop::*;
pub use error::*;
pub use worker::*;

use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    pub revision: u64,
    pub label: String,
    pub secret: String,
}

impl Drop for Candidate {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.secret.zeroize();
    }
}

impl std::fmt::Debug for Candidate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never render the secret: Debug output may reach logs or panic messages.
        f.debug_struct("Candidate")
            .field("id", &self.id)
            .field("revision", &self.revision)
            .field("label", &self.label)
            .field(
                "secret",
                &if self.secret.is_empty() {
                    "empty"
                } else {
                    "[REDACTED]"
                },
            )
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuccessEvent {
    pub event_id: String,
    pub password_id: String,
    pub password_revision: u64,
    pub fingerprint: Option<String>,
}

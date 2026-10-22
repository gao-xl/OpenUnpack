use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CandidateOrigin {
    VaultEntry,
    FileNameHint,
    ExplicitInput,
    Derived,
}

impl CandidateOrigin {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::VaultEntry => "vault",
            Self::FileNameHint => "hint",
            Self::ExplicitInput => "input",
            Self::Derived => "derived",
        }
    }
}

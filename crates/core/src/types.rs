pub use openunpack_protocol::*;

#[derive(Clone)]
pub struct Candidate {
    pub id: String, pub revision: u64, pub label: String, pub secret: String,
}
impl Drop for Candidate { fn drop(&mut self) { use zeroize::Zeroize; self.secret.zeroize(); } }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuccessEvent {
    pub event_id: String, pub password_id: String, pub password_revision: u64,
    pub fingerprint: Option<String>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Publication {
    pub id: String, pub job_id: String, pub temporary_path: String, pub final_path: String,
    pub manifest_hash: String, pub state: String, pub files: Vec<CopyFile>,
    pub verification: String,
}

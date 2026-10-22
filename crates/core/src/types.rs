pub use openunpack_protocol::*;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Publication {
    pub id: String,
    pub job_id: String,
    pub temporary_path: String,
    pub final_path: String,
    pub manifest_hash: String,
    pub state: String,
    pub files: Vec<CopyFile>,
    pub verification: String,
}

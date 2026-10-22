use super::options::FrozenJobOptions;
use super::state_machine::{ExecutionStage, JobStatus};
use crate::error::DomainError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeAggregate {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub depth: u32,
    pub status: JobStatus,
    pub stage: ExecutionStage,
    pub progress: f64,
    pub attempts: u64,
    pub message: String,
    pub password_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobAggregate {
    #[serde(default = "legacy_aggregate_version")]
    pub schema_version: u32,
    pub id: String,
    pub source_path: String,
    pub display_name: String,
    pub volumes: Vec<String>,
    pub total_size: u64,
    pub missing_volume: bool,
    pub status: JobStatus,
    pub stage: ExecutionStage,
    pub progress: f64,
    pub message: String,
    pub attempts: u64,
    pub generation: u64,
    pub output_path: Option<String>,
    pub verification_scope: String,
    pub created_at: String,
    pub finished_at: Option<String>,
    pub workspace: Option<String>,
    pub nodes: Vec<NodeAggregate>,
    pub stage_times: BTreeMap<String, u64>,
    /// None only for pre-freeze aggregate records. Never replace it with live settings.
    #[serde(default)]
    pub options: Option<FrozenJobOptions>,
}

fn legacy_aggregate_version() -> u32 {
    1
}

impl JobAggregate {
    pub fn new(
        id: String,
        source_path: String,
        display_name: String,
        volumes: Vec<String>,
        total_size: u64,
        created_at: String,
    ) -> Self {
        let root_node = NodeAggregate {
            id: format!("{}-root", id),
            parent_id: None,
            name: display_name.clone(),
            depth: 0,
            status: JobStatus::Queued,
            stage: ExecutionStage::Queued,
            progress: 0.0,
            attempts: 0,
            message: String::new(),
            password_label: None,
        };

        Self {
            schema_version: 2,
            id,
            source_path,
            display_name,
            volumes,
            total_size,
            missing_volume: false,
            status: JobStatus::Queued,
            stage: ExecutionStage::Queued,
            progress: 0.0,
            message: String::new(),
            attempts: 0,
            generation: 1,
            output_path: None,
            verification_scope: "standard".into(),
            created_at,
            finished_at: None,
            workspace: None,
            nodes: vec![root_node],
            stage_times: BTreeMap::new(),
            options: None,
        }
    }

    pub fn set_status(&mut self, next: JobStatus) -> Result<(), DomainError> {
        self.status.transition_to(next)
    }

    pub fn set_stage(&mut self, stage: ExecutionStage) {
        self.stage = stage;
    }

    pub fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    pub fn update_node_status(
        &mut self,
        node_id: &str,
        status: JobStatus,
        stage: ExecutionStage,
        progress: f64,
        message: String,
    ) -> Result<(), DomainError> {
        if let Some(node) = self.nodes.iter_mut().find(|n| n.id == node_id) {
            node.status.transition_to(status)?;
            node.stage = stage;
            node.progress = progress;
            node.message = message;
            Ok(())
        } else {
            Err(DomainError::NodeNotFound(node_id.to_string()))
        }
    }
}

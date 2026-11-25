//! 任务聚合根与节点模型。

use super::options::FrozenJobOptions;
use super::state_machine::{ExecutionStage, JobStatus};
use crate::error::DomainError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 解压任务树的节点聚合（根节点即任务本身，子节点为嵌套/分卷子任务）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeAggregate {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    /// 节点在任务树中的深度，根节点为 0。
    pub depth: u32,
    pub status: JobStatus,
    pub stage: ExecutionStage,
    /// 节点进度，取值 [0, 1]。
    pub progress: f64,
    /// 重试次数。
    pub attempts: u64,
    pub message: String,
    /// 命中的密码候选标签（未命中或不需要密码时为 `None`）。
    pub password_label: Option<String>,
}

/// 任务聚合根：一个解压任务的完整领域状态。
///
/// 序列化时拒绝未知字段（`deny_unknown_fields`），字段集合即持久化契约；
/// 持久化记录含 `schema_version`，旧数据按 v1 反序列化。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobAggregate {
    #[serde(default = "legacy_aggregate_version")]
    pub schema_version: u32,
    pub id: String,
    /// 源压缩包绝对路径。
    pub source_path: String,
    /// UI 展示名（通常为文件名）。
    pub display_name: String,
    /// 组成该任务的所有分卷路径（单文件压缩包也有一项）。
    pub volumes: Vec<String>,
    /// 所有分卷的字节总量。
    pub total_size: u64,
    /// 扫描时是否发现分卷缺失。
    pub missing_volume: bool,
    pub status: JobStatus,
    pub stage: ExecutionStage,
    pub progress: f64,
    pub message: String,
    pub attempts: u64,
    /// 乐观并发版本号，每次被协调器更新时递增。
    pub generation: u64,
    pub output_path: Option<String>,
    /// 校验范围（standard / strict / none）。
    pub verification_scope: String,
    pub created_at: String,
    pub finished_at: Option<String>,
    /// 临时工作区路径（完成后应被回收）。
    pub workspace: Option<String>,
    pub nodes: Vec<NodeAggregate>,
    /// 各阶段耗时统计（阶段名 → 毫秒）。
    pub stage_times: BTreeMap<String, u64>,
    /// 仅在冻结前的聚合记录中为 `None`。绝不要用实时设置项替代它。
    #[serde(default)]
    pub options: Option<FrozenJobOptions>,
}

fn legacy_aggregate_version() -> u32 {
    1
}

impl JobAggregate {
    /// 创建处于 Queued 初始状态的新任务，并附带根节点。
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

    /// 按状态机规则切换任务状态，非法跳变返回
    /// [`DomainError::InvalidStateTransition`]。
    pub fn set_status(&mut self, next: JobStatus) -> Result<(), DomainError> {
        self.status.transition_to(next)
    }

    /// 更新当前执行阶段（不做状态机校验，阶段仅用于 UI 细分展示）。
    pub fn set_stage(&mut self, stage: ExecutionStage) {
        self.stage = stage;
    }

    /// 递增乐观并发版本号并返回新值。
    pub fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    /// 更新指定节点的状态、阶段、进度与消息。
    ///
    /// 节点状态同样经过状态机校验；节点不存在时返回
    /// [`DomainError::NodeNotFound`]。
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

//! Worker 协议 DTO：协调器与提取引擎之间的请求/事件契约。
//!
//! 请求与事件均携带 `protocol_version`，版本协商见
//! [`WORKER_PROTOCOL_VERSION_V1`] / [`WORKER_PROTOCOL_VERSION_V2`]。
//! 凭据字段（密码、候选明文）在 Drop 时 zeroize，Debug 输出一律脱敏。

use serde::{Deserialize, Serialize};

/// 协议 v1（单请求/单事件流，无 hello 协商）。
pub const WORKER_PROTOCOL_VERSION_V1: u32 = 1;
/// 协议 v2（hello 能力协商 + 密码候选批量提交）。
pub const WORKER_PROTOCOL_VERSION_V2: u32 = 2;
/// 单条协议消息的最大字节数（8 MiB），防御异常巨型载荷。
pub const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

/// In-process native worker & engine sentinel identifiers used by desktop host & coordinator.
pub const INPROCESS_WORKER_MARKER: &str = "native-worker";
pub const INPROCESS_ENGINE_MARKER: &str = "native-engine";

fn known_size() -> bool {
    true
}

/// 解压结果中的单个条目（文件或目录）记录。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRecord {
    /// 条目在压缩包内的相对路径。
    pub relative_path: String,
    pub size: u64,
    /// `size` 是否可信；流式格式可能无法预知体积（未知时为 false）。
    #[serde(default = "known_size")]
    pub size_known: bool,
    pub sha256: String,
    #[serde(default)]
    pub is_directory: bool,
    /// 条目带加密标记（加密压缩包内可能仍有未加密条目）。
    #[serde(default)]
    pub is_encrypted: bool,
    /// 条目数据校验失败（残卷救援场景下仍可能被保留）。
    #[serde(default)]
    pub is_corrupted: bool,
}

impl Default for FileRecord {
    fn default() -> Self {
        Self {
            relative_path: String::new(),
            size: 0,
            size_known: true,
            sha256: String::new(),
            is_directory: false,
            is_encrypted: false,
            is_corrupted: false,
        }
    }
}

/// 直通复制（pass-through）阶段的单个源文件指令。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyFile {
    /// 源文件的绝对路径。
    pub source_path: String,
    /// 复制目标内的相对路径。
    pub relative_path: String,
    pub size: u64,
    #[serde(default = "known_size")]
    pub size_known: bool,
    pub sha256: String,
    #[serde(default)]
    pub is_directory: bool,
    #[serde(default)]
    pub is_corrupted: bool,
}

impl Default for CopyFile {
    fn default() -> Self {
        Self {
            source_path: String::new(),
            relative_path: String::new(),
            size: 0,
            size_known: true,
            sha256: String::new(),
            is_directory: false,
            is_corrupted: false,
        }
    }
}

/// 密码候选条目（v2 批量提交用）。
///
/// `secret` 为明文，Drop 时 zeroize，Debug 输出脱敏。
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateItem {
    pub candidate_id: String,
    pub source: String,
    #[serde(default)]
    pub revision: u64,
    pub label: String,
    pub secret: String,
}

/// 引擎能力声明（hello 协商的载荷）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkerCapabilities {
    pub backend_id: String,
    pub backend_version: String,
    /// 是否支持读取压缩包元数据（列表/统计）。
    pub read_metadata: bool,
    /// 是否支持带密码候选的解压（否则只能单密码重试）。
    pub extract_with_candidates: bool,
    /// 是否支持固实压缩包优化。
    pub solid_optimization: bool,
    /// 单批密码候选上限。
    pub max_batch_candidates: usize,
    /// 多后端（backend）细分能力列表。
    pub backends: Vec<WorkerBackend>,
}

/// 单个解压后端（如 7-Zip / HatchZip 原地引擎）的格式能力。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkerBackend {
    pub backend_id: String,
    pub backend_version: String,
    /// 支持的压缩格式扩展名列表。
    pub formats: Vec<String>,
    pub read_metadata: bool,
    /// 是否支持加密压缩包。
    pub encrypted_archives: bool,
    /// 解压体积是否不可预知（流式格式）。
    pub unknown_expanded_size: bool,
    /// 支持的校验范围（standard / strict / none）。
    pub verification_scopes: Vec<String>,
}

impl Default for WorkerCapabilities {
    fn default() -> Self {
        Self {
            backend_id: "7-Zip".into(),
            backend_version: "26.04".into(),
            read_metadata: true,
            extract_with_candidates: false,
            solid_optimization: false,
            max_batch_candidates: 128,
            backends: vec![],
        }
    }
}

/// 引擎握手事件：版本与能力协商的应答。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerHello {
    pub protocol_version: u32,
    pub major: u32,
    pub minor: u32,
    pub capabilities: WorkerCapabilities,
}

impl Default for WorkerHello {
    fn default() -> Self {
        Self {
            protocol_version: WORKER_PROTOCOL_VERSION_V2,
            major: 2,
            minor: 0,
            capabilities: WorkerCapabilities::default(),
        }
    }
}

/// 协调器发给引擎的操作请求。
///
/// 几乎所有字段带 `#[serde(default)]`：旧版本协调器发出的请求
/// 可无损反序列化，字段集只增不删。凭据字段在 Drop 时全部擦除。
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerRequest {
    pub protocol_version: u32,
    /// 请求唯一 ID，事件回执据此关联。
    pub request_id: String,
    /// 操作名（如 list / extract / probe / copy）。
    pub operation: String,
    /// 幂等键：同一操作重试时保持不变。
    #[serde(default)]
    pub operation_id: Option<String>,
    /// 任务代数（乐观并发）。
    #[serde(default)]
    pub generation: Option<u64>,
    #[serde(default)]
    pub archive_path: String,
    #[serde(default)]
    pub destination_directory: String,
    /// 单密码模式下的显式密码。
    #[serde(default)]
    pub password: Option<String>,
    /// 分卷解压允许访问的卷路径白名单（安全边界）。
    #[serde(default)]
    pub allowed_volumes: Vec<String>,
    // —— 资源预算（见 domain::ResourceBudget）——
    #[serde(default)]
    pub max_entries: u64,
    #[serde(default)]
    pub max_total_bytes: u64,
    #[serde(default)]
    pub max_written_bytes: u64,
    #[serde(default)]
    pub min_free_bytes: u64,
    #[serde(default)]
    pub files: Vec<CopyFile>,
    /// 校验范围（standard / strict / none）。
    #[serde(default)]
    pub verification: String,
    #[serde(default)]
    pub exclusions: Vec<String>,
    #[serde(default)]
    pub inclusions: Vec<String>,
    #[serde(default)]
    pub candidate_passwords: Vec<String>,
    #[serde(default)]
    pub candidates: Vec<CandidateItem>,
    /// 引擎线程数（0 = 自动）。
    #[serde(default)]
    pub engine_threads: u32,
    /// 引擎进程优先级策略。
    #[serde(default)]
    pub worker_priority: String,
    /// 是否启用残卷救援（跳过损坏条目尽可能解出内容）。
    #[serde(default)]
    pub salvage_corrupted: bool,
}

impl Default for WorkerRequest {
    fn default() -> Self {
        Self {
            protocol_version: WORKER_PROTOCOL_VERSION_V1,
            request_id: String::new(),
            operation: String::new(),
            operation_id: None,
            generation: None,
            archive_path: String::new(),
            destination_directory: String::new(),
            password: None,
            allowed_volumes: vec![],
            max_entries: 0,
            max_total_bytes: 0,
            max_written_bytes: 0,
            min_free_bytes: 0,
            files: vec![],
            verification: String::new(),
            exclusions: vec![],
            inclusions: vec![],
            candidate_passwords: vec![],
            candidates: vec![],
            engine_threads: 0,
            worker_priority: String::new(),
            salvage_corrupted: false,
        }
    }
}

/// 引擎回传协调器的事件（进度、hello、完成、失败等）。
///
/// `kind` 决定载荷语义；`matched_password` 若被违规回显，会在
/// Drop 时防御性擦除（v2 协议本不允许回显明文）。
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerEvent {
    pub protocol_version: u32,
    /// 关联的请求 ID。
    pub request_id: String,
    /// 事件类别（hello / progress / completed / failed 等）。
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hello: Option<WorkerHello>,
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub generation: Option<u64>,
    #[serde(default)]
    pub completed_bytes: u64,
    #[serde(default)]
    pub total_bytes: u64,
    #[serde(default = "known_size")]
    pub total_bytes_known: bool,
    #[serde(default)]
    pub message: String,
    /// 失败时的 [`crate::WireErrorCode`] 短名。
    #[serde(default)]
    pub error_code: String,
    #[serde(default)]
    pub files: Vec<FileRecord>,
    // —— 结果清单指纹 ——
    #[serde(default)]
    pub manifest_path: Option<String>,
    #[serde(default)]
    pub manifest_id: Option<String>,
    #[serde(default)]
    pub manifest_sha256: Option<String>,
    #[serde(default)]
    pub manifest_byte_length: Option<u64>,
    #[serde(default)]
    pub password_used: bool,
    #[serde(default)]
    pub matched_password: Option<String>,
    /// 命中的候选 ID（密码回执归因用，避免传输明文）。
    #[serde(default)]
    pub matched_candidate_id: Option<String>,
    #[serde(default)]
    pub probe_status: Option<String>,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub verification_scope: String,
}

impl Default for WorkerEvent {
    fn default() -> Self {
        Self {
            protocol_version: WORKER_PROTOCOL_VERSION_V1,
            request_id: String::new(),
            kind: String::new(),
            hello: None,
            operation_id: None,
            generation: None,
            completed_bytes: 0,
            total_bytes: 0,
            total_bytes_known: true,
            message: String::new(),
            error_code: String::new(),
            files: vec![],
            manifest_path: None,
            manifest_id: None,
            manifest_sha256: None,
            manifest_byte_length: None,
            password_used: false,
            matched_password: None,
            matched_candidate_id: None,
            probe_status: None,
            fingerprint: None,
            verification_scope: String::new(),
        }
    }
}

impl Drop for CandidateItem {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.secret.zeroize();
    }
}

impl std::fmt::Debug for CandidateItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never render the secret: Debug output may reach logs or panic messages.
        f.debug_struct("CandidateItem")
            .field("candidate_id", &self.candidate_id)
            .field("source", &self.source)
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

impl Drop for WorkerRequest {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        if let Some(secret) = self.password.as_mut() {
            secret.zeroize();
        }
        for secret in &mut self.candidate_passwords {
            secret.zeroize();
        }
        // `candidates` needs no extra pass: CandidateItem wipes its own secret on drop.
    }
}

impl std::fmt::Debug for WorkerRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never render credential fields: Debug output may reach logs or panic messages.
        f.debug_struct("WorkerRequest")
            .field("protocol_version", &self.protocol_version)
            .field("request_id", &self.request_id)
            .field("operation", &self.operation)
            .field("operation_id", &self.operation_id)
            .field("generation", &self.generation)
            .field("archive_path", &self.archive_path)
            .field("destination_directory", &self.destination_directory)
            .field("password", &self.password.as_deref().map(|_| "[REDACTED]"))
            .field("allowed_volumes", &self.allowed_volumes)
            .field("max_entries", &self.max_entries)
            .field("max_total_bytes", &self.max_total_bytes)
            .field("max_written_bytes", &self.max_written_bytes)
            .field("min_free_bytes", &self.min_free_bytes)
            .field("files", &self.files)
            .field("verification", &self.verification)
            .field("exclusions", &self.exclusions)
            .field(
                "candidate_passwords",
                &self
                    .candidate_passwords
                    .iter()
                    .map(|_| "[REDACTED]")
                    .collect::<Vec<_>>(),
            )
            .field("candidates", &self.candidates)
            .field("engine_threads", &self.engine_threads)
            .field("worker_priority", &self.worker_priority)
            .field("salvage_corrupted", &self.salvage_corrupted)
            .finish()
    }
}

impl Drop for WorkerEvent {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        // v2 rejects echoed secrets before use; wipe defensively in case a
        // protocol-violating payload is parsed and then discarded.
        if let Some(secret) = self.matched_password.as_mut() {
            secret.zeroize();
        }
    }
}

impl std::fmt::Debug for WorkerEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never render the echoed credential: Debug output may reach logs or panic messages.
        f.debug_struct("WorkerEvent")
            .field("protocol_version", &self.protocol_version)
            .field("request_id", &self.request_id)
            .field("kind", &self.kind)
            .field("hello", &self.hello)
            .field("operation_id", &self.operation_id)
            .field("generation", &self.generation)
            .field("completed_bytes", &self.completed_bytes)
            .field("total_bytes", &self.total_bytes)
            .field("message", &self.message)
            .field("error_code", &self.error_code)
            .field("files", &self.files)
            .field("manifest_path", &self.manifest_path)
            .field("manifest_id", &self.manifest_id)
            .field("manifest_sha256", &self.manifest_sha256)
            .field("manifest_byte_length", &self.manifest_byte_length)
            .field("password_used", &self.password_used)
            .field(
                "matched_password",
                &self.matched_password.as_deref().map(|_| "[REDACTED]"),
            )
            .field("matched_candidate_id", &self.matched_candidate_id)
            .field("probe_status", &self.probe_status)
            .field("fingerprint", &self.fingerprint)
            .field("verification_scope", &self.verification_scope)
            .finish()
    }
}

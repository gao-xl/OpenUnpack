//! OpenUnpack Domain - 纯业务领域模型与规则层
//!
//! 零 IO、纯规则、不依赖任何外部框架与存储实现。

pub mod archive;
pub mod error;
pub mod jobs;
pub mod passwords;
pub mod storage;

// 常用类型重导出
pub use archive::{
    VolumePattern, generate_unique_name, matches_exclusion, parse_volume_name,
    should_unwrap_single_root,
};
pub use error::DomainError;
pub use jobs::{ExecutionStage, JobAggregate, JobStatus, NodeAggregate};
pub use passwords::{
    CandidateOrigin, DomainCandidate, apply_candidate_mutations, derive_bounded_mutations,
    sort_and_dedup_candidates,
};
pub use storage::{DriveMedium, ExtractionRoute, ResourceBudget, decide_route};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_status_transition_matrix() {
        let queued = JobStatus::Queued;
        assert!(queued.can_transition_to(JobStatus::Running));
        assert!(queued.can_transition_to(JobStatus::Paused));
        assert!(queued.can_transition_to(JobStatus::Cancelled));
        assert!(!queued.can_transition_to(JobStatus::Completed));

        let running = JobStatus::Running;
        assert!(running.can_transition_to(JobStatus::Stopping));
        assert!(running.can_transition_to(JobStatus::Completed));
        assert!(running.can_transition_to(JobStatus::Failed));
        assert!(running.can_transition_to(JobStatus::NeedsPassword));

        let stopping = JobStatus::Stopping;
        assert!(stopping.can_transition_to(JobStatus::Cancelled));
        assert!(stopping.can_transition_to(JobStatus::Failed));
        assert!(stopping.can_transition_to(JobStatus::Completed));
        assert!(!stopping.can_transition_to(JobStatus::Running));

        // 终态重试进入 Queued
        let completed = JobStatus::Completed;
        assert!(completed.can_transition_to(JobStatus::Queued));
        assert!(!completed.can_transition_to(JobStatus::Running));
    }

    #[test]
    fn test_job_aggregate_transition_guard() {
        let mut job = JobAggregate::new(
            "job-1".to_string(),
            "C:/archives/test.zip".to_string(),
            "test.zip".to_string(),
            vec!["test.zip".to_string()],
            1024,
            "2026-03-30T10:00:00Z".to_string(),
        );
        assert_eq!(job.status, JobStatus::Queued);

        assert!(job.set_status(JobStatus::Running).is_ok());
        assert_eq!(job.status, JobStatus::Running);

        assert!(job.set_status(JobStatus::Completed).is_ok());
        assert_eq!(job.status, JobStatus::Completed);

        // 终态无法直接进入 Running（必须先重试进入 Queued）
        assert!(job.set_status(JobStatus::Running).is_err());
        assert!(job.set_status(JobStatus::Queued).is_ok());
    }

    #[test]
    fn test_archive_volume_and_exclusion() {
        let vol = parse_volume_name("archive.part01.rar");
        match vol {
            VolumePattern::RarPart { base, part, width } => {
                assert_eq!(base, "archive");
                assert_eq!(part, 1);
                assert_eq!(width, 2);
            }
            _ => panic!("Expected RarPart pattern"),
        }

        assert!(matches_exclusion(
            "node_modules/express/index.js",
            &["node_modules/**".to_string()]
        ));
        assert!(!matches_exclusion(
            "src/index.js",
            &["node_modules/**".to_string()]
        ));
    }

    #[test]
    fn test_password_candidate_dedup_and_sort() {
        let c1 = DomainCandidate {
            id: "1".into(),
            revision: 1,
            origin: CandidateOrigin::VaultEntry,
            label: "low".into(),
            secret: "pass1".into(),
            success_count: 5,
        };
        let c2 = DomainCandidate {
            id: "2".into(),
            revision: 1,
            origin: CandidateOrigin::FileNameHint,
            label: "high".into(),
            secret: "pass2".into(),
            success_count: 50,
        };
        let c3 = DomainCandidate {
            id: "3".into(),
            revision: 1,
            origin: CandidateOrigin::ExplicitInput,
            label: "dup".into(),
            secret: "pass1".into(),
            success_count: 1,
        };

        let sorted = sort_and_dedup_candidates(vec![c1, c2, c3]);
        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].label, "high");
        assert_eq!(sorted[1].label, "low");
    }

    #[test]
    fn test_password_bounded_mutations() {
        let base = DomainCandidate {
            id: "b1".into(),
            revision: 1,
            origin: CandidateOrigin::VaultEntry,
            label: "Base".into(),
            secret: "[pwd:Secret_Pass]".into(),
            success_count: 10,
        };

        let result = apply_candidate_mutations(vec![base]);
        // 原始排在第一位
        assert_eq!(result[0].secret, "[pwd:Secret_Pass]");
        assert_eq!(result[0].origin, CandidateOrigin::VaultEntry);

        // 验证派生变体
        let secrets: Vec<&str> = result.iter().map(|c| c.secret.as_str()).collect();
        // 应该包含剥离括号和前缀后的纯净值
        assert!(secrets.contains(&"Secret_Pass"));
        // 变体的 origin 均为 Derived
        for c in &result[1..] {
            assert_eq!(c.origin, CandidateOrigin::Derived);
            assert!(c.label.contains("变体"));
        }
    }
}

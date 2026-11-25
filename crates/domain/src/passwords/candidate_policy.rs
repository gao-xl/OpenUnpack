//! 密码候选派生策略：去重排序与受限变体生成。
//!
//! 安全约束：所有持有明文密码的结构在 Drop 时 zeroize，
//! Debug 输出永不渲染明文。

use super::origin::CandidateOrigin;
use zeroize::{Zeroize, Zeroizing};

/// 一个密码候选的领域表示。
///
/// `secret` 为明文密码，生命周期结束后内存会被清零；
/// Debug 实现输出 `[REDACTED]`，防止泄漏进日志与 panic 信息。
#[derive(Clone, PartialEq, Eq)]
pub struct DomainCandidate {
    pub id: String,
    /// 密钥库条目版本号，用于乐观并发控制。
    pub revision: u64,
    pub origin: CandidateOrigin,
    pub label: String,
    pub secret: String,
    /// 历史命中次数，决定尝试优先级。
    pub success_count: u64,
}

impl std::fmt::Debug for DomainCandidate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DomainCandidate")
            .field("id", &self.id)
            .field("revision", &self.revision)
            .field("origin", &self.origin)
            .field("label", &self.label)
            .field("secret", &"[REDACTED]")
            .field("success_count", &self.success_count)
            .finish()
    }
}

#[derive(PartialEq, Eq, Hash)]
struct TemporarySecret(String);
impl Drop for TemporarySecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl Drop for DomainCandidate {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

/// 去重并排序密码候选：按成功次数降序，同 secret 保留优先级最高的一条。
pub fn sort_and_dedup_candidates(mut candidates: Vec<DomainCandidate>) -> Vec<DomainCandidate> {
    // 1. 去重保留优先级更高的（按已出现 secret 过滤）
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::with_capacity(candidates.len());

    // 按成功次数降序
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.success_count));

    for cand in candidates {
        if seen.insert(TemporarySecret(cand.secret.clone())) {
            result.push(cand);
        }
    }

    result
}

/// 基于常见规则为基准候选词派生受限变体（大小写、去修饰符、分隔符替换等）
pub fn derive_bounded_mutations(base_candidates: &[DomainCandidate]) -> Vec<DomainCandidate> {
    let mut existing_secrets: std::collections::HashSet<TemporarySecret> = base_candidates
        .iter()
        .map(|c| TemporarySecret(c.secret.clone()))
        .collect();
    let mut mutations = Vec::new();

    for base in base_candidates {
        let trimmed = base.secret.trim();
        if trimmed.is_empty() || trimmed.len() > 256 {
            continue;
        }

        let mut variants = Vec::new();

        // 1. 修饰符与标签剥离（如 [pwd:xxx] 或 密码:xxx）
        if let Some(inner) = strip_affixes_and_tags(trimmed)
            && !inner.is_empty()
            && inner != trimmed
        {
            variants.push(Zeroizing::new(inner));
        }

        // 2. 大小写变换
        if trimmed.chars().any(|c| c.is_ascii_uppercase()) {
            variants.push(Zeroizing::new(trimmed.to_ascii_lowercase()));
        }
        if trimmed.chars().any(|c| c.is_ascii_lowercase()) {
            variants.push(Zeroizing::new(trimmed.to_ascii_uppercase()));
        }
        let capitalized = Zeroizing::new(capitalize_first(trimmed));
        if capitalized.as_str() != trimmed {
            variants.push(capitalized);
        }

        // 3. 分隔符互换（_ 与 -）
        if trimmed.contains('_') && !trimmed.contains('-') {
            variants.push(Zeroizing::new(trimmed.replace('_', "-")));
        } else if trimmed.contains('-') && !trimmed.contains('_') {
            variants.push(Zeroizing::new(trimmed.replace('-', "_")));
        }

        // 限制单基准词最多产出 3 个变体
        let mut added_for_base = 0;
        for mut var in variants {
            if added_for_base >= 3 {
                break;
            }
            if !var.is_empty()
                && var.len() <= 4096
                && existing_secrets.insert(TemporarySecret(var.to_string()))
            {
                added_for_base += 1;
                mutations.push(DomainCandidate {
                    id: format!("{}#d{}", base.id, added_for_base),
                    revision: base.revision,
                    origin: CandidateOrigin::Derived,
                    label: format!("{} (变体)", base.label),
                    secret: std::mem::take(&mut *var),
                    success_count: 0,
                });
            }
        }
    }

    mutations
}

/// 综合候选集：基准候选去重排序后，将派生变体追加到队列后方
pub fn apply_candidate_mutations(candidates: Vec<DomainCandidate>) -> Vec<DomainCandidate> {
    let mut deduped = sort_and_dedup_candidates(candidates);
    let mutations = derive_bounded_mutations(&deduped);
    deduped.extend(mutations);
    deduped
}

fn strip_affixes_and_tags(secret: &str) -> Option<String> {
    // 剥离包裹修饰符与标签前缀，提取候选核心值；
    // 无任何命中时返回 None。
    let mut s = secret.trim();
    let mut changed = false;

    // 括号剥离 [xxx], 【xxx】, (xxx)
    if ((s.starts_with('[') && s.ends_with(']'))
        || (s.starts_with('【') && s.ends_with('】'))
        || (s.starts_with('(') && s.ends_with(')'))
        || (s.starts_with('（') && s.ends_with('）')))
        && let (Some(first), Some(last)) = (s.chars().next(), s.chars().next_back())
    {
        s = s[first.len_utf8()..s.len() - last.len_utf8()].trim();
        changed = true;
    }

    // 标签前缀剥离
    let prefixes = [
        "密码:",
        "密码：",
        "pwd:",
        "pwd：",
        "pwd=",
        "解压码:",
        "解压码：",
        "提取码:",
        "提取码：",
    ];
    for p in prefixes {
        if let Some(stripped) = s.strip_prefix(p) {
            s = stripped.trim();
            changed = true;
            break;
        }
        if s.get(..p.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(p))
        {
            s = s[p.len()..].trim();
            changed = true;
            break;
        }
    }

    // 网址前缀剥离 (http://, https://, www.)
    if let Some(rest) = s.strip_prefix("https://") {
        s = rest.trim_start_matches("www.").trim();
        changed = true;
    } else if let Some(rest) = s.strip_prefix("http://") {
        s = rest.trim_start_matches("www.").trim();
        changed = true;
    } else if let Some(rest) = s.strip_prefix("www.") {
        s = rest.trim();
        changed = true;
    }

    if changed && !s.is_empty() {
        Some(s.to_string())
    } else {
        None
    }
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

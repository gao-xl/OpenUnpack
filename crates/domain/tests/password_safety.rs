use openunpack_domain::passwords::{
    CandidateOrigin, DomainCandidate, derive_bounded_mutations, sort_and_dedup_candidates,
};

fn candidate(id: &str, secret: &str, successes: u64) -> DomainCandidate {
    DomainCandidate {
        id: id.into(),
        revision: 1,
        origin: CandidateOrigin::VaultEntry,
        label: "synthetic label".into(),
        secret: secret.into(),
        success_count: successes,
    }
}

#[test]
fn candidate_debug_never_emits_secret() {
    let candidate = candidate("one", "synthetic-hidden-password", 2);
    let debug = format!("{candidate:?}");
    assert!(!debug.contains(&candidate.secret));
    assert!(debug.contains("[REDACTED]"));
    assert!(debug.contains("synthetic label"));
}

#[test]
fn dedup_keeps_success_priority_and_whitespace_distinctions() {
    let candidates = sort_and_dedup_candidates(vec![
        candidate("low", " pass ", 1),
        candidate("trimmed", "pass", 2),
        candidate("high", " pass ", 9),
    ]);
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].id, "high");
    assert_eq!(candidates[0].secret, " pass ");
    assert_eq!(candidates[1].secret, "pass");
}

#[test]
fn mutations_remain_bounded_and_preserve_the_base_secret() {
    let base = vec![candidate("base", "  [PwD:Ab_C]  ", 0)];
    let mutations = derive_bounded_mutations(&base);
    assert!(mutations.len() <= 3);
    assert!(mutations.iter().any(|c| c.secret == "Ab_C"));
    assert_eq!(base[0].secret, "  [PwD:Ab_C]  ");
    for (index, candidate) in mutations.iter().enumerate() {
        assert!(
            !mutations[..index]
                .iter()
                .any(|prior| prior.secret == candidate.secret)
        );
    }
}

use crate::core::{
    PERSONAS, RunLock, atomic_write_json, now_iso, read_json, run_dir_for, validate_vote,
};
use crate::sha256_value;
use anyhow::{Result, anyhow, bail, ensure};
use rand::seq::SliceRandom;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

pub const CANDIDATES: [&str; 3] = ["candidate-a", "candidate-b", "candidate-c"];
pub const CATEGORIES: [&str; 10] = [
    "assumption",
    "logic",
    "counter_evidence",
    "boundary_condition",
    "security",
    "reliability",
    "data_integrity",
    "rollback",
    "human_impact",
    "precedent_misuse",
];
pub const SEVERITIES: [&str; 4] = ["low", "medium", "high", "critical"];

pub fn enabled(request: &Value) -> bool {
    request
        .pointer("/adversarialReview/enabled")
        .and_then(Value::as_bool)
        == Some(true)
}

pub fn mode(request: &Value) -> &str {
    request
        .pointer("/adversarialReview/mode")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            if enabled(request) {
                "enabled"
            } else {
                "disabled"
            }
        })
}

fn vote_evidence(vote: &Value) -> BTreeMap<String, Value> {
    let mut evidence = BTreeMap::new();
    for reason in vote["reasons"].as_array().into_iter().flatten() {
        for item in reason["evidence"].as_array().into_iter().flatten() {
            if let Some(id) = item.get("id").and_then(Value::as_str) {
                evidence.insert(id.to_owned(), item.clone());
            }
        }
    }
    evidence
}

pub fn analyze_votes(request: &Value, votes: &Map<String, Value>) -> Result<Value> {
    let mut decision_kinds = BTreeSet::new();
    let mut evidence_by_persona = BTreeMap::<String, BTreeSet<String>>::new();
    let mut definitions = BTreeMap::<String, Value>::new();
    let mut definition_conflicts = BTreeSet::new();
    let mut impacts = BTreeMap::<String, BTreeSet<String>>::new();
    let mut supported_critical = BTreeSet::new();
    let mut unsupported_critical = Vec::new();

    for (persona, vote) in votes {
        validate_vote(vote, Some(persona))?;
        decision_kinds.insert(vote["decision"].as_str().unwrap_or("abstain").to_owned());
        let evidence = vote_evidence(vote);
        let ids = evidence.keys().cloned().collect::<BTreeSet<_>>();
        for (id, item) in evidence {
            if definitions
                .get(&id)
                .is_some_and(|existing| existing != &item)
            {
                definition_conflicts.insert(id.clone());
            } else {
                definitions.entry(id).or_insert(item);
            }
        }
        evidence_by_persona.insert(persona.clone(), ids.clone());

        for assessment in vote["evidenceAssessments"].as_array().into_iter().flatten() {
            if let (Some(id), Some(impact)) = (
                assessment["evidenceRef"].as_str(),
                assessment["impact"].as_str(),
            ) {
                impacts
                    .entry(id.to_owned())
                    .or_default()
                    .insert(impact.to_owned());
            }
        }
        for risk in vote["risks"].as_array().into_iter().flatten() {
            if risk["severity"] != "critical" || risk["mitigated"] != false {
                continue;
            }
            if vote["schemaVersion"] != "1.2" {
                supported_critical.insert(format!("legacy:{persona}"));
                continue;
            }
            let refs = risk["evidenceRefs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<BTreeSet<_>>();
            if !refs.is_empty() && refs.iter().all(|id| ids.contains(id)) {
                supported_critical.extend(refs);
            } else {
                unsupported_critical.push(json!({
                    "persona": persona,
                    "statement": risk["statement"],
                    "evidenceIds": refs
                }));
            }
        }
    }

    let mut triggers = Vec::new();
    if decision_kinds.len() > 1 {
        triggers.push(json!({"type": "split_vote", "evidenceIds": []}));
    }
    if !unsupported_critical.is_empty() {
        let ids = unsupported_critical
            .iter()
            .flat_map(|risk| risk["evidenceIds"].as_array().into_iter().flatten())
            .filter_map(Value::as_str)
            .collect::<BTreeSet<_>>();
        triggers.push(json!({"type": "unsupported_critical", "evidenceIds": ids}));
    }

    let approve = votes
        .values()
        .filter(|vote| vote["decision"] == "approve")
        .count();
    let reject = votes
        .values()
        .filter(|vote| vote["decision"] == "reject")
        .count();
    let majority = if approve >= 2 {
        Some("approve")
    } else if reject >= 2 {
        Some("reject")
    } else {
        None
    };
    if let Some(majority) = majority {
        let majority_ids = votes
            .iter()
            .filter(|(_, vote)| vote["decision"] == majority)
            .flat_map(|(persona, _)| {
                evidence_by_persona
                    .get(persona)
                    .into_iter()
                    .flatten()
                    .cloned()
            })
            .collect::<BTreeSet<_>>();
        let minority_ids = votes
            .iter()
            .filter(|(_, vote)| vote["decision"] != majority)
            .flat_map(|(persona, _)| {
                evidence_by_persona
                    .get(persona)
                    .into_iter()
                    .flatten()
                    .cloned()
            })
            .filter(|id| !majority_ids.contains(id))
            .collect::<BTreeSet<_>>();
        if !minority_ids.is_empty() {
            triggers.push(json!({"type": "minority_unique_evidence", "evidenceIds": minority_ids}));
        }
    }

    let mut conflict_ids = impacts
        .iter()
        .filter(|(_, values)| {
            values.contains("supports_approve") && values.contains("supports_reject")
        })
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>();
    conflict_ids.extend(definition_conflicts);
    if !conflict_ids.is_empty() {
        triggers.push(json!({"type": "evidence_conflict", "evidenceIds": conflict_ids}));
    }

    let high_risk_domains = request
        .pointer("/riskProfile/highRiskDomains")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !high_risk_domains.is_empty() {
        triggers.push(json!({
            "type": "explicit_high_risk",
            "evidenceIds": [],
            "domains": high_risk_domains
        }));
    }
    let review_required = mode(request) == "enabled"
        || (mode(request) == "auto" && supported_critical.is_empty() && !triggers.is_empty());
    Ok(json!({
        "schemaVersion": "1.0",
        "runId": request["runId"],
        "reviewMode": mode(request),
        "triggers": triggers,
        "supportedCriticalEvidenceIds": supported_critical,
        "unsupportedCriticalRisks": unsupported_critical,
        "evidenceSufficiency": "traceable_locator_only",
        "reviewRequired": review_required
    }))
}

fn text<'a>(value: &'a Value, key: &str, label: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow!("{label}.{key} must be a non-empty string."))
}

pub fn validate_challenges(value: &Value, run_id: &str, require_test: bool) -> Result<()> {
    ensure!(
        value.get("schemaVersion").and_then(Value::as_str) == Some("1.0"),
        "challenges.schemaVersion must be 1.0."
    );
    ensure!(
        value.get("runId").and_then(Value::as_str) == Some(run_id),
        "challenges.runId mismatch."
    );
    let challenges = value
        .get("challenges")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("challenges.challenges must be an array."))?;
    let mut ids = HashSet::new();
    for (index, challenge) in challenges.iter().enumerate() {
        let label = format!("challenges[{index}]");
        let id = text(challenge, "id", &label)?;
        ensure!(ids.insert(id), "Duplicate challenge id: {id}");
        ensure!(
            CANDIDATES.contains(&text(challenge, "targetCandidate", &label)?),
            "{label}.targetCandidate is invalid."
        );
        ensure!(
            CATEGORIES.contains(&text(challenge, "category", &label)?),
            "{label}.category is invalid."
        );
        ensure!(
            SEVERITIES.contains(&text(challenge, "severity", &label)?),
            "{label}.severity is invalid."
        );
        text(challenge, "claimUnderChallenge", &label)?;
        text(challenge, "counterArgument", &label)?;
        ensure!(
            matches!(
                challenge.get("status").and_then(Value::as_str),
                Some("unresolved" | "resolved")
            ),
            "{label}.status is invalid."
        );
        if require_test || challenge.get("falsificationTest").is_some() {
            let test = challenge
                .get("falsificationTest")
                .ok_or_else(|| anyhow!("{label}.falsificationTest is required."))?;
            text(test, "description", &format!("{label}.falsificationTest"))?;
            ensure!(
                test.get("expectedEvidence")
                    .and_then(Value::as_array)
                    .is_some(),
                "{label}.falsificationTest.expectedEvidence must be an array."
            );
        }
    }
    Ok(())
}

pub fn prepare(root: &Path, run_id: &str) -> Result<Value> {
    let run_dir = run_dir_for(root, run_id)?;
    let _lock = RunLock::acquire(&run_dir)?;
    let request_file = run_dir.join("request.json");
    let manifest_file = run_dir.join("manifest.json");
    let mut request = read_json(&request_file)?;
    ensure!(
        enabled(&request),
        "Adversarial review is disabled for this run."
    );
    ensure!(
        request.get("status").and_then(Value::as_str) == Some("initial_ready"),
        "Initial votes are not ready."
    );
    let mut manifest = read_json(&manifest_file)?;
    let mut initial_votes = Map::new();
    for persona in PERSONAS {
        let vote = read_json(
            &run_dir
                .join("rounds/initial/sealed")
                .join(format!("{persona}.json")),
        )?;
        validate_vote(&vote, Some(persona))?;
        ensure!(
            manifest
                .pointer(&format!("/rounds/initial/{persona}/sha256"))
                .and_then(Value::as_str)
                == Some(&sha256_value(&vote)?),
            "Initial vote hash mismatch: {persona}"
        );
        initial_votes.insert(persona.to_owned(), vote);
    }
    let analysis = analyze_votes(&request, &initial_votes)?;
    fs::create_dir_all(run_dir.join("adversarial"))?;
    atomic_write_json(
        &run_dir.join("adversarial/review-analysis.json"),
        &analysis,
        0o600,
    )?;
    let analysis_hash = sha256_value(&analysis)?;
    manifest["adversarial"]["reviewAnalysisSha256"] = Value::String(analysis_hash.clone());

    if mode(&request) == "auto" && analysis["reviewRequired"] == false {
        request["status"] = Value::String("ready".to_owned());
        atomic_write_json(&manifest_file, &manifest, 0o600)?;
        atomic_write_json(&request_file, &request, 0o600)?;
        return Ok(json!({
            "prepared": false,
            "reviewRequired": false,
            "runId": run_id,
            "status": "ready",
            "reviewAnalysisSha256": analysis_hash,
            "receipt": format!("REVIEW_NOT_REQUIRED run={run_id} sha256={}", &analysis_hash[..16])
        }));
    }
    if mode(&request) == "auto"
        && request
            .pointer("/adversarialReview/thomasAvailable")
            .and_then(Value::as_bool)
            == Some(false)
    {
        request["status"] = Value::String("suspended_for_human_review".to_owned());
        manifest["adversarial"]["suspensionReason"] =
            Value::String("thomas_unavailable".to_owned());
        atomic_write_json(&manifest_file, &manifest, 0o600)?;
        atomic_write_json(&request_file, &request, 0o600)?;
        return Ok(json!({
            "prepared": false,
            "reviewRequired": true,
            "runId": run_id,
            "status": "suspended_for_human_review",
            "reason": "thomas_unavailable",
            "reviewAnalysisSha256": analysis_hash,
            "receipt": format!("REVIEW_SUSPENDED run={run_id} sha256={}", &analysis_hash[..16])
        }));
    }
    let mut personas = PERSONAS;
    personas.shuffle(&mut rand::rng());
    let mut mapping = Map::new();
    let mut candidates = Vec::new();
    for (candidate, persona) in CANDIDATES.iter().zip(personas) {
        mapping.insert((*candidate).to_owned(), Value::String(persona.to_owned()));
        let vote = &initial_votes[persona];
        candidates.push(json!({
            "candidate": candidate,
            "decision": vote["decision"], "confidence": vote["confidence"], "summary": vote["summary"],
            "reasons": vote["reasons"], "conditions": vote["conditions"], "risks": vote["risks"],
            "assumptions": vote["assumptions"]
        }));
    }
    let mapping_value = Value::Object(mapping);
    let input = json!({
        "schemaVersion": "1.0", "runId": run_id, "question": request["question"],
        "context": request["context"],
        "reviewPolicy": {
            "maxChallengesPerCandidate": request.pointer("/adversarialReview/maxChallengesPerCandidate").and_then(Value::as_u64).unwrap_or(5),
            "requireFalsificationTest": request.pointer("/adversarialReview/requireFalsificationTest").and_then(Value::as_bool).unwrap_or(true)
        },
        "reviewTriggers": analysis["triggers"],
        "candidates": candidates
    });
    atomic_write_json(
        &run_dir.join("adversarial/mapping.json"),
        &mapping_value,
        0o600,
    )?;
    atomic_write_json(&run_dir.join("adversarial/input.json"), &input, 0o600)?;
    let input_hash = sha256_value(&input)?;
    manifest["adversarial"] = json!({
        "reviewAnalysisSha256": analysis_hash,
        "mappingSha256": sha256_value(&mapping_value)?, "inputSha256": input_hash, "challengesSha256": null
    });
    request["status"] = Value::String("challenging".to_owned());
    atomic_write_json(&manifest_file, &manifest, 0o600)?;
    atomic_write_json(&request_file, &request, 0o600)?;
    Ok(json!({
        "prepared": true,
        "runId": run_id,
        "candidateCount": candidates.len(),
        "sha256": input_hash,
        "receipt": format!("THOMAS_INPUT_PREPARED run={run_id} sha256={}", &input_hash[..16])
    }))
}

pub fn seal_challenges(root: &Path, challenges: &Value, agent_id: Option<&str>) -> Result<Value> {
    let run_id = challenges
        .get("runId")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("challenges.runId is required."))?;
    let run_dir = run_dir_for(root, run_id)?;
    let _lock = RunLock::acquire(&run_dir)?;
    let request_file = run_dir.join("request.json");
    let manifest_file = run_dir.join("manifest.json");
    let mut request = read_json(&request_file)?;
    ensure!(
        request.get("status").and_then(Value::as_str) == Some("challenging"),
        "Run is not accepting THOMAS challenges."
    );
    let require_test = request
        .pointer("/adversarialReview/requireFalsificationTest")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    validate_challenges(challenges, run_id, require_test)?;
    let max = request
        .pointer("/adversarialReview/maxChallengesPerCandidate")
        .and_then(Value::as_u64)
        .unwrap_or(5) as usize;
    for candidate in CANDIDATES {
        let count = challenges["challenges"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["targetCandidate"] == candidate)
            .count();
        ensure!(count <= max, "Too many challenges for {candidate}.");
    }
    let path = run_dir.join("adversarial/challenges.json");
    let hash = sha256_value(challenges)?;
    if path.exists() {
        ensure!(
            sha256_value(&read_json(&path)?)? == hash,
            "THOMAS already sealed different challenges; overwriting is forbidden."
        );
    } else {
        atomic_write_json(&path, challenges, 0o600)?;
    }
    let mut manifest = read_json(&manifest_file)?;
    manifest["adversarial"]["challengesSha256"] = Value::String(hash.clone());
    manifest["adversarial"]["sealedAt"] = Value::String(now_iso());
    manifest["adversarial"]["agentId"] =
        agent_id.map_or(Value::Null, |s| Value::String(s.to_owned()));
    request["status"] = Value::String("challenge_ready".to_owned());
    atomic_write_json(&manifest_file, &manifest, 0o600)?;
    atomic_write_json(&request_file, &request, 0o600)?;
    Ok(
        json!({"sealed": true, "runId": run_id, "sha256": hash, "receipt": format!("THOMAS: CHALLENGES_SEALED run={run_id} sha256={}", &hash[..16])}),
    )
}

pub fn context_for(root: &Path, run_id: &str, agent: &str) -> Result<String> {
    let run_dir = run_dir_for(root, run_id)?;
    let mut request = read_json(&run_dir.join("request.json"))?;
    match agent {
        "thomas" => {
            ensure!(
                request.get("status").and_then(Value::as_str) == Some("challenging"),
                "THOMAS is not active for this run."
            );
            Ok(serde_json::to_string_pretty(&read_json(
                &run_dir.join("adversarial/input.json"),
            )?)?)
        }
        persona if PERSONAS.contains(&persona) => {
            ensure!(
                matches!(
                    request.get("status").and_then(Value::as_str),
                    Some("challenge_ready" | "collecting_final")
                ),
                "Final voting is not ready."
            );
            let mapping = read_json(&run_dir.join("adversarial/mapping.json"))?;
            let candidate = mapping
                .as_object()
                .and_then(|m| m.iter().find(|(_, p)| p.as_str() == Some(persona)))
                .map(|(c, _)| c.clone())
                .ok_or_else(|| anyhow!("Candidate mapping is invalid."))?;
            let challenges = read_json(&run_dir.join("adversarial/challenges.json"))?;
            let own = challenges["challenges"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|c| c["targetCandidate"] == candidate)
                .cloned()
                .collect::<Vec<_>>();
            let initial = read_json(
                &run_dir
                    .join("rounds/initial/sealed")
                    .join(format!("{persona}.json")),
            )?;
            if request.get("status").and_then(Value::as_str) == Some("challenge_ready") {
                request["status"] = Value::String("collecting_final".to_owned());
                atomic_write_json(&run_dir.join("request.json"), &request, 0o600)?;
            }
            Ok(serde_json::to_string_pretty(
                &json!({"runId": run_id, "question": request["question"], "context": request["context"], "initialVote": initial, "challenges": own}),
            )?)
        }
        _ => bail!("Unknown adversarial agent."),
    }
}

pub fn seal_round_vote(
    root: &Path,
    persona: &str,
    round: &str,
    vote: &Value,
    agent_id: Option<&str>,
) -> Result<Value> {
    ensure!(PERSONAS.contains(&persona), "Unknown persona: {persona}");
    ensure!(
        matches!(round, "initial" | "final"),
        "round must be initial or final."
    );
    validate_vote(vote, Some(persona))?;
    let run_id = vote["runId"].as_str().unwrap();
    let run_dir = run_dir_for(root, run_id)?;
    let _lock = RunLock::acquire(&run_dir)?;
    let request_file = run_dir.join("request.json");
    let manifest_file = run_dir.join("manifest.json");
    let mut request = read_json(&request_file)?;
    ensure!(enabled(&request), "This run uses the legacy vote flow.");
    let expected = if round == "initial" {
        &["collecting_initial", "initial_ready"][..]
    } else {
        &["collecting_final", "final_ready"][..]
    };
    ensure!(
        request
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|s| expected.contains(&s)),
        "Run state does not accept {round} votes."
    );
    if round == "final" {
        ensure!(
            vote.get("challengeResponses")
                .and_then(Value::as_array)
                .is_some(),
            "Final vote requires challengeResponses."
        );
    } else {
        ensure!(
            vote.get("challengeResponses").is_none(),
            "Initial vote must not include challengeResponses."
        );
    }
    let path = run_dir.join(format!("rounds/{round}/sealed/{persona}.json"));
    let hash = sha256_value(vote)?;
    if path.exists() {
        ensure!(
            sha256_value(&read_json(&path)?)? == hash,
            "{persona} already sealed a different {round} vote; overwriting is forbidden."
        );
    } else {
        atomic_write_json(&path, vote, 0o600)?;
    }
    let mut manifest = read_json(&manifest_file)?;
    manifest["rounds"][round][persona] =
        json!({"sha256": hash, "sealedAt": now_iso(), "agentId": agent_id});
    let ready = PERSONAS.iter().all(|p| {
        manifest
            .pointer(&format!("/rounds/{round}/{p}/sha256"))
            .is_some()
    });
    if ready {
        request["status"] = Value::String(
            if round == "initial" {
                "initial_ready"
            } else {
                "final_ready"
            }
            .to_owned(),
        );
    }
    atomic_write_json(&manifest_file, &manifest, 0o600)?;
    atomic_write_json(&request_file, &request, 0o600)?;
    Ok(
        json!({"sealed": true, "persona": persona, "round": round, "runId": run_id, "sha256": hash, "receipt": format!("{}: {}_VOTE_SEALED run={run_id} sha256={}", persona.to_uppercase(), round.to_uppercase(), &hash[..16])}),
    )
}

pub fn challenge_resolution(
    root: &Path,
    run_id: &str,
    final_votes: &Map<String, Value>,
) -> Result<Value> {
    let run_dir = run_dir_for(root, run_id)?;
    let mapping = read_json(&run_dir.join("adversarial/mapping.json"))?;
    let challenges = read_json(&run_dir.join("adversarial/challenges.json"))?;
    let mut responses = HashMap::new();
    for (persona, vote) in final_votes {
        for response in vote["challengeResponses"].as_array().into_iter().flatten() {
            responses.insert(
                (
                    persona.clone(),
                    response["challengeId"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                ),
                response.clone(),
            );
        }
    }
    let mut accepted = Vec::new();
    let mut rejected = Vec::new();
    let mut unresolved = Vec::new();
    for challenge in challenges["challenges"].as_array().into_iter().flatten() {
        let candidate = challenge["targetCandidate"].as_str().unwrap();
        let persona = mapping[candidate].as_str().unwrap();
        let response = responses.get(&(
            persona.to_owned(),
            challenge["id"].as_str().unwrap().to_owned(),
        ));
        match response.and_then(|r| r["response"].as_str()) {
            Some("revise" | "reverse" | "abstain") => accepted.push(challenge.clone()),
            Some("uphold")
                if response.unwrap()["evidence"]
                    .as_array()
                    .is_some_and(|e| !e.is_empty()) =>
            {
                rejected.push(challenge.clone())
            }
            _ if matches!(challenge["severity"].as_str(), Some("high" | "critical")) => {
                unresolved.push(challenge.clone())
            }
            _ => {}
        }
    }
    let suspend = unresolved
        .iter()
        .any(|c| c["severity"] == "critical" && c.get("falsificationTest").is_some());
    Ok(
        json!({"accepted": accepted, "rejectedWithEvidence": rejected, "unresolvedHighOrCritical": unresolved, "suspendForHumanReview": suspend}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn review_request(domains: Value) -> Value {
        json!({
            "schemaVersion": "1.2", "runId": "magi-20260805120000-abcdef123456",
            "adversarialReview": {"mode": "auto", "enabled": true},
            "riskProfile": {"highRiskDomains": domains}
        })
    }

    fn review_vote(
        persona: &str,
        decision: &str,
        evidence_ids: &[&str],
        impact: &str,
        critical_refs: Option<Value>,
    ) -> Value {
        let evidence = evidence_ids
            .iter()
            .map(|id| {
                json!({
                    "id": id, "type": "file", "claim": format!("Claim for {id}"),
                    "observedAt": "2026-08-14T00:00:00Z", "path": format!("src/{id}.rs")
                })
            })
            .collect::<Vec<_>>();
        let assessments = evidence_ids
            .iter()
            .map(|id| json!({"evidenceRef": id, "impact": impact}))
            .collect::<Vec<_>>();
        let risks = critical_refs.map_or_else(Vec::new, |refs| {
            vec![json!({
                "severity": "critical", "statement": "Critical risk",
                "mitigated": false, "evidenceRefs": refs
            })]
        });
        json!({
            "schemaVersion": "1.2", "runId": "magi-20260805120000-abcdef123456",
            "persona": persona, "decision": decision, "confidence": 80,
            "summary": "Summary", "reasons": [{"code": "R1", "statement": "Reason", "evidence": evidence}],
            "conditions": [], "risks": risks, "assumptions": [],
            "evidenceAssessments": assessments, "memoryCandidates": []
        })
    }

    fn votes(items: [Value; 3]) -> Map<String, Value> {
        PERSONAS
            .into_iter()
            .zip(items)
            .map(|(persona, vote)| (persona.to_owned(), vote))
            .collect()
    }

    #[test]
    fn unanimous_votes_without_risk_do_not_trigger_auto_review() {
        let votes = votes([
            review_vote(
                "melchior",
                "approve",
                &["ev-shared"],
                "supports_approve",
                None,
            ),
            review_vote(
                "balthasar",
                "approve",
                &["ev-shared"],
                "supports_approve",
                None,
            ),
            review_vote(
                "casper",
                "approve",
                &["ev-shared"],
                "supports_approve",
                None,
            ),
        ]);
        let analysis = analyze_votes(&review_request(json!([])), &votes).unwrap();
        assert_eq!(analysis["triggers"], json!([]));
        assert_eq!(analysis["reviewRequired"], false);
    }

    #[test]
    fn detects_split_minority_unique_evidence_and_conflicting_impact() {
        let votes = votes([
            review_vote(
                "melchior",
                "approve",
                &["ev-shared"],
                "supports_approve",
                None,
            ),
            review_vote(
                "balthasar",
                "approve",
                &["ev-shared"],
                "supports_approve",
                None,
            ),
            review_vote(
                "casper",
                "reject",
                &["ev-shared", "ev-minority"],
                "supports_reject",
                None,
            ),
        ]);
        let analysis = analyze_votes(&review_request(json!([])), &votes).unwrap();
        let kinds = analysis["triggers"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|trigger| trigger["type"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            [
                "split_vote",
                "minority_unique_evidence",
                "evidence_conflict"
            ]
        );
        assert_eq!(analysis["reviewRequired"], true);
    }

    #[test]
    fn supported_critical_veto_precedes_auto_review() {
        let votes = votes([
            review_vote(
                "melchior",
                "approve",
                &["ev-shared"],
                "supports_approve",
                None,
            ),
            review_vote(
                "balthasar",
                "approve",
                &["ev-shared"],
                "supports_approve",
                None,
            ),
            review_vote(
                "casper",
                "reject",
                &["ev-critical"],
                "supports_reject",
                Some(json!(["ev-critical"])),
            ),
        ]);
        let analysis = analyze_votes(&review_request(json!([])), &votes).unwrap();
        assert_eq!(
            analysis["supportedCriticalEvidenceIds"],
            json!(["ev-critical"])
        );
        assert_eq!(analysis["reviewRequired"], false);
    }

    #[test]
    fn unsupported_critical_and_explicit_high_risk_trigger_review() {
        let votes = votes([
            review_vote("melchior", "approve", &[], "uncertain", None),
            review_vote("balthasar", "approve", &[], "uncertain", None),
            review_vote("casper", "reject", &[], "uncertain", Some(json!([]))),
        ]);
        let analysis = analyze_votes(&review_request(json!(["healthcare"])), &votes).unwrap();
        let kinds = analysis["triggers"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|trigger| trigger["type"].as_str())
            .collect::<Vec<_>>();
        assert!(kinds.contains(&"unsupported_critical"));
        assert!(kinds.contains(&"explicit_high_risk"));
        assert_eq!(analysis["evidenceSufficiency"], "traceable_locator_only");
    }

    #[test]
    fn rejects_unknown_challenge_enums() {
        let value = json!({
            "schemaVersion": "1.0", "runId": "magi-20260805120000-abcdef123456",
            "challenges": [{
                "id": "challenge-1", "targetCandidate": "candidate-a", "category": "opinion",
                "severity": "urgent", "claimUnderChallenge": "claim", "counterArgument": "counter",
                "falsificationTest": {"description": "test", "expectedEvidence": []}, "status": "unresolved"
            }]
        });
        assert!(validate_challenges(&value, "magi-20260805120000-abcdef123456", true).is_err());
    }
}

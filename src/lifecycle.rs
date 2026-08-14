use crate::adversarial;
use crate::core::{
    PERSONAS, RunLock, atomic_write_json, atomic_write_text, now_iso, random_hex, read_json,
    run_dir_for, state_dir, validate_request, validate_vote, verify_request_hash,
};
use crate::{hash_request, sha256_value};
use anyhow::{Result, anyhow, ensure};
use chrono::Utc;
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub fn create_run(root: &Path, input: &Value) -> Result<Value> {
    let input = input
        .as_object()
        .ok_or_else(|| anyhow!("request input must be an object."))?;
    let question = input
        .get("question")
        .and_then(Value::as_str)
        .filter(|question| !question.trim().is_empty())
        .ok_or_else(|| anyhow!("question is required."))?;
    let context = input
        .get("context")
        .ok_or_else(|| anyhow!("context must be an object."))?;
    ensure!(context.is_object(), "context must be an object.");

    let created_at = now_iso();
    let stamp = Utc::now().format("%Y%m%d%H%M%S");
    let run_id = format!("magi-{stamp}-{}", random_hex(6));
    let state = state_dir(root);
    let config = read_json(&state.join("config.json"))?;
    let critical_risk_veto = config
        .pointer("/voting/criticalRiskVeto")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let execution_mode = if input.get("executionMode").and_then(Value::as_str) == Some("inline") {
        "inline"
    } else {
        "sealed-subagents"
    };
    let configured_mode = config
        .pointer("/adversarialReview/mode")
        .and_then(Value::as_str)
        .unwrap_or("disabled");
    ensure!(
        matches!(configured_mode, "disabled" | "enabled" | "auto"),
        "adversarialReview.mode must be disabled, enabled, or auto."
    );
    let review_mode = match input.get("adversarialReview").and_then(Value::as_bool) {
        Some(true) => "enabled",
        Some(false) => "disabled",
        None => configured_mode,
    };
    let adversarial_enabled = review_mode != "disabled";
    ensure!(
        !(adversarial_enabled && execution_mode == "inline"),
        "Adversarial review cannot guarantee independence in inline mode; use sealed-subagents."
    );
    let initial_status = if adversarial_enabled {
        "collecting_initial"
    } else {
        "collecting"
    };
    let high_risk_domains = input
        .get("riskProfile")
        .and_then(|profile| profile.get("highRiskDomains"))
        .or_else(|| config.pointer("/riskProfile/highRiskDomains"))
        .cloned()
        .unwrap_or_else(|| json!([]));
    let request = json!({
        "schemaVersion": "1.2",
        "runId": run_id,
        "createdAt": created_at,
        "status": initial_status,
        "executionMode": execution_mode,
        "question": question.trim(),
        "context": context,
        "expectedPersonas": PERSONAS,
        "voting": {
            "method": "majority",
            "criticalRiskVeto": critical_risk_veto
        },
        "adversarialReview": {
            "mode": review_mode,
            "enabled": adversarial_enabled,
            "thomasAvailable": config.pointer("/adversarialReview/thomasAvailable").and_then(Value::as_bool).unwrap_or(true),
            "anonymizePersonas": true,
            "maxChallengesPerCandidate": config.pointer("/adversarialReview/maxChallengesPerCandidate").and_then(Value::as_u64).unwrap_or(5),
            "minimumSeverity": config.pointer("/adversarialReview/minimumSeverity").and_then(Value::as_str).unwrap_or("medium"),
            "requireFalsificationTest": config.pointer("/adversarialReview/requireFalsificationTest").and_then(Value::as_bool).unwrap_or(true),
            "unresolvedCriticalAction": "human_review"
        },
        "riskProfile": {
            "highRiskDomains": high_risk_domains
        }
    });
    validate_request(&request)?;
    let run_dir = run_dir_for(root, &run_id)?;
    fs::create_dir_all(run_dir.join("sealed"))?;
    if adversarial_enabled {
        fs::create_dir_all(run_dir.join("rounds/initial/sealed"))?;
        fs::create_dir_all(run_dir.join("rounds/final/sealed"))?;
        fs::create_dir_all(run_dir.join("adversarial"))?;
    }
    fs::create_dir_all(run_dir.join("candidates"))?;
    atomic_write_json(&run_dir.join("request.json"), &request, 0o600)?;
    atomic_write_json(
        &run_dir.join("manifest.json"),
        &json!({
            "schemaVersion": "1.0",
            "runId": run_id,
            "requestSha256": hash_request(&request)?,
            "votes": {},
            "rounds": {"initial": {}, "final": {}},
            "finalized": false,
            "createdAt": created_at
        }),
        0o600,
    )?;
    Ok(json!({
        "runId": run_id,
        "status": initial_status,
        "requestPath": format!(".magi/runs/{run_id}/request.json")
    }))
}

pub fn run_status(root: &Path, run_id: &str) -> Result<Value> {
    let run_dir = run_dir_for(root, run_id)?;
    let request = read_json(&run_dir.join("request.json"))?;
    let mut sealed = Map::new();
    let manifest = read_json(&run_dir.join("manifest.json"))?;
    let adversarial_enabled = adversarial::enabled(&request);
    let review_performed = manifest
        .pointer("/adversarial/challengesSha256")
        .and_then(Value::as_str)
        .is_some();
    let round = if review_performed { "final" } else { "initial" };
    for persona in PERSONAS {
        sealed.insert(
            persona.to_owned(),
            Value::Bool(
                run_dir
                    .join(if adversarial_enabled {
                        format!("rounds/{round}/sealed")
                    } else {
                        "sealed".to_owned()
                    })
                    .join(format!("{persona}.json"))
                    .exists(),
            ),
        );
    }
    let ready = sealed.values().all(|value| value == &Value::Bool(true));
    Ok(json!({
        "runId": run_id,
        "status": request.get("status").cloned().unwrap_or(Value::Null),
        "sealed": sealed,
        "ready": ready,
        "adversarialReview": adversarial_enabled,
        "reviewMode": adversarial::mode(&request),
        "reviewPerformed": review_performed,
        "round": if adversarial_enabled { Value::String(round.to_owned()) } else { Value::Null }
    }))
}

pub fn import_inline_votes(root: &Path, run_id: &str, input: &Value) -> Result<Value> {
    let votes = input
        .as_array()
        .filter(|votes| votes.len() == 3)
        .ok_or_else(|| anyhow!("Input must be an array containing exactly three votes."))?;
    let run_dir = run_dir_for(root, run_id)?;
    let _lock = RunLock::acquire(&run_dir)?;
    let request_file = run_dir.join("request.json");
    let manifest_file = run_dir.join("manifest.json");
    let mut request = read_json(&request_file)?;
    validate_request(&request)?;
    let status = request
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    ensure!(
        matches!(status, "collecting" | "ready"),
        "Run is not accepting votes: {status}"
    );

    let mut validated_votes = Vec::with_capacity(PERSONAS.len());
    for persona in PERSONAS {
        let vote = votes
            .iter()
            .find(|vote| vote.get("persona").and_then(Value::as_str) == Some(persona))
            .ok_or_else(|| anyhow!("Missing vote for {persona}."))?;
        validate_vote(vote, Some(persona))?;
        ensure!(
            vote.get("runId").and_then(Value::as_str) == Some(run_id),
            "{persona} runId mismatch."
        );
        let vote_file = run_dir.join("sealed").join(format!("{persona}.json"));
        if vote_file.exists() {
            let existing = read_json(&vote_file)?;
            validate_vote(&existing, Some(persona))?;
            ensure!(
                sha256_value(&existing)? == sha256_value(vote)?,
                "{persona} already sealed a different vote; overwriting is forbidden."
            );
        }
        validated_votes.push((persona, vote));
    }

    request["executionMode"] = Value::String("inline".to_owned());
    let mut manifest = read_json(&manifest_file)?;
    manifest["requestSha256"] = Value::String(hash_request(&request)?);
    for (persona, vote) in validated_votes {
        atomic_write_json(
            &run_dir.join("sealed").join(format!("{persona}.json")),
            vote,
            0o600,
        )?;
        manifest["votes"][persona] = json!({
            "sha256": sha256_value(vote)?,
            "sealedAt": now_iso(),
            "agentId": null,
            "warning": "inline execution; independence not guaranteed"
        });
    }
    request["status"] = Value::String("ready".to_owned());
    atomic_write_json(&request_file, &request, 0o600)?;
    atomic_write_json(&manifest_file, &manifest, 0o600)?;
    Ok(json!({
        "runId": run_id,
        "imported": true,
        "warning": "Inline votes share one model context; persona independence is not guaranteed."
    }))
}

fn with_persona(object: &Map<String, Value>, persona: &str) -> Value {
    let mut output = Map::new();
    output.insert("persona".to_owned(), Value::String(persona.to_owned()));
    for (key, value) in object {
        output.insert(key.clone(), value.clone());
    }
    Value::Object(output)
}

fn unique_strings(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn decision_markdown(decision: &Value) -> Result<String> {
    let counts = decision
        .get("voteCounts")
        .ok_or_else(|| anyhow!("decision.voteCounts is missing"))?;
    let confidence = decision
        .get("confidence")
        .ok_or_else(|| anyhow!("decision.confidence is missing"))?;
    let mut lines = vec![
        format!(
            "# MAGI decision: {}",
            decision["decision"].as_str().unwrap_or_default()
        ),
        String::new(),
        format!(
            "- Run: `{}`",
            decision["runId"].as_str().unwrap_or_default()
        ),
        format!(
            "- Votes: approve {}, reject {}, abstain {}",
            counts["approve"], counts["reject"], counts["abstain"]
        ),
        format!(
            "- Self-reported confidence (uncalibrated; not a probability): min {}, median {}, max {}",
            confidence["min"], confidence["median"], confidence["max"]
        ),
        format!(
            "- Critical-risk veto: {}",
            if decision["veto"]["applied"].as_bool() == Some(true) {
                "applied"
            } else {
                "not applied"
            }
        ),
        String::new(),
        "## Conditions".to_owned(),
    ];
    let conditions = decision["conditions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if conditions.is_empty() {
        lines.push("- None".to_owned());
    } else {
        lines.extend(
            conditions
                .iter()
                .map(|item| format!("- {}", item.as_str().unwrap_or_default())),
        );
    }
    lines.extend([String::new(), "## High and critical risks".to_owned()]);
    let risks = decision["highRisks"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if risks.is_empty() {
        lines.push("- None".to_owned());
    } else {
        lines.extend(risks.iter().map(|risk| {
            format!(
                "- **{}/{}**: {}",
                risk["persona"].as_str().unwrap_or_default(),
                risk["severity"].as_str().unwrap_or_default(),
                risk["statement"].as_str().unwrap_or_default()
            )
        }));
    }
    lines.extend([String::new(), "## Dissent".to_owned()]);
    let dissent = decision["dissent"].as_array().cloned().unwrap_or_default();
    if dissent.is_empty() {
        lines.push("- None".to_owned());
    } else {
        lines.extend(dissent.iter().map(|item| {
            format!(
                "- **{} ({})**: {}",
                item["persona"].as_str().unwrap_or_default(),
                item["decision"].as_str().unwrap_or_default(),
                item["summary"].as_str().unwrap_or_default()
            )
        }));
    }
    lines.extend([String::new(), "## Assumptions".to_owned()]);
    let assumptions = decision["assumptions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if assumptions.is_empty() {
        lines.push("- None".to_owned());
    } else {
        lines.extend(
            assumptions
                .iter()
                .map(|item| format!("- {}", item.as_str().unwrap_or_default())),
        );
    }
    if decision
        .get("adversarialReview")
        .is_some_and(Value::is_object)
    {
        lines.extend([String::new(), "## Adversarial review".to_owned()]);
        lines.push(format!(
            "- Mode: {}",
            decision
                .pointer("/adversarialReview/mode")
                .and_then(Value::as_str)
                .unwrap_or("legacy")
        ));
        lines.push(format!(
            "- Performed: {}",
            decision
                .pointer("/adversarialReview/performed")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ));
        lines.push(format!(
            "- Resolution: {}",
            decision
                .pointer("/adversarialReview/resolution")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ));
        let triggers = decision
            .pointer("/adversarialReview/reviewTriggers")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if triggers.is_empty() {
            lines.push("- Review triggers: None".to_owned());
        } else {
            lines.push("- Review triggers:".to_owned());
            for trigger in triggers {
                let ids = trigger["evidenceIds"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(format!(
                    "  - **{}**{}",
                    trigger["type"].as_str().unwrap_or_default(),
                    if ids.is_empty() {
                        String::new()
                    } else {
                        format!(": {ids}")
                    }
                ));
            }
        }
        if let Some(reason) = decision
            .pointer("/adversarialReview/suspensionReason")
            .and_then(Value::as_str)
        {
            lines.push(format!("- Suspension reason: {reason}"));
        }
        let changes = decision
            .pointer("/adversarialReview/changes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        lines.push(format!("- Changed personas: {}", changes.len()));
        let accepted = decision
            .pointer("/adversarialReview/challenges/accepted")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let rejected = decision
            .pointer("/adversarialReview/challenges/rejectedWithEvidence")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let unresolved = decision
            .pointer("/adversarialReview/challenges/unresolvedHighOrCritical")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        lines.push(format!("- Accepted challenges: {accepted}"));
        lines.push(format!("- Rejected with evidence: {rejected}"));
        lines.push(format!(
            "- Unresolved high/critical challenges: {}",
            unresolved.len()
        ));
        for challenge in unresolved {
            lines.push(format!(
                "  - **{} / {}**: {}",
                challenge["severity"].as_str().unwrap_or_default(),
                challenge["category"].as_str().unwrap_or_default(),
                challenge["counterArgument"].as_str().unwrap_or_default()
            ));
        }
    }
    lines.extend([
        String::new(),
        "## Integrity".to_owned(),
        format!(
            "- Decision SHA-256: `{}`",
            decision["integrity"]["decisionSha256"]
                .as_str()
                .unwrap_or_default()
        ),
    ]);
    Ok(format!("{}\n", lines.join("\n")))
}

pub fn tally_votes(root: &Path, run_id: &str) -> Result<Value> {
    let run_dir = run_dir_for(root, run_id)?;
    let _lock = RunLock::acquire(&run_dir)?;
    let request_file = run_dir.join("request.json");
    let manifest_file = run_dir.join("manifest.json");
    let mut request = read_json(&request_file)?;
    validate_request(&request)?;
    let decision_file = run_dir.join("decision.json");
    if request.get("status").and_then(Value::as_str) == Some("finalized") && decision_file.exists()
    {
        return read_json(&decision_file);
    }
    let mut manifest = read_json(&manifest_file)?;
    verify_request_hash(&request, &manifest)?;
    let adversarial_enabled = adversarial::enabled(&request);
    let review_performed = manifest
        .pointer("/adversarial/challengesSha256")
        .and_then(Value::as_str)
        .is_some();
    if adversarial_enabled {
        if review_performed {
            ensure!(
                request.get("status").and_then(Value::as_str) == Some("final_ready"),
                "Final votes are not ready."
            );
        } else {
            ensure!(
                adversarial::mode(&request) == "auto"
                    && matches!(
                        request.get("status").and_then(Value::as_str),
                        Some("ready" | "suspended_for_human_review")
                    ),
                "Review analysis is not ready."
            );
        }
    }

    let mut votes = Map::new();
    for persona in PERSONAS {
        let vote_file = if review_performed {
            run_dir
                .join("rounds/final/sealed")
                .join(format!("{persona}.json"))
        } else if adversarial_enabled {
            run_dir
                .join("rounds/initial/sealed")
                .join(format!("{persona}.json"))
        } else {
            run_dir.join("sealed").join(format!("{persona}.json"))
        };
        ensure!(vote_file.exists(), "Missing sealed vote: {persona}");
        let vote = read_json(&vote_file)?;
        validate_vote(&vote, Some(persona))?;
        ensure!(
            manifest
                .pointer(&if review_performed {
                    format!("/rounds/final/{persona}/sha256")
                } else if adversarial_enabled {
                    format!("/rounds/initial/{persona}/sha256")
                } else {
                    format!("/votes/{persona}/sha256")
                })
                .and_then(Value::as_str)
                == Some(&sha256_value(&vote)?),
            "Vote hash mismatch: {persona}"
        );
        votes.insert(persona.to_owned(), vote);
    }

    let mut approve = 0;
    let mut reject = 0;
    let mut abstain = 0;
    let mut unmitigated_critical = Vec::new();
    let mut supported_critical = Vec::new();
    let mut unsupported_critical = Vec::new();
    let mut high_risks = Vec::new();
    for (persona, vote) in &votes {
        match vote["decision"].as_str() {
            Some("approve") => approve += 1,
            Some("reject") => reject += 1,
            _ => abstain += 1,
        }
        for risk in vote["risks"].as_array().into_iter().flatten() {
            let risk_object = risk.as_object().expect("validated risk");
            if matches!(risk["severity"].as_str(), Some("high" | "critical")) {
                high_risks.push(with_persona(risk_object, persona));
            }
            if risk["severity"].as_str() == Some("critical")
                && risk["mitigated"].as_bool() == Some(false)
            {
                unmitigated_critical.push(with_persona(risk_object, persona));
                let sufficient = !matches!(vote["schemaVersion"].as_str(), Some("1.2" | "1.3"))
                    || risk["evidenceRefs"]
                        .as_array()
                        .is_some_and(|refs| !refs.is_empty());
                if sufficient {
                    supported_critical.push(with_persona(risk_object, persona));
                } else {
                    unsupported_critical.push(with_persona(risk_object, persona));
                }
            }
        }
    }
    let veto_enabled = request["voting"]["criticalRiskVeto"].as_bool() == Some(true);
    let veto_applied = veto_enabled && !supported_critical.is_empty();
    let has_approve_conditions = votes.values().any(|vote| {
        vote["decision"].as_str() == Some("approve")
            && vote["conditions"]
                .as_array()
                .is_some_and(|items| !items.is_empty())
    });
    let result = if veto_applied {
        "rejected_by_veto"
    } else if approve >= 2 {
        if has_approve_conditions {
            "approved_with_conditions"
        } else {
            "approved"
        }
    } else if reject >= 2 {
        "rejected"
    } else {
        "undecided"
    };
    let winning_decision = if result.starts_with("approved") {
        Some("approve")
    } else if result.starts_with("rejected") {
        Some("reject")
    } else {
        None
    };
    let mut confidences = votes
        .values()
        .filter_map(|vote| vote["confidence"].as_i64())
        .collect::<Vec<_>>();
    confidences.sort_unstable();

    let conditions = unique_strings(votes.values().flat_map(|vote| {
        if vote["decision"].as_str() == Some("approve") {
            vote["conditions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        }
    }));
    let assumptions = unique_strings(votes.values().flat_map(|vote| {
        vote["assumptions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>()
    }));
    let dissent = votes
        .iter()
        .filter(|(_, vote)| {
            winning_decision.is_none() || vote["decision"].as_str() != winning_decision
        })
        .map(|(persona, vote)| {
            json!({
                "persona": persona,
                "decision": vote["decision"],
                "summary": vote["summary"]
            })
        })
        .collect::<Vec<_>>();
    let mut persona_summaries = Map::new();
    let mut memory_candidates = Vec::new();
    let mut vote_hashes = Map::new();
    for persona in PERSONAS {
        let vote = &votes[persona];
        persona_summaries.insert(
            persona.to_owned(),
            json!({
                "decision": vote["decision"],
                "confidence": vote["confidence"],
                "summary": vote["summary"],
                "reasons": vote["reasons"],
                "conditions": vote["conditions"],
                "risks": vote["risks"]
            }),
        );
        vote_hashes.insert(
            persona.to_owned(),
            if review_performed {
                manifest["rounds"]["final"][persona]["sha256"].clone()
            } else if adversarial_enabled {
                manifest["rounds"]["initial"][persona]["sha256"].clone()
            } else {
                manifest["votes"][persona]["sha256"].clone()
            },
        );
        for (index, candidate) in vote["memoryCandidates"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let candidate_object = candidate.as_object().expect("validated candidate");
            let mut output = Map::new();
            output.insert(
                "id".to_owned(),
                Value::String(format!(
                    "{persona}-{}-{}",
                    index + 1,
                    &sha256_value(candidate)?[..8]
                )),
            );
            output.insert("persona".to_owned(), Value::String(persona.to_owned()));
            output.insert("sourceRunId".to_owned(), Value::String(run_id.to_owned()));
            output.insert("status".to_owned(), Value::String("candidate".to_owned()));
            for (key, value) in candidate_object {
                output.insert(key.clone(), value.clone());
            }
            memory_candidates.push(Value::Object(output));
        }
    }

    let finalized_at = now_iso();
    let challenge_resolution = if review_performed {
        adversarial::challenge_resolution(root, run_id, &votes)?
    } else {
        json!({"accepted": [], "rejectedWithEvidence": [], "unresolvedHighOrCritical": [], "suspendForHumanReview": false})
    };
    let changes = if review_performed {
        PERSONAS.iter().filter_map(|persona| {
            let initial = read_json(&run_dir.join("rounds/initial/sealed").join(format!("{persona}.json"))).ok()?;
            let final_vote = &votes[*persona];
            (initial["decision"] != final_vote["decision"] || initial["confidence"] != final_vote["confidence"]).then(|| json!({"persona": persona, "initialDecision": initial["decision"], "finalDecision": final_vote["decision"], "initialConfidence": initial["confidence"], "finalConfidence": final_vote["confidence"]}))
        }).collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let capability_suspended = manifest
        .pointer("/adversarial/suspensionReason")
        .and_then(Value::as_str)
        == Some("thomas_unavailable");
    let result = if challenge_resolution["suspendForHumanReview"].as_bool() == Some(true)
        || !unsupported_critical.is_empty()
        || capability_suspended
    {
        "suspended_for_human_review"
    } else {
        result
    };
    let review_analysis = if manifest
        .pointer("/adversarial/reviewAnalysisSha256")
        .is_some()
    {
        read_json(&run_dir.join("adversarial/review-analysis.json"))?
    } else {
        json!({"triggers": [], "reviewRequired": false})
    };
    let review_resolution = if result == "suspended_for_human_review" {
        "suspended_for_human_review"
    } else if review_performed {
        "completed"
    } else if veto_applied {
        "veto_without_review"
    } else {
        "not_required"
    };
    let mut decision = json!({
        "schemaVersion": "1.0",
        "runId": run_id,
        "finalizedAt": finalized_at,
        "executionMode": request["executionMode"],
        "decision": result,
        "voteCounts": {"approve": approve, "reject": reject, "abstain": abstain},
        "confidence": {
            "type": "self_reported",
            "calibrated": false,
            "min": confidences[0], "median": confidences[1], "max": confidences[2]
        },
        "veto": {
            "enabled": veto_enabled,
            "applied": veto_applied,
            "criticalRisks": unmitigated_critical,
            "supportedCriticalRisks": supported_critical,
            "unsupportedCriticalRisks": unsupported_critical
        },
        "conditions": conditions,
        "highRisks": high_risks,
        "dissent": dissent,
        "assumptions": assumptions,
        "personaSummaries": persona_summaries,
        "memoryCandidates": memory_candidates,
        "adversarialReview": {
            "enabled": adversarial_enabled,
            "mode": adversarial::mode(&request),
            "performed": review_performed,
            "reviewRequired": review_analysis["reviewRequired"],
            "reviewTriggers": review_analysis["triggers"],
            "resolution": review_resolution,
            "changes": changes,
            "challenges": challenge_resolution,
            "suspensionReason": manifest.pointer("/adversarial/suspensionReason").cloned().unwrap_or(Value::Null)
        },
        "integrity": {
            "requestSha256": manifest["requestSha256"],
            "voteSha256": vote_hashes,
            "reviewAnalysisSha256": manifest.pointer("/adversarial/reviewAnalysisSha256").cloned().unwrap_or(Value::Null),
            "decisionSha256": ""
        }
    });
    let decision_hash = sha256_value(&decision)?;
    decision["integrity"]["decisionSha256"] = Value::String(decision_hash.clone());
    atomic_write_json(&decision_file, &decision, 0o600)?;
    atomic_write_text(
        &run_dir.join("decision.md"),
        &decision_markdown(&decision)?,
        0o600,
    )?;
    request["status"] = Value::String(
        if result == "suspended_for_human_review" {
            "suspended_for_human_review"
        } else {
            "finalized"
        }
        .to_owned(),
    );
    atomic_write_json(&request_file, &request, 0o600)?;
    manifest["finalized"] = Value::Bool(true);
    manifest["finalizedAt"] = Value::String(finalized_at);
    manifest["decisionSha256"] = Value::String(decision_hash);
    atomic_write_json(&manifest_file, &manifest, 0o600)?;
    Ok(decision)
}

pub fn audit_run(root: &Path, run_id: &str) -> Result<Value> {
    let run_dir = run_dir_for(root, run_id)?;
    let request = read_json(&run_dir.join("request.json"))?;
    validate_request(&request)?;
    let manifest = read_json(&run_dir.join("manifest.json"))?;
    let mut errors = Vec::new();
    if manifest.get("requestSha256").and_then(Value::as_str) != Some(&hash_request(&request)?) {
        errors.push("request hash mismatch".to_owned());
    }
    let adversarial_enabled = adversarial::enabled(&request);
    let review_performed = manifest
        .pointer("/adversarial/challengesSha256")
        .and_then(Value::as_str)
        .is_some();
    for persona in PERSONAS {
        let vote_file = if review_performed {
            run_dir
                .join("rounds/final/sealed")
                .join(format!("{persona}.json"))
        } else if adversarial_enabled {
            run_dir
                .join("rounds/initial/sealed")
                .join(format!("{persona}.json"))
        } else {
            run_dir.join("sealed").join(format!("{persona}.json"))
        };
        if !vote_file.exists() {
            errors.push(format!("missing {persona} vote"));
            continue;
        }
        let vote = read_json(&vote_file)?;
        validate_vote(&vote, Some(persona))?;
        if manifest
            .pointer(&if review_performed {
                format!("/rounds/final/{persona}/sha256")
            } else if adversarial_enabled {
                format!("/rounds/initial/{persona}/sha256")
            } else {
                format!("/votes/{persona}/sha256")
            })
            .and_then(Value::as_str)
            != Some(&sha256_value(&vote)?)
        {
            errors.push(format!("{persona} vote hash mismatch"));
        }
    }
    if adversarial_enabled {
        let mut initial_votes = Map::new();
        for persona in PERSONAS {
            let path = run_dir
                .join("rounds/initial/sealed")
                .join(format!("{persona}.json"));
            if !path.exists() {
                errors.push(format!("missing initial {persona} vote"));
                continue;
            }
            let vote = read_json(&path)?;
            if let Err(error) = validate_vote(&vote, Some(persona)) {
                errors.push(format!("invalid initial {persona} vote: {error}"));
                continue;
            }
            if manifest
                .pointer(&format!("/rounds/initial/{persona}/sha256"))
                .and_then(Value::as_str)
                != Some(&sha256_value(&vote)?)
            {
                errors.push(format!("initial {persona} vote hash mismatch"));
            }
            initial_votes.insert(persona.to_owned(), vote);
        }
        let mut protected = vec![("review-analysis.json", "reviewAnalysisSha256")];
        if review_performed {
            protected.extend([
                ("mapping.json", "mappingSha256"),
                ("input.json", "inputSha256"),
                ("challenges.json", "challengesSha256"),
            ]);
        }
        for (name, field) in protected {
            let path = run_dir.join("adversarial").join(name);
            let expected = manifest
                .pointer(&format!("/adversarial/{field}"))
                .and_then(Value::as_str);
            if expected.is_some()
                && (!path.exists()
                    || manifest
                        .pointer(&format!("/adversarial/{field}"))
                        .and_then(Value::as_str)
                        != read_json(&path)
                            .ok()
                            .and_then(|v| sha256_value(&v).ok())
                            .as_deref())
            {
                errors.push(format!("adversarial {name} hash mismatch"));
            }
        }
        let analysis_path = run_dir.join("adversarial/review-analysis.json");
        if manifest
            .pointer("/adversarial/reviewAnalysisSha256")
            .is_some()
            && initial_votes.len() == PERSONAS.len()
            && read_json(&analysis_path).ok().as_ref()
                != adversarial::analyze_votes(&request, &initial_votes)
                    .ok()
                    .as_ref()
        {
            errors.push("review analysis does not match initial votes".to_owned());
        }
    }
    let decision_file = run_dir.join("decision.json");
    if decision_file.exists() {
        let mut decision = read_json(&decision_file)?;
        let recorded = decision
            .pointer("/integrity/decisionSha256")
            .and_then(Value::as_str)
            .map(str::to_owned);
        decision["integrity"]["decisionSha256"] = Value::String(String::new());
        let expected = sha256_value(&decision)?;
        if recorded.as_deref() != Some(&expected) {
            errors.push("decision content hash mismatch".to_owned());
        }
        if manifest.get("decisionSha256").and_then(Value::as_str) != Some(&expected) {
            errors.push("manifest decision hash mismatch".to_owned());
        }
        if decision
            .pointer("/integrity/reviewAnalysisSha256")
            .and_then(Value::as_str)
            != manifest
                .pointer("/adversarial/reviewAnalysisSha256")
                .and_then(Value::as_str)
        {
            errors.push("decision review analysis hash mismatch".to_owned());
        }
    }
    Ok(json!({"runId": run_id, "valid": errors.is_empty(), "errors": errors}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_values_keep_first_seen_order() {
        assert_eq!(
            unique_strings(["one".to_owned(), "two".to_owned(), "one".to_owned()]),
            ["one", "two"]
        );
    }
}

use assert_cmd::Command;
use magi_council_cli::sha256_value;
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn project() -> TempDir {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    let skill = root.join(".agents").join("skills").join("magi-council");
    let state = root.join(".magi");
    fs::create_dir_all(&skill).unwrap();
    fs::create_dir_all(skill.join("references")).unwrap();
    fs::create_dir_all(state.join("constitution")).unwrap();
    fs::create_dir_all(state.join("memory").join("personas")).unwrap();
    fs::create_dir_all(state.join("runs")).unwrap();
    fs::write(skill.join("SKILL.md"), "# Test skill\n").unwrap();
    for persona in ["melchior", "balthasar", "casper"] {
        fs::write(
            skill
                .join("references")
                .join(format!("persona-{persona}.md")),
            format!("# {persona} foundation\n"),
        )
        .unwrap();
        fs::write(
            state
                .join("memory")
                .join("personas")
                .join(format!("{persona}.json")),
            serde_json::to_vec(&json!({
                "schemaVersion": "1.0",
                "persona": persona,
                "entries": []
            }))
            .unwrap(),
        )
        .unwrap();
    }
    fs::write(
        state.join("config.json"),
        serde_json::to_vec(&json!({
            "schemaVersion": "1.0",
            "voting": {"method": "majority", "criticalRiskVeto": true},
            "memory": {"maxItemsPerPersona": 12}
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        state.join("constitution").join("principles.md"),
        "# Principles\n",
    )
    .unwrap();
    project
}

fn magi(root: &Path) -> Command {
    let mut command = Command::cargo_bin("magi").unwrap();
    command.current_dir(root);
    command
}

fn output_json(command: &mut Command) -> Value {
    let output = command.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&output).unwrap()
}

fn vote(run_id: &str, persona: &str, decision: &str, conditions: Value) -> Value {
    json!({
        "schemaVersion": "1.0",
        "runId": run_id,
        "persona": persona,
        "decision": decision,
        "confidence": 80,
        "summary": format!("{persona} summary"),
        "reasons": [{"code": "R1", "statement": "Reason", "evidence": []}],
        "conditions": conditions,
        "risks": [],
        "assumptions": [],
        "memoryCandidates": []
    })
}

#[test]
fn creates_imports_tallies_and_audits_run() {
    let project = project();
    let created = output_json(
        magi(project.path())
            .args(["run", "create", "--stdin"])
            .write_stdin(r#"{"question":"Release?","context":{"evidence":[]}}"#),
    );
    let run_id = created["runId"].as_str().unwrap();
    let votes = json!([
        vote(run_id, "melchior", "approve", json!(["Add tests"])),
        vote(run_id, "balthasar", "reject", json!([])),
        vote(run_id, "casper", "approve", json!([]))
    ]);
    let mut votes = votes;
    votes[2]["memoryCandidates"] = json!([{
        "principle": "Use staged rollout when rollback exists.",
        "scopes": ["release"],
        "applicableWhen": ["Rollback is available"],
        "notApplicableWhen": ["An unmitigated critical risk exists"],
        "rationale": "Staging contains delivery risk."
    }]);
    output_json(
        magi(project.path())
            .args(["run", "import-votes", run_id])
            .write_stdin(votes.to_string()),
    );
    let mut changed_votes = votes;
    changed_votes[0]["decision"] = Value::String("reject".to_owned());
    let rejected = magi(project.path())
        .args(["run", "import-votes", run_id])
        .write_stdin(changed_votes.to_string())
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    assert!(
        String::from_utf8(rejected)
            .unwrap()
            .contains("overwriting is forbidden")
    );

    let decision = output_json(magi(project.path()).args(["run", "tally", run_id]));
    assert_eq!(decision["decision"], "approved_with_conditions");
    assert_eq!(decision["voteCounts"]["approve"], 2);

    let candidate_id = decision["memoryCandidates"][0]["id"].as_str().unwrap();
    let approved = output_json(magi(project.path()).args([
        "memory",
        "approve",
        run_id,
        candidate_id,
        "--approved-by",
        "test-reviewer",
    ]));
    assert_eq!(approved["approved"], true);
    let persona_output = magi(project.path())
        .args(["persona", "load", "casper"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(
        String::from_utf8(persona_output)
            .unwrap()
            .contains("Use staged rollout when rollback exists.")
    );

    let audit = output_json(magi(project.path()).args(["run", "audit", run_id]));
    assert_eq!(audit["valid"], true);
    assert_eq!(audit["errors"], json!([]));
}

#[test]
fn guard_denies_protected_read_and_allows_source_read() {
    let project = project();
    let protected_path = format!("{}{}", ".magi", "/runs/example/sealed/melchior.json");
    let denied = output_json(
        magi(project.path())
            .args(["hook", "guard-tool-use"])
            .write_stdin(
                json!({"toolName": "view", "toolArgs": {"path": protected_path}}).to_string(),
            ),
    );
    assert_eq!(denied["permissionDecision"], "deny");

    let allowed = output_json(
        magi(project.path())
            .args(["hook", "guard-tool-use"])
            .write_stdin(
                json!({"toolName": "view", "toolArgs": {"path": "src/main.rs"}}).to_string(),
            ),
    );
    assert_eq!(allowed["permissionDecision"], "allow");
}

#[test]
fn sealing_hook_fails_closed_with_json() {
    let project = project();
    let output = output_json(
        magi(project.path())
            .args(["hook", "subagent-stop"])
            .write_stdin("not json"),
    );
    assert_eq!(output["decision"], "block");
    assert!(
        output["reason"]
            .as_str()
            .unwrap()
            .contains("MAGI sealing failed")
    );
}

#[test]
fn host_hooks_inject_seal_verify_and_redact() {
    let project = project();
    let context = output_json(
        magi(project.path())
            .args(["hook", "subagent-start"])
            .write_stdin(json!({"agentName": "magi-melchior"}).to_string()),
    );
    assert!(
        context["additionalContext"]
            .as_str()
            .unwrap()
            .contains("melchior foundation")
    );

    let created =
        output_json(magi(project.path()).args(["run", "create", "--question", "Release?"]));
    let run_id = created["runId"].as_str().unwrap();
    let melchior_vote = vote(run_id, "melchior", "approve", json!([]));
    let blocked = output_json(
        magi(project.path())
            .args(["hook", "claude-subagent-stop"])
            .write_stdin(json!({"response": melchior_vote.to_string()}).to_string()),
    );
    assert_eq!(blocked["decision"], "block");
    assert!(
        blocked["reason"]
            .as_str()
            .unwrap()
            .contains("vote body must never reach the parent")
    );

    let vote_hash = sha256_value(&melchior_vote).unwrap();
    let receipt = format!(
        "MELCHIOR: VOTE_SEALED run={run_id} sha256={}",
        &vote_hash[..16]
    );
    let accepted = output_json(
        magi(project.path())
            .args(["hook", "claude-subagent-stop"])
            .write_stdin(json!({"response": receipt}).to_string()),
    );
    assert_eq!(accepted, json!({}));

    let balthasar_vote = vote(run_id, "balthasar", "reject", json!([]));
    let github_receipt = output_json(
        magi(project.path())
            .args(["hook", "subagent-stop"])
            .write_stdin(
                json!({
                    "agentName": "magi-balthasar",
                    "agentId": "agent-balthasar",
                    "response": balthasar_vote.to_string()
                })
                .to_string(),
            ),
    );
    assert_eq!(github_receipt["decision"], "allow");
    assert!(
        github_receipt["modifiedResponse"]
            .as_str()
            .unwrap()
            .starts_with("BALTHASAR: VOTE_SEALED")
    );

    let protected_path = format!("{}{}", ".magi", "/runs/example/manifest.json");
    let redacted = output_json(
        magi(project.path())
            .args(["hook", "redact-tool-result"])
            .write_stdin(json!({"toolResult": protected_path}).to_string()),
    );
    assert_eq!(
        redacted["modifiedResult"]["textResultForLlm"],
        "[MAGI protected content redacted by policy Hook]"
    );
}

#[test]
fn init_creates_defaults_without_overwriting_policy() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    let skill = root.join(".agents").join("skills").join("magi-council");
    fs::create_dir_all(skill.join("templates")).unwrap();
    fs::write(skill.join("SKILL.md"), "# Test skill\n").unwrap();
    fs::write(
        skill.join("templates").join("config.json"),
        r#"{"schemaVersion":"1.0","voting":{"method":"majority","criticalRiskVeto":true}}"#,
    )
    .unwrap();
    fs::write(
        skill.join("templates").join("constitution.md"),
        "# Default constitution\n",
    )
    .unwrap();

    magi(root).arg("init").assert().success();
    let state = root.join(".magi");
    assert!(state.join("config.json").exists());
    assert!(
        state
            .join("memory")
            .join("personas")
            .join("melchior.json")
            .exists()
    );
    fs::write(state.join("config.json"), "custom-policy").unwrap();
    magi(root).arg("init").assert().success();
    assert_eq!(
        fs::read_to_string(state.join("config.json")).unwrap(),
        "custom-policy"
    );
}

#[test]
fn adversarial_review_seals_two_rounds_and_tallies_only_final_votes() {
    let project = project();
    let created = output_json(
        magi(project.path())
            .args(["run", "create", "--stdin"])
            .write_stdin(r#"{"question":"Release?","context":{},"adversarialReview":true}"#),
    );
    let run_id = created["runId"].as_str().unwrap();
    for persona in ["melchior", "balthasar", "casper"] {
        let sealed = output_json(
            magi(project.path())
                .args(["vote", "seal", "--persona", persona, "--round", "initial"])
                .write_stdin(vote(run_id, persona, "reject", json!([])).to_string()),
        );
        let accepted = output_json(
            magi(project.path())
                .args(["hook", "claude-subagent-stop"])
                .write_stdin(json!({"response": sealed["receipt"]}).to_string()),
        );
        assert_eq!(accepted, json!({}));
    }
    let mut premature_final = vote(run_id, "melchior", "approve", json!([]));
    premature_final["challengeResponses"] = json!([]);
    let premature = output_json(
        magi(project.path())
            .args(["hook", "claude-subagent-stop"])
            .write_stdin(json!({"response": premature_final.to_string()}).to_string()),
    );
    assert_eq!(premature["decision"], "block");
    assert!(
        premature["reason"]
            .as_str()
            .unwrap()
            .contains("Initial vote must not include challengeResponses")
    );
    let prepared = output_json(magi(project.path()).args(["run", "prepare-adversarial", run_id]));
    assert_eq!(prepared["prepared"], true);
    assert_eq!(prepared["candidateCount"], 3);
    assert!(prepared.get("candidates").is_none());
    let thomas_context = output_json(
        magi(project.path())
            .args(["hook", "subagent-start"])
            .write_stdin(json!({"agent_type": "magi-thomas"}).to_string()),
    );
    assert!(
        thomas_context["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains(run_id)
    );
    assert!(
        thomas_context["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("maxChallengesPerCandidate")
    );
    let challenges = json!({
        "schemaVersion": "1.0", "runId": run_id,
        "challenges": [{
            "id": "challenge-001", "targetCandidate": "candidate-a", "category": "security",
            "severity": "critical", "claimUnderChallenge": "Safe release", "counterArgument": "Rollback is unproven",
            "falsificationTest": {"description": "Exercise rollback", "expectedEvidence": ["test log"]}, "status": "unresolved"
        }]
    });
    let transcript = project.path().join("subagent-transcript.jsonl");
    fs::write(
        &transcript,
        json!({"type": "assistant", "message": {"content": challenges.to_string()}}).to_string(),
    )
    .unwrap();
    let thomas_blocked = output_json(
        magi(project.path())
            .args(["hook", "subagent-stop"])
            .write_stdin(
                json!({
                    "agent_type": "magi-thomas",
                    "transcript_path": transcript,
                    "stop_hook_active": false
                })
                .to_string(),
            ),
    );
    assert_eq!(thomas_blocked["decision"], "block");
    let thomas_receipt = thomas_blocked["reason"]
        .as_str()
        .unwrap()
        .split("nothing else: ")
        .nth(1)
        .unwrap();
    fs::write(
        &transcript,
        json!({"type": "assistant", "message": {"content": thomas_receipt}}).to_string(),
    )
    .unwrap();
    let thomas_accepted = output_json(
        magi(project.path())
            .args(["hook", "subagent-stop"])
            .write_stdin(
                json!({
                    "agent_type": "magi-thomas",
                    "transcript_path": transcript,
                    "stop_hook_active": true
                })
                .to_string(),
            ),
    );
    assert_eq!(thomas_accepted, json!({}));
    let claude_thomas_accepted = output_json(
        magi(project.path())
            .args(["hook", "claude-subagent-stop"])
            .write_stdin(json!({"response": thomas_receipt}).to_string()),
    );
    assert_eq!(claude_thomas_accepted, json!({}));

    let final_context = output_json(
        magi(project.path())
            .args(["hook", "subagent-start"])
            .write_stdin(json!({"agent_type": "magi-melchior"}).to_string()),
    );
    assert!(
        final_context["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("initialVote")
    );
    for persona in ["melchior", "balthasar", "casper"] {
        let mut final_vote = vote(run_id, persona, "approve", json!([]));
        final_vote["challengeResponses"] = json!([{
            "challengeId": "challenge-001", "response": "uphold", "rebuttal": "Rollback was tested",
            "acceptedConditions": [], "evidence": ["test log"]
        }]);
        if persona == "melchior" {
            fs::write(
                &transcript,
                json!({"type": "assistant", "message": {"content": final_vote.to_string()}})
                    .to_string(),
            )
            .unwrap();
            let blocked = output_json(
                magi(project.path())
                    .args(["hook", "subagent-stop"])
                    .write_stdin(
                        json!({
                            "agent_type": "magi-melchior",
                            "transcript_path": transcript,
                            "stop_hook_active": false
                        })
                        .to_string(),
                    ),
            );
            assert_eq!(blocked["decision"], "block");
            let receipt = blocked["reason"]
                .as_str()
                .unwrap()
                .split("nothing else: ")
                .nth(1)
                .unwrap();
            fs::write(
                &transcript,
                json!({"type": "assistant", "message": {"content": receipt}}).to_string(),
            )
            .unwrap();
            let accepted = output_json(
                magi(project.path())
                    .args(["hook", "subagent-stop"])
                    .write_stdin(
                        json!({
                            "agent_type": "magi-melchior",
                            "transcript_path": transcript,
                            "stop_hook_active": true
                        })
                        .to_string(),
                    ),
            );
            assert_eq!(accepted, json!({}));
        } else {
            let sealed = output_json(
                magi(project.path())
                    .args(["vote", "seal", "--persona", persona, "--round", "final"])
                    .write_stdin(final_vote.to_string()),
            );
            let accepted = output_json(
                magi(project.path())
                    .args(["hook", "claude-subagent-stop"])
                    .write_stdin(json!({"response": sealed["receipt"]}).to_string()),
            );
            assert_eq!(accepted, json!({}));
        }
    }
    let decision = output_json(magi(project.path()).args(["run", "tally", run_id]));
    assert_eq!(decision["decision"], "approved");
    assert_eq!(decision["voteCounts"]["approve"], 3);
    assert_eq!(
        decision["adversarialReview"]["changes"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let audit = output_json(magi(project.path()).args(["run", "audit", run_id]));
    assert_eq!(audit["valid"], true);
}

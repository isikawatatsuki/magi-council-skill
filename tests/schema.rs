use magi_council_cli::core::{validate_request, validate_vote};
use magi_council_cli::lifecycle::create_run;
use serde_json::{Value, json};
use std::fs;
use tempfile::TempDir;

fn schema(source: &str) -> Value {
    serde_json::from_str(source).expect("schema must be valid JSON")
}

fn request() -> Value {
    json!({
        "schemaVersion": "1.1",
        "runId": "magi-20260814000000-abcdef123456",
        "createdAt": "2026-08-14T00:00:00Z",
        "status": "collecting_initial",
        "executionMode": "sealed-subagents",
        "question": "Release?",
        "context": {"evidence": []},
        "expectedPersonas": ["melchior", "balthasar", "casper"],
        "voting": {"method": "majority", "criticalRiskVeto": true},
        "adversarialReview": {
            "enabled": true,
            "anonymizePersonas": true,
            "maxChallengesPerCandidate": 5,
            "minimumSeverity": "medium",
            "requireFalsificationTest": true,
            "unresolvedCriticalAction": "human_review"
        }
    })
}

fn vote() -> Value {
    json!({
        "schemaVersion": "1.1",
        "runId": "magi-20260814000000-abcdef123456",
        "persona": "melchior",
        "decision": "approve",
        "confidence": 90,
        "summary": "The evidence is traceable.",
        "reasons": [{
            "code": "TRACEABLE",
            "statement": "Code, tests, and review records support the decision.",
            "evidence": [
                {"id": "ev-file-auth", "type": "file", "claim": "The guard is implemented.", "observedAt": "2026-08-14T00:00:00Z", "path": "src/auth.rs", "lineStart": 10, "lineEnd": 24, "commitSha": "abcdef1"},
                {"id": "ev-test-auth", "type": "test", "claim": "The regression test passes.", "observedAt": "2026-08-14T00:01:00Z", "command": "cargo test auth", "outcome": "passed", "output": "1 passed", "commitSha": "abcdef1"},
                {"id": "ev-issue-12", "type": "issue", "claim": "The issue defines the contract.", "observedAt": "2026-08-14T00:02:00Z", "url": "https://github.com/isikawatatsuki/magi-council-skill/issues/12", "title": "Schema parity"},
                {"id": "ev-pr-25", "type": "pull_request", "claim": "The prerequisite was merged.", "observedAt": "2026-08-14T00:03:00Z", "url": "https://github.com/isikawatatsuki/magi-council-skill/pull/25"},
                {"id": "ev-doc-report", "type": "external_document", "claim": "The research report motivates traceability.", "observedAt": "2026-08-14T00:04:00Z", "url": "https://www.anthropic.com/research/multi-agent-research-system"}
            ]
        }],
        "conditions": [],
        "risks": [],
        "assumptions": [],
        "memoryCandidates": [],
        "challengeResponses": [{
            "challengeId": "challenge-001",
            "response": "uphold",
            "rebuttal": "The test is reproducible.",
            "acceptedConditions": [],
            "evidence": ["ev-test-auth"]
        }]
    })
}

fn schema_accepts(schema: &Value, instance: &Value) -> bool {
    jsonschema::draft202012::options()
        .should_validate_formats(true)
        .build(schema)
        .expect("schema must compile")
        .is_valid(instance)
}

fn runtime_project(adversarial_mode: &str) -> TempDir {
    let project = tempfile::tempdir().unwrap();
    let state = project.path().join(".magi");
    fs::create_dir_all(state.join("runs")).unwrap();
    fs::write(
        state.join("config.json"),
        serde_json::to_vec(&json!({
            "schemaVersion": "1.0",
            "voting": {"method": "majority", "criticalRiskVeto": true},
            "adversarialReview": {"mode": adversarial_mode}
        }))
        .unwrap(),
    )
    .unwrap();
    project
}

#[test]
fn schema_accepts_runtime_generated_normal_and_adversarial_requests() {
    let request_schema = schema(include_str!(
        "../.agents/skills/magi-council/schemas/request.schema.json"
    ));
    for mode in ["disabled", "enabled"] {
        let project = runtime_project(mode);
        let created = create_run(
            project.path(),
            &json!({"question": "Release?", "context": {"evidence": []}}),
        )
        .unwrap();
        let request_path = project
            .path()
            .join(".magi/runs")
            .join(created["runId"].as_str().unwrap())
            .join("request.json");
        let generated: Value = serde_json::from_slice(&fs::read(request_path).unwrap()).unwrap();

        assert_eq!(generated["schemaVersion"], "1.1");
        assert!(schema_accepts(&request_schema, &generated));
        validate_request(&generated).unwrap();
    }
}

#[test]
fn schemas_accept_runtime_v11_request_and_structured_vote() {
    let request_schema = schema(include_str!(
        "../.agents/skills/magi-council/schemas/request.schema.json"
    ));
    let vote_schema = schema(include_str!(
        "../.agents/skills/magi-council/schemas/vote.schema.json"
    ));
    let request = request();
    let vote = vote();

    assert!(schema_accepts(&request_schema, &request));
    assert!(schema_accepts(&vote_schema, &vote));
    validate_request(&request).unwrap();
    validate_vote(&vote, Some("melchior")).unwrap();
}

#[test]
fn schemas_and_runtime_reject_malformed_v11_evidence() {
    let vote_schema = schema(include_str!(
        "../.agents/skills/magi-council/schemas/vote.schema.json"
    ));
    let cases = [
        (
            "missing locator",
            json!({"id": "ev-file-bad", "type": "file", "claim": "Missing path.", "observedAt": "2026-08-14T00:00:00Z"}),
        ),
        (
            "unknown field",
            json!({"id": "ev-test-bad", "type": "test", "claim": "Unexpected data.", "observedAt": "2026-08-14T00:00:00Z", "command": "cargo test", "outcome": "passed", "confidence": 1}),
        ),
        (
            "bad timestamp",
            json!({"id": "ev-issue-bad", "type": "issue", "claim": "Bad time.", "observedAt": "yesterday", "url": "https://example.com/issues/1"}),
        ),
        (
            "bad URL",
            json!({"id": "ev-doc-bad", "type": "external_document", "claim": "Bad URL.", "observedAt": "2026-08-14T00:00:00Z", "url": "not-a-url"}),
        ),
    ];

    for (label, evidence) in cases {
        let mut candidate = vote();
        candidate["reasons"][0]["evidence"] = json!([evidence]);
        assert!(
            !schema_accepts(&vote_schema, &candidate),
            "schema accepted {label}"
        );
        assert!(
            validate_vote(&candidate, None).is_err(),
            "runtime accepted {label}"
        );
    }
}

#[test]
fn runtime_rejects_reversed_file_line_range() {
    let mut candidate = vote();
    candidate["reasons"][0]["evidence"] = json!([{
        "id": "ev-file-lines", "type": "file", "claim": "Lines reversed.",
        "observedAt": "2026-08-14T00:00:00Z", "path": "src/lib.rs",
        "lineStart": 20, "lineEnd": 10
    }]);
    assert!(validate_vote(&candidate, None).is_err());
}

#[test]
fn runtime_rejects_duplicate_evidence_ids_across_reasons() {
    let mut candidate = vote();
    let duplicate = json!({
        "id": "ev-file-auth", "type": "file", "claim": "A different claim.",
        "observedAt": "2026-08-14T00:05:00Z", "path": "src/other.rs"
    });
    candidate["reasons"].as_array_mut().unwrap().push(json!({
        "code": "DUPLICATE",
        "statement": "Evidence IDs identify one record within a vote.",
        "evidence": [duplicate]
    }));
    assert!(validate_vote(&candidate, None).is_err());
}

#[test]
fn legacy_v10_remains_readable_for_audit_compatibility() {
    let vote_schema = schema(include_str!(
        "../.agents/skills/magi-council/schemas/vote.schema.json"
    ));
    let mut legacy = vote();
    legacy["schemaVersion"] = json!("1.0");
    legacy["reasons"][0]["evidence"] = json!([{"legacyNote": "preserved"}]);

    assert!(schema_accepts(&vote_schema, &legacy));
    validate_vote(&legacy, Some("melchior")).unwrap();
}

#[test]
fn v11_request_rejects_unknown_fields_in_schema_and_runtime() {
    let request_schema = schema(include_str!(
        "../.agents/skills/magi-council/schemas/request.schema.json"
    ));
    let mut candidate = request();
    candidate["voting"]["threshold"] = json!(2);

    assert!(!schema_accepts(&request_schema, &candidate));
    assert!(validate_request(&candidate).is_err());
}

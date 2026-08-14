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
        "schemaVersion": "1.2",
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
            "mode": "enabled",
            "thomasAvailable": true,
            "anonymizePersonas": true,
            "maxChallengesPerCandidate": 5,
            "minimumSeverity": "medium",
            "requireFalsificationTest": true,
            "unresolvedCriticalAction": "human_review"
        },
        "riskProfile": {"highRiskDomains": []}
    })
}

fn vote() -> Value {
    json!({
        "schemaVersion": "1.3",
        "runId": "magi-20260814000000-abcdef123456",
        "persona": "melchior",
        "decision": "approve",
        "confidence": 90,
        "confidenceType": "self_reported",
        "confidenceCalibrated": false,
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
        "evidenceAssessments": [
            {"evidenceRef": "ev-file-auth", "impact": "supports_approve"},
            {"evidenceRef": "ev-test-auth", "impact": "supports_approve"},
            {"evidenceRef": "ev-issue-12", "impact": "supports_approve"},
            {"evidenceRef": "ev-pr-25", "impact": "supports_approve"},
            {"evidenceRef": "ev-doc-report", "impact": "supports_approve"}
        ],
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

        assert_eq!(generated["schemaVersion"], "1.2");
        assert!(schema_accepts(&request_schema, &generated));
        validate_request(&generated).unwrap();
    }
}

#[test]
fn schemas_accept_runtime_v12_request_and_v13_structured_vote() {
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
fn schemas_and_runtime_reject_malformed_v13_evidence() {
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
fn structured_v11_remains_readable_without_v12_relationship_fields() {
    let request_schema = schema(include_str!(
        "../.agents/skills/magi-council/schemas/request.schema.json"
    ));
    let vote_schema = schema(include_str!(
        "../.agents/skills/magi-council/schemas/vote.schema.json"
    ));

    let mut old_request = request();
    old_request["schemaVersion"] = json!("1.1");
    old_request["adversarialReview"]
        .as_object_mut()
        .unwrap()
        .remove("mode");
    old_request["adversarialReview"]
        .as_object_mut()
        .unwrap()
        .remove("thomasAvailable");
    old_request.as_object_mut().unwrap().remove("riskProfile");
    assert!(schema_accepts(&request_schema, &old_request));
    validate_request(&old_request).unwrap();

    let mut old_vote = vote();
    old_vote["schemaVersion"] = json!("1.1");
    old_vote
        .as_object_mut()
        .unwrap()
        .remove("evidenceAssessments");
    assert!(schema_accepts(&vote_schema, &old_vote));
    validate_vote(&old_vote, Some("melchior")).unwrap();
}

#[test]
fn evidence_v12_remains_readable_without_v13_confidence_metadata() {
    let vote_schema = schema(include_str!(
        "../.agents/skills/magi-council/schemas/vote.schema.json"
    ));
    let mut old_vote = vote();
    old_vote["schemaVersion"] = json!("1.2");
    old_vote.as_object_mut().unwrap().remove("confidenceType");
    old_vote
        .as_object_mut()
        .unwrap()
        .remove("confidenceCalibrated");
    assert!(schema_accepts(&vote_schema, &old_vote));
    validate_vote(&old_vote, Some("melchior")).unwrap();
}

#[test]
fn v13_requires_self_reported_uncalibrated_confidence_metadata() {
    let vote_schema = schema(include_str!(
        "../.agents/skills/magi-council/schemas/vote.schema.json"
    ));
    let mut missing = vote();
    missing.as_object_mut().unwrap().remove("confidenceType");
    assert!(!schema_accepts(&vote_schema, &missing));
    assert!(validate_vote(&missing, None).is_err());

    let mut falsely_calibrated = vote();
    falsely_calibrated["confidenceCalibrated"] = json!(true);
    assert!(!schema_accepts(&vote_schema, &falsely_calibrated));
    assert!(validate_vote(&falsely_calibrated, None).is_err());
}

#[test]
fn v12_request_rejects_unknown_fields_in_schema_and_runtime() {
    let request_schema = schema(include_str!(
        "../.agents/skills/magi-council/schemas/request.schema.json"
    ));
    let mut candidate = request();
    candidate["voting"]["threshold"] = json!(2);

    assert!(!schema_accepts(&request_schema, &candidate));
    assert!(validate_request(&candidate).is_err());
}

#[test]
fn v13_schema_requires_evidence_relationships_and_runtime_resolves_refs() {
    let vote_schema = schema(include_str!(
        "../.agents/skills/magi-council/schemas/vote.schema.json"
    ));
    let mut missing_assessments = vote();
    missing_assessments
        .as_object_mut()
        .unwrap()
        .remove("evidenceAssessments");
    assert!(!schema_accepts(&vote_schema, &missing_assessments));

    let mut missing_risk_refs = vote();
    missing_risk_refs["risks"] = json!([{
        "severity": "critical", "statement": "Risk", "mitigated": false
    }]);
    assert!(!schema_accepts(&vote_schema, &missing_risk_refs));

    let mut unresolved = vote();
    unresolved["risks"] = json!([{
        "severity": "critical", "statement": "Risk", "mitigated": false,
        "evidenceRefs": ["ev-does-not-exist"]
    }]);
    assert!(schema_accepts(&vote_schema, &unresolved));
    assert!(validate_vote(&unresolved, None).is_err());

    unresolved["risks"] = json!([]);
    unresolved["evidenceAssessments"] = json!([{
        "evidenceRef": "ev-does-not-exist", "impact": "supports_reject"
    }]);
    assert!(validate_vote(&unresolved, None).is_err());
}

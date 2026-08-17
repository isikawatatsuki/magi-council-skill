use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

pub const REQUIRED_CAPABILITIES: [&str; 7] = [
    "customAgents",
    "isolatedSubagentContexts",
    "subagentStartHook",
    "subagentStopHook",
    "preToolUseHook",
    "postToolUseHook",
    "voteBodyConfidential",
];

pub fn evaluate_host_capabilities(value: Option<&Value>) -> Value {
    let Some(value) = value else {
        return json!({
            "status": "unknown",
            "sealedEligible": false,
            "reason": "Host capability metadata was not provided; capabilities were not inferred."
        });
    };
    let Some(object) = value.as_object() else {
        return json!({
            "status": "invalid",
            "sealedEligible": false,
            "reason": "hostCapabilities must be an object."
        });
    };
    let missing = REQUIRED_CAPABILITIES
        .iter()
        .filter(|name| !object.contains_key(**name))
        .copied()
        .collect::<Vec<_>>();
    let unknown = REQUIRED_CAPABILITIES
        .iter()
        .filter(|name| object.get(**name).is_some_and(|value| !value.is_boolean()))
        .copied()
        .collect::<Vec<_>>();
    let unavailable = REQUIRED_CAPABILITIES
        .iter()
        .filter(|name| object.get(**name).and_then(Value::as_bool) == Some(false))
        .copied()
        .collect::<Vec<_>>();
    let eligible = missing.is_empty() && unknown.is_empty() && unavailable.is_empty();
    let status = if eligible {
        "available"
    } else if !unavailable.is_empty() {
        "unavailable"
    } else {
        "unknown"
    };
    let reason = if eligible {
        "All required sealed-subagents capabilities were explicitly attested."
    } else if !unavailable.is_empty() {
        "One or more required sealed-subagents capabilities are explicitly unavailable."
    } else {
        "Required sealed-subagents capabilities are missing or unknown."
    };
    json!({
        "status": status,
        "sealedEligible": eligible,
        "reason": reason,
        "missing": missing,
        "unknown": unknown,
        "unavailable": unavailable
    })
}

fn check(id: &str, status: &str, reason: impl Into<String>) -> Value {
    json!({"id": id, "status": status, "reason": reason.into()})
}

fn all_files(root: &Path, relative: &[String]) -> bool {
    relative.iter().all(|path| root.join(path).is_file())
}

pub fn doctor(root: &Path, capabilities_path: Option<&Path>) -> Result<Value> {
    let mut checks = Vec::new();
    let state_ok = root.join(".magi/config.json").is_file()
        && root.join(".magi/constitution/principles.md").is_file();
    checks.push(check(
        "project_state",
        if state_ok { "ok" } else { "fail" },
        if state_ok {
            ".magi configuration and constitution are present."
        } else {
            ".magi/config.json or constitution/principles.md is missing; run magi init."
        },
    ));

    let skill = root.join(".agents/skills/magi-council/SKILL.md");
    let skill_text = fs::read_to_string(&skill).unwrap_or_default();
    let version = env!("CARGO_PKG_VERSION");
    let version_ok = skill_text.contains(&format!("version: \"{version}\""));
    checks.push(check(
        "version_alignment",
        if version_ok { "ok" } else { "warn" },
        if version_ok {
            format!("CLI and Skill declare version {version}.")
        } else {
            format!("CLI is {version}; the Skill version is missing or different.")
        },
    ));

    let persona_files = ["melchior", "balthasar", "casper"]
        .into_iter()
        .flat_map(|name| {
            [
                format!(".github/agents/magi-{name}.agent.md"),
                format!(".claude/agents/magi-{name}.md"),
            ]
        })
        .collect::<Vec<_>>();
    let personas_ok = all_files(root, &persona_files);
    checks.push(check(
        "personas",
        if personas_ok { "ok" } else { "fail" },
        if personas_ok {
            "All three GitHub and Claude persona definitions are present."
        } else {
            "One or more GitHub or Claude persona definitions are missing."
        },
    ));

    let thomas_ok = all_files(
        root,
        &[
            ".github/agents/magi-thomas.agent.md".to_owned(),
            ".claude/agents/magi-thomas.md".to_owned(),
        ],
    );
    checks.push(check(
        "thomas",
        if thomas_ok { "ok" } else { "warn" },
        if thomas_ok {
            "THOMAS definitions are present for GitHub and Claude."
        } else {
            "THOMAS is unavailable on at least one bundled Host configuration."
        },
    ));

    let github_hooks =
        fs::read_to_string(root.join(".github/hooks/magi-council.json")).unwrap_or_default();
    let claude_hooks =
        fs::read_to_string(root.join(".claude/settings.json.authoring-off")).unwrap_or_default();
    let hooks_ok = [
        "subagent-start",
        "subagent-stop",
        "guard-tool-use",
        "redact-tool-result",
    ]
    .iter()
    .all(|hook| github_hooks.contains(hook))
        && [
            "claude-subagent-stop",
            "guard-tool-use",
            "redact-tool-result",
        ]
        .iter()
        .all(|hook| claude_hooks.contains(hook));
    checks.push(check("required_hooks", if hooks_ok { "ok" } else { "fail" }, if hooks_ok {
        "Bundled Host configurations reference required lifecycle, guard, and redaction Hooks."
    } else {
        "A required lifecycle, guard, or redaction Hook is missing from bundled Host configuration."
    }));

    let protected_ok = root.join("src/hooks.rs").is_file()
        && root.join("docs/THREAT_MODEL.md").is_file()
        && skill_text.contains("manifest.json");
    checks.push(check(
        "protected_assets",
        if protected_ok { "ok" } else { "fail" },
        if protected_ok {
            "Protected assets, guard implementation, and threat model are present."
        } else {
            "Protected asset policy or guard implementation is incomplete."
        },
    ));

    let capability_value = match capabilities_path {
        Some(path) => Some(
            serde_json::from_str::<Value>(&fs::read_to_string(path).map_err(|error| {
                anyhow!(
                    "failed to read capability metadata {}: {error}",
                    path.display()
                )
            })?)
            .map_err(|error| anyhow!("capability metadata is not valid JSON: {error}"))?,
        ),
        None => None,
    };
    let capability_check = evaluate_host_capabilities(capability_value.as_ref());
    let capability_status = if capability_check["sealedEligible"] == true {
        "ok"
    } else {
        "warn"
    };
    checks.push(check(
        "sealed_subagents",
        capability_status,
        capability_check["reason"]
            .as_str()
            .unwrap_or("Capability status is unknown."),
    ));
    let model_metadata_available = capability_value
        .as_ref()
        .and_then(|value| value.get("modelMetadata"))
        .is_some_and(|value| !value.is_null());
    checks.push(check(
        "model_metadata",
        if model_metadata_available { "ok" } else { "warn" },
        if model_metadata_available {
            "Host model metadata was supplied for reproducibility records."
        } else {
            "Host model metadata is unavailable; it was not guessed. This does not by itself invalidate project setup."
        },
    ));

    let has_fail = checks.iter().any(|item| item["status"] == "fail");
    let has_warn = checks.iter().any(|item| item["status"] == "warn");
    Ok(json!({
        "valid": !has_fail,
        "status": if has_fail { "fail" } else if has_warn { "warn" } else { "ok" },
        "cliVersion": version,
        "sealedSubagents": capability_check,
        "checks": checks
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_evaluation_is_fail_closed() {
        assert_eq!(evaluate_host_capabilities(None)["status"], "unknown");
        assert_eq!(
            evaluate_host_capabilities(Some(&json!({})))["sealedEligible"],
            false
        );
        let available = json!({
            "customAgents": true, "isolatedSubagentContexts": true,
            "subagentStartHook": true, "subagentStopHook": true,
            "preToolUseHook": true, "postToolUseHook": true,
            "voteBodyConfidential": true
        });
        assert_eq!(
            evaluate_host_capabilities(Some(&available))["sealedEligible"],
            true
        );
    }
}

use crate::adversarial::{context_for, seal_challenges, seal_round_vote, validate_challenges};
use crate::core::{
    Agent, PERSONAS, agent_for_name, build_persona_context, extract_json_object, find_repo_root,
    normalize_hook_payload, persona_for_agent, read_json, read_last_assistant_message, run_dir_for,
    seal_vote, validate_vote,
};
use anyhow::{Result, anyhow};
use regex::Regex;
use serde_json::{Value, json};

fn flatten(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| serde_json::to_string(value).unwrap_or_else(|_| value.to_string()))
}

fn blocked(reason: impl Into<String>) -> Value {
    json!({"decision": "block", "reason": reason.into()})
}

pub fn subagent_start(input: &Value) -> Result<Value> {
    let payload = normalize_hook_payload(input)?;
    let Some(agent) = agent_for_name(payload.agent_name.as_deref()) else {
        return Ok(json!({}));
    };
    let root = find_repo_root(Some(&payload.cwd))?;
    let run_id = input
        .get("runId")
        .or_else(|| input.get("run_id"))
        .and_then(Value::as_str);
    let context = match (agent, run_id) {
        (Agent::Thomas, Some(run_id)) => context_for(&root, run_id, "thomas")?,
        (Agent::Persona(persona), Some(run_id)) => {
            let request = read_json(&run_dir_for(&root, run_id)?.join("request.json"))?;
            if matches!(
                request.get("status").and_then(Value::as_str),
                Some("challenge_ready" | "collecting_final")
            ) {
                format!(
                    "{}\n\n# Adversarial review\n{}",
                    build_persona_context(&root, persona)?,
                    context_for(&root, run_id, persona)?
                )
            } else {
                build_persona_context(&root, persona)?
            }
        }
        (Agent::Persona(persona), None) => build_persona_context(&root, persona)?,
        (Agent::Thomas, None) => {
            return Ok(blocked(
                "THOMAS requires runId in the subagentStart payload.",
            ));
        }
    };
    Ok(json!({"additionalContext": context}))
}

pub fn subagent_stop(input: &Value) -> Result<Value> {
    let payload = normalize_hook_payload(input)?;
    if payload.agent_name.as_deref() == Some("magi-thomas") {
        let response = payload.response.as_deref().unwrap_or_default();
        let value = match extract_json_object(response) {
            Ok(v) => v,
            Err(e) => return Ok(blocked(format!("THOMAS challenge JSON was rejected: {e}"))),
        };
        let run_id = value
            .get("runId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if let Err(e) = validate_challenges(&value, run_id, true) {
            return Ok(blocked(format!("THOMAS challenge JSON was rejected: {e}")));
        }
        let root = find_repo_root(Some(&payload.cwd))?;
        return match seal_challenges(&root, &value, payload.agent_id.as_deref()) {
            Ok(sealed) => Ok(json!({"decision": "allow", "modifiedResponse": sealed["receipt"]})),
            Err(e) => Ok(blocked(format!("MAGI challenge sealing failed: {e}"))),
        };
    }
    let Some(persona) = persona_for_agent(payload.agent_name.as_deref()) else {
        return Ok(json!({}));
    };
    let response = payload.response.as_deref().unwrap_or_default();
    let vote = match extract_json_object(response).and_then(|vote| {
        validate_vote(&vote, Some(persona))?;
        Ok(vote)
    }) {
        Ok(vote) => vote,
        Err(error) => {
            return Ok(blocked(format!(
                "Your sealed vote was rejected: {error} Return one corrected JSON object only. Do not add markdown or commentary."
            )));
        }
    };
    let root = find_repo_root(Some(&payload.cwd))?;
    let request = read_json(
        &run_dir_for(&root, vote["runId"].as_str().unwrap_or_default())?.join("request.json"),
    )?;
    let status = request
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let sealed = if matches!(status, "collecting_initial" | "initial_ready") {
        seal_round_vote(
            &root,
            persona,
            "initial",
            &vote,
            payload.agent_id.as_deref(),
        )
    } else if matches!(status, "collecting_final" | "final_ready") {
        seal_round_vote(&root, persona, "final", &vote, payload.agent_id.as_deref())
    } else {
        seal_vote(&root, persona, &vote, payload.agent_id.as_deref())
            .map(|s| json!({"receipt": s.receipt}))
    };
    match sealed {
        Ok(sealed) => Ok(json!({"decision": "allow", "modifiedResponse": sealed["receipt"]})),
        Err(error) => Ok(blocked(format!(
            "MAGI sealing failed: {error}. Do not change the question or persona; return a valid vote JSON again."
        ))),
    }
}

pub fn claude_subagent_stop(input: &Value) -> Result<Value> {
    if input.get("stop_hook_active").and_then(Value::as_bool) == Some(true) {
        return Ok(json!({}));
    }
    let payload = normalize_hook_payload(input)?;
    let message = payload
        .response
        .unwrap_or_else(|| read_last_assistant_message(payload.transcript_path.as_deref()));
    if message.is_empty() {
        return Ok(json!({}));
    }
    let receipt_pattern =
        Regex::new(r"(?i)VOTE_SEALED\s+run=(magi-[a-z0-9-]+)\s+sha256=([0-9a-f]{16})")?;
    let vote_pattern = Regex::new(r#""persona"\s*:\s*"(melchior|balthasar|casper)""#)?;
    let receipt = receipt_pattern.captures(&message);
    let looks_like_vote = vote_pattern.is_match(&message);
    if receipt.is_none() && !looks_like_vote {
        return Ok(json!({}));
    }
    let root = match find_repo_root(Some(&payload.cwd)) {
        Ok(root) => root,
        Err(error) => {
            return Ok(blocked(format!(
                "MAGI stop hook failed: {error}. Seal your vote with the MAGI CLI and return only the receipt line."
            )));
        }
    };
    if looks_like_vote {
        let vote = match extract_json_object(&message).and_then(|vote| {
            validate_vote(&vote, None)?;
            Ok(vote)
        }) {
            Ok(vote) => vote,
            Err(error) => {
                return Ok(blocked(format!(
                    "Your vote was rejected: {error} Pipe one corrected vote JSON to the MAGI vote seal command and return only the receipt line."
                )));
            }
        };
        let persona = vote["persona"].as_str().unwrap_or_default();
        if let Err(error) = seal_vote(&root, persona, &vote, payload.agent_id.as_deref()) {
            return Ok(blocked(format!(
                "MAGI sealing failed: {error} Fix the vote, pipe it to the MAGI vote seal command, and return only the receipt line."
            )));
        }
        return Ok(blocked(
            "Your vote body must never reach the parent agent. It has been sealed for you. Reply with the single receipt line printed by the MAGI vote seal command and nothing else.",
        ));
    }
    let captures = receipt.ok_or_else(|| anyhow!("receipt capture is missing"))?;
    let run_id = &captures[1];
    let short_hash = captures[2].to_lowercase();
    let manifest_file = run_dir_for(&root, run_id)?.join("manifest.json");
    if !manifest_file.exists() {
        return Ok(blocked(format!(
            "No MAGI run named {run_id} exists. Seal your vote with the MAGI vote seal command before finishing."
        )));
    }
    let manifest = read_json(&manifest_file)?;
    let matched = PERSONAS.iter().any(|persona| {
        manifest
            .pointer(&format!("/votes/{persona}/sha256"))
            .and_then(Value::as_str)
            .is_some_and(|hash| hash.starts_with(&short_hash))
    });
    if !matched {
        return Ok(blocked(format!(
            "No sealed vote in run {run_id} matches that receipt. Pipe your vote JSON to the MAGI vote seal command and return the receipt it prints."
        )));
    }
    Ok(json!({}))
}

fn deny(reason: &str) -> Value {
    json!({
        "permissionDecision": "deny",
        "permissionDecisionReason": reason,
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason
        }
    })
}

fn protected_patterns() -> Result<(Vec<Regex>, Vec<Regex>)> {
    let dot = r"\.";
    let state = format!(r"{dot}magi");
    let protected_read = vec![
        Regex::new(&format!(r"{state}/runs/[^/]+/sealed(?:/|\b)"))?,
        Regex::new(&format!(
            r"{state}/runs/[^/]+/rounds/(?:initial|final)/sealed(?:/|\b)"
        ))?,
        Regex::new(&format!(
            r"{state}/runs/[^/]+/adversarial/(?:mapping|challenges)\.json"
        ))?,
        Regex::new(&format!(r"{state}/runs/[^/]+/manifest\.json"))?,
        Regex::new(&format!(r"{state}/memory/personas(?:/|\b)"))?,
    ];
    let protected_mutation = vec![
        Regex::new(&format!(r"{dot}github/hooks(?:/|\b)"))?,
        Regex::new(&format!(r"{dot}github/agents/magi-"))?,
        Regex::new(&format!(r"{dot}claude/settings(?:\.local)?\.json"))?,
        Regex::new(&format!(r"{dot}claude/agents/magi-"))?,
        Regex::new(&format!(r"{dot}claude/skills/magi-council(?:/|\b)"))?,
        Regex::new(&format!(r"{dot}agents/skills/magi-council/scripts(?:/|\b)"))?,
        Regex::new(&format!(r"{state}/constitution(?:/|\b)"))?,
        Regex::new(&format!(r"{state}/config\.json"))?,
        Regex::new(&format!(r"{state}/memory(?:/|\b)"))?,
        Regex::new(&format!(
            r"{state}/runs/[^/]+/(?:sealed|rounds|adversarial|manifest\.json|decision\.json|decision\.md)"
        ))?,
    ];
    Ok((protected_read, protected_mutation))
}

pub fn guard_tool_use(input: &Value) -> Result<Value> {
    let payload = normalize_hook_payload(input)?;
    let text = flatten(&payload.tool_args)
        .replace('\\', "/")
        .to_lowercase();
    let tool = payload.tool_name.unwrap_or_default().to_lowercase();
    let (protected_read, protected_mutation) = protected_patterns()?;
    let is_read_like = ["view", "grep", "glob", "read"].contains(&tool.as_str());
    let is_mutation = [
        "create",
        "edit",
        "write",
        "apply_patch",
        "str_replace_editor",
    ]
    .contains(&tool.as_str());
    if is_read_like && protected_read.iter().any(|pattern| pattern.is_match(&text)) {
        return Ok(deny(
            "MAGI sealed votes, manifests, and persona-private memory are not model-readable.",
        ));
    }
    if is_mutation
        && protected_mutation
            .iter()
            .any(|pattern| pattern.is_match(&text))
    {
        return Ok(deny(
            "Protected MAGI state may be changed only by the reviewed MAGI binary and explicit human memory approval.",
        ));
    }
    if ["bash", "powershell", "execute"].contains(&tool.as_str()) {
        let direct_secret_access = protected_read.iter().any(|pattern| pattern.is_match(&text));
        let mutation_command = Regex::new(
            r"rm|del|remove|write|set-content|out-file|sed\s+-i|perl\s+-i|node\s+-e|python\s+-c",
        )?;
        let source_mutation = protected_mutation
            .iter()
            .any(|pattern| pattern.is_match(&text))
            && mutation_command.is_match(&text);
        if direct_secret_access || source_mutation {
            return Ok(deny(
                "Direct shell access to protected MAGI state or policy implementation is denied.",
            ));
        }
    }
    Ok(json!({"permissionDecision": "allow"}))
}

pub fn redact_tool_result(input: &Value) -> Result<Value> {
    let payload = normalize_hook_payload(input)?;
    let result = payload
        .tool_result
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            payload
                .tool_result
                .get("textResultForLlm")
                .or_else(|| payload.tool_result.get("text_result_for_llm"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| flatten(&payload.tool_result));
    let state = format!(r"{}magi", r"\.");
    let sensitive = Regex::new(&format!(
        r#"(?i){state}/runs/[^\s"']+/(?:sealed|rounds/(?:initial|final)/sealed|adversarial/(?:mapping|challenges)\.json|manifest\.json)|{state}/memory/personas"#
    ))?;
    if !sensitive.is_match(&result.replace('\\', "/")) {
        return Ok(json!({}));
    }
    let warning =
        "Do not attempt to recover or infer protected MAGI vote or persona-memory content.";
    Ok(json!({
        "modifiedResult": {
            "resultType": "success",
            "textResultForLlm": "[MAGI protected content redacted by policy Hook]"
        },
        "additionalContext": warning,
        "hookSpecificOutput": {"hookEventName": "PostToolUse", "additionalContext": warning}
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_denies_sealed_vote_read() {
        let protected_path = format!("{}{}", ".magi", "/runs/example/sealed/melchior.json");
        let output = guard_tool_use(&json!({
            "toolName": "view",
            "toolArgs": {"path": protected_path}
        }))
        .unwrap();
        assert_eq!(output["permissionDecision"], "deny");
    }

    #[test]
    fn guard_allows_ordinary_read() {
        let output = guard_tool_use(&json!({
            "toolName": "view",
            "toolArgs": {"path": "src/main.rs"}
        }))
        .unwrap();
        assert_eq!(output["permissionDecision"], "allow");
    }
}

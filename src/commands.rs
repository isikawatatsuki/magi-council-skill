use crate::core::{
    PERSONAS, atomic_write_json, atomic_write_text, build_persona_context, now_iso, read_json,
    run_dir_for, seal_vote, state_dir,
};
use crate::sha256_value;
use anyhow::{Result, anyhow, ensure};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

pub fn init_project(root: &Path) -> Result<Vec<String>> {
    let skill = root.join(".agents").join("skills").join("magi-council");
    let state = state_dir(root);
    let copies = [
        (
            skill.join("templates").join("config.json"),
            state.join("config.json"),
        ),
        (
            skill.join("templates").join("constitution.md"),
            state.join("constitution").join("principles.md"),
        ),
    ];
    let mut messages = Vec::new();
    for (source, target) in copies {
        let relative = target.strip_prefix(root).unwrap_or(&target).display();
        if target.exists() {
            messages.push(format!("kept {relative}"));
        } else {
            let contents = fs::read_to_string(&source)?;
            atomic_write_text(&target, &contents, 0o600)?;
            messages.push(format!("created {relative}"));
        }
    }
    for persona in PERSONAS {
        let target = state
            .join("memory")
            .join("personas")
            .join(format!("{persona}.json"));
        if !target.exists() {
            atomic_write_json(
                &target,
                &json!({"schemaVersion": "1.0", "persona": persona, "entries": []}),
                0o600,
            )?;
        }
    }
    let approved = state.join("memory").join("approved").join("index.json");
    if !approved.exists() {
        atomic_write_json(
            &approved,
            &json!({"schemaVersion": "1.0", "entries": []}),
            0o600,
        )?;
    }
    for directory in ["runs", "tmp", "locks"] {
        fs::create_dir_all(state.join(directory))?;
    }
    messages.push("MAGI project state initialized without overwriting existing policy.".to_owned());
    Ok(messages)
}

pub fn load_persona(root: &Path, persona: &str) -> Result<String> {
    ensure!(
        PERSONAS.contains(&persona),
        "persona must be one of {}.",
        PERSONAS.join(", ")
    );
    build_persona_context(root, persona)
}

pub fn approve_memory(
    root: &Path,
    run_id: &str,
    candidate_id: &str,
    approved_by: &str,
) -> Result<Value> {
    ensure!(!approved_by.trim().is_empty(), "approved-by is required.");
    let run_dir = run_dir_for(root, run_id)?;
    let decision = read_json(&run_dir.join("decision.json"))?;
    let candidate = decision["memoryCandidates"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(candidate_id))
        .ok_or_else(|| anyhow!("Memory candidate not found in finalized decision."))?;
    let persona = candidate["persona"]
        .as_str()
        .ok_or_else(|| anyhow!("Memory candidate persona is invalid."))?;
    ensure!(
        PERSONAS.contains(&persona),
        "Memory candidate persona is invalid."
    );
    let state = state_dir(root);
    let memory_file = state
        .join("memory")
        .join("personas")
        .join(format!("{persona}.json"));
    let mut memory = read_json(&memory_file)?;
    let entry_id = format!(
        "memory-{}",
        &sha256_value(&json!({"candidate": candidate, "approvedBy": approved_by}))?[..12]
    );
    let approved_at = now_iso();
    let entry = json!({
        "id": entry_id,
        "status": "approved",
        "enabled": true,
        "priority": 50,
        "approvedBy": approved_by,
        "approvedAt": approved_at,
        "sourceRunId": run_id,
        "principle": candidate["principle"],
        "scopes": candidate["scopes"],
        "applicableWhen": candidate["applicableWhen"],
        "notApplicableWhen": candidate["notApplicableWhen"],
        "rationale": candidate["rationale"]
    });
    let entries = memory["entries"]
        .as_array_mut()
        .ok_or_else(|| anyhow!("persona memory entries must be an array."))?;
    if !entries.iter().any(|item| item.get("id") == entry.get("id")) {
        entries.push(entry.clone());
    }
    atomic_write_json(&memory_file, &memory, 0o600)?;

    let index_file = state.join("memory").join("approved").join("index.json");
    let mut index = if index_file.exists() {
        read_json(&index_file)?
    } else {
        json!({"schemaVersion": "1.0", "entries": []})
    };
    let index_entries = index["entries"]
        .as_array_mut()
        .ok_or_else(|| anyhow!("approved memory index entries must be an array."))?;
    if !index_entries
        .iter()
        .any(|item| item.get("id") == entry.get("id"))
    {
        index_entries.push(json!({
            "id": entry_id,
            "persona": persona,
            "sourceRunId": run_id,
            "approvedAt": approved_at
        }));
    }
    atomic_write_json(&index_file, &index, 0o600)?;
    Ok(json!({"approved": true, "persona": persona, "entry": entry}))
}

pub fn seal_vote_input(
    root: &Path,
    expected_persona: Option<&str>,
    vote: &Value,
    agent_id: Option<&str>,
) -> Result<Value> {
    if let Some(persona) = expected_persona {
        ensure!(
            PERSONAS.contains(&persona),
            "--persona must be one of {}.",
            PERSONAS.join(", ")
        );
    }
    let persona = vote
        .get("persona")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("vote.persona is invalid."))?;
    if let Some(expected) = expected_persona {
        ensure!(persona == expected, "vote.persona must be {expected}.");
    }
    let sealed = seal_vote(root, persona, vote, agent_id)?;
    Ok(json!({
        "sealed": true,
        "persona": persona,
        "runId": vote["runId"],
        "sha256": sealed.vote_hash,
        "receipt": sealed.receipt
    }))
}

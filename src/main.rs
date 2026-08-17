use anyhow::{Result, anyhow};
use clap::{Args, Parser, Subcommand};
use magi_council_cli::adversarial::{context_for, prepare, seal_challenges, seal_round_vote};
use magi_council_cli::capabilities::doctor;
use magi_council_cli::commands::{approve_memory, init_project, load_persona, seal_vote_input};
use magi_council_cli::core::{extract_json_object, find_repo_root, parse_json, read_stdin};
use magi_council_cli::hooks::{
    claude_subagent_stop, guard_tool_use, redact_tool_result, subagent_start, subagent_stop,
};
use magi_council_cli::lifecycle::{
    audit_run, create_run, import_inline_votes, run_status, tally_votes,
};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(name = "magi", version, about = "MAGI Council single-binary CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Verify the CLI installation.
    Version,
    /// Diagnose project setup and sealed-subagents Host capabilities.
    Doctor(DoctorArgs),
    /// Create, inspect, import, tally, or audit a council run.
    Run {
        #[command(subcommand)]
        command: RunCommand,
    },
    /// Initialize project-local MAGI state.
    Init,
    /// Load private policy for one persona.
    Persona {
        #[command(subcommand)]
        command: PersonaCommand,
    },
    /// Approve a finalized memory candidate.
    Memory {
        #[command(subcommand)]
        command: MemoryCommand,
    },
    /// Validate and seal a persona vote.
    Vote {
        #[command(subcommand)]
        command: VoteCommand,
    },
    /// Validate and seal THOMAS adversarial challenges.
    Thomas {
        #[command(subcommand)]
        command: ThomasCommand,
    },
    /// Execute an editor or agent host hook using JSON stdin/stdout.
    Hook {
        #[command(subcommand)]
        command: HookCommand,
    },
}

#[derive(Debug, Subcommand)]
enum RunCommand {
    /// Create a new council run.
    Create(CreateArgs),
    /// Show vote collection status.
    Status { run_id: String },
    /// Import exactly three inline votes from stdin.
    ImportVotes { run_id: String },
    /// Verify and tally three sealed votes.
    Tally { run_id: String },
    /// Audit run integrity.
    Audit { run_id: String },
    /// Prepare randomized anonymous input for THOMAS.
    PrepareAdversarial { run_id: String },
    /// Produce phase-scoped context for THOMAS or one final voter.
    Context { run_id: String, agent: String },
}

#[derive(Debug, Args)]
struct CreateArgs {
    /// Read an object containing question and context from stdin.
    #[arg(long, conflicts_with = "question")]
    stdin: bool,
    /// Council question. Context defaults to an empty object.
    #[arg(long, conflicts_with = "stdin")]
    question: Option<String>,
    /// Explicit execution mode. Required with --question; stdin JSON must also declare executionMode.
    #[arg(long, value_parser = ["sealed-subagents", "inline"])]
    mode: Option<String>,
}

#[derive(Debug, Args)]
struct DoctorArgs {
    /// Emit a machine-readable JSON report.
    #[arg(long)]
    json: bool,
    /// Read explicit Host capability attestation JSON from this path.
    #[arg(long)]
    capabilities: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum PersonaCommand {
    Load { persona: String },
}

#[derive(Debug, Subcommand)]
enum MemoryCommand {
    Approve {
        run_id: String,
        candidate_id: String,
        #[arg(long)]
        approved_by: String,
    },
}

#[derive(Debug, Subcommand)]
enum VoteCommand {
    Seal {
        #[arg(long)]
        persona: Option<String>,
        #[arg(long)]
        round: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum ThomasCommand {
    Seal,
}

#[derive(Debug, Subcommand)]
enum HookCommand {
    SubagentStart,
    SubagentStop,
    ClaudeSubagentStop,
    GuardToolUse,
    RedactToolResult,
}

fn execute() -> Result<bool> {
    match Cli::parse().command {
        Command::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(true)
        }
        Command::Doctor(args) => {
            let root = find_repo_root(None)?;
            let report = doctor(&root, args.capabilities.as_deref())?;
            if args.json {
                println!("{}", serde_json::to_string(&report)?);
            } else {
                println!(
                    "MAGI doctor: {}",
                    report["status"]
                        .as_str()
                        .unwrap_or("unknown")
                        .to_uppercase()
                );
                for check in report["checks"].as_array().into_iter().flatten() {
                    println!(
                        "[{}] {}: {}",
                        check["status"].as_str().unwrap_or("unknown").to_uppercase(),
                        check["id"].as_str().unwrap_or("check"),
                        check["reason"].as_str().unwrap_or("")
                    );
                }
            }
            Ok(report["valid"] == true)
        }
        Command::Run { command } => {
            let root = find_repo_root(None)?;
            let output = match command {
                RunCommand::Create(args) => {
                    let mut input = if args.stdin {
                        parse_json(&read_stdin()?, "stdin request")?
                    } else if let Some(question) = args.question {
                        json!({"question": question, "context": {}, "executionMode": args.mode.clone()})
                    } else {
                        return Err(anyhow!("Use --stdin with JSON or --question \"...\"."));
                    };
                    if args.stdin {
                        if let Some(mode) = args.mode {
                            let object = input
                                .as_object_mut()
                                .ok_or_else(|| anyhow!("stdin request must be an object."))?;
                            if object
                                .get("executionMode")
                                .is_some_and(|value| value != &Value::String(mode.clone()))
                            {
                                return Err(anyhow!("--mode conflicts with stdin executionMode."));
                            }
                            object.insert("executionMode".to_owned(), Value::String(mode));
                        }
                    }
                    create_run(&root, &input)?
                }
                RunCommand::Status { run_id } => run_status(&root, &run_id)?,
                RunCommand::ImportVotes { run_id } => {
                    let input = parse_json(&read_stdin()?, "inline votes")?;
                    import_inline_votes(&root, &run_id, &input)?
                }
                RunCommand::Tally { run_id } => tally_votes(&root, &run_id)?,
                RunCommand::Audit { run_id } => audit_run(&root, &run_id)?,
                RunCommand::PrepareAdversarial { run_id } => prepare(&root, &run_id)?,
                RunCommand::Context { run_id, agent } => {
                    json!({"additionalContext": context_for(&root, &run_id, &agent)?})
                }
            };
            let valid = output.get("valid").and_then(Value::as_bool) != Some(false);
            println!("{}", serde_json::to_string(&output)?);
            Ok(valid)
        }
        Command::Init => {
            let root = find_repo_root(None)?;
            for message in init_project(&root)? {
                println!("{message}");
            }
            Ok(true)
        }
        Command::Persona { command } => {
            let root = find_repo_root(None)?;
            match command {
                PersonaCommand::Load { persona } => println!("{}", load_persona(&root, &persona)?),
            }
            Ok(true)
        }
        Command::Memory { command } => {
            let root = find_repo_root(None)?;
            let output = match command {
                MemoryCommand::Approve {
                    run_id,
                    candidate_id,
                    approved_by,
                } => approve_memory(&root, &run_id, &candidate_id, &approved_by)?,
            };
            println!("{}", serde_json::to_string(&output)?);
            Ok(true)
        }
        Command::Vote { command } => {
            let root = find_repo_root(None)?;
            let output = match command {
                VoteCommand::Seal { persona, round } => {
                    let raw = read_stdin()?;
                    if raw.is_empty() {
                        return Err(anyhow!("Vote JSON is required on stdin."));
                    }
                    let vote = extract_json_object(&raw)?;
                    let agent_id = std::env::var("CLAUDE_AGENT_ID").ok();
                    if let Some(round) = round {
                        let persona = persona
                            .as_deref()
                            .or_else(|| vote.get("persona").and_then(Value::as_str))
                            .ok_or_else(|| anyhow!("--persona is required."))?;
                        seal_round_vote(&root, persona, &round, &vote, agent_id.as_deref())?
                    } else {
                        seal_vote_input(&root, persona.as_deref(), &vote, agent_id.as_deref())?
                    }
                }
            };
            println!("{}", serde_json::to_string(&output)?);
            Ok(true)
        }
        Command::Thomas { command } => {
            let root = find_repo_root(None)?;
            let output = match command {
                ThomasCommand::Seal => {
                    let value = extract_json_object(&read_stdin()?)?;
                    let agent_id = std::env::var("CLAUDE_AGENT_ID").ok();
                    seal_challenges(&root, &value, agent_id.as_deref())?
                }
            };
            println!("{}", serde_json::to_string(&output)?);
            Ok(true)
        }
        Command::Hook { command } => {
            let raw = read_stdin()?;
            let input = parse_json(if raw.is_empty() { "{}" } else { &raw }, "hook input");
            let output = match command {
                HookCommand::SubagentStop => match input.and_then(|value| subagent_stop(&value)) {
                    Ok(output) => output,
                    Err(error) => json!({
                        "decision": "block",
                        "reason": format!("MAGI sealing failed: {error}. Do not change the question or persona; return a valid vote JSON again.")
                    }),
                },
                HookCommand::ClaudeSubagentStop => {
                    match input.and_then(|value| claude_subagent_stop(&value)) {
                        Ok(output) => output,
                        Err(error) => json!({
                            "decision": "block",
                            "reason": format!("MAGI stop hook failed: {error}. Seal your vote with the MAGI CLI and return only the receipt line.")
                        }),
                    }
                }
                HookCommand::SubagentStart => subagent_start(&input?)?,
                HookCommand::GuardToolUse => guard_tool_use(&input?)?,
                HookCommand::RedactToolResult => redact_tool_result(&input?)?,
            };
            println!("{}", serde_json::to_string(&output)?);
            Ok(true)
        }
    }
}

fn main() -> ExitCode {
    match execute() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("Error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

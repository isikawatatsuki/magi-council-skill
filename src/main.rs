use anyhow::{Result, anyhow};
use clap::{Args, Parser, Subcommand};
use magi_council_cli::commands::{approve_memory, init_project, load_persona, seal_vote_input};
use magi_council_cli::core::{extract_json_object, find_repo_root, parse_json, read_stdin};
use magi_council_cli::hooks::{
    claude_subagent_stop, guard_tool_use, redact_tool_result, subagent_start, subagent_stop,
};
use magi_council_cli::lifecycle::{
    audit_run, create_run, import_inline_votes, run_status, tally_votes,
};
use serde_json::{Value, json};
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
}

#[derive(Debug, Args)]
struct CreateArgs {
    /// Read an object containing question and context from stdin.
    #[arg(long, conflicts_with = "question")]
    stdin: bool,
    /// Council question. Context defaults to an empty object.
    #[arg(long, conflicts_with = "stdin")]
    question: Option<String>,
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
    },
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
        Command::Run { command } => {
            let root = find_repo_root(None)?;
            let output = match command {
                RunCommand::Create(args) => {
                    let input = if args.stdin {
                        parse_json(&read_stdin()?, "stdin request")?
                    } else if let Some(question) = args.question {
                        json!({"question": question, "context": {}})
                    } else {
                        return Err(anyhow!("Use --stdin with JSON or --question \"...\"."));
                    };
                    create_run(&root, &input)?
                }
                RunCommand::Status { run_id } => run_status(&root, &run_id)?,
                RunCommand::ImportVotes { run_id } => {
                    let input = parse_json(&read_stdin()?, "inline votes")?;
                    import_inline_votes(&root, &run_id, &input)?
                }
                RunCommand::Tally { run_id } => tally_votes(&root, &run_id)?,
                RunCommand::Audit { run_id } => audit_run(&root, &run_id)?,
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
                VoteCommand::Seal { persona } => {
                    let raw = read_stdin()?;
                    if raw.is_empty() {
                        return Err(anyhow!("Vote JSON is required on stdin."));
                    }
                    let vote = extract_json_object(&raw)?;
                    let agent_id = std::env::var("CLAUDE_AGENT_ID").ok();
                    seal_vote_input(&root, persona.as_deref(), &vote, agent_id.as_deref())?
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

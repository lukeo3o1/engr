use clap::{Args, Parser, Subcommand, ValueEnum};
use engr::protocol::{canonical_json, HANDSHAKE};
use engr::state::canonical_chain;
use engr::store::{
    append_event, confirm_candidate, create_snapshot, doctor, find_project_root, init_project,
    list_streams, load_current_state, load_events, prepare_candidate, read_data, read_record,
    replay_stream, run_conformance, verify_project, version_object,
};
use engr::view::render_state;
use engr::{EngrError, Result, EXIT_USAGE, IMPLEMENTATION_VERSION, PROTOCOL_VERSION};
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name="engr", version=IMPLEMENTATION_VERSION, about="Engineering Record protocol-v1 CLI")]
struct Cli {
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Init,
    Version {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        handshake: bool,
    },
    Doctor {
        #[arg(long)]
        json: bool,
    },
    Prepare(WriteCandidate),
    Confirm {
        #[arg(long)]
        response: String,
        #[arg(long)]
        json: bool,
    },
    Discard {
        code: String,
        #[arg(long, default_value = "candidate_corrected")]
        reason: String,
        #[arg(long)]
        json: bool,
    },
    Append(Append),
    Replay {
        stream: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Show {
        stream: String,
        #[arg(long)]
        brief: bool,
        #[arg(long)]
        provenance: bool,
        #[arg(long, value_enum, default_value = "text")]
        format: OutputFormat,
    },
    Backlog {
        #[arg(long, value_enum, default_value = "text")]
        format: OutputFormat,
    },
    Why {
        stream: String,
        subject: Option<String>,
        #[arg(long, value_enum, default_value = "text")]
        format: WhyFormat,
    },
    Snapshot {
        stream: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Verify {
        stream: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Conformance {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args)]
struct WriteCandidate {
    #[arg(long)]
    stream: String,
    #[arg(long)]
    event: String,
    #[arg(long)]
    record_file: PathBuf,
    #[arg(long)]
    data_file: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}
#[derive(Args)]
struct Append {
    #[arg(long)]
    stream: String,
    #[arg(long)]
    event: String,
    #[arg(long)]
    record_file: PathBuf,
    #[arg(long)]
    data_file: Option<PathBuf>,
    #[arg(long)]
    initiator: String,
    #[arg(long)]
    basis: String,
    #[arg(long)]
    expected_parent: String,
    #[arg(long)]
    json: bool,
}
#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum OutputFormat {
    Text,
    Markdown,
    Json,
}
#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum WhyFormat {
    Text,
    Json,
}

fn print_json(value: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("JSON serialization cannot fail")
    );
}
fn project(cli: &Cli) -> Result<PathBuf> {
    let root = find_project_root(cli.root.as_deref())?;
    engr::store::validate_format_versions(&root)?;
    Ok(root)
}
fn parent(value: &str) -> Result<Value> {
    if value == "none" {
        return Ok(Value::Null);
    }
    if engr::protocol::valid_event_id(value) {
        Ok(Value::String(value.into()))
    } else {
        Err(EngrError::new(
            EXIT_USAGE,
            "--expected-parent must be an event id or 'none'",
        ))
    }
}

fn execute(cli: Cli) -> Result<()> {
    if let Command::Version { json, handshake } = &cli.command {
        if *handshake {
            println!("{HANDSHAKE}");
        } else if *json {
            print_json(&version_object());
        } else {
            println!(
                "engr {} (protocol={}, event-schema=1, state-schema=1, implementation=rust)",
                IMPLEMENTATION_VERSION, PROTOCOL_VERSION
            );
        }
        return Ok(());
    }
    if matches!(&cli.command, Command::Init) {
        let root = cli
            .root
            .as_deref()
            .ok_or_else(|| EngrError::new(EXIT_USAGE, "init requires --root <project-root>"))?;
        let result = init_project(root)?;
        print_json(&result);
        return Ok(());
    }
    let root = project(&cli)?;
    match cli.command {
        Command::Doctor { json } => {
            let report = doctor(&root)?;
            if json {
                print_json(&report)
            } else {
                println!("PASS protocol-v{PROTOCOL_VERSION}\nProject: {}\nSelected implementation: engr\nStreams: {}",root.display(),report["streams"].as_array().map_or(0,Vec::len));
            }
        }
        Command::Prepare(command) => {
            let receipt = prepare_candidate(
                &root,
                &command.stream,
                &command.event,
                &read_record(&command.record_file)?,
                read_data(command.data_file.as_deref())?,
            )?;
            if command.json {
                print_json(&receipt)
            } else {
                println!(
                    "Candidate {}\nConfirm exactly: CONFIRM {}\n\n{}",
                    receipt["event"].as_str().unwrap_or("?"),
                    receipt["challenge"].as_str().unwrap_or("?"),
                    receipt["record"]["text"].as_str().unwrap_or("")
                );
            }
        }
        Command::Confirm { response, json } => {
            let (event, state, path) = confirm_candidate(&root, &response)?;
            let result = json!({"ok":true,"event_id":event["event_id"],"stream":event["stream"],"head":state["head"],"event_path":path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\',"/")});
            if json {
                print_json(&result)
            } else {
                println!(
                    "APPENDED {} {} rev {}\nState: {} through rev {}",
                    event["event_id"].as_str().unwrap(),
                    event["stream"].as_str().unwrap(),
                    event["rev"],
                    state["status"],
                    state["head"]["rev"]
                );
            }
        }
        Command::Discard { code, reason, json } => {
            let receipt = engr::store::discard_candidate(&root, &code, &reason)?;
            if json {
                print_json(&receipt)
            } else {
                println!("DISCARDED {code}");
            }
        }
        Command::Append(command) => {
            if !matches!(command.initiator.as_str(), "agent" | "system") {
                return Err(EngrError::new(
                    EXIT_USAGE,
                    "append only permits agent or system provenance; use prepare and confirm for human truth",
                ));
            }
            let provenance = json!({"initiator":command.initiator,"basis":command.basis});
            let (event, state, path) = append_event(
                &root,
                &command.stream,
                &command.event,
                &read_record(&command.record_file)?,
                read_data(command.data_file.as_deref())?,
                provenance,
                parent(&command.expected_parent)?,
            )?;
            let result = json!({"ok":true,"event_id":event["event_id"],"stream":event["stream"],"head":state["head"],"event_path":path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\',"/")});
            if command.json {
                print_json(&result)
            } else {
                println!(
                    "APPENDED {} {} rev {}\nState: {} through rev {}",
                    event["event_id"].as_str().unwrap(),
                    event["stream"].as_str().unwrap(),
                    event["rev"],
                    state["status"],
                    state["head"]["rev"]
                );
            }
        }
        Command::Replay { stream, json } => {
            let streams = stream
                .map(|stream| vec![stream])
                .unwrap_or(list_streams(&root)?);
            if streams.is_empty() {
                return Err(EngrError::new(
                    engr::EXIT_NOT_FOUND,
                    "EventStore contains no streams",
                ));
            }
            let mut values = Vec::new();
            for stream in streams {
                let (state, chain) = replay_stream(&root, &stream, true)?;
                values.push(json!({"stream":stream,"head":state["head"],"status":state["status"],"rejected_events":chain.rejected.len(),"reconciliations":chain.resolutions.len()}));
            }
            if json {
                print_json(&json!({"ok":true,"streams":values}))
            } else {
                for value in values {
                    println!(
                        "REPLAYED {} rev {} — {}",
                        value["stream"].as_str().unwrap(),
                        value["head"]["rev"],
                        value["status"].as_str().unwrap()
                    );
                }
            }
        }
        Command::Show {
            stream,
            brief: _,
            provenance,
            format,
        } => {
            let state = load_current_state(&root, &stream)?;
            if format == OutputFormat::Json {
                print_json(&state)
            } else {
                print!(
                    "{}",
                    render_state(&state, provenance, format == OutputFormat::Markdown)?
                );
            }
        }
        Command::Backlog { format } => {
            let mut states = Vec::new();
            for stream in list_streams(&root)? {
                if engr::protocol::stream_kind(&stream)? == "work_item" {
                    states.push(load_current_state(&root, &stream)?);
                }
            }
            if format == OutputFormat::Json {
                print_json(&json!({ "work_items": states }))
            } else if format == OutputFormat::Markdown {
                println!("# Engineering Backlog");
                for state in states {
                    println!(
                        "- **{}** — {} — {}",
                        state["stream"].as_str().unwrap(),
                        state["status"].as_str().unwrap(),
                        state["title"]["text"].as_str().unwrap()
                    );
                }
            } else {
                for state in states {
                    println!(
                        "{} — {} — {}",
                        state["stream"].as_str().unwrap(),
                        state["status"].as_str().unwrap(),
                        state["title"]["text"].as_str().unwrap()
                    );
                }
            }
        }
        Command::Why {
            stream,
            subject,
            format,
        } => {
            let events = load_events(&root, &stream)?;
            let chain = canonical_chain(&events, &stream)?;
            let needle = subject.as_ref().map(|item| item.to_lowercase());
            let mut values = Vec::new();
            for event in events {
                let search = format!(
                    "{} {} {} {}",
                    event["event_id"].as_str().unwrap_or(""),
                    event["event"].as_str().unwrap_or(""),
                    event["record"]["text"].as_str().unwrap_or(""),
                    canonical_json(&event["data"])
                )
                .to_lowercase();
                if needle
                    .as_ref()
                    .is_some_and(|needle| !search.contains(needle))
                {
                    continue;
                }
                values.push(json!({"event_id":event["event_id"],"rev":event["rev"],"event":event["event"],"text":event["record"]["text"],"data":event["data"],"provenance":event["provenance"],"canonical":chain.chain.iter().any(|item|item["event_id"]==event["event_id"]),"rejected_by_reconciliation":chain.rejected.contains(event["event_id"].as_str().unwrap_or(""))}));
            }
            values.sort_by_key(|item| {
                (
                    item["rev"].as_i64().unwrap_or(0),
                    item["event_id"].as_str().unwrap_or("").to_owned(),
                )
            });
            if subject.is_none() && values.len() > 20 {
                values = values.split_off(values.len() - 20);
            }
            if format == WhyFormat::Json {
                print_json(&json!({"stream":stream,"subject":subject,"events":values}))
            } else {
                for value in values {
                    println!(
                        "rev {} {} {} [{}]\n  {}",
                        value["rev"],
                        value["event_id"].as_str().unwrap(),
                        value["event"].as_str().unwrap(),
                        if value["canonical"].as_bool() == Some(true) {
                            "canonical"
                        } else {
                            "rejected-history"
                        },
                        value["text"].as_str().unwrap()
                    );
                }
            }
        }
        Command::Snapshot { stream, name, json } => {
            let (path, value) = create_snapshot(&root, &stream, name.as_deref())?;
            let result = json!({"ok":true,"path":path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\',"/"),"through":value["through"]});
            if json {
                print_json(&result)
            } else {
                println!(
                    "SNAPSHOT {} through rev {}",
                    result["path"].as_str().unwrap(),
                    result["through"]["rev"]
                );
            }
        }
        Command::Verify { stream, json } => {
            let report = verify_project(&root, stream.as_deref())?;
            if json {
                print_json(&report)
            } else {
                println!("PASS protocol-v{PROTOCOL_VERSION}");
                for item in report["verified_streams"].as_array().unwrap() {
                    println!(
                        "{}: rev {}, events={}, rejected={}",
                        item["stream"].as_str().unwrap(),
                        item["head"]["rev"],
                        item["events"],
                        item["rejected_events"]
                    );
                }
            }
        }
        Command::Conformance { json } => {
            let report = run_conformance(&root)?;
            if json {
                print_json(&report)
            } else {
                println!("PASS protocol-v{PROTOCOL_VERSION}");
                for fixture in report["fixtures"].as_array().unwrap() {
                    println!("{}: passed", fixture["name"].as_str().unwrap());
                }
            }
        }
        Command::Init | Command::Version { .. } => unreachable!(),
    }
    Ok(())
}
fn main() {
    let cli = Cli::parse();
    if let Err(error) = execute(cli) {
        eprintln!("ERROR[{}] {}", error.code, error.message);
        std::process::exit(error.code);
    }
}

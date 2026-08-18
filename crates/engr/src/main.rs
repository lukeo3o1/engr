use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};
use engr::backlog::{self, Subject};
use engr::model::{self, Action, Payload, Ref};
use engr::semantics::{self, Relation, Supplement, Target};
use engr::{gate, git, ops, store, view};
use engr::{Error, Result, EXIT_NOT_FOUND, EXIT_SCHEMA, EXIT_USAGE};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "engr",
    version = engr::IMPLEMENTATION_VERSION,
    about = "Engineering records whose every word a human confirmed"
)]
struct Cli {
    /// Workspace root. Defaults to the nearest ancestor containing .engr
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a workspace in the current directory
    Init,
    /// Explicitly upgrade a recognized legacy v0 workspace
    Migrate,
    /// Print the protocol this build implements
    Protocol,
    /// Put a change up for a human to confirm
    Prepare(Prepare),
    /// List candidates awaiting confirmation, or show one in full
    Candidate {
        /// Challenge code. Omit to list everything pending
        code: Option<String>,
    },
    /// Admit a candidate. The response must be exactly `CONFIRM <code>`
    Confirm { response: String },
    /// List objects. Only the ones needing attention unless --all
    Ls {
        /// Keyword to filter by, matched against titles and section text
        keyword: Option<String>,
        #[arg(long)]
        all: bool,
        /// One line per section, so grep can reach the text
        #[arg(long)]
        sections: bool,
        /// Only what needs attention
        #[arg(long)]
        stale: bool,
    },
    /// Show one object: its sections, and how much each can be trusted
    Show {
        object: String,
        #[arg(long, value_enum, default_value = "text")]
        format: Format,
    },
    /// Recompute section hashes
    Verify { object: Option<String> },
    /// Unresolved staging. Nothing here is confirmed
    #[command(subcommand)]
    Backlog(Backlog),
}

/// Backlog edits do not go through the gate, and must not look as though they
/// might: a separate namespace keeps `ls`, `show` and `verify` meaning exactly
/// what they meant before, which is confirmed record and nothing else.
#[derive(Subcommand)]
enum Backlog {
    /// Start a topic with its first unresolved point
    New {
        #[arg(long)]
        topic: String,
        #[command(flatten)]
        text: TextArg,
        #[command(flatten)]
        subjects: SubjectArgs,
    },
    /// List unresolved topics
    Ls {
        /// Keyword, matched against topics and section text
        keyword: Option<String>,
    },
    /// Show one topic: its unresolved points, subjects and produced outcomes
    Show {
        item: String,
        #[arg(long, value_enum, default_value = "text")]
        format: Format,
    },
    /// Replace the topic. Does not touch section activity
    Rename {
        item: String,
        #[arg(long)]
        topic: String,
    },
    /// Add another unresolved point to a topic
    Add {
        item: String,
        #[command(flatten)]
        text: TextArg,
        #[command(flatten)]
        subjects: SubjectArgs,
    },
    /// Reword an unresolved point
    Revise {
        item: String,
        #[arg(long)]
        section: u64,
        #[command(flatten)]
        text: TextArg,
    },
    /// Replace what an unresolved point concerns
    Subjects {
        item: String,
        #[arg(long)]
        section: u64,
        #[command(flatten)]
        subjects: SubjectArgs,
    },
    /// Consolidate unresolved points into one
    Merge {
        item: String,
        #[arg(long, value_name = "SECTIONS", value_delimiter = ',')]
        sections: Vec<u64>,
        #[command(flatten)]
        text: TextArg,
        #[command(flatten)]
        subjects: SubjectArgs,
    },
    /// Remove an unresolved point, or the whole topic
    Rm {
        item: String,
        /// Remove one point. The topic goes when its last one does
        #[arg(long)]
        section: Option<u64>,
    },
}

#[derive(Args)]
#[command(group(ArgGroup::new("wording").required(true).args(["text", "text_file"])))]
struct TextArg {
    /// Wording, inline
    #[arg(long)]
    text: Option<String>,
    /// Wording, from a file
    #[arg(long)]
    text_file: Option<PathBuf>,
}

impl TextArg {
    fn read(&self) -> Result<String> {
        match (&self.text, &self.text_file) {
            (Some(text), None) => Ok(text.clone()),
            (None, Some(path)) => std::fs::read_to_string(path)
                .map_err(|error| engr::tool_error(path.display(), error)),
            _ => Err(Error::new(
                EXIT_USAGE,
                "use --text or --text-file, not both",
            )),
        }
    }
}

#[derive(Args)]
struct SubjectArgs {
    /// Something this concerns, as engr:obj:<id>[:<section>] or engr:backlog:<id>[:<section>]
    #[arg(long = "subject", value_name = "ENGR_REF")]
    subject: Vec<String>,
    /// A repository file this concerns
    #[arg(long = "subject-file", value_name = "PATH")]
    subject_file: Vec<String>,
    /// A source symbol this concerns
    #[arg(long = "subject-symbol", value_names = ["PATH", "SYMBOL"], num_args = 2)]
    subject_symbol: Vec<String>,
    /// Committed revision to pin file and symbol subjects at. Defaults to HEAD
    /// only while the path itself is clean
    #[arg(long, value_name = "REVISION")]
    subject_commit: Option<String>,
}

impl SubjectArgs {
    fn build(&self, root: &Path) -> Result<Vec<Subject>> {
        let revision = self.subject_commit.as_deref();
        let mut subjects = Vec::new();
        // Validated here rather than left to storage. A subject the domain
        // would refuse is a person mistyping an argument, and it has to read as
        // that — deferring it turns a typo into a report that the workspace is
        // malformed.
        for spec in &self.subject {
            let relative = spec.strip_prefix("engr:").ok_or_else(|| {
                Error::new(
                    EXIT_USAGE,
                    format!("--subject {spec:?} must be an engr: reference"),
                )
            })?;
            let subject = Subject::engr(relative.to_owned());
            subject
                .validate()
                .map_err(|error| malformed_argument("--subject", spec, error))?;
            subjects.push(subject);
        }
        for path in &self.subject_file {
            subjects.push(Subject::File {
                commit: backlog::pin(root, path, revision)
                    .map_err(|error| malformed_argument("--subject-file", path, error))?,
                path: path.clone(),
            });
        }
        for pair in self.subject_symbol.chunks(2) {
            let [path, symbol] = pair else {
                return Err(Error::new(
                    EXIT_USAGE,
                    "--subject-symbol takes a path and a symbol name",
                ));
            };
            let subject = Subject::Symbol {
                commit: backlog::pin(root, path, revision)
                    .map_err(|error| malformed_argument("--subject-symbol", path, error))?,
                path: path.clone(),
                symbol: symbol.clone(),
            };
            subject
                .validate()
                .map_err(|error| malformed_argument("--subject-symbol", symbol, error))?;
            subjects.push(subject);
        }
        Ok(subjects)
    }
}

#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum Format {
    Text,
    Json,
}

/// The Phase 3 vocabularies, spelled for a command line. They are the protocol
/// values, so `clap` rejects anything outside them before a payload exists.
#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum TypeArg {
    Design,
    Decision,
    Risk,
}

impl TypeArg {
    fn model(self) -> semantics::ObjectType {
        match self {
            TypeArg::Design => semantics::ObjectType::Design,
            TypeArg::Decision => semantics::ObjectType::Decision,
            TypeArg::Risk => semantics::ObjectType::Risk,
        }
    }
}

#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum StateArg {
    Open,
    Closed,
    Draft,
    Proposed,
    Accepted,
    Rejected,
    Superseded,
    Identified,
    Mitigated,
    Invalidated,
}

impl StateArg {
    fn model(self) -> semantics::State {
        match self {
            StateArg::Open => semantics::State::Open,
            StateArg::Closed => semantics::State::Closed,
            StateArg::Draft => semantics::State::Draft,
            StateArg::Proposed => semantics::State::Proposed,
            StateArg::Accepted => semantics::State::Accepted,
            StateArg::Rejected => semantics::State::Rejected,
            StateArg::Superseded => semantics::State::Superseded,
            StateArg::Identified => semantics::State::Identified,
            StateArg::Mitigated => semantics::State::Mitigated,
            StateArg::Invalidated => semantics::State::Invalidated,
        }
    }
}

/// `snake_case`, not clap's default kebab: the value written here is the
/// protocol value, and `acceptance-criterion` on the command line for
/// `acceptance_criterion` in the record would be a second spelling of a closed
/// vocabulary.
#[derive(Clone, Copy, PartialEq, ValueEnum)]
#[clap(rename_all = "snake_case")]
enum RoleArg {
    Decision,
    Risk,
    Supersession,
    AcceptanceCriterion,
}

impl RoleArg {
    fn model(self) -> semantics::Role {
        match self {
            RoleArg::Decision => semantics::Role::Decision,
            RoleArg::Risk => semantics::Role::Risk,
            RoleArg::Supersession => semantics::Role::Supersession,
            RoleArg::AcceptanceCriterion => semantics::Role::AcceptanceCriterion,
        }
    }
}

#[derive(Args)]
#[command(group(
    ArgGroup::new("action")
        .required(true)
        .args(["new", "rename", "add", "revise", "merge", "delete", "close", "reopen", "classify", "supersede"])
))]
struct Prepare {
    /// Propose a new object
    #[arg(long)]
    new: bool,
    /// Replace the object's title
    #[arg(long)]
    rename: bool,
    /// Propose a new section
    #[arg(long)]
    add: bool,
    /// Replace the wording of a section
    #[arg(long, value_name = "SECTION")]
    revise: Option<u64>,
    /// Consolidate sections into one
    #[arg(long, value_name = "SECTIONS", value_delimiter = ',')]
    merge: Option<Vec<u64>>,
    /// Remove a section
    #[arg(long, value_name = "SECTION")]
    delete: Option<u64>,
    /// Declare an untyped object finished. Shorthand for --classify --untyped
    /// --state closed
    #[arg(long)]
    close: bool,
    /// Take a finished untyped object back up
    #[arg(long)]
    reopen: bool,
    /// Declare what this object is and what state it is in. Both are explicit
    #[arg(long, requires = "state")]
    classify: bool,
    /// Replace this object with another, confirming the state, the replacement
    /// and the reason together
    #[arg(long, value_name = "OBJECT")]
    supersede: Option<String>,

    /// The object to act on. Any unique id prefix. Omit only with --new
    #[arg(long)]
    object: Option<String>,
    /// Destination type, with --classify
    #[arg(
        long = "type",
        value_enum,
        value_name = "TYPE",
        conflicts_with = "untyped"
    )]
    object_type: Option<TypeArg>,
    /// Destination is an untyped object, with --classify
    #[arg(long)]
    untyped: bool,
    /// Destination state, valid for the destination type
    #[arg(long, value_enum, value_name = "STATE")]
    state: Option<StateArg>,
    /// Wording, inline
    #[arg(long)]
    text: Option<String>,
    /// Wording, from a file
    #[arg(long)]
    text_file: Option<PathBuf>,
    /// What this section asserts, semantically
    #[arg(long, value_enum, value_name = "ROLE")]
    role: Option<RoleArg>,
    /// A bounded literal excerpt, as code.<tag> or data.<tag> and a body.
    /// Repeatable; order is significant
    #[arg(long = "content", value_names = ["TYPE", "BODY"], num_args = 2)]
    content: Vec<String>,
    /// The same, with the body read from a file
    #[arg(long = "content-file", value_names = ["TYPE", "PATH"], num_args = 2)]
    content_file: Vec<String>,
    /// A repository file that implements this assertion
    #[arg(long = "implemented-by-file", value_name = "PATH")]
    implemented_by_file: Vec<String>,
    /// A source symbol that implements this assertion
    #[arg(long = "implemented-by-symbol", value_names = ["PATH", "SYMBOL"], num_args = 2)]
    implemented_by_symbol: Vec<String>,
    /// Committed revision to pin implemented_by targets at. Defaults to HEAD
    /// only while the path itself is clean
    #[arg(long, value_name = "REVISION")]
    implemented_at: Option<String>,
    /// Committed repository basis. Defaults to HEAD only when source is clean
    #[arg(long)]
    based_on: Option<String>,
    /// Record that this wording has no repository basis
    #[arg(long, conflicts_with = "based_on")]
    no_based_on: bool,
    /// A section this wording depends on, as OBJECT:SECTION
    #[arg(long = "ref", value_name = "OBJECT:SECTION")]
    references: Vec<String>,
    /// Retry a proposal a size threshold already refused once
    #[arg(long)]
    oversize: bool,
    #[arg(long)]
    json: bool,
}

impl Prepare {
    /// The supplementary entries, in the order the caller wrote them.
    ///
    /// Inline and from-file entries are two spellings of one list, and clap
    /// cannot interleave two options into one sequence — so the inline ones come
    /// first and the file-backed ones follow. Order is semantic, which is
    /// exactly why that has to be said rather than left to be discovered.
    fn supplements(&self) -> Result<Vec<Supplement>> {
        let mut entries = Vec::new();
        for pair in self.content.chunks(2) {
            let [content_type, body] = pair else {
                return Err(Error::new(EXIT_USAGE, "--content takes a type and a body"));
            };
            entries.push(Supplement::new(content_type.clone(), body.clone()));
        }
        for pair in self.content_file.chunks(2) {
            let [content_type, path] = pair else {
                return Err(Error::new(
                    EXIT_USAGE,
                    "--content-file takes a type and a path",
                ));
            };
            let body = std::fs::read_to_string(path)
                .map_err(|error| engr::tool_error(path.clone(), error))?;
            entries.push(Supplement::new(content_type.clone(), body));
        }
        for entry in &entries {
            entry
                .validate()
                .map_err(|error| malformed_argument("--content", &entry.content_type, error))?;
        }
        Ok(entries)
    }

    /// `implemented_by` targets, each pinned to a real committed snapshot.
    fn relations(&self, root: &Path) -> Result<Vec<Relation>> {
        let revision = self.implemented_at.as_deref();
        let mut relations = Vec::new();
        for path in &self.implemented_by_file {
            relations.push(Relation {
                relation: semantics::RelationType::ImplementedBy,
                target: Target::File {
                    commit: backlog::pin(root, path, revision).map_err(|error| {
                        malformed_argument("--implemented-by-file", path, error)
                    })?,
                    path: path.clone(),
                },
            });
        }
        for pair in self.implemented_by_symbol.chunks(2) {
            let [path, symbol] = pair else {
                return Err(Error::new(
                    EXIT_USAGE,
                    "--implemented-by-symbol takes a path and a symbol name",
                ));
            };
            relations.push(Relation {
                relation: semantics::RelationType::ImplementedBy,
                target: Target::Symbol {
                    commit: backlog::pin(root, path, revision).map_err(|error| {
                        malformed_argument("--implemented-by-symbol", path, error)
                    })?,
                    path: path.clone(),
                    symbol: symbol.clone(),
                },
            });
        }
        for relation in &relations {
            relation
                .validate()
                .map_err(|error| malformed_argument("--implemented-by", "", error))?;
        }
        Ok(relations)
    }
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("error: {}", error.message);
        std::process::exit(error.code);
    }
}

fn run(cli: Cli) -> Result<()> {
    // Before the workspace is located, like `init`: the protocol is what you
    // read to decide whether to adopt engr at all, so needing an adopted
    // workspace to reach it would put it behind the decision it informs.
    if matches!(cli.command, Command::Protocol) {
        print!("{}", engr::PROTOCOL);
        return Ok(());
    }
    if matches!(cli.command, Command::Init) {
        let root = match cli.root {
            Some(path) => path,
            None => std::env::current_dir()
                .map_err(|error| engr::tool_error("current directory", error))?,
        };
        let dir = store::init(&root)?;
        println!("initialised {}", dir.display());
        if git::is_repo(&root) {
            println!("git          ok");
        } else {
            println!(
                "git          not a repository — commit {}/objects and {}/events to preserve the record",
                store::DIR,
                store::DIR
            );
        }
        return Ok(());
    }

    let root = store::find_root(cli.root.as_deref())?;
    if matches!(cli.command, Command::Migrate) {
        store::with_lock(&root, || store::migrate(&root))?;
        println!(
            "migrated {} to workspace version {}",
            store::engr_dir(&root).display(),
            engr::WORKSPACE_VERSION
        );
        return Ok(());
    }
    let workspace_format = store::validate_format(&root)?;

    match cli.command {
        Command::Init | Command::Protocol | Command::Migrate => unreachable!("handled above"),
        Command::Prepare(command) => {
            store::require_current(&root)?;
            prepare(&root, command)
        }
        Command::Candidate { code } => candidate(&root, code.as_deref()),
        Command::Confirm { response } => {
            store::require_current(&root)?;
            let admitted = gate::confirm(&root, &response)?;
            println!(
                "CONFIRMED  {}  {}  rev {}",
                shorten(&admitted.object.id, view::width(&root)),
                admitted.event.payload.action.label(),
                admitted.object.rev
            );
            report_backlog(&root, &admitted.backlog);
            warn_uncommitted(&root, &admitted.object.id);
            Ok(())
        }
        Command::Ls {
            keyword,
            all,
            sections,
            stale,
        } => ls(&root, keyword.as_deref(), all, sections, stale),
        Command::Show { object, format } => {
            let id = resolve_object_argument(&root, "show", &object)?;
            let object = if workspace_format == store::WorkspaceFormat::Current {
                store::with_lock(&root, || ops::reconcile(&root, &id))?
            } else {
                ops::effective(&root, &id)?
            };
            if format == Format::Json {
                println!("{}", view::render_show_json(&root, &object)?);
            } else {
                print!("{}", view::render_show(&root, &object));
            }
            // `show` asserts about one object, so a broken one must not exit 0
            // and let `set -e` carry on. `ls` surveys, and keeps exiting 0.
            let forged = view::assess(&root, &object)
                .iter()
                .filter(|(_, status)| status.forged())
                .count();
            if forged > 0 {
                return Err(Error::new(
                    engr::EXIT_INVARIANT,
                    format!("{forged} sections are not what was confirmed; run: engr verify"),
                ));
            }
            Ok(())
        }
        Command::Verify { object } => verify(&root, object.as_deref()),
        Command::Backlog(command) => backlog_command(&root, command),
    }
}

/// Say what confirmation did to unresolved staging, in the same breath as the
/// admission. A source that moved needs a decision, and the moment the human is
/// still here is the moment to say so.
fn report_backlog(root: &Path, outcomes: &[backlog::Outcome]) {
    let width = view::backlog_width(root);
    for outcome in outcomes {
        let item = shorten(&outcome.item, width);
        let line = match &outcome.result {
            backlog::Reconciliation::Recorded { added: 0 } => {
                "already recorded — nothing to add".to_owned()
            }
            backlog::Reconciliation::Recorded { added } => {
                format!("recorded {added} produced outcome(s); still unresolved")
            }
            backlog::Reconciliation::Consumed { item_removed: true } => {
                "resolved and consumed; the topic had nothing else unresolved".to_owned()
            }
            backlog::Reconciliation::Consumed { .. } => "resolved and consumed".to_owned(),
            backlog::Reconciliation::SourceChanged => {
                "CHANGED since this was prepared — left untouched; reconcile it yourself".to_owned()
            }
            backlog::Reconciliation::SourceGone => "already gone — nothing to reconcile".to_owned(),
        };
        println!("backlog    {item} §{}  {line}", outcome.section);
    }
}

fn backlog_command(root: &Path, command: Backlog) -> Result<()> {
    match command {
        Backlog::New {
            topic,
            text,
            subjects,
        } => {
            let item = backlog::create(root, &topic, &text.read()?, subjects.build(root)?)?;
            print!("{}", view::render_backlog_show(root, &item));
            Ok(())
        }
        Backlog::Ls { keyword } => {
            let items = backlog::all(root)?;
            if items.is_empty() {
                println!("nothing unresolved");
                return Ok(());
            }
            print!(
                "{}",
                view::render_backlog_ls(root, &items, keyword.as_deref())
            );
            Ok(())
        }
        Backlog::Show { item, format } => {
            let item = backlog::load(root, &resolve_backlog_argument(root, "backlog", &item)?)?;
            if format == Format::Json {
                println!("{}", view::render_backlog_json(&item)?);
            } else {
                print!("{}", view::render_backlog_show(root, &item));
            }
            Ok(())
        }
        Backlog::Rename { item, topic } => {
            let id = resolve_backlog_argument(root, "backlog", &item)?;
            let item = backlog::rename(root, &id, &topic)?;
            println!("renamed {}", shorten(&item.id, view::backlog_width(root)));
            Ok(())
        }
        Backlog::Add {
            item,
            text,
            subjects,
        } => {
            let id = resolve_backlog_argument(root, "backlog", &item)?;
            let section = backlog::add_section(root, &id, &text.read()?, subjects.build(root)?)?;
            println!("added §{section}");
            Ok(())
        }
        Backlog::Revise {
            item,
            section,
            text,
        } => {
            let id = resolve_backlog_argument(root, "backlog", &item)?;
            backlog::revise_section(root, &id, section, &text.read()?)?;
            println!("revised §{section}");
            Ok(())
        }
        Backlog::Subjects {
            item,
            section,
            subjects,
        } => {
            let id = resolve_backlog_argument(root, "backlog", &item)?;
            let subjects = subjects.build(root)?;
            let count = subjects.len();
            backlog::set_subjects(root, &id, section, subjects)?;
            println!("§{section} now concerns {count} subject(s)");
            Ok(())
        }
        Backlog::Merge {
            item,
            sections,
            text,
            subjects,
        } => {
            let id = resolve_backlog_argument(root, "backlog", &item)?;
            let section = backlog::merge_sections(
                root,
                &id,
                &sections,
                &text.read()?,
                subjects.build(root)?,
            )?;
            println!("merged into §{section}");
            Ok(())
        }
        Backlog::Rm { item, section } => {
            let id = resolve_backlog_argument(root, "backlog", &item)?;
            match section {
                Some(section) => {
                    if backlog::delete_section(root, &id, section)? {
                        println!(
                            "removed §{section}, and the topic with it — nothing else was unresolved"
                        );
                    } else {
                        println!("removed §{section}");
                    }
                }
                None => {
                    backlog::delete_item(root, &id)?;
                    println!("removed {}", shorten(&id, view::backlog_width(root)));
                }
            }
            Ok(())
        }
    }
}

fn prepare(root: &Path, command: Prepare) -> Result<()> {
    let action = if command.new {
        Action::ObjectCreated
    } else if command.rename {
        Action::ObjectRenamed
    } else if command.add {
        Action::SectionAdded
    } else if let Some(section) = command.revise {
        Action::SectionRevised { section }
    } else if let Some(absorbs) = command.merge.clone() {
        Action::SectionMerged { absorbs }
    } else if let Some(section) = command.delete {
        Action::SectionDeleted { section }
    } else if command.close {
        Action::ObjectClosed
    } else if command.reopen {
        Action::ObjectReopened
    } else if command.classify {
        // Both halves, always. `--untyped` is a word rather than the absence of
        // `--type` because "I want no type" and "I forgot to say" look identical
        // otherwise, and one of them is an authoritative change to what the
        // object is.
        if command.object_type.is_none() && !command.untyped {
            return Err(Error::new(
                EXIT_USAGE,
                "--classify needs the destination type: --type <TYPE>, or --untyped",
            ));
        }
        Action::ObjectClassified {
            object_type: command.object_type.map(TypeArg::model),
            state: command
                .state
                .ok_or_else(|| Error::new(EXIT_USAGE, "--classify needs a destination --state"))?
                .model(),
        }
    } else {
        Action::ObjectSuperseded
    };
    if command.state.is_some() && !command.classify {
        return Err(Error::new(
            EXIT_USAGE,
            "--state sets a destination for --classify; --close, --reopen and --supersede already \
             name the state they produce",
        ));
    }
    if (command.object_type.is_some() || command.untyped) && !command.classify {
        return Err(Error::new(
            EXIT_USAGE,
            "--type and --untyped set a destination for --classify",
        ));
    }

    let object = match (&action, &command.object) {
        (Action::ObjectCreated, Some(_)) => {
            return Err(Error::new(
                EXIT_USAGE,
                "--new mints its own id; drop --object",
            ))
        }
        (Action::ObjectCreated, None) => model::new_id(),
        (_, Some(prefix)) => resolve_object_argument(root, "--object", prefix)?,
        (_, None) => {
            return Err(Error::new(
                EXIT_USAGE,
                "--object is required for everything except --new",
            ))
        }
    };

    let text = match (&command.text, &command.text_file) {
        (Some(_), Some(_)) => {
            return Err(Error::new(
                EXIT_USAGE,
                "use --text or --text-file, not both",
            ))
        }
        (Some(text), None) => Some(text.clone()),
        (None, Some(path)) => Some(
            std::fs::read_to_string(path)
                .map_err(|error| engr::tool_error(path.display(), error))?,
        ),
        (None, None) => None,
    };

    if action.carries_title() && (command.based_on.is_some() || command.no_based_on) {
        return Err(Error::new(
            EXIT_USAGE,
            "a title has no repository basis; use --based-on or --no-based-on only for section wording",
        ));
    }
    let supplements = command.supplements()?;
    let mut relations = command.relations(root)?;
    if action.carries_title()
        && (!command.references.is_empty()
            || command.role.is_some()
            || !supplements.is_empty()
            || !relations.is_empty())
    {
        return Err(Error::new(
            EXIT_USAGE,
            "a title is a label: --ref, --role, --content and --implemented-by apply only to \
             section wording",
        ));
    }
    if !action.carries_content()
        && (command.role.is_some() || !supplements.is_empty() || !relations.is_empty())
    {
        return Err(Error::new(
            EXIT_USAGE,
            format!(
                "{} carries no wording, so it carries no role, content or relations",
                action.label()
            ),
        ));
    }

    let mut references = Vec::new();
    for spec in &command.references {
        references.push(parse_ref(root, spec)?);
    }
    check_unique_arguments(&references, "--ref")?;
    if !action.carries_content() && command.no_based_on {
        return Err(Error::new(
            EXIT_USAGE,
            "--no-based-on applies only to section wording",
        ));
    }

    // The replacement is an argument to `--supersede` rather than another
    // relation flag, because it is not optional metadata on the action — the
    // action means nothing without it, and the relation, the state and the
    // reason are one thing a human confirms once.
    let role = match (&command.supersede, command.role) {
        (Some(target), role) => {
            if !matches!(role, None | Some(RoleArg::Supersession)) {
                return Err(Error::new(
                    EXIT_USAGE,
                    "--supersede writes the reason it was replaced, so its section is \
                     role=supersession",
                ));
            }
            let target = resolve_object_argument(root, "--supersede", target)?;
            let compact = engr::reference::encode_uuid_str(&target)?;
            relations.push(Relation::superseded_by(format!("obj:{compact}")));
            Some(semantics::Role::Supersession)
        }
        (None, role) => role.map(RoleArg::model),
    };
    check_unique_arguments(&relations, "--implemented-by")?;

    let mut content = gate::content(
        root,
        text,
        command.based_on.clone(),
        command.no_based_on || action.carries_title(),
        references,
    )?;
    content.role = role;
    content.content = supplements;
    content.relations = relations;
    let payload = Payload {
        action,
        object,
        content,
    };
    let prepared = if command.oversize {
        gate::prepare_oversize(root, payload)?
    } else {
        gate::prepare(root, payload)?
    };

    if command.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&prepared.candidate)
                .map_err(|error| Error::new(engr::EXIT_SCHEMA, format!("json: {error}")))?
        );
        return Ok(());
    }
    print!(
        "{}",
        render_candidate(root, &prepared.candidate, &prepared.notes)
    );
    for code in &prepared.superseded {
        println!("(candidate {code} was superseded by this one)");
    }
    Ok(())
}

/// The shared parser and the domain both report a malformed value as a schema
/// error, because both also run against stored authority. Typed at a command
/// line the same value is a person getting an argument wrong, and the two must
/// not share an exit code: one says "your input is invalid", the other says
/// "the workspace on disk is". Translated here, at that boundary, and nowhere
/// deeper — a missing resource stays not-found, and a malformed stored file
/// reached through a valid argument stays schema.
/// Sets are checked where the caller typed them, so a repeat reads as the typo
/// it is rather than as a report that the workspace on disk is malformed. The
/// domain refuses the same duplicate again on the way in; this is the message,
/// not the guarantee.
fn check_unique_arguments<T: PartialEq>(items: &[T], flag: &str) -> Result<()> {
    for (index, item) in items.iter().enumerate() {
        if items[..index].contains(item) {
            return Err(Error::new(
                EXIT_USAGE,
                format!("the same {flag} is given twice; it is a set, so listing it again says nothing new"),
            ));
        }
    }
    Ok(())
}

fn malformed_argument(field: &str, spec: &str, error: Error) -> Error {
    if error.code == EXIT_SCHEMA {
        Error::new(EXIT_USAGE, format!("{field} {spec:?}: {}", error.message))
    } else {
        error
    }
}

fn resolve_object_argument(root: &Path, field: &str, spec: &str) -> Result<String> {
    if spec.starts_with("engr:") {
        let reference = engr::reference::EngrRef::parse_standalone(spec)
            .map_err(|error| malformed_argument(field, spec, error))?;
        if reference.kind() != engr::reference::ResourceKind::Object
            || reference.section().is_some()
            || reference.snapshot_selector().is_some()
        {
            return Err(Error::new(
                EXIT_USAGE,
                format!("{field} {spec:?} must identify a current whole Object"),
            ));
        }
    }
    store::resolve_id(root, spec).map_err(|error| malformed_argument(field, spec, error))
}

/// The same boundary for the Backlog namespace. Every `engr backlog` command
/// addresses a whole item, so a Section reference is a legal thing to write and
/// the wrong thing to write here — a usage error, not a missing resource.
fn resolve_backlog_argument(root: &Path, field: &str, spec: &str) -> Result<String> {
    if spec.starts_with("engr:") {
        let reference = engr::reference::EngrRef::parse_standalone(spec)
            .map_err(|error| malformed_argument(field, spec, error))?;
        if reference.kind() != engr::reference::ResourceKind::Backlog
            || reference.section().is_some()
            || reference.snapshot_selector().is_some()
        {
            return Err(Error::new(
                EXIT_USAGE,
                format!("{field} {spec:?} must identify a current whole Backlog item"),
            ));
        }
    }
    backlog::resolve_id(root, spec).map_err(|error| malformed_argument(field, spec, error))
}

fn parse_ref(root: &Path, spec: &str) -> Result<Ref> {
    if spec.starts_with("engr:") {
        let reference = engr::reference::EngrRef::parse_standalone(spec)
            .map_err(|error| malformed_argument("--ref", spec, error))?;
        if reference.kind() != engr::reference::ResourceKind::Object
            || reference.section().is_none()
        {
            return Err(Error::new(
                EXIT_USAGE,
                format!("--ref {spec:?} must identify an Object section"),
            ));
        }
        let canonical = reference
            .canonicalize(|revision| git::resolve(root, revision))
            .map_err(|error| malformed_argument("--ref", spec, error))?;
        let id = engr::reference::decode_uuid(canonical.id())
            .map_err(|error| malformed_argument("--ref", spec, error))?
            .to_string();
        let section = canonical
            .section()
            .expect("checked before canonicalization");
        let target_section = ops::effective_section(root, &id, section)?;
        let commit = match canonical.snapshot() {
            Some(commit) => commit.to_owned(),
            None => git::head(root).ok_or_else(|| {
                Error::new(
                    engr::EXIT_INVARIANT,
                    "a reference records the commit it was read at, which needs a git repository",
                )
            })?,
        };
        return Ok(Ref {
            object: id,
            section,
            sha256: target_section.sha256.clone(),
            commit,
        });
    }
    let (prefix, section) = spec.split_once(':').ok_or_else(|| {
        Error::new(
            EXIT_USAGE,
            format!("--ref {spec:?} must be OBJECT:SECTION, for example 019ff800:2"),
        )
    })?;
    let section: u64 = section.parse().map_err(|_| {
        Error::new(
            EXIT_USAGE,
            format!("--ref {spec:?}: section must be a number"),
        )
    })?;
    let id = resolve_object_argument(root, "--ref", prefix)?;
    let target_section = ops::effective_section(root, &id, section)?;
    let commit = git::head(root).ok_or_else(|| {
        Error::new(
            engr::EXIT_INVARIANT,
            "a reference records the commit it was read at, which needs a git repository",
        )
    })?;
    Ok(Ref {
        object: id,
        section,
        sha256: target_section.sha256.clone(),
        commit,
    })
}

fn shorten(id: &str, width: usize) -> &str {
    &id[..width.min(id.len())]
}

fn render_ref(reference: &Ref, width: usize) -> String {
    format!(
        "{} §{}  sha256 {}  commit {}",
        shorten(&reference.object, width),
        reference.section,
        shorten(&reference.sha256, 8),
        shorten(&reference.commit, 8)
    )
}

fn render_relation(relation: &Relation) -> String {
    relation.render(|commit| shorten(commit, 8).to_owned())
}

/// Role is a machine-readable claim about what the wording means, so a change
/// to it is shown even when the wording itself is untouched.
fn render_role(
    out: &mut String,
    previous: Option<semantics::Role>,
    proposed: Option<semantics::Role>,
) {
    let name = |role: Option<semantics::Role>| role.map_or("none", |role| role.as_str());
    if previous != proposed {
        out.push_str(&format!(
            "Role     - {}\nRole     + {}\n",
            name(previous),
            name(proposed)
        ));
    } else if proposed.is_some() {
        out.push_str(&format!("Role       {}\n", name(proposed)));
    }
}

/// Supplementary content is ordered, so a moved entry is a change. Rendering it
/// as an index-by-index comparison rather than as two sets is what makes a
/// reorder visible instead of silently matching up.
fn render_supplement_diff(out: &mut String, previous: &[Supplement], proposed: &[Supplement]) {
    for index in 0..previous.len().max(proposed.len()) {
        match (previous.get(index), proposed.get(index)) {
            (Some(before), Some(after)) if before == after => {
                out.push_str(&format!("Content    [{index}] {}\n", after.content_type));
            }
            (before, after) => {
                if let Some(before) = before {
                    out.push_str(&format!("Content  - [{index}] {}\n", before.content_type));
                }
                if let Some(after) = after {
                    out.push_str(&format!("Content  + [{index}] {}\n", after.content_type));
                }
            }
        }
    }
}

/// The bodies themselves, diffed against the entry that held the same position
/// when there was one.
fn render_supplement_bodies(
    out: &mut String,
    previous: &[Supplement],
    proposed: &[Supplement],
    revising: bool,
) {
    for (index, entry) in proposed.iter().enumerate() {
        let before = revising.then(|| previous.get(index)).flatten();
        match before {
            Some(before) if before == entry => continue,
            Some(before) => {
                out.push_str(&format!(
                    "\n── content [{index}] {} ──\n",
                    entry.content_type
                ));
                if before.content_type != entry.content_type {
                    out.push_str(&format!("(was {})\n", before.content_type));
                }
                let diff = similar::TextDiff::from_lines(&before.body, &entry.body);
                out.push_str(
                    &diff
                        .unified_diff()
                        .context_radius(3)
                        .header("previous", "candidate")
                        .to_string(),
                );
            }
            None => {
                out.push_str(&format!(
                    "\n── content [{index}] {} ──\n",
                    entry.content_type
                ));
                out.push_str(entry.body.trim_end());
                out.push('\n');
            }
        }
    }
    if revising {
        for (index, entry) in previous.iter().enumerate().skip(proposed.len()) {
            out.push_str(&format!(
                "\n── content [{index}] {} ── removed\n",
                entry.content_type
            ));
            // The body, not only the type. Duplicate types are valid, so with
            // two `code.rs` entries a heading names a position rather than a
            // thing — and a human asked to admit a deletion has to be shown what
            // is being deleted, the same way removed wording appears in the text
            // diff. It is hashed with the section; it is not a detail.
            out.push_str(entry.body.trim_end());
            out.push('\n');
        }
    }
}

fn render_basis(basis: Option<&str>) -> String {
    basis
        .map(|commit| shorten(commit, 8).to_owned())
        .unwrap_or_else(|| "none (explicit)".to_owned())
}

/// Two namespaces, two abbreviation widths. An id is only short enough when it
/// is still unique among its own kind, and Backlog ids abbreviate against
/// Backlog ids — borrowing the Object width can print two different unresolved
/// points identically on the one screen that says which of them confirming
/// consumes.
fn render_candidate(root: &Path, candidate: &gate::Candidate, notes: &[gate::Note]) -> String {
    let width = view::width(root);
    let backlog_width = view::backlog_width(root);
    let mut out = String::new();
    out.push_str(&format!(
        "Candidate  {}\nObject     {}\n",
        candidate.payload.action.label(),
        shorten(&candidate.payload.object, width)
    ));
    // A title is a label, not wording written against code, so the commit it
    // happened to be typed at says nothing about the change being confirmed.
    // It stays in the payload; it just does not belong on this screen, where
    // every line that means nothing is a line that trains people to skim.
    // The whole destination, both halves, because that is what is being
    // confirmed: a state read without the type it belongs to is a word that
    // means different things on different objects.
    if let Action::ObjectClassified { object_type, state } = &candidate.payload.action {
        out.push_str(&format!(
            "Type       {}\nState      {}\nAttention  {}\n",
            object_type.map_or("none", |value| value.as_str()),
            state.as_str(),
            if semantics::needs_attention(*object_type, *state) {
                "yes — it stays in the default listing"
            } else {
                "no — it leaves the default listing"
            }
        ));
    }
    if !candidate.payload.action.carries_title() && candidate.payload.action.carries_content() {
        if matches!(candidate.payload.action, Action::SectionRevised { .. }) {
            if !candidate.context.previous_semantics_recorded {
                out.push_str(
                    "WARNING    semantic revision metadata is unavailable; this legacy candidate cannot be confirmed\n",
                );
            }
            render_role(
                &mut out,
                candidate.context.previous_role,
                candidate.payload.content.role,
            );
            render_supplement_diff(
                &mut out,
                &candidate.context.previous_content,
                &candidate.payload.content.content,
            );
            for relation in &candidate.context.previous_relations {
                if !candidate.payload.content.relations.contains(relation) {
                    out.push_str(&format!("Relation - {}\n", render_relation(relation)));
                }
            }
            for relation in &candidate.payload.content.relations {
                if !candidate.context.previous_relations.contains(relation) {
                    out.push_str(&format!("Relation + {}\n", render_relation(relation)));
                }
            }
            if candidate.context.previous_based_on != candidate.payload.content.based_on {
                out.push_str(&format!(
                    "Based on - {}\nBased on + {}\n",
                    render_basis(candidate.context.previous_based_on.as_deref()),
                    render_basis(candidate.payload.content.based_on.as_deref())
                ));
            } else {
                out.push_str(&format!(
                    "Based on   {}\n",
                    render_basis(candidate.payload.content.based_on.as_deref())
                ));
            }
            for reference in &candidate.context.previous_refs {
                if !candidate.payload.content.refs.contains(reference) {
                    out.push_str(&format!("Ref      - {}\n", render_ref(reference, width)));
                }
            }
            for reference in &candidate.payload.content.refs {
                if !candidate.context.previous_refs.contains(reference) {
                    out.push_str(&format!("Ref      + {}\n", render_ref(reference, width)));
                }
            }
        } else {
            if let Some(role) = candidate.payload.content.role {
                out.push_str(&format!("Role       {}\n", role.as_str()));
            }
            for (index, entry) in candidate.payload.content.content.iter().enumerate() {
                out.push_str(&format!("Content    [{index}] {}\n", entry.content_type));
            }
            for relation in &candidate.payload.content.relations {
                out.push_str(&format!("Relation   {}\n", render_relation(relation)));
            }
            out.push_str(&format!(
                "Based on   {}\n",
                render_basis(candidate.payload.content.based_on.as_deref())
            ));
            for reference in &candidate.payload.content.refs {
                out.push_str(&format!("Ref        {}\n", render_ref(reference, width)));
            }
        }
        if matches!(candidate.payload.action, Action::ObjectSuperseded) {
            out.push_str(
                "State      superseded — this object leaves the default listing, and the \
                 relation above is where a reader is sent instead\n",
            );
        }
        // Loud, and above the wording. The human is being asked to admit
        // something engr already refused once, and the only way that stays a
        // decision rather than a formality is if the screen says so before they
        // read the text.
        if candidate.context.oversize {
            let exceeded = semantics::exceeded(
                &candidate.payload.content.text,
                &candidate.payload.content.content,
            );
            out.push_str(&format!(
                "OVERSIZE   admitted by exception: {}\n",
                exceeded
                    .iter()
                    .map(|item| item.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
    }
    // Confirming this will also edit unresolved staging, so the human reading
    // the change is shown that before they type, not told about it afterwards.
    for source in &candidate.context.backlog {
        out.push_str(&format!(
            "Backlog    {} §{}  {}\n",
            shorten(&source.item, backlog_width),
            source.section,
            if source.resolves {
                "resolved by this — will be consumed"
            } else {
                "still unresolved after this"
            }
        ));
        for produced in &source.produced {
            out.push_str(&format!(
                "           produced engr:{}\n",
                produced.target.reference
            ));
        }
    }
    out.push('\n');
    // Show the change, not the whole section again: making a human re-read
    // everything is how confirmation decays into rubber-stamping.
    match (&candidate.context.previous_text, &candidate.payload.action) {
        (Some(previous), _) => {
            let diff = similar::TextDiff::from_lines(previous, &candidate.payload.content.text);
            out.push_str(
                &diff
                    .unified_diff()
                    .context_radius(3)
                    .header("previous", "candidate")
                    .to_string(),
            );
        }
        (None, _) if candidate.payload.action.carries_content() => {
            out.push_str(candidate.payload.content.text.trim_end());
            out.push('\n');
        }
        (None, action) => {
            out.push_str(&format!("({})\n", action.label()));
        }
    }
    // In full, and never elided. Supplementary content is part of the assertion
    // being confirmed and part of what gets hashed, so a human who was shown
    // only its type has not read what they are about to admit. It is bounded
    // precisely so that printing all of it stays reasonable.
    render_supplement_bodies(
        &mut out,
        &candidate.context.previous_content,
        &candidate.payload.content.content,
        candidate.context.previous_semantics_recorded,
    );
    // Above the code, not below it: the point of a note is to be read while
    // there is still a decision to make.
    for note in notes {
        match note {
            gate::Note::DuplicateTitle { object } => out.push_str(&format!(
                "\nnote       an existing object has this title: {}\n",
                shorten(object, width)
            )),
        }
    }
    out.push_str(&format!(
        "\nType this exactly to confirm:  CONFIRM {}\n",
        candidate.challenge
    ));
    out
}

fn candidate(root: &Path, code: Option<&str>) -> Result<()> {
    match code {
        Some(code) => {
            let candidate = gate::find(root, code)?;
            let notes = gate::notes_for(root, &candidate);
            print!("{}", render_candidate(root, &candidate, &notes));
            match gate::candidate_state(root, &candidate)? {
                gate::CandidateState::Pending => {}
                gate::CandidateState::AlreadyApplied(_) => println!(
                    "\nThis candidate was already applied. Retry the same confirmation to finish cleanup."
                ),
                gate::CandidateState::Stale { .. } => println!(
                    "\nThis candidate is dead — the object moved after it was prepared. Prepare again."
                ),
            }
            Ok(())
        }
        None => {
            let codes = gate::pending_codes(root)?;
            if codes.is_empty() {
                println!("nothing is awaiting confirmation");
                return Ok(());
            }
            let width = view::width(root);
            // Per code, not the whole list at once. One candidate this build
            // will not admit — left by an older one, or edited on disk — is
            // exactly what somebody runs this to find out about, so it belongs
            // on a line of its own rather than replacing the listing.
            for code in codes {
                let candidate = match gate::find(root, &code) {
                    Ok(candidate) => candidate,
                    Err(error) => {
                        println!("{code}   {:<16} {}", "unusable", error.message);
                        continue;
                    }
                };
                println!(
                    "{}   {:<16} {} {:<8} {}",
                    candidate.challenge,
                    candidate.payload.action.label(),
                    shorten(&candidate.payload.object, width),
                    match gate::candidate_state(root, &candidate)? {
                        gate::CandidateState::Pending => "pending",
                        gate::CandidateState::AlreadyApplied(_) => "retry",
                        gate::CandidateState::Stale { .. } => "stale",
                    },
                    candidate.created_at
                );
            }
            Ok(())
        }
    }
}

fn load_all(root: &Path, all: bool) -> Result<Vec<engr::model::Object>> {
    let mut objects = Vec::new();
    for id in store::object_ids(root)? {
        let object = ops::effective(root, &id)?;
        if all || object.needs_attention() {
            objects.push(object);
        }
    }
    objects.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(objects)
}

fn ls(root: &Path, keyword: Option<&str>, all: bool, sections: bool, stale: bool) -> Result<()> {
    // A closed object whose basis moved is the one case that must surface
    // unprompted. `--stale` therefore cannot inherit `ls`'s open-only default.
    let objects = load_all(root, all || stale)?;
    if objects.is_empty() {
        println!("no objects");
        return Ok(());
    }
    if sections {
        // stdout stays byte for byte what it was — this is the surface people
        // pipe into grep — so the alarm goes to stderr, which survives the pipe.
        print!("{}", view::render_ls_sections(root, &objects));
        let forged = view::tampered_count(&objects);
        if forged > 0 {
            eprintln!("!! {forged} sections do not match their hashes; run: engr verify");
        }
    } else if stale {
        let out = view::render_stale(root, &objects);
        if out.is_empty() {
            println!("all ok");
        } else {
            print!("{out}");
        }
    } else {
        print!("{}", view::render_ls(root, &objects, keyword));
    }
    Ok(())
}

fn verify(root: &Path, object: Option<&str>) -> Result<()> {
    let ids = match object {
        Some(prefix) => vec![resolve_object_argument(root, "verify", prefix)?],
        None => store::object_ids(root)?,
    };
    if ids.is_empty() {
        return Err(Error::new(EXIT_NOT_FOUND, "no objects to verify"));
    }
    let mut failed = false;
    let width = view::width(root);
    for id in ids {
        let report = ops::verify(root, &id)?;
        let verdict = if report.passed() { "PASS" } else { "FAIL" };
        println!(
            "{}  {:<4}  {} sections  {}",
            shorten(&report.object, width),
            verdict,
            report.sections,
            report.title
        );
        for section in &report.tampered {
            println!("          §{section} content does not match its recorded hash");
        }
        for stood in &report.standing_on_tampered {
            println!(
                "          §{} stands on {} §{}, which does not match its own hash",
                stood.section,
                shorten(&stood.target, width),
                stood.target_section
            );
        }
        if report.unprojected > 0 {
            println!(
                "          {} events are not reflected in the sections",
                report.unprojected
            );
        }
        if report.uncommitted == Some(true) {
            println!("          uncommitted — git holds no record of the current wording yet");
        }
        failed |= !report.passed();
    }
    if failed {
        return Err(Error::new(engr::EXIT_INVARIANT, "verification failed"));
    }
    Ok(())
}

fn warn_uncommitted(root: &Path, id: &str) {
    if git::uncommitted(root, &store::object_path(root, id)) == Some(true) {
        println!(
            "note       commit {}/objects and {}/events to preserve history and look-back",
            store::DIR,
            store::DIR
        );
    }
}

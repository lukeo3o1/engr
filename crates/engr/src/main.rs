use clap::{ArgGroup, Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use engr::backlog::{self, Subject};
use engr::model::{self, Action, Payload, Ref};
use engr::semantics::{self, Relation, Supplement, Target};
use engr::{collection, gate, git, ops, store, view, work};
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
        /// Sections whose basis or references moved, or that will not verify
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
    /// Execution memory an agent keeps for an Object. Nothing here is confirmed
    #[command(subcommand)]
    Work(Work),
    /// Planning metadata: what is grouped together. Nothing here is confirmed
    #[command(subcommand)]
    Collection(CollectionCommand),
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
    /// Declare an untyped object finished. Not a spelling of --classify: it is
    /// its own action, and it refuses an object nobody is looking at
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
    /// Destination type. With --classify, or on a settled object with --add,
    /// --revise, --merge, --delete or --rename, to do both in one confirmation
    #[arg(
        long = "type",
        value_enum,
        value_name = "TYPE",
        conflicts_with = "untyped"
    )]
    object_type: Option<TypeArg>,
    /// Destination is an untyped object. Same two uses as --type
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
    /// Which flag wrote each `content[]` entry, in the order they appeared on
    /// the command line. Filled from the parsed matches rather than by clap's
    /// derive, which has no way to interleave two lists — see [`content_order`].
    #[arg(skip)]
    content_order: Vec<ContentSource>,
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
        let mut inline = Vec::new();
        for pair in self.content.chunks(2) {
            let [content_type, body] = pair else {
                return Err(Error::new(EXIT_USAGE, "--content takes a type and a body"));
            };
            inline.push(Supplement::new(content_type.clone(), body.clone()));
        }
        let mut from_file = Vec::new();
        for pair in self.content_file.chunks(2) {
            let [content_type, path] = pair else {
                return Err(Error::new(
                    EXIT_USAGE,
                    "--content-file takes a type and a path",
                ));
            };
            let body = std::fs::read_to_string(path)
                .map_err(|error| engr::tool_error(path.clone(), error))?;
            from_file.push(Supplement::new(content_type.clone(), body));
        }
        // `content[]` is ordered, and the order is part of the assertion — a
        // reader goes through the excerpts in sequence, and moving one is a
        // revision. So the entries come out in the order they were written on
        // the command line, not grouped by which flag spelled them: clap keeps
        // each flag's values in its own list, and taking one list after the
        // other would turn `--content A --content-file B --content C` into
        // A, C, B. That is authoritative input being reordered.
        //
        // `content_order` is the interleaving clap actually saw. Empty when
        // nothing recorded it — a direct constructor in a test — in which case
        // the two lists follow each other, the same answer whenever only one
        // spelling was used.
        let mut inline = inline.into_iter();
        let mut from_file = from_file.into_iter();
        let mut entries = Vec::new();
        for source in &self.content_order {
            let next = match source {
                ContentSource::Inline => inline.next(),
                ContentSource::File => from_file.next(),
            };
            if let Some(entry) = next {
                entries.push(entry);
            }
        }
        entries.extend(inline);
        entries.extend(from_file);
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

/// Which flag a `content[]` entry was written with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ContentSource {
    Inline,
    File,
}

/// Recover the order the caller wrote `--content` and `--content-file` in.
///
/// `content[]` order is semantic, and the derive API cannot express one
/// interleaved sequence across two options — each keeps its own list, and the
/// lists say nothing about which came first. The parsed matches do: every value
/// carries its position on the command line. Two values per occurrence, so the
/// first index of each pair marks where that entry was written.
///
/// Read from the matches rather than from `std::env::args`, because a body is
/// arbitrary text and one that happens to read `--content-file` would fool a
/// scan of the raw arguments. clap already knows which tokens were flags.
fn content_order(matches: &clap::ArgMatches) -> Vec<ContentSource> {
    let Some(prepare) = matches.subcommand_matches("prepare") else {
        return Vec::new();
    };
    let mut written: Vec<(usize, ContentSource)> = Vec::new();
    for (id, source) in [
        ("content", ContentSource::Inline),
        ("content_file", ContentSource::File),
    ] {
        let Some(indices) = prepare.indices_of(id) else {
            continue;
        };
        let indices: Vec<usize> = indices.collect();
        for pair in indices.chunks(2) {
            written.push((pair[0], source));
        }
    }
    written.sort_by_key(|(index, _)| *index);
    written.into_iter().map(|(_, source)| source).collect()
}

fn main() {
    // The long way round `Cli::parse()`, which is exactly this pair, because the
    // matches are thrown away by `parse` and one thing here needs them.
    let matches = Cli::command().get_matches();
    let mut cli = match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(error) => error.exit(),
    };
    if let Command::Prepare(prepare) = &mut cli.command {
        prepare.content_order = content_order(&matches);
    }
    if let Err(error) = run(cli) {
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
                // The event's rev, not the object's. They coincide except on
                // the already-applied retry — the one path that exists to
                // reassure someone after a crash, and the one where naming a
                // later revision would say the wrong thing happened.
                admitted.event.rev
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
        Command::Work(command) => work_command(&root, command),
        Command::Collection(command) => collection_command(&root, command),
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
    // A destination belongs either to `--classify`, which is only a
    // classification, or to an action that needs the object back in the
    // attention set to run at all — where it is applied in the same
    // confirmation, so no state the object was never really in gets recorded on
    // the way. Every other action already names the state it produces.
    //
    // Only the shape is settled here. Whether *this* object may take a
    // destination at all depends on the state it is in, and that is the
    // reducer's call, not the parser's: `gate::prepare` projects a trial event,
    // so an object that already needs attention is refused there, once, with
    // the authority that will still be enforcing it when the event replays.
    let becomes = if command.classify {
        None
    } else if command.state.is_some() || command.object_type.is_some() || command.untyped {
        if !action.requires_attention() {
            return Err(Error::new(
                EXIT_USAGE,
                format!(
                    "{} already names the state it produces, so it takes no destination",
                    action.label()
                ),
            ));
        }
        if command.object_type.is_none() && !command.untyped {
            return Err(Error::new(
                EXIT_USAGE,
                "a destination needs its type: --type <TYPE>, or --untyped",
            ));
        }
        Some(model::Destination {
            object_type: command.object_type.map(TypeArg::model),
            state: command
                .state
                .ok_or_else(|| Error::new(EXIT_USAGE, "a destination needs a --state"))?
                .model(),
        })
    } else {
        None
    };

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
        becomes,
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
                    "\n── content [{index}] {} ──{}\n",
                    entry.content_type,
                    tail_suffix(" ", both_tails(Some(&before.body), Some(&entry.body)))
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
                    "\n── content [{index}] {} ──{}\n",
                    entry.content_type,
                    tail_suffix(" ", both_tails(None, Some(&entry.body)))
                ));
                push_body(out, &entry.body);
            }
        }
    }
    if revising {
        for (index, entry) in previous.iter().enumerate().skip(proposed.len()) {
            out.push_str(&format!(
                "\n── content [{index}] {} ── removed{}\n",
                entry.content_type,
                tail_suffix(", ", both_tails(Some(&entry.body), None))
            ));
            // The body, not only the type. Duplicate types are valid, so with
            // two `code.rs` entries a heading names a position rather than a
            // thing — and a human asked to admit a deletion has to be shown what
            // is being deleted, the same way removed wording appears in the text
            // diff. It is hashed with the section; it is not a detail.
            push_body(out, &entry.body);
        }
    }
}

/// A literal body, exactly as it is stored or proposed.
///
/// Never trimmed. Every byte of a body is inside the Section hash, so a
/// renderer that tidied one would be able to draw two different authoritative
/// values the same way — which is the one thing this gate exists to prevent.
/// The single newline is the renderer's own line break, added only when the
/// body does not already end in one; what that break might be hiding is said
/// out loud by [`tail_note`].
fn push_body(out: &mut String, body: &str) {
    out.push_str(body);
    if !body.ends_with('\n') {
        out.push('\n');
    }
}

/// How a body ends, when the way it ends cannot be seen.
///
/// A terminal draws `"x"`, `"x\n"` and `"x   "` identically, and a body of
/// nothing but spaces as nothing at all — yet those are different literals with
/// different Section hashes, and a human is being asked to admit one of them
/// specifically. #14 defines a body as literal non-empty content and says
/// nothing about normalising it, so v0 keeps every byte and describes the ones
/// that do not show. Counted, not merely named: "2 spaces" and "3 spaces" are
/// as different as anything else here, and a quoted run of spaces is no more
/// countable on screen than an unquoted one.
///
/// `None` when the body ends in something visible, which is the ordinary case —
/// a note on every entry would be a note nobody reads.
fn tail_note(body: &str) -> Option<String> {
    let visible = body.trim_end_matches(char::is_whitespace);
    if visible.len() == body.len() {
        return None;
    }
    let mut runs: Vec<String> = Vec::new();
    let mut characters = body[visible.len()..].chars().peekable();
    while let Some(character) = characters.next() {
        let mut count = 1;
        while characters.peek() == Some(&character) {
            characters.next();
            count += 1;
        }
        runs.push(format!("{count} {}", whitespace_name(character, count)));
    }
    let listed = match runs.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => return None,
    };
    Some(if visible.is_empty() {
        format!("{listed}, and nothing else")
    } else {
        format!("ends with {listed}")
    })
}

fn whitespace_name(character: char, count: usize) -> String {
    let name = match character {
        ' ' => "space",
        '\t' => "tab",
        '\n' => "newline",
        '\r' => "carriage return",
        other => return format!("U+{:04X}", other as u32),
    };
    if count == 1 {
        name.to_owned()
    } else {
        format!("{name}s")
    }
}

/// The heading suffix for one entry, naming whichever side has an invisible
/// tail. Both sides are named when both do, because a revision that only moves
/// trailing whitespace is a real change to a real hash and the diff below
/// cannot show it.
fn both_tails(previous: Option<&str>, proposed: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(note) = previous.and_then(tail_note) {
        parts.push(match proposed {
            Some(_) => format!("previous {note}"),
            None => note,
        });
    }
    if let Some(note) = proposed.and_then(tail_note) {
        parts.push(match previous {
            Some(_) => format!("candidate {note}"),
            None => note,
        });
    }
    parts.join("; ")
}

/// The note as a heading suffix, punctuated for whichever heading it follows.
fn tail_suffix(separator: &str, note: String) -> String {
    if note.is_empty() {
        String::new()
    } else {
        format!("{separator}{note}")
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
    // The action names what is being done; without the section it applies to,
    // it does not name *what to*. Two sections can carry identical wording, and
    // then `--delete 1` and `--delete 2` render the same screen for two
    // mutations that are not interchangeable: ids are never reused, so
    // confirming the wrong one breaks every reference pinning it with no way
    // back. `Payload`'s own rustdoc promises "delete §3 cannot become delete §5
    // after it was displayed" — the hash kept that promise, the screen did not.
    //
    // The object gets its title for the same reason. A human asked to assent to
    // a change is entitled to be told which record they are changing by a name
    // they would recognise, not only by an abbreviated uuid.
    //
    // It comes from the prepared context, never from a fresh read. A live
    // lookup would put part of the confirmation identity outside the candidate
    // and outside its integrity value, so a title rewritten afterwards would
    // change what a pending candidate presents while the payload hash, the
    // integrity hash and `expected_rev` all still checked out. Omitted when
    // there is none, which is the case for `object.created` — its title is the
    // wording below — and for candidates prepared before the snapshot existed.
    let subject = match &candidate.payload.action {
        Action::SectionRevised { section } | Action::SectionDeleted { section } => {
            format!(" §{section}")
        }
        Action::SectionMerged { absorbs } => format!(
            " absorbing {}",
            absorbs
                .iter()
                .map(|section| format!("§{section}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => String::new(),
    };
    let title = candidate
        .context
        .object_title
        .as_deref()
        .map(|title| format!("  {title}"))
        .unwrap_or_default();
    out.push_str(&format!(
        "Candidate  {}{}\nObject     {}{}\n",
        candidate.payload.action.label(),
        subject,
        shorten(&candidate.payload.object, width),
        title
    ));
    // A title is a label, not wording written against code, so the commit it
    // happened to be typed at says nothing about the change being confirmed.
    // It stays in the payload; it just does not belong on this screen, where
    // every line that means nothing is a line that trains people to skim.
    // The whole destination, both halves, because that is what is being
    // confirmed: a state read without the type it belongs to is a word that
    // means different things on different objects.
    // Both spellings of a destination reach this screen the same way: one is a
    // classification on its own, the other rides along with a section action to
    // bring the object back into attention in the same confirmation. Either way
    // it is part of what is being confirmed, so it is part of what is shown.
    let destination = match &candidate.payload.action {
        Action::ObjectClassified { object_type, state } => Some((*object_type, *state)),
        _ => candidate
            .payload
            .becomes
            .as_ref()
            .map(|becomes| (becomes.object_type, becomes.state)),
    };
    if let Some((object_type, state)) = destination {
        out.push_str(&format!(
            "Type       {}\nState      {}\nAttention  {}\n",
            object_type.map_or("none", |value| value.as_str()),
            state.as_str(),
            if semantics::needs_attention(object_type, state) {
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
        // Said separately from tampering, and from each other. "Not there" and
        // "will not load" are different problems with different answers, and
        // both used to be silence.
        for stood in &report.standing_on_missing {
            println!(
                "          §{} stands on {} §{}, which is not there",
                stood.section,
                shorten(&stood.target, width),
                stood.target_section
            );
        }
        for stood in &report.standing_on_unreadable {
            println!(
                "          §{} stands on {} §{}, which will not load: {}",
                stood.section,
                shorten(&stood.target, width),
                stood.target_section,
                stood.reason
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

/// Execution memory an agent keeps for an Object.
///
/// Its own namespace, like `backlog`, and for the same reason: nothing here
/// goes through the gate, so it must not be reachable by a command that looks
/// like one that does. `ls`, `show` and `verify` still mean confirmed record
/// and nothing else.
#[derive(Subcommand)]
enum Work {
    /// Begin keeping execution memory for an Object
    Start {
        object: String,
        /// Where execution currently stands
        #[arg(long)]
        summary: Option<String>,
    },
    /// List the Objects with execution memory
    Ls,
    /// Show one Object's execution memory
    Show {
        object: String,
        #[arg(long, value_enum, default_value = "text")]
        format: Format,
    },
    /// Replace the checkpoint. Omit --text to clear it
    Summary {
        object: String,
        #[arg(long)]
        text: Option<String>,
    },
    /// Suspend autonomous execution. Only on explicit human direction
    Pause { object: String },
    /// Resume it. Only on explicit human direction
    Resume { object: String },
    /// Stop keeping execution memory. Deleting paused work says so
    Rm { object: String },
    /// Record something this work relies on
    Depend {
        object: String,
        /// engr:obj:<id> or engr:backlog:<id>
        #[arg(long = "on", value_name = "ENGR_REF")]
        on: String,
        /// Why the target matters here
        #[arg(long)]
        reason: Option<String>,
    },
    /// Drop a dependency
    Undepend {
        object: String,
        #[arg(long = "on", value_name = "ENGR_REF")]
        on: String,
    },
    /// Record a condition preventing useful progress
    Block {
        object: String,
        #[arg(long)]
        reason: Option<String>,
        /// engr:obj:<id> or engr:backlog:<id>
        #[arg(long, value_name = "ENGR_REF")]
        target: Option<String>,
    },
    /// Clear a blocker by its position
    Unblock {
        object: String,
        #[arg(long)]
        index: usize,
    },
    /// The steps of the current decomposition
    #[command(subcommand)]
    Item(WorkItem),
}

#[derive(Subcommand)]
enum WorkItem {
    /// Add a step
    Add {
        object: String,
        #[arg(long)]
        text: String,
    },
    /// Reword a step
    Revise {
        object: String,
        #[arg(long)]
        item: u64,
        #[arg(long)]
        text: String,
    },
    /// Move a step's progress
    State {
        object: String,
        #[arg(long)]
        item: u64,
        #[arg(long, value_enum)]
        state: ItemStateArg,
    },
    /// Record what a step produced. Omit --text to clear it
    Result {
        object: String,
        #[arg(long)]
        item: u64,
        #[arg(long)]
        text: Option<String>,
    },
    /// Point a step at a commit, as navigation rather than proof
    Commit {
        object: String,
        #[arg(long)]
        item: u64,
        #[arg(long, value_name = "REVISION")]
        commit: String,
    },
    /// Prune a step. Its id is not reused
    Rm {
        object: String,
        #[arg(long)]
        item: u64,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ItemStateArg {
    Pending,
    Active,
    Done,
}

impl From<ItemStateArg> for work::ItemState {
    fn from(value: ItemStateArg) -> Self {
        match value {
            ItemStateArg::Pending => work::ItemState::Pending,
            ItemStateArg::Active => work::ItemState::Active,
            ItemStateArg::Done => work::ItemState::Done,
        }
    }
}

/// A dependency or blocker target, held to what Work is allowed to point at.
///
/// Objects and Backlog items only. Not sections, files, symbols or another
/// sidecar: Work says what this execution is waiting on, and the finer the
/// target the more it reads like the authoritative dependency it must never
/// become.
fn work_target(root: &Path, field: &str, spec: &str) -> Result<String> {
    let relative = spec.strip_prefix("engr:").ok_or_else(|| {
        Error::new(
            EXIT_USAGE,
            format!("{field} {spec:?} must be an engr: reference"),
        )
    })?;
    work::check_target(field, relative).map_err(|error| malformed_argument(field, spec, error))?;
    // Resolved for existence as well as shape, because a target nobody can
    // follow is a note to the next agent that wastes their time. Backlog items
    // do get consumed, so this is checked when written and reported when read.
    let id = engr::reference::EngrRef::parse_embedded(relative)
        .and_then(|parsed| engr::reference::decode_uuid(parsed.id()))
        .map_err(|error| malformed_argument(field, spec, error))?
        .to_string();
    let outcome = if relative.starts_with("backlog:") {
        backlog::load(root, &id).map(|_| ())
    } else {
        ops::effective(root, &id).map(|_| ())
    };
    // Absence and unreadable authority stay apart: "does not exist" would send
    // someone to create what is already there and hide the real fault.
    match outcome {
        Ok(()) => Ok(relative.to_owned()),
        Err(error) if error.code == EXIT_NOT_FOUND => Err(Error::new(
            EXIT_NOT_FOUND,
            format!("{field} {spec:?} does not exist"),
        )),
        Err(error) => Err(Error::new(
            error.code,
            format!("{field} {spec:?} cannot be read: {}", error.message),
        )),
    }
}

fn work_command(root: &Path, command: Work) -> Result<()> {
    match command {
        Work::Start { object, summary } => {
            let id = resolve_object_argument(root, "object", &object)?;
            work::start(root, &id, summary.as_deref())?;
            print!(
                "{}",
                view::render_work_show(root, &id, &work::load(root, &id)?)
            );
        }
        Work::Ls => {
            let mut entries = Vec::new();
            for id in work::ids(root)? {
                entries.push((id.clone(), work::load(root, &id)?));
            }
            // As instants, not as strings: two valid RFC3339 values written in
            // different offsets do not compare correctly as text, and the most
            // recently touched sidecar is the whole point of this ordering.
            entries.sort_by_key(|(_, item)| std::cmp::Reverse(item.updated_at()));
            print!("{}", view::render_work_ls(root, &entries));
        }
        Work::Show { object, format } => {
            let id = resolve_object_argument(root, "object", &object)?;
            let item = work::load(root, &id)?;
            match format {
                Format::Text => print!("{}", view::render_work_show(root, &id, &item)),
                Format::Json => println!("{}", view::render_work_json(&id, &item)?),
            }
        }
        Work::Summary { object, text } => {
            let id = resolve_object_argument(root, "object", &object)?;
            let item = work::set_summary(root, &id, text.as_deref())?;
            print!("{}", view::render_work_show(root, &id, &item));
        }
        Work::Pause { object } => {
            let id = resolve_object_argument(root, "object", &object)?;
            let item = work::set_state(root, &id, work::State::Paused)?;
            print!("{}", view::render_work_show(root, &id, &item));
        }
        Work::Resume { object } => {
            let id = resolve_object_argument(root, "object", &object)?;
            let item = work::set_state(root, &id, work::State::Active)?;
            print!("{}", view::render_work_show(root, &id, &item));
        }
        Work::Rm { object } => {
            let id = resolve_object_argument(root, "object", &object)?;
            let removed = work::remove(root, &id)?;
            println!(
                "no execution memory for {}",
                shorten(&id, view::width(root))
            );
            // Reported, not refused. Whether a human directed this is not
            // something engr can know, so it carries the deletion out and says
            // what went with it — a stop signal disappearing in silence is the
            // part worth avoiding, while refusing would invent a lifecycle rule
            // #12 deliberately left as a rule for the agent to follow.
            if removed.was_paused {
                println!("that work was paused; a human's stop signal went with it");
            }
        }
        Work::Depend { object, on, reason } => {
            let id = resolve_object_argument(root, "object", &object)?;
            let target = work_target(root, "--on", &on)?;
            let item = work::add_dependency(root, &id, &target, reason.as_deref())?;
            print!("{}", view::render_work_show(root, &id, &item));
        }
        Work::Undepend { object, on } => {
            let id = resolve_object_argument(root, "object", &object)?;
            let target = on.strip_prefix("engr:").unwrap_or(&on).to_owned();
            let item = work::remove_dependency(root, &id, &target)?;
            print!("{}", view::render_work_show(root, &id, &item));
        }
        Work::Block {
            object,
            reason,
            target,
        } => {
            let id = resolve_object_argument(root, "object", &object)?;
            let target = target
                .map(|spec| work_target(root, "--target", &spec))
                .transpose()?;
            let item = work::add_blocker(root, &id, reason.as_deref(), target.as_deref())?;
            print!("{}", view::render_work_show(root, &id, &item));
        }
        Work::Unblock { object, index } => {
            let id = resolve_object_argument(root, "object", &object)?;
            let item = work::remove_blocker(root, &id, index)?;
            print!("{}", view::render_work_show(root, &id, &item));
        }
        Work::Item(command) => return work_item_command(root, command),
    }
    Ok(())
}

fn work_item_command(root: &Path, command: WorkItem) -> Result<()> {
    match command {
        WorkItem::Add { object, text } => {
            let id = resolve_object_argument(root, "object", &object)?;
            let item = work::add_item(root, &id, &text)?;
            println!("work item {item}");
            print!(
                "{}",
                view::render_work_show(root, &id, &work::load(root, &id)?)
            );
        }
        WorkItem::Revise { object, item, text } => {
            let id = resolve_object_argument(root, "object", &object)?;
            let work = work::set_item_text(root, &id, item, &text)?;
            print!("{}", view::render_work_show(root, &id, &work));
        }
        WorkItem::State {
            object,
            item,
            state,
        } => {
            let id = resolve_object_argument(root, "object", &object)?;
            let work = work::set_item_state(root, &id, item, state.into())?;
            print!("{}", view::render_work_show(root, &id, &work));
        }
        WorkItem::Result { object, item, text } => {
            let id = resolve_object_argument(root, "object", &object)?;
            let work = work::set_item_result(root, &id, item, text.as_deref())?;
            print!("{}", view::render_work_show(root, &id, &work));
        }
        WorkItem::Commit {
            object,
            item,
            commit,
        } => {
            let id = resolve_object_argument(root, "object", &object)?;
            // Resolved here so `HEAD` and short ids are accepted as input while
            // the sidecar stores the full object id — the same rule every other
            // commit in engr follows, even though this one anchors nothing.
            let resolved = git::resolve(root, &commit).ok_or_else(|| {
                Error::new(
                    EXIT_NOT_FOUND,
                    format!("--commit {commit:?} does not name a commit in this repository"),
                )
            })?;
            let work = work::add_item_commit(root, &id, item, &resolved)?;
            print!("{}", view::render_work_show(root, &id, &work));
        }
        WorkItem::Rm { object, item } => {
            let id = resolve_object_argument(root, "object", &object)?;
            let work = work::remove_item(root, &id, item)?;
            print!("{}", view::render_work_show(root, &id, &work));
        }
    }
    Ok(())
}

/// Planning metadata: what is grouped together, and in what order.
///
/// Its own namespace, like `backlog` and `work`, and for the same reason:
/// nothing here goes through the gate, so it must not be reachable by a command
/// that looks like one that does.
#[derive(Subcommand)]
enum CollectionCommand {
    /// Start a plan
    New {
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
        #[command(flatten)]
        schedule: ScheduleArgs,
    },
    /// List plans
    Ls,
    /// Show one plan, its schedule and its members
    Show {
        collection: String,
        #[arg(long, value_enum, default_value = "text")]
        format: Format,
    },
    /// Replace the name
    Rename {
        collection: String,
        #[arg(long)]
        name: String,
    },
    /// Replace the description. Omit --text to clear it
    Describe {
        collection: String,
        #[arg(long)]
        text: Option<String>,
    },
    /// Declare where the plan stands
    State {
        collection: String,
        #[arg(long, value_enum)]
        state: CollectionStateArg,
    },
    /// Replace the schedule. Give none of the dates to clear it
    Schedule {
        collection: String,
        #[command(flatten)]
        schedule: ScheduleArgs,
    },
    /// Put something in the plan
    Add {
        collection: String,
        /// engr:obj:<id> or engr:backlog:<id>
        #[arg(long, value_name = "ENGR_REF")]
        target: String,
        /// Intended sequencing. Omit to leave it unranked
        #[arg(long)]
        order: Option<i64>,
        #[arg(long, value_enum)]
        priority: Option<LevelArg>,
        /// Why it has that priority in this plan
        #[arg(long)]
        reason: Option<String>,
    },
    /// Take something out of the plan
    Rm {
        collection: String,
        #[arg(long, value_name = "ENGR_REF")]
        target: String,
    },
    /// Rank a member, or omit --order to unrank it
    Order {
        collection: String,
        #[arg(long, value_name = "ENGR_REF")]
        target: String,
        #[arg(long)]
        order: Option<i64>,
    },
    /// Set a member's priority, or omit --priority to clear it
    Priority {
        collection: String,
        #[arg(long, value_name = "ENGR_REF")]
        target: String,
        #[arg(long, value_enum)]
        priority: Option<LevelArg>,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Delete the whole plan. Only on explicit human direction
    Delete { collection: String },
}

#[derive(Args)]
struct ScheduleArgs {
    /// Expected beginning, as YYYY-MM-DD
    #[arg(long)]
    start: Option<String>,
    /// Expected end, as YYYY-MM-DD
    #[arg(long)]
    end: Option<String>,
    /// Desired achievement point, as YYYY-MM-DD
    #[arg(long)]
    target_date: Option<String>,
}

impl ScheduleArgs {
    /// `None` when the caller named no date at all, which is how a schedule is
    /// left off or cleared — a schedule that is present says something.
    fn build(&self) -> Option<collection::Schedule> {
        let schedule = collection::Schedule {
            start: self.start.clone(),
            end: self.end.clone(),
            target: self.target_date.clone(),
        };
        let empty = schedule.start.is_none() && schedule.end.is_none() && schedule.target.is_none();
        (!empty).then_some(schedule)
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum CollectionStateArg {
    Open,
    Completed,
    Cancelled,
}

impl From<CollectionStateArg> for collection::State {
    fn from(value: CollectionStateArg) -> Self {
        match value {
            CollectionStateArg::Open => collection::State::Open,
            CollectionStateArg::Completed => collection::State::Completed,
            CollectionStateArg::Cancelled => collection::State::Cancelled,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum LevelArg {
    Low,
    Normal,
    High,
}

impl From<LevelArg> for collection::Level {
    fn from(value: LevelArg) -> Self {
        match value {
            LevelArg::Low => collection::Level::Low,
            LevelArg::Normal => collection::Level::Normal,
            LevelArg::High => collection::Level::High,
        }
    }
}

/// A member target, held to what a plan is allowed to group.
/// Strip the `engr:` prefix a caller writes, leaving the embedded form the
/// domain stores.
///
/// Shape and existence are **not** checked here. They used to be, and that was
/// the bug: the command line enforced a membership rule the library did not, so
/// what a plan could contain depended on which door it came through. The rule
/// lives in `collection::add_member` now, and this only translates the spelling.
fn collection_target(spec: &str) -> Result<String> {
    spec.strip_prefix("engr:")
        .map(str::to_owned)
        .ok_or_else(|| {
            Error::new(
                EXIT_USAGE,
                format!("--target {spec:?} must be an engr: reference"),
            )
        })
}

fn priority_of(
    level: Option<LevelArg>,
    reason: Option<String>,
) -> Result<Option<collection::Priority>> {
    match (level, reason) {
        (Some(level), reason) => Ok(Some(collection::Priority {
            level: level.into(),
            reason,
        })),
        (None, None) => Ok(None),
        (None, Some(_)) => Err(Error::new(
            EXIT_USAGE,
            "--reason explains a priority, so it needs --priority".to_owned(),
        )),
    }
}

fn collection_command(root: &Path, command: CollectionCommand) -> Result<()> {
    match command {
        CollectionCommand::New {
            name,
            description,
            schedule,
        } => {
            let item = collection::create(root, &name, description.as_deref(), schedule.build())?;
            print!("{}", view::render_collection_show(root, &item));
        }
        CollectionCommand::Ls => {
            let mut found = Vec::new();
            for id in collection::ids(root)? {
                found.push(collection::load(root, &id)?);
            }
            // Open plans first, then by name: what is still being pursued is
            // what a listing is for, and nothing here records activity to sort
            // by instead.
            //
            // By an explicit rank, not by the state's spelling. Alphabetically
            // `cancelled` and `completed` both precede `open`, so sorting on the
            // name of the state puts every abandoned plan above every live one —
            // exactly backwards, and silently, because the code above says the
            // opposite and nothing checks.
            found.sort_by_key(|item| {
                (
                    match item.state {
                        collection::State::Open => 0,
                        collection::State::Completed => 1,
                        collection::State::Cancelled => 2,
                    },
                    item.name.to_lowercase(),
                )
            });
            print!("{}", view::render_collection_ls(root, &found));
        }
        CollectionCommand::Show { collection, format } => {
            let id = collection::resolve_id(root, &collection)?;
            let item = collection::load(root, &id)?;
            match format {
                Format::Text => print!("{}", view::render_collection_show(root, &item)),
                Format::Json => println!("{}", view::render_collection_json(&item)?),
            }
        }
        CollectionCommand::Rename { collection, name } => {
            let id = collection::resolve_id(root, &collection)?;
            let item = collection::rename(root, &id, &name)?;
            print!("{}", view::render_collection_show(root, &item));
        }
        CollectionCommand::Describe { collection, text } => {
            let id = collection::resolve_id(root, &collection)?;
            let item = collection::describe(root, &id, text.as_deref())?;
            print!("{}", view::render_collection_show(root, &item));
        }
        CollectionCommand::State { collection, state } => {
            let id = collection::resolve_id(root, &collection)?;
            let item = collection::set_state(root, &id, state.into())?;
            print!("{}", view::render_collection_show(root, &item));
        }
        CollectionCommand::Schedule {
            collection,
            schedule,
        } => {
            let id = collection::resolve_id(root, &collection)?;
            let item = collection::set_schedule(root, &id, schedule.build())?;
            print!("{}", view::render_collection_show(root, &item));
        }
        CollectionCommand::Add {
            collection,
            target,
            order,
            priority,
            reason,
        } => {
            let id = collection::resolve_id(root, &collection)?;
            let target = collection_target(&target)?;
            let priority = priority_of(priority, reason)?;
            let item = collection::add_member(root, &id, &target, order, priority)?;
            print!("{}", view::render_collection_show(root, &item));
        }
        CollectionCommand::Rm { collection, target } => {
            let id = collection::resolve_id(root, &collection)?;
            let target = target.strip_prefix("engr:").unwrap_or(&target).to_owned();
            let item = collection::remove_member(root, &id, &target)?;
            print!("{}", view::render_collection_show(root, &item));
        }
        CollectionCommand::Order {
            collection,
            target,
            order,
        } => {
            let id = collection::resolve_id(root, &collection)?;
            let target = target.strip_prefix("engr:").unwrap_or(&target).to_owned();
            let item = collection::set_order(root, &id, &target, order)?;
            print!("{}", view::render_collection_show(root, &item));
        }
        CollectionCommand::Priority {
            collection,
            target,
            priority,
            reason,
        } => {
            let id = collection::resolve_id(root, &collection)?;
            let target = target.strip_prefix("engr:").unwrap_or(&target).to_owned();
            let priority = priority_of(priority, reason)?;
            let item = collection::set_priority(root, &id, &target, priority)?;
            print!("{}", view::render_collection_show(root, &item));
        }
        CollectionCommand::Delete { collection } => {
            let id = collection::resolve_id(root, &collection)?;
            let removed = collection::remove(root, &id)?;
            // Carried out and reported, not refused. #10 makes this a rule for
            // the agent and says a technical guard can come later if real use
            // shows one is needed; engr cannot tell who asked, so it says what
            // was discarded rather than pretending it can.
            println!(
                "deleted collection {id} — {:?}, {} member(s) of planning context",
                removed.name, removed.members
            );
        }
    }
    Ok(())
}

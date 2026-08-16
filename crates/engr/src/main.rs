use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};
use engr::model::{self, Action, Payload, Ref};
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
    /// List objects. Only open ones unless --all
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
}

#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum Format {
    Text,
    Json,
}

#[derive(Args)]
#[command(group(
    ArgGroup::new("action")
        .required(true)
        .args(["new", "rename", "add", "revise", "merge", "delete", "close", "reopen"])
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
    /// Declare the object finished
    #[arg(long)]
    close: bool,
    /// Take a finished object back up
    #[arg(long)]
    reopen: bool,

    /// The object to act on. Any unique id prefix. Omit only with --new
    #[arg(long)]
    object: Option<String>,
    /// Wording, inline
    #[arg(long)]
    text: Option<String>,
    /// Wording, from a file
    #[arg(long)]
    text_file: Option<PathBuf>,
    /// Committed repository basis. Defaults to HEAD only when source is clean
    #[arg(long)]
    based_on: Option<String>,
    /// Record that this wording has no repository basis
    #[arg(long, conflicts_with = "based_on")]
    no_based_on: bool,
    /// A section this wording depends on, as OBJECT:SECTION
    #[arg(long = "ref", value_name = "OBJECT:SECTION")]
    references: Vec<String>,
    #[arg(long)]
    json: bool,
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
            engr::FORMAT_VERSION
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
            let (event, object) = store::with_lock(&root, || gate::confirm(&root, &response))?;
            println!(
                "CONFIRMED  {}  {}  rev {}",
                shorten(&object.id, view::width(&root)),
                event.payload.action.label(),
                object.rev
            );
            warn_uncommitted(&root, &object.id);
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
    } else {
        Action::ObjectReopened
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

    let mut references = Vec::new();
    for spec in &command.references {
        references.push(parse_ref(root, spec)?);
    }

    if action.carries_title() && (command.based_on.is_some() || command.no_based_on) {
        return Err(Error::new(
            EXIT_USAGE,
            "a title has no repository basis; use --based-on or --no-based-on only for section wording",
        ));
    }
    if !action.carries_content() && command.no_based_on {
        return Err(Error::new(
            EXIT_USAGE,
            "--no-based-on applies only to section wording",
        ));
    }

    let content = gate::content(
        root,
        text,
        command.based_on.clone(),
        command.no_based_on || action.carries_title(),
        references,
    )?;
    let payload = Payload {
        action,
        object,
        content,
    };
    let prepared = store::with_lock(root, || gate::prepare(root, payload))?;

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
        render_candidate(&prepared.candidate, view::width(root), &prepared.notes)
    );
    for code in &prepared.superseded {
        println!("(candidate {code} was superseded by this one)");
    }
    Ok(())
}

/// `OBJECT:SECTION` — the pinned hash and commit are read from the target now,
/// so the caller cannot pin something the target never said.
fn malformed_engr_reference(field: &str, spec: &str, error: Error) -> Error {
    if error.code == EXIT_SCHEMA {
        Error::new(EXIT_USAGE, format!("{field} {spec:?}: {}", error.message))
    } else {
        error
    }
}

/// The shared `engr:` parser treats malformed values as schema errors because
/// it is also used for stored authority. At the command line they are a person
/// supplying an invalid argument, so translate only at this boundary.
fn resolve_object_argument(root: &Path, field: &str, spec: &str) -> Result<String> {
    store::resolve_id(root, spec).map_err(|error| malformed_engr_reference(field, spec, error))
}

fn parse_ref(root: &Path, spec: &str) -> Result<Ref> {
    if spec.starts_with("engr:") {
        let reference = engr::reference::EngrRef::parse_standalone(spec)
            .map_err(|error| malformed_engr_reference("--ref", spec, error))?;
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
            .map_err(|error| malformed_engr_reference("--ref", spec, error))?;
        let id = engr::reference::decode_uuid(canonical.id())
            .map_err(|error| malformed_engr_reference("--ref", spec, error))?
            .to_string();
        let section = canonical
            .section()
            .expect("checked before canonicalization");
        let target = store::load_object(root, &id)?;
        let target_section = target.section(section)?;
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
    let target = store::load_object(root, &id)?;
    let target_section = target.section(section)?;
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

fn render_basis(basis: Option<&str>) -> String {
    basis
        .map(|commit| shorten(commit, 8).to_owned())
        .unwrap_or_else(|| "none (explicit)".to_owned())
}

fn render_candidate(candidate: &gate::Candidate, width: usize, notes: &[gate::Note]) -> String {
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
    if !candidate.payload.action.carries_title() && candidate.payload.action.carries_content() {
        if matches!(candidate.payload.action, Action::SectionRevised { .. }) {
            if !candidate.previous_semantics_recorded {
                out.push_str(
                    "WARNING    semantic revision metadata is unavailable; this legacy candidate cannot be confirmed\n",
                );
            }
            if candidate.previous_based_on != candidate.payload.content.based_on {
                out.push_str(&format!(
                    "Based on - {}\nBased on + {}\n",
                    render_basis(candidate.previous_based_on.as_deref()),
                    render_basis(candidate.payload.content.based_on.as_deref())
                ));
            } else {
                out.push_str(&format!(
                    "Based on   {}\n",
                    render_basis(candidate.payload.content.based_on.as_deref())
                ));
            }
            for reference in &candidate.previous_refs {
                if !candidate.payload.content.refs.contains(reference) {
                    out.push_str(&format!("Ref      - {}\n", render_ref(reference, width)));
                }
            }
            for reference in &candidate.payload.content.refs {
                if !candidate.previous_refs.contains(reference) {
                    out.push_str(&format!("Ref      + {}\n", render_ref(reference, width)));
                }
            }
        } else {
            out.push_str(&format!(
                "Based on   {}\n",
                render_basis(candidate.payload.content.based_on.as_deref())
            ));
            for reference in &candidate.payload.content.refs {
                out.push_str(&format!("Ref        {}\n", render_ref(reference, width)));
            }
        }
    }
    out.push('\n');
    // Show the change, not the whole section again: making a human re-read
    // everything is how confirmation decays into rubber-stamping.
    match (&candidate.previous_text, &candidate.payload.action) {
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
            print!(
                "{}",
                render_candidate(&candidate, view::width(root), &notes)
            );
            if !gate::is_live(root, &candidate) {
                println!(
                    "\nThis candidate is dead — the object moved after it was prepared. Prepare again."
                );
            }
            Ok(())
        }
        None => {
            let pending = gate::pending(root)?;
            if pending.is_empty() {
                println!("nothing is awaiting confirmation");
                return Ok(());
            }
            let width = view::width(root);
            for candidate in pending {
                println!(
                    "{}   {:<16} {} {:<8} {}",
                    candidate.challenge,
                    candidate.payload.action.label(),
                    shorten(&candidate.payload.object, width),
                    if gate::is_live(root, &candidate) {
                        "pending"
                    } else {
                        "stale"
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
        if all || object.state == engr::model::Status::Open {
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

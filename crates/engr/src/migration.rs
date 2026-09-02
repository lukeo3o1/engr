//! Migrating the one supported predecessor workspace.
//!
//! The predecessor is the officially released `latest` workspace and nothing
//! else. It is validated as a whole, converted deterministically, frozen into a
//! plan, and published only after a human confirms that exact plan through the
//! ordinary Challenge primitive — `subject.type = migration`.
//!
//! Three properties are the whole design:
//!
//! - **Effective state, not stored state.** The released build appended its
//!   Event before saving its Object, so a crash between the two leaves durable
//!   history the projection has not caught up with. The migration reads the
//!   predecessor's own reducer over its own history and requires the stored
//!   projection, where there is one, to be exactly what that history derives.
//! - **History is discarded, not translated.** The predecessor's Events are read
//!   to establish what each Object *is*, and are then dropped. Translating them
//!   into the new vocabulary would mint records nobody admitted, in a vocabulary
//!   that did not exist when they were admitted. Each migrated Object gets one
//!   `object.migrated.v1` bootstrap at revision 1, and the Object itself is at
//!   revision 1.
//! - **Atomic or nothing.** A durable staging marker makes the commit phase
//!   resumable, and `.engr/VERSION` is written last. A workspace half way
//!   between generations is never a steady state anyone can act on.

use crate::model::{
    Action, Event, EventAdmission, HumanConfirmation, Object, Payload, Section, SectionValue,
    Snapshot, SnapshotSection,
};
use crate::proof::{sha256_of, stored_within_safe_integers};
use crate::semantics::Admission;
use crate::store::{self, WorkspaceFormat};
use crate::{ensure, tool_error, Error, Result, EXIT_INVARIANT, EXIT_NOT_FOUND, EXIT_SCHEMA};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

const STAGE: &str = "migration";
const STAGE_TEMP: &str = "migration.tmp";
const MANIFEST: &str = "manifest.json";

/// The staging directory, under `local/` with every other resumable local file.
///
/// Public because `store::validate_format` names it in the one error a person
/// has to act on, and a path a diagnostic prints is a path a caller may need to
/// look at.
pub fn stage_dir(root: &Path) -> PathBuf {
    store::local_dir(root).join(STAGE)
}

fn stage_temp(root: &Path) -> PathBuf {
    store::local_dir(root).join(STAGE_TEMP)
}

/// One predecessor Section, in both spellings that matter.
///
/// A predecessor Ref pins the seal the *predecessor* took over the
/// *predecessor's* content, and the redesigned digest has to be taken over the
/// migrated content. Those are two different hashes of the same Section whenever
/// it carries references of its own, so the conversion needs both and must not
/// derive one from the other.
#[derive(Clone)]
struct HistoricalSection {
    /// The seal the predecessor recorded, proven against the predecessor content
    /// before anything was converted.
    legacy_seal: String,
    /// The same Section in the redesigned spelling.
    migrated: Section,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct HistoricalKey {
    commit: String,
    object: String,
    section: u64,
}

struct RefClosure<'a> {
    root: &'a Path,
    cache: BTreeMap<HistoricalKey, HistoricalSection>,
    visiting: BTreeSet<HistoricalKey>,
}

/// The semantic fields the predecessor's whole-content seal actually covered.
///
/// Three, because the predecessor Section *was* three semantic members. Its
/// seal is taken over `{based_on, refs, text}` — see
/// [`crate::predecessor::Section`], whose shape is the released schema — and a
/// whole-content reference therefore attested those and could not have attested
/// anything else.
///
/// Every later field is deliberately absent, and the reason is the same for all
/// of them. `admission`, `header`, `role`, `content` and `relations` did not
/// exist in the released contract, so a migrated Ref that selected them would
/// claim a dependency nobody ever declared — and would then report drift the
/// first time somebody legitimately gave the target a role, a supplement, a
/// heading or a relation. Migration converts a dependency; it does not widen
/// one.
const PREDECESSOR_REF_FIELDS: [crate::dependency::SemanticField; 3] = [
    crate::dependency::SemanticField::BasedOn,
    crate::dependency::SemanticField::Refs,
    crate::dependency::SemanticField::Text,
];

impl<'a> RefClosure<'a> {
    fn new(root: &'a Path) -> Self {
        Self {
            root,
            cache: BTreeMap::new(),
            visiting: BTreeSet::new(),
        }
    }

    /// Convert one predecessor Section into the redesigned shape.
    ///
    /// Every predecessor Section is Human-admitted, because while that
    /// generation was current the Human Gate was the only door there was, and
    /// the instant it recorded as `confirmed_at` becomes `admitted.at`. Nothing
    /// is invented: no `header`, no `role`, no `content`, no `relations`,
    /// because the predecessor had none of them.
    fn convert_section(&mut self, section: &crate::predecessor::Section) -> Result<Section> {
        let mut refs = Vec::with_capacity(section.refs.len());
        for reference in &section.refs {
            refs.push(self.convert_ref(reference)?);
        }
        crate::proof::canonical_set(&mut refs, "reference")?;
        let content = crate::model::Content {
            header: None,
            role: None,
            text: section.text.clone(),
            content: Vec::new(),
            based_on: crate::predecessor::based_on(section),
            refs,
            relations: Vec::new(),
        };
        let value = SectionValue::new(crate::predecessor::admitted(section), content);
        value.validate()?;
        Section::from_value(section.id, value)
    }

    /// Convert one predecessor Ref, attesting exactly what it attested.
    fn convert_ref(&mut self, reference: &crate::predecessor::Ref) -> Result<crate::model::Ref> {
        let key = HistoricalKey {
            commit: reference.commit.clone(),
            object: reference.object.clone(),
            section: reference.section,
        };
        let historical = self.historical_section(key)?;
        // Against the seal the predecessor took, not against a hash of the
        // migrated Section. They are the same number only when the target
        // carries no references of its own, because converting one rewrites
        // exactly the member both hashes cover.
        ensure!(
            historical.legacy_seal == reference.sha256,
            EXIT_INVARIANT,
            "{} §{} at {} seals as {}, not the predecessor reference seal {}",
            reference.object,
            reference.section,
            reference.commit,
            historical.legacy_seal,
            reference.sha256
        );
        let fields = crate::dependency::canonical_fields(&PREDECESSOR_REF_FIELDS)?;
        let target = crate::proof::section_target(&reference.object, reference.section)?;
        let snapshot = crate::dependency::ref_snapshot(
            target.clone(),
            &fields,
            &historical.migrated,
            reference.commit.clone(),
        )?;
        crate::dependency::SelectiveRef::stored(
            target,
            fields,
            reference.commit.clone(),
            snapshot.digest()?.to_string(),
        )
    }

    fn historical_section(&mut self, key: HistoricalKey) -> Result<HistoricalSection> {
        if let Some(section) = self.cache.get(&key) {
            return Ok(section.clone());
        }
        ensure!(
            self.visiting.insert(key.clone()),
            EXIT_SCHEMA,
            "predecessor reference closure cycles through {} §{} at {}",
            key.object,
            key.section,
            key.commit
        );
        let result = (|| {
            let object = match crate::git::object_at(self.root, &key.commit, &key.object)? {
                Some(crate::git::HistoricalObject::Predecessor(object)) => object,
                Some(crate::git::HistoricalObject::Current(_)) => {
                    return Err(Error::new(
                        EXIT_SCHEMA,
                        format!(
                            "{} at {} is already a migrated Object, so a predecessor reference cannot be converted against it",
                            key.object, key.commit
                        ),
                    ))
                }
                None => {
                    return Err(Error::new(
                        EXIT_NOT_FOUND,
                        format!(
                            "predecessor reference target {} is absent at {}",
                            crate::proof::section_target(&key.object, key.section)?,
                            key.commit
                        ),
                    ))
                }
            };
            let section = object.section(key.section)?.clone();
            // Proves the predecessor seal against the predecessor content, which
            // is the only moment both are in hand. Read after this and it is a
            // checked fact rather than a stored claim.
            section.check_seal()?;
            let legacy_seal = section.sha256.clone();
            Ok(HistoricalSection {
                legacy_seal,
                migrated: self.convert_section(&section)?,
            })
        })();
        self.visiting.remove(&key);
        let section = result?;
        self.cache.insert(key, section.clone());
        Ok(section)
    }
}

/// One predecessor Section from a commit, in the migrated spelling.
///
/// The read path needs this: a Ref written before the migration pins a
/// predecessor commit, and verifying it means projecting the target as this
/// build's projection understands it. Using the same conversion the migration
/// used is what makes a Ref recomputed after the migration agree with the one
/// recorded during it.
pub(crate) fn migrated_historical_section(
    root: &Path,
    commit: &str,
    object: &str,
    section: u64,
) -> Result<Section> {
    RefClosure::new(root)
        .historical_section(HistoricalKey {
            commit: commit.to_owned(),
            object: object.to_owned(),
            section,
        })
        .map(|historical| historical.migrated)
}

/// What the destination will hold for one Object, and what it was derived from.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct PlannedObject {
    pub object: String,
    pub title: String,
    /// How many Sections survive into the migrated Object. Presentation: a
    /// person confirming a migration is being told the size of what moves.
    pub sections: u64,
    /// The revision the predecessor stood at. Not the destination's, which is
    /// always 1 — this is the history being left behind.
    pub predecessor_rev: u64,
    /// The digest the migrated Object will carry.
    pub digest: String,
}

/// The frozen `subject.data` of a released-predecessor migration.
///
/// This predecessor/destination pair owns its own subject shape, because there
/// is no generic migration-effects schema and inventing one would freeze a
/// vocabulary for migrations nobody has designed. What it has to do is make the
/// question exact: which predecessor bytes, becoming which destination Objects.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct MigrationSubject {
    /// The predecessor bootstrap, in the spelling that release wrote.
    pub from: crate::predecessor::Format,
    /// The destination generation.
    pub to: u32,
    pub objects: Vec<PlannedObject>,
    /// Every predecessor file the plan was derived from, by its `.engr`-relative
    /// path, with the digest of the exact bytes that were read.
    ///
    /// This is what makes the confirmation about *these* bytes. A predecessor
    /// file edited between the question and the answer no longer matches, and
    /// the migration refuses rather than publishing a derivation of something
    /// nobody was shown.
    pub source: BTreeMap<String, String>,
}

/// The staged plan: which question was asked, and which code answers it.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    subject: MigrationSubject,
    challenge: String,
}

/// One read of a predecessor file, with its digest kept.
///
/// The capture and the validation see the same bytes because they are the same
/// bytes: the read happens once, and everything downstream works from the text
/// it returned.
fn capture(source: &mut BTreeMap<String, String>, root: &Path, path: &Path) -> Result<String> {
    let text = fs::read_to_string(path).map_err(|error| tool_error(path.display(), error))?;
    source.insert(
        relative_to_engr(root, path)?,
        crate::digest::OBJECT.emit(sha256_of(&text))?.to_string(),
    );
    Ok(text)
}

fn relative_to_engr(root: &Path, path: &Path) -> Result<String> {
    let engr = store::engr_dir(root);
    let relative = path.strip_prefix(&engr).map_err(|_| {
        Error::new(
            EXIT_SCHEMA,
            format!("{} is not inside {}", path.display(), engr.display()),
        )
    })?;
    Ok(relative
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/"))
}

/// The domains that arrived after the released generation.
///
/// That release wrote `format.json`, `objects/`, `events/` and `candidates/`,
/// and had no Rule, Backlog, Work or Collection subsystem at all — those landed
/// in later builds that were never published. So a declared predecessor
/// workspace holding one of them is not the workspace this migration is defined
/// over, and it is refused before anything is written.
///
/// `local/` is deliberately absent from the list. It is *this* generation's
/// directory, and taking the workspace lock creates it — so a predecessor
/// workspace acquires one the moment anything looks at it, including this
/// check. Refusing it would make the migration refuse every workspace it is
/// defined over.
///
/// `rules/` is the one that changes what the record can *do*: a rule file is
/// authored by a human and never by engr, so its presence says nothing about
/// which build made the workspace — and migrating it would make policy the
/// released build never recognized start governing agent admission because
/// somebody ran `engr migrate`. That is authority arriving through a
/// representation change.
const LATER_DOMAINS: &[&str] = &["rules", "backlog", "work", "collections", "eventstore"];

fn check_released_domains(root: &Path) -> Result<()> {
    for domain in LATER_DOMAINS {
        let path = store::engr_dir(root).join(domain);
        ensure!(
            !path.exists(),
            EXIT_SCHEMA,
            "{} holds {domain}/, which the released version {} workspace never had; this is a later unreleased build's workspace and engr {} defines no route from it",
            store::engr_dir(root).display(),
            crate::PREDECESSOR_WORKSPACE_VERSION,
            crate::IMPLEMENTATION_VERSION
        );
    }
    Ok(())
}

/// Every predecessor Object identity, from projections and history alike.
///
/// History as well as projections, because the released build wrote the Event
/// first: an Object whose file never landed is still an Object its own admitted
/// history establishes, and dropping it would silently lose a record.
fn predecessor_ids(root: &Path) -> Result<Vec<String>> {
    let mut ids = BTreeSet::new();
    for (dir, suffix) in [
        (store::objects_dir(root), ".json"),
        (predecessor_events_dir(root), ".jsonl"),
    ] {
        if !dir.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&dir).map_err(|error| tool_error(dir.display(), error))? {
            let entry = entry.map_err(|error| tool_error(dir.display(), error))?;
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(id) = name.strip_suffix(suffix) {
                ids.insert(id.to_owned());
            }
        }
    }
    Ok(ids.into_iter().collect())
}

fn predecessor_events_dir(root: &Path) -> PathBuf {
    store::engr_dir(root).join("events")
}

/// The migrated snapshot of one validated predecessor Object.
fn snapshot_of(
    predecessor: &crate::predecessor::Object,
    closure: &mut RefClosure,
) -> Result<Snapshot> {
    let mut sections = Vec::with_capacity(predecessor.sections.len());
    for section in &predecessor.sections {
        let converted = closure.convert_section(section)?;
        sections.push(SnapshotSection {
            id: converted.id,
            value: converted.value(),
        });
    }
    sections.sort_by_key(|section| section.id);
    Ok(Snapshot {
        // The released generation had no Object `type`, so a migrated Object is
        // untyped. Inventing one would be engr classifying a record nobody
        // classified.
        title: predecessor.title.clone(),
        object_type: None,
        state: predecessor.state,
        next_section_id: predecessor.next_section_id,
        sections,
    })
}

/// What one migrated Object becomes.
struct Derived {
    object: Object,
    event: Event,
}

/// Derive the destination Object by *replaying* its own bootstrap Event.
///
/// Not by assembling it beside one. The Object a migration publishes has to be
/// exactly what its history derives, and deriving it is the only way to be sure
/// — a second construction path would be a second answer that could differ.
///
/// `admitted` is the migration's own Human confirmation and instant, while the
/// Sections inside the snapshot keep the provenance the predecessor recorded.
/// That is the case #66 keeps `Section.admitted` and `Event.metadata.admitted`
/// apart for: here they legitimately name different people at different times.
fn derive(
    closure: &mut RefClosure,
    predecessor: &crate::predecessor::Object,
    admitted: EventAdmission,
) -> Result<Derived> {
    let action = Action::ObjectMigrated {
        snapshot: Box::new(snapshot_of(predecessor, closure)?),
    };
    Payload::new(predecessor.id.clone(), action.clone()).validate()?;
    let event = Event::sealed(&predecessor.id, crate::model::new_id(), action, 1, admitted)?;
    let mut object = empty(&predecessor.id)?;
    crate::model::project(&mut object, &event)?;
    object.validate()?;
    let value = serde_json::to_value(&object)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("object {}: {error}", predecessor.id)))?;
    stored_within_safe_integers(&value, &format!("object {}", predecessor.id))?;
    crate::integrity::check_stored_object_integrity(&object)?;
    Ok(Derived { object, event })
}

fn empty(id: &str) -> Result<Object> {
    let mut object = Object::new(id.to_owned(), String::new())?;
    object.rev = 0;
    object.reseal()?;
    Ok(object)
}

/// The admission provenance a plan uses while it is only a plan.
///
/// A fixed instant and a fixed code, so the Object digest a plan promises is a
/// function of the predecessor alone. The Object digest does not cover the Event
/// at all, which is what makes this safe: the real confirmation supplies the
/// real values and the Object still derives identically.
fn planning_admission() -> EventAdmission {
    EventAdmission {
        by: Admission::Human,
        at: "1970-01-01T00:00:00Z".to_owned(),
        confirmation: Some(HumanConfirmation {
            challenge: "AAAAAA".to_owned(),
        }),
        review: None,
    }
}

/// The preflight: read, validate and convert the whole predecessor.
///
/// Nothing is written by this. It answers what the destination would be, and the
/// plan derived from it is what a human is then asked about.
fn preflight(root: &Path) -> Result<(MigrationSubject, Vec<crate::predecessor::Object>)> {
    check_released_domains(root)?;
    let mut source = BTreeMap::new();
    let bootstrap_path = store::engr_dir(root).join("format.json");
    let bootstrap_text = capture(&mut source, root, &bootstrap_path)?;
    let format: crate::predecessor::Format =
        serde_json::from_str(&bootstrap_text).map_err(|error| {
            Error::new(
                EXIT_SCHEMA,
                format!("{}: {error}", bootstrap_path.display()),
            )
        })?;
    ensure!(
        format.format == crate::predecessor::WORKSPACE_FORMAT
            && format.version == crate::PREDECESSOR_WORKSPACE_VERSION,
        EXIT_SCHEMA,
        "{} is not the released predecessor bootstrap",
        bootstrap_path.display()
    );

    let mut predecessors = Vec::new();
    let mut planned = Vec::new();
    let mut closure = RefClosure::new(root);
    for id in predecessor_ids(root)? {
        crate::model::validate_object_id(&id)?;
        let object_path = store::object_path(root, &id);
        let events_path = predecessor_events_dir(root).join(format!("{id}.jsonl"));
        let stored = if object_path.exists() {
            Some(capture(&mut source, root, &object_path)?)
        } else {
            None
        };
        // A missing history file is a shape the release wrote: its own
        // `load_events` returned an empty list for one, so an Object whose
        // history was pruned away entirely is still a workspace it called
        // sound. What must exist is one of the two, and `effective_state`
        // decides which of them is authority.
        let history = if events_path.exists() {
            capture(&mut source, root, &events_path)?
        } else {
            ensure!(
                stored.is_some(),
                EXIT_SCHEMA,
                "{id}: a predecessor Object has neither a projection nor any admitted history"
            );
            String::new()
        };
        let predecessor = crate::predecessor::effective_state(
            &object_path,
            &events_path,
            &id,
            stored.as_deref(),
            &history,
        )?;
        let derived = derive(&mut closure, &predecessor, planning_admission())?;
        planned.push(PlannedObject {
            object: id.clone(),
            title: derived.object.title.clone(),
            sections: derived.object.sections.len() as u64,
            predecessor_rev: predecessor.rev,
            digest: derived.object.digest.clone(),
        });
        predecessors.push(predecessor);
    }
    Ok((
        MigrationSubject {
            from: format,
            to: crate::WORKSPACE_GENERATION,
            objects: planned,
            source,
        },
        predecessors,
    ))
}

/// What `engr migrate` produced.
#[derive(Debug)]
pub struct Proposed {
    pub challenge: String,
    pub subject: MigrationSubject,
    /// Whether this run re-rendered an existing staged plan rather than deriving
    /// a new one.
    pub resumed: bool,
}

/// Prepare the migration and mint the Challenge that admits it.
pub fn prepare(root: &Path) -> Result<Proposed> {
    store::with_lock(root, || prepare_locked(root))
}

fn prepare_locked(root: &Path) -> Result<Proposed> {
    // A completed transaction that did not finish sweeping up.
    //
    // `VERSION` is written after the last destination byte, so once it exists
    // there is no migration left to resume — only residue from a crash between
    // spending the Challenge and removing the stage. Reading the stage as an
    // unfinished transaction here is what made that window unrecoverable: the
    // resume branch below demands the Challenge the same `finish` had already
    // removed, and answers with an instruction to delete files by hand.
    if store::version_path(root).exists() {
        sweep_completed_stage(root)?;
    }
    if let Some(manifest) = staged(root)? {
        // A staged plan is the question a human is already holding, so resuming
        // returns their code rather than minting a second one for the same
        // migration. That is true only while the question is still one this
        // build can put to them.
        //
        // Two ways it stops being: the file is gone, or it is there and this
        // build cannot interpret it — a Challenge minted by a generator whose
        // contract has since changed, which `Challenge::validate` refuses by
        // design and tells the reader to prepare again. Returning that code
        // anyway made "prepare again" impossible to reach: confirming it failed
        // the fingerprint check, and so did withdrawing it, because disposal has
        // to load the file to learn whose it is. The only way out was deleting
        // local files by hand.
        //
        // Both cases converge the same way, and the destination is what makes it
        // safe. No destination means nobody ever answered exactly, so nothing
        // was published and the local question and plan are simply retired and
        // asked again. With one, somebody did answer and publication may have
        // begun: that is forward-only, and it says so.
        let unusable = match store::load_challenge(root, &manifest.challenge) {
            Ok(_) => None,
            // Absent, or present and not interpretable by this build.
            Err(error) if error.code == EXIT_NOT_FOUND || error.code == EXIT_SCHEMA => {
                Some(error.message)
            }
            // Anything else is the filesystem failing, not an answer about the
            // question, and retiring a plan on a transient read would throw away
            // the one thing that can finish the transaction.
            Err(error) => return Err(error),
        };
        match unusable {
            None => {
                return Ok(Proposed {
                    challenge: manifest.challenge,
                    subject: manifest.subject,
                    resumed: true,
                })
            }
            Some(why) => {
                ensure!(
                    !destination_dir(root).exists(),
                    EXIT_INVARIANT,
                    "the staged migration was confirmed as challenge {} and is part-published, and that question is no longer usable ({why}); {} holds the only copy of what was confirmed, so it can only be finished",
                    manifest.challenge,
                    stage_dir(root).display()
                );
                retire_prepared(root, &manifest.challenge)?;
            }
        }
    }
    ensure!(
        matches!(
            store::validate_format(root),
            Ok(WorkspaceFormat::Predecessor)
        ),
        EXIT_SCHEMA,
        "this workspace is not the released predecessor, so there is nothing to migrate"
    );
    let (subject, _) = preflight(root)?;
    // Before anything local is written. The stage and the minted Challenge both
    // live under `local/`, and the predecessor's own `.gitignore` names neither —
    // so a person who prepares a migration and then commits would otherwise put a
    // live challenge code into the repository. Done through git's local exclude,
    // so asking the question changes no tracked byte; the tracked line is part of
    // the publication a human confirms.
    exclude_local_from_git(root)?;
    let data = serde_json::to_value(&subject)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("migration subject: {error}")))?;
    let taken = crate::gate::taken_codes(root)?;
    let challenge = crate::confirmation::Challenge::mint(
        crate::confirmation::Subject {
            kind: crate::confirmation::SubjectType::Migration,
            data,
        },
        &taken,
        now(),
    )?;
    stage(
        root,
        &Manifest {
            subject: subject.clone(),
            challenge: challenge.id.clone(),
        },
    )?;
    store::write_json(&store::challenge_path(root, &challenge.id)?, &challenge)?;
    Ok(Proposed {
        challenge: challenge.id,
        subject,
        resumed: false,
    })
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("formatting a timestamp cannot fail")
}

/// The code a staged plan is waiting on, if there is one.
///
/// Read straight off the manifest and held to nothing: this answers "which code
/// is spoken for", and a plan whose Challenge this build cannot interpret speaks
/// for its code exactly as much as one it can.
pub(crate) fn staged_code(root: &Path) -> Result<Option<String>> {
    Ok(staged(root)?.map(|manifest| manifest.challenge))
}

fn staged(root: &Path) -> Result<Option<Manifest>> {
    let path = stage_dir(root).join(MANIFEST);
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display()))),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(tool_error(path.display(), error)),
    }
}

/// Install the plan atomically, so a crash leaves either no stage or a complete
/// one — never a half-written question.
fn stage(root: &Path, manifest: &Manifest) -> Result<()> {
    let temp = stage_temp(root);
    let final_dir = stage_dir(root);
    if temp.exists() {
        fs::remove_dir_all(&temp).map_err(|error| tool_error(temp.display(), error))?;
    }
    fs::create_dir_all(&temp).map_err(|error| tool_error(temp.display(), error))?;
    store::write_json(&temp.join(MANIFEST), manifest)?;
    fs::rename(&temp, &final_dir).map_err(|error| tool_error(final_dir.display(), error))
}

/// What a confirmed migration did.
#[derive(Debug)]
pub struct Report {
    pub objects: Vec<String>,
    pub sections: usize,
}

/// Apply the migration a human has just confirmed.
///
/// Every derivation is redone here rather than read out of the stage. The stage
/// says which question was asked; it is not evidence about the answer, and a
/// commit phase that trusted it would publish whatever a staged file happened to
/// contain.
pub(crate) fn apply(root: &Path, challenge: &crate::confirmation::Challenge) -> Result<Report> {
    let manifest = staged(root)?.ok_or_else(|| {
        Error::new(
            EXIT_INVARIANT,
            format!(
                "challenge {} names a migration, and no migration is staged; run `engr migrate`",
                challenge.id
            ),
        )
    })?;
    ensure!(
        manifest.challenge == challenge.id,
        EXIT_INVARIANT,
        "the staged migration is waiting on challenge {}, not {}",
        manifest.challenge,
        challenge.id
    );
    let subject: MigrationSubject = serde_json::from_value(challenge.subject.data.clone())
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("migration subject: {error}")))?;
    ensure!(
        subject == manifest.subject,
        EXIT_INVARIANT,
        "the confirmed migration is not the one that was staged"
    );

    // A destination staged by an earlier run is what finishes the transaction.
    // Publication overwrites the predecessor Object files in place, so once it
    // has begun there is no predecessor left to re-derive from — and there does
    // not need to be, because every staged byte is checked back against the
    // digest the confirmed subject pins before any of it is published.
    if let Some(ready) = confirmed_destination(root, &challenge.id, &subject)? {
        let sections = subject
            .objects
            .iter()
            .map(|one| one.sections as usize)
            .sum();
        return finish(root, challenge, ready, sections);
    }

    // Re-derive from the predecessor as it stands now, and require it to be the
    // predecessor the human was shown. A file edited between the question and
    // the answer changes what the answer would mean.
    let (current, predecessors) = preflight(root)?;
    ensure!(
        current == subject,
        EXIT_INVARIANT,
        "the predecessor workspace moved after challenge {} was prepared; prepare it again",
        challenge.id
    );

    // One instant for the whole migration, read once here. It is the moment the
    // migration was admitted, not the moment it was proposed: a Challenge can
    // sit for days, and #66 has Event metadata record the confirmation. A clock
    // read per Object would stamp one confirmation with as many admission times
    // as it happened to take.
    let admitted = EventAdmission {
        by: Admission::Human,
        at: now(),
        confirmation: Some(HumanConfirmation {
            challenge: challenge.id.clone(),
        }),
        review: None,
    };
    let mut closure = RefClosure::new(root);
    let mut derived = Vec::new();
    let mut sections = 0usize;
    for predecessor in &predecessors {
        let one = derive(&mut closure, predecessor, admitted.clone())?;
        let planned = subject
            .objects
            .iter()
            .find(|planned| planned.object == one.object.id)
            .ok_or_else(|| {
                Error::new(
                    EXIT_INVARIANT,
                    format!("{} is not in the confirmed migration plan", one.object.id),
                )
            })?;
        ensure!(
            planned.digest == one.object.digest,
            EXIT_INVARIANT,
            "{} does not migrate to the value that was confirmed",
            one.object.id
        );
        sections += one.object.sections.len();
        derived.push(one);
    }
    stage_destination(root, &challenge.id, &derived)?;
    let ready = confirmed_destination(root, &challenge.id, &subject)?.ok_or_else(|| {
        Error::new(
            EXIT_INVARIANT,
            "the destination was staged and cannot be read back".to_owned(),
        )
    })?;
    stop_for_test(Stage::AfterDestination, &challenge.id)?;
    finish(root, challenge, ready, sections)
}

/// The points in the confirmed transaction a crash can land between.
///
/// Each one is a real boundary the publication crosses, named so a test can ask
/// for it. They exist because the properties that matter here — converges, does
/// not duplicate, does not lie about when it was admitted — are properties *of
/// the windows between writes*, and a window nothing can stop inside is a window
/// nothing checks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Destination staged and verified; nothing published.
    AfterDestination,
    /// Every destination byte published; `VERSION` not yet written.
    BeforeVersion,
    /// `VERSION` written; the spent Challenge and the stage still on disk.
    BeforeChallenge,
}

impl Stage {
    fn as_str(self) -> &'static str {
        match self {
            Stage::AfterDestination => "destination",
            Stage::BeforeVersion => "version",
            Stage::BeforeChallenge => "challenge",
        }
    }
}

/// Stop at a named boundary, for tests that need the real one.
///
/// The variable carries `<stage>:<challenge>`, so an inherited or stale value
/// cannot stop a migration it was not aimed at. Release builds contain no
/// failure hook at all.
#[cfg(debug_assertions)]
fn stop_for_test(stage: Stage, challenge: &str) -> Result<()> {
    let requested = format!("{}:{challenge}", stage.as_str());
    if std::env::var_os("ENGR_TEST_STOP_MIGRATION")
        .is_some_and(|value| value == std::ffi::OsStr::new(&requested))
    {
        return Err(Error::new(
            EXIT_INVARIANT,
            format!("test interruption at migration stage {}", stage.as_str()),
        ));
    }
    Ok(())
}

#[cfg(not(debug_assertions))]
fn stop_for_test(_stage: Stage, _challenge: &str) -> Result<()> {
    Ok(())
}
/// Everything from the staged destination onward, which is also the whole of
/// what resuming has to do.
fn finish(
    root: &Path,
    challenge: &crate::confirmation::Challenge,
    ready: Vec<(String, String, String)>,
    sections: usize,
) -> Result<Report> {
    publish(root, &ready)?;
    let ids = ready.iter().map(|(id, _, _)| id.clone()).collect();
    stop_for_test(Stage::BeforeVersion, &challenge.id)?;
    // Last of the authoritative writes, and only after everything else landed.
    // While `VERSION` is absent the transaction is unfinished and the staged
    // destination is what finishes it; the moment it exists, every read surface
    // is looking at the new generation and nothing left under `local/` can make
    // it not so.
    store::write_generation(root)?;
    stop_for_test(Stage::BeforeChallenge, &challenge.id)?;
    sweep_completed_stage(root)?;
    Ok(Report {
        objects: ids,
        sections,
    })
}

/// Withdraw a prepared migration a human declined to assent to.
///
/// The Challenge and the local plan go together, because neither means anything
/// without the other: a plan whose code is gone can never be applied, and a code
/// whose plan is gone names nothing. Removing only the file — which is all the
/// Object family's disposal knows how to do — would leave `engr migrate` stuck
/// on a manifest naming a Challenge that no longer exists.
///
/// Nothing tracked is touched. Both live under `local/`, which is the whole
/// point of preparing there.
pub(crate) fn discard_locked(root: &Path, code: &str) -> Result<()> {
    let path = store::challenge_path(root, code)?;
    ensure!(
        path.exists(),
        EXIT_NOT_FOUND,
        "no challenge awaiting {code}"
    );
    // A destination exists only after somebody answered exactly. Past that
    // point the transaction is under way and publication may already have
    // begun, so withdrawing is no longer a thing anybody can do — the only way
    // out is forward.
    ensure!(
        !destination_dir(root).exists(),
        EXIT_INVARIANT,
        "migration {code} was already confirmed and is part-published; finish it with `engr confirm CONFIRM {code}` rather than withdrawing it"
    );
    retire_prepared(root, code)
}

/// Take a prepared migration out of existence: the question, then the plan.
///
/// Only ever called where nothing has been published — a withdrawal before any
/// answer, or a preparation this build can no longer put to anybody. The order
/// matters. The code goes first, so a crash between the two leaves a plan whose
/// Challenge is gone, which `prepare` recognises and finishes; the other way
/// round would leave a live-looking code with nothing behind it, and preparing
/// again would mint a second one beside it.
pub(crate) fn retire_prepared(root: &Path, code: &str) -> Result<()> {
    let path = store::challenge_path(root, code)?;
    if path.exists() {
        fs::remove_file(&path).map_err(|error| tool_error(path.display(), error))?;
    }
    if staged(root)?.is_some_and(|manifest| manifest.challenge == code) {
        let stage = stage_dir(root);
        if stage.exists() {
            fs::remove_dir_all(&stage).map_err(|error| tool_error(stage.display(), error))?;
        }
    }
    Ok(())
}

/// Dispose of what a completed migration leaves behind.
///
/// Called at the end of `finish` and again by `prepare` whenever `VERSION`
/// already exists, because those are the two moments the answer is knowable and
/// the writer lock is held. Every step is conditional and order-independent, so
/// a crash part way through leaves work this can simply do again.
///
/// The spent Challenge goes first. It is the one piece of residue that still
/// looks actionable — a code sitting in `challenges/` reads as a question
/// somebody could answer — and the stage is what tells a later sweep which code
/// that was.
///
/// **The file at that code is identified before it is removed**, because the
/// code alone is not an identity. Six characters are unique among *live*
/// questions, not for all time: a crash between the two removals below leaves
/// the plan naming a code with no file behind it, and the workspace is already
/// current, so ordinary Human questions can be prepared again. Deleting by name
/// would eventually take an unrelated one. `taken_codes` keeps the code
/// reserved while the residue stands, and this proves the file is the same
/// question the plan was waiting on before removing it — two answers to the
/// same hazard, because the sweep runs on a workspace somebody is already using.
fn sweep_completed_stage(root: &Path) -> Result<()> {
    if let Some(manifest) = staged(root)? {
        let spent = store::challenge_path(root, &manifest.challenge)?;
        if spent.exists() && is_this_migration(root, &manifest)? {
            fs::remove_file(&spent).map_err(|error| tool_error(spent.display(), error))?;
        }
    }
    let stage = stage_dir(root);
    if stage.exists() {
        fs::remove_dir_all(&stage).map_err(|error| tool_error(stage.display(), error))?;
    }
    Ok(())
}

/// Whether the Challenge at the plan's code is still that plan's own question.
///
/// Family and subject together, not the code. A question this build cannot even
/// read is left alone: it cannot be shown to be ours, and leaving residue is
/// always safer than removing something that is not.
fn is_this_migration(root: &Path, manifest: &Manifest) -> Result<bool> {
    let Ok(challenge) = store::load_challenge(root, &manifest.challenge) else {
        return Ok(false);
    };
    if challenge.subject.kind != crate::confirmation::SubjectType::Migration {
        return Ok(false);
    }
    let subject: MigrationSubject = match serde_json::from_value(challenge.subject.data) {
        Ok(subject) => subject,
        Err(_) => return Ok(false),
    };
    Ok(subject == manifest.subject)
}

/// The destination workspace, written where the predecessor cannot be harmed by
/// it, so a crash mid-publication has something to finish from.
///
/// Every path here is under `local/`, which is neither generation's authority.
const DESTINATION: &str = "destination";
const DESTINATION_MANIFEST: &str = "destination.json";

fn destination_dir(root: &Path) -> PathBuf {
    stage_dir(root).join(DESTINATION)
}

/// One derived Object and its bootstrap Event, as bytes with their digests.
///
/// The digests are what make resuming safe. Re-deriving from the predecessor is
/// impossible once publication has begun — the predecessor Object files are the
/// ones being overwritten — so recovery finishes forward from these bytes
/// instead, and it may only do that if it can still prove they are the bytes the
/// human confirmed.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
struct DestinationFile {
    object: String,
    /// The canonical Object bytes, and the digest the confirmed subject pins.
    object_digest: String,
    /// The bootstrap Event's own seal. Not in the confirmed subject — it covers
    /// a fresh Event id and the admission instant, neither of which existed when
    /// the question was asked — so the transaction pins it here instead, written
    /// once and never re-minted.
    event_digest: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
struct Destination {
    challenge: String,
    files: Vec<DestinationFile>,
}

/// Write the whole destination beside the predecessor, atomically.
///
/// Nothing canonical is touched until this returns. A crash before it leaves a
/// predecessor workspace with a stage that names no destination, which
/// `prepare` re-derives from scratch; a crash after it leaves one that can be
/// finished without reading the predecessor at all.
fn stage_destination(root: &Path, challenge: &str, derived: &[Derived]) -> Result<()> {
    let dir = destination_dir(root);
    let temp = stage_dir(root).join("destination.tmp");
    if temp.exists() {
        fs::remove_dir_all(&temp).map_err(|error| tool_error(temp.display(), error))?;
    }
    fs::create_dir_all(temp.join("objects")).map_err(|error| tool_error(temp.display(), error))?;
    fs::create_dir_all(temp.join("eventstore"))
        .map_err(|error| tool_error(temp.display(), error))?;

    let mut files = Vec::new();
    for one in derived {
        let value = serde_json::to_value(&one.object)
            .map_err(|error| Error::new(EXIT_SCHEMA, format!("object: {error}")))?;
        let object = crate::proof::canonical_bytes(&value, "object")?;
        let event = crate::proof::canonical_bytes(&one.event, "Event")?;
        store::write_text(
            &temp.join("objects").join(format!("{}.json", one.object.id)),
            &object,
        )?;
        store::write_text(
            &temp
                .join("eventstore")
                .join(format!("{}.jsonl", one.object.id)),
            &format!("{event}\n"),
        )?;
        files.push(DestinationFile {
            object: one.object.id.clone(),
            object_digest: one.object.digest.clone(),
            event_digest: one.event.digest.clone(),
        });
    }
    store::write_json(
        &temp.join(DESTINATION_MANIFEST),
        &Destination {
            challenge: challenge.to_owned(),
            files,
        },
    )?;
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|error| tool_error(dir.display(), error))?;
    }
    fs::rename(&temp, &dir).map_err(|error| tool_error(dir.display(), error))
}

/// Read back a staged destination, holding it to what the human confirmed.
///
/// The bytes are re-decoded rather than trusted: each Object must still be the
/// digest the confirmed subject named, and each Event must still be the seal the
/// transaction recorded. That is the same claim re-deriving would establish, so
/// finishing forward from here is not the commit phase trusting whatever a
/// staged file happened to contain.
fn confirmed_destination(
    root: &Path,
    challenge: &str,
    subject: &MigrationSubject,
) -> Result<Option<Vec<(String, String, String)>>> {
    let dir = destination_dir(root);
    let manifest_path = dir.join(DESTINATION_MANIFEST);
    let text = match fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(tool_error(manifest_path.display(), error)),
    };
    let destination: Destination = serde_json::from_str(&text).map_err(|error| {
        Error::new(EXIT_SCHEMA, format!("{}: {error}", manifest_path.display()))
    })?;
    ensure!(
        destination.challenge == challenge,
        EXIT_INVARIANT,
        "the staged destination was written for challenge {}, not {challenge}",
        destination.challenge
    );
    // The exact set, not the count and a lookup each way.
    //
    // Counting entries and then finding a plan for each one is satisfied by a
    // manifest that names one Object twice and another not at all: every entry
    // finds its plan, the lengths match, and `ready` comes out holding the
    // duplicate while the missing Object is never published. `VERSION` would
    // then be written over a workspace where that Object's predecessor bytes
    // still sit at the canonical current path with no stream behind them.
    let staged: BTreeSet<&str> = destination
        .files
        .iter()
        .map(|file| file.object.as_str())
        .collect();
    ensure!(
        staged.len() == destination.files.len(),
        EXIT_INVARIANT,
        "the staged destination names the same Object more than once"
    );
    let planned: BTreeSet<&str> = subject
        .objects
        .iter()
        .map(|object| object.object.as_str())
        .collect();
    ensure!(
        planned.len() == subject.objects.len(),
        EXIT_INVARIANT,
        "the confirmed migration plan names the same Object more than once"
    );
    ensure!(
        staged == planned,
        EXIT_INVARIANT,
        "the staged destination is not the set of Objects the confirmed plan names"
    );
    let mut ready = Vec::new();
    // One confirmation, one instant. Every Event this migration produced was
    // stamped from a single clock read, so a destination whose Events disagree
    // about when they were admitted is not a destination this migration staged.
    let mut migration_instant: Option<String> = None;
    for file in &destination.files {
        let planned = subject
            .objects
            .iter()
            .find(|planned| planned.object == file.object)
            .ok_or_else(|| {
                Error::new(
                    EXIT_INVARIANT,
                    format!("{} is not in the confirmed migration plan", file.object),
                )
            })?;
        ensure!(
            planned.digest == file.object_digest,
            EXIT_INVARIANT,
            "{} was staged as a value the confirmed plan does not name",
            file.object
        );
        let object_path = dir.join("objects").join(format!("{}.json", file.object));
        let event_path = dir
            .join("eventstore")
            .join(format!("{}.jsonl", file.object));
        let object_text = read_staged(&object_path)?;
        let event_text = read_staged(&event_path)?;

        // Read through the ordinary current-generation readers, on the exact
        // bytes publication will write.
        //
        // Publication copies these strings verbatim, so anything this accepts is
        // something the workspace will hold — and `VERSION` goes down after it,
        // declaring the result current. A check that only proved the bytes
        // *parse into* the right value would let a semantically identical
        // rewrite through: different member order, different whitespace, an
        // explicit null where the writer omits the member. All of those satisfy
        // the digests, because the digests are taken over the value. None of
        // them satisfy the read path, which requires the canonical JCS bytes —
        // so the transaction would activate a workspace unable to read its own
        // migrated resources.
        let object = store::decode_object_text(&object_path, &file.object, &object_text)?;
        ensure!(
            object.digest == file.object_digest
                && object.recomputed_digest()? == file.object_digest,
            EXIT_INVARIANT,
            "the staged Object for {} is not the value it was staged as",
            file.object
        );
        let events = store::decode_events(&event_path, &file.object, &event_text)?;
        let [event] = events.as_slice() else {
            return Err(Error::new(
                EXIT_INVARIANT,
                format!(
                    "the staged stream for {} holds {} Events; a migrated Object bootstraps with exactly one",
                    file.object,
                    events.len()
                ),
            ));
        };
        ensure!(
            event.digest == file.event_digest,
            EXIT_INVARIANT,
            "the staged bootstrap Event for {} is not the record it was staged as",
            file.object
        );

        // And the Event has to be *this* migration's bootstrap, deriving *this*
        // Object.
        //
        // Everything above it is satisfiable by a different record. The Object
        // is pinned to the confirmed plan, but the Event was pinned only to its
        // own seal and to `event_digest` — which lives in the same local
        // manifest an interrupted transaction leaves lying around, and is no
        // part of what a human confirmed. A canonical, self-sealed rev-1 Event
        // for the same Object, with the local digest updated to match, passed
        // every check; `decode_events` would even accept an `object.created.v1`
        // as a legitimate bootstrap. The permanent revision-1 history could
        // therefore stop reproducing the Object it belongs to, under a `VERSION`
        // saying the workspace is current.
        //
        // So it is re-derived rather than trusted: project the staged Event from
        // nothing, exactly as `derive` did when the plan was built, and require
        // the result to be the Object the confirmed plan pins. That binds the
        // Event to the human's answer through the Object, which is the only
        // thing in this transaction the human actually saw.
        ensure!(
            matches!(event.action, Action::ObjectMigrated { .. }),
            EXIT_INVARIANT,
            "revision 1 of a migrated Object is its object.migrated.v1 bootstrap, and {} was staged with {}",
            file.object,
            event.action.event_type()
        );
        // Admission metadata is outside replay, so the check above cannot reach
        // it: `project` derives Object state, and neither the review member nor
        // the instant is Object state. Left unstated, a staged Event could be
        // given a structurally valid Rule Review — which no migration has, since
        // there are no Object Rules to review a generation change against — or
        // its own admission time, then re-sealed with the local manifest updated
        // to match. It would replay to the same Object and publish provenance
        // describing something that never happened.
        //
        // The shape is exact, so it is stated exactly: Human, this
        // confirmation, no review, and one instant across the whole
        // destination.
        let admitted = &event.metadata.admitted;
        ensure!(
            admitted.by == Admission::Human
                && admitted
                    .confirmation
                    .as_ref()
                    .is_some_and(|confirmation| confirmation.challenge == challenge),
            EXIT_INVARIANT,
            "the staged bootstrap Event for {} does not record this migration's confirmation",
            file.object
        );
        ensure!(
            admitted.review.is_none(),
            EXIT_INVARIANT,
            "the staged bootstrap Event for {} records a Rule Review, and a migration is not reviewed against Object Rules",
            file.object
        );
        match &migration_instant {
            None => migration_instant = Some(admitted.at.clone()),
            Some(instant) => ensure!(
                &admitted.at == instant,
                EXIT_INVARIANT,
                "the staged bootstrap Event for {} was admitted at {}, and this migration was confirmed once, at {instant}",
                file.object,
                admitted.at
            ),
        }
        let mut projected = empty(&file.object)?;
        crate::model::project(&mut projected, event)?;
        projected.validate()?;
        ensure!(
            projected == object,
            EXIT_INVARIANT,
            "the staged bootstrap Event for {} does not replay to the Object the confirmed plan names",
            file.object
        );
        ready.push((file.object.clone(), object_text, event_text));
    }
    Ok(Some(ready))
}

fn read_staged(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|error| tool_error(path.display(), error))
}

/// Copy the staged destination into place, then remove what the predecessor
/// owned.
///
/// Idempotent by construction: every write is the same bytes the stage holds, so
/// re-running after a crash at any point converges rather than compounding. The
/// predecessor's own directories go last, because until they are gone the
/// workspace still reads as the predecessor and `VERSION`'s absence still says
/// the transaction is unfinished.
fn publish(root: &Path, ready: &[(String, String, String)]) -> Result<()> {
    for (id, _, event) in ready {
        let path = store::events_path(root, id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| tool_error(parent.display(), error))?;
        }
        store::write_text(&path, event)?;
    }
    for (id, object, _) in ready {
        store::write_text(&store::object_path(root, id), object)?;
    }
    // Old pending Human-Gate state is unsupported and is not migrated: a question
    // asked under the predecessor's contract cannot be answered under this one,
    // and the material it was asked about has moved representation underneath
    // it. Prepare it again.
    for domain in ["events", "candidates"] {
        let path = store::engr_dir(root).join(domain);
        if path.exists() {
            fs::remove_dir_all(&path).map_err(|error| tool_error(path.display(), error))?;
        }
    }
    let bootstrap = store::engr_dir(root).join("format.json");
    if bootstrap.exists() {
        fs::remove_file(&bootstrap).map_err(|error| tool_error(bootstrap.display(), error))?;
    }
    for path in [
        store::challenges_dir(root),
        crate::backlog::dir(root),
        crate::collection::dir(root),
        crate::rules::dir(root),
    ] {
        fs::create_dir_all(&path).map_err(|error| tool_error(path.display(), error))?;
    }
    for path in crate::work::dirs(root) {
        fs::create_dir_all(&path).map_err(|error| tool_error(path.display(), error))?;
    }
    ensure_local_ignored(root)
}

/// Keep the local directory out of git for the duration of the question, using
/// git's own local exclude file rather than the tracked one.
///
/// Preparation is not an act on the record. Writing `/local/` into the tracked
/// `.engr/.gitignore` at prepare time means merely *asking* to migrate leaves a
/// modified tracked file behind, and a person who then declines is left holding
/// a change they never confirmed — which is precisely the boundary the Human
/// Gate exists to draw.
///
/// `.git/info/exclude` is the right place for it: same effect, same instant,
/// and never part of anything anybody commits. The tracked line is added later,
/// as part of the publication a human did confirm.
///
/// Outside a repository there is nothing to keep the code out of, so this is
/// satisfied by there being no git.
fn exclude_local_from_git(root: &Path) -> Result<()> {
    let Some(path) = crate::git::git_path(root, "info/exclude") else {
        return Ok(());
    };
    let Some(relative) = crate::git::repo_relative_dir(root, &store::local_dir(root)) else {
        return Ok(());
    };
    let entry = format!("/{relative}/");
    let mut text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => return Err(tool_error(path.display(), error)),
    };
    if text.lines().any(|line| line.trim() == entry) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| tool_error(parent.display(), error))?;
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!(
        "# engr: a live challenge code is a filename here, and it is meant for one\n\
         # person. Added while a migration was prepared; the migrated workspace\n\
         # carries the same rule in its own tracked .gitignore.\n\
         {entry}\n"
    ));
    fs::write(&path, text).map_err(|error| tool_error(path.display(), error))
}

/// The predecessor's `.gitignore` named `lock` and `candidates/`, which this
/// generation does not have. One line replaces both, and it is added rather than
/// the file being rewritten: an ignore file is a person's, not engr's.
fn ensure_local_ignored(root: &Path) -> Result<()> {
    let path = store::engr_dir(root).join(".gitignore");
    let mut text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => return Err(tool_error(path.display(), error)),
    };
    if text.lines().any(|line| line.trim() == "/local/") {
        return Ok(());
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str("/local/\n");
    fs::write(&path, text).map_err(|error| tool_error(path.display(), error))
}

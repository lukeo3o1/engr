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

/// The six semantic fields the predecessor's whole-content seal covered.
///
/// `admission` and `header` are deliberately absent: the original Ref never
/// attested either, and adding them would make the migrated Ref claim a
/// dependency nobody declared — which would then drift the first time a Section
/// was promoted or given a header.
const PREDECESSOR_REF_FIELDS: [crate::dependency::SemanticField; 6] = [
    crate::dependency::SemanticField::BasedOn,
    crate::dependency::SemanticField::Content,
    crate::dependency::SemanticField::Refs,
    crate::dependency::SemanticField::Relations,
    crate::dependency::SemanticField::Role,
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
        let target = crate::proof::section_target(&reference.object, reference.section);
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
                            crate::proof::section_target(&key.object, key.section),
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
    if let Some(manifest) = staged(root)? {
        // A staged plan is the question a human is already holding. Re-deriving
        // it would mint a second code for the same migration and void the one
        // they have.
        let path = store::challenge_path(root, &manifest.challenge)?;
        ensure!(
            path.exists(),
            EXIT_INVARIANT,
            "the staged migration names challenge {}, which is gone; remove {} to start again",
            manifest.challenge,
            stage_dir(root).display()
        );
        return Ok(Proposed {
            challenge: manifest.challenge,
            subject: manifest.subject,
            resumed: true,
        });
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
    let taken = pending_challenge_codes(root)?;
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

/// Challenge codes already on disk, read without holding any of them to this
/// build's rules — minting must avoid a code even where the file is unreadable.
fn pending_challenge_codes(root: &Path) -> Result<Vec<String>> {
    let dir = store::challenges_dir(root);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut codes = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|error| tool_error(dir.display(), error))? {
        let entry = entry.map_err(|error| tool_error(dir.display(), error))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(code) = name.strip_suffix(".json") {
            if crate::confirmation::valid_challenge(code) {
                codes.push(code.to_owned());
            }
        }
    }
    codes.sort();
    Ok(codes)
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
    finish(root, challenge, ready, sections)
}

/// Stage the destination and stop, which is the state a crash between staging
/// and publication leaves behind.
///
/// A test seam, and deliberately a blunt one. There is no legitimate reason for
/// a caller to want half a migration, so this is hidden from the documented
/// surface rather than dressed up as an option — but the resumable property it
/// exists to exercise is the one that decides whether a crash mid-publication
/// is recoverable, and a property nothing can reach is a property nothing
/// checks.
#[doc(hidden)]
pub fn stage_destination_only(root: &Path, challenge: &str) -> Result<()> {
    store::with_lock(root, || {
        let manifest = staged(root)?
            .ok_or_else(|| Error::new(EXIT_INVARIANT, "no migration is staged".to_owned()))?;
        ensure!(
            manifest.challenge == challenge,
            EXIT_INVARIANT,
            "the staged migration is waiting on challenge {}, not {challenge}",
            manifest.challenge
        );
        let (_, predecessors) = preflight(root)?;
        let admitted = EventAdmission {
            by: Admission::Human,
            at: now(),
            confirmation: Some(HumanConfirmation {
                challenge: challenge.to_owned(),
            }),
            review: None,
        };
        let mut closure = RefClosure::new(root);
        let mut derived = Vec::new();
        for predecessor in &predecessors {
            derived.push(derive(&mut closure, predecessor, admitted.clone())?);
        }
        stage_destination(root, challenge, &derived)
    })
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
    // Last, and only after everything else landed. While `VERSION` is absent the
    // transaction is unfinished and the staged destination is what finishes it;
    // the moment it exists, every read surface is looking at the new generation.
    store::write_generation(root)?;
    // A spent code resolves to nothing, so the file goes with the question it
    // asked — the same disposal an ordinary confirmation performs.
    let spent = store::challenge_path(root, &challenge.id)?;
    if spent.exists() {
        fs::remove_file(&spent).map_err(|error| tool_error(spent.display(), error))?;
    }
    let stage = stage_dir(root);
    if stage.exists() {
        fs::remove_dir_all(&stage).map_err(|error| tool_error(stage.display(), error))?;
    }
    Ok(Report {
        objects: ids,
        sections,
    })
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
    ensure!(
        destination.files.len() == subject.objects.len(),
        EXIT_INVARIANT,
        "the staged destination holds {} objects and the confirmed plan names {}",
        destination.files.len(),
        subject.objects.len()
    );
    let mut ready = Vec::new();
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
        let object_text = read_staged(&dir.join("objects").join(format!("{}.json", file.object)))?;
        let event_text = read_staged(
            &dir.join("eventstore")
                .join(format!("{}.jsonl", file.object)),
        )?;
        let object = store::decode_object(
            Path::new(&file.object),
            &file.object,
            serde_json::from_str(&object_text).map_err(|error| {
                Error::new(EXIT_SCHEMA, format!("staged {}: {error}", file.object))
            })?,
        )?;
        ensure!(
            object.digest == file.object_digest
                && object.recomputed_digest()? == file.object_digest,
            EXIT_INVARIANT,
            "the staged Object for {} is not the value it was staged as",
            file.object
        );
        let event: Event = serde_json::from_str(event_text.trim_end())
            .map_err(|error| Error::new(EXIT_SCHEMA, format!("staged {}: {error}", file.object)))?;
        event.validate(&file.object)?;
        ensure!(
            event.digest == file.event_digest,
            EXIT_INVARIANT,
            "the staged bootstrap Event for {} is not the record it was staged as",
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

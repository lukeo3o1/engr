//! Coordinated workspace-generation migration.
//!
//! The predecessor is validated as a whole before one authoritative Object is
//! replaced. A durable staging marker then makes the commit phase resumable:
//! Objects may be copied from the already-validated plan more than once, and
//! `format.json` advances only after every copy succeeds.

use crate::dependency::{self, SemanticField};
use crate::model::{LegacyRef, Object, Provenance, Ref, Section};
use crate::proof::{sha256_of, stored_within_safe_integers};
use crate::semantics::Admission;
use crate::store::{self, WorkspaceFormat};
use crate::{ensure, tool_error, Error, Result, EXIT_INVARIANT, EXIT_SCHEMA};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

const STAGE: &str = "migration-v3";
const STAGE_TEMP: &str = "migration-v3.tmp";
const MANIFEST: &str = "manifest.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    source_version: u32,
    target_version: u32,
    objects: BTreeMap<String, String>,
    /// Retained resources whose bytes this migration rewrites, by their
    /// `.engr`-relative path.
    resources: BTreeMap<String, String>,
    source: BTreeMap<String, String>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct HistoricalKey {
    commit: String,
    object: String,
    section: u64,
}

struct RefClosure<'a> {
    root: &'a Path,
    cache: BTreeMap<HistoricalKey, Section>,
    visiting: BTreeSet<HistoricalKey>,
}

impl<'a> RefClosure<'a> {
    fn new(root: &'a Path) -> Self {
        Self {
            root,
            cache: BTreeMap::new(),
            visiting: BTreeSet::new(),
        }
    }

    fn convert_section(&mut self, mut section: Section) -> Result<Section> {
        ensure!(
            section.admission == Admission::Human,
            EXIT_SCHEMA,
            "a predecessor Section can only reconstruct as human admission"
        );
        let mut converted = Vec::with_capacity(section.refs.len());
        for reference in std::mem::take(&mut section.refs) {
            let Ref::Legacy(reference) = reference else {
                return Err(Error::new(
                    EXIT_SCHEMA,
                    "a predecessor workspace cannot already carry a selective reference".to_owned(),
                ));
            };
            converted.push(Ref::selective(self.convert_ref(reference)?));
        }
        section.refs = converted;
        section.content().validate_for_migration()?;
        Ok(section)
    }

    fn convert_ref(&mut self, reference: LegacyRef) -> Result<dependency::SelectiveRef> {
        ensure!(
            reference.section > 0,
            EXIT_SCHEMA,
            "a legacy reference cannot name section 0"
        );
        ensure!(
            reference.section <= crate::proof::MAX_SAFE_INTEGER,
            EXIT_SCHEMA,
            "legacy reference section {} is outside the shared safe-integer range",
            reference.section
        );
        ensure_lower_sha256(&reference.sha256, "legacy reference seal")?;
        let key = HistoricalKey {
            commit: reference.commit.clone(),
            object: reference.object.clone(),
            section: reference.section,
        };
        let historical = self.historical_section(key.clone())?;
        let legacy_seal = historical.recomputed_sha256()?;
        ensure!(
            legacy_seal == reference.sha256,
            EXIT_INVARIANT,
            "{} §{} at {} seals as {}, not the legacy reference seal {}",
            reference.object,
            reference.section,
            reference.commit,
            legacy_seal,
            reference.sha256
        );
        let fields = dependency::canonical_fields(&[
            SemanticField::Role,
            SemanticField::Text,
            SemanticField::Content,
            SemanticField::BasedOn,
            SemanticField::Refs,
            SemanticField::Relations,
        ])?;
        let target = crate::proof::section_target(&reference.object, reference.section);
        let snapshot = dependency::ref_snapshot(
            target.clone(),
            &fields,
            &historical,
            reference.commit.clone(),
        )?;
        dependency::SelectiveRef::stored(
            target,
            fields,
            reference.commit,
            snapshot.digest()?.to_string(),
        )
    }

    fn historical_section(&mut self, key: HistoricalKey) -> Result<Section> {
        if let Some(section) = self.cache.get(&key) {
            return Ok(section.clone());
        }
        ensure!(
            self.visiting.insert(key.clone()),
            EXIT_SCHEMA,
            "legacy reference closure cycles through {} §{} at {}",
            key.object,
            key.section,
            key.commit
        );
        let result = (|| {
            let object =
                crate::git::object_at(self.root, &key.commit, &key.object)?.ok_or_else(|| {
                    Error::new(
                        crate::EXIT_NOT_FOUND,
                        format!(
                            "legacy reference target {} is absent at {}",
                            crate::proof::section_target(&key.object, key.section),
                            key.commit
                        ),
                    )
                })?;
            let section = object.section(key.section)?.clone();
            check_legacy_section(&section)?;
            self.convert_section(section)
        })();
        self.visiting.remove(&key);
        let section = result?;
        self.cache.insert(key, section.clone());
        Ok(section)
    }
}

pub(crate) fn migrated_historical_section(
    root: &Path,
    commit: &str,
    object: &str,
    section: u64,
) -> Result<Section> {
    RefClosure::new(root).historical_section(HistoricalKey {
        commit: commit.to_owned(),
        object: object.to_owned(),
        section,
    })
}

/// The migrated-v3 reading of an Object reconstructed from retained Event-v1
/// history.
///
/// Retained history is deliberately not rewritten: it is read under its own
/// generation. So replaying it produces the predecessor in its legacy spelling,
/// while everything current — the stored Object, and any digest taken against
/// it — is in the migrated one. Anything that reconstructs a predecessor and
/// then compares it with current material has to convert first, or it is
/// comparing two spellings of the same thing and calling them different.
///
/// A Section already carrying selective refs is left alone; the closure refuses
/// those by design, and after a partial history there can be both.
///
/// `wanted` decides how much of the Object has to make that trip. Converting a
/// Section reopens the historical Git target of every legacy Ref on it, so
/// converting the whole Object makes every legacy Ref in it a precondition for
/// whatever the caller was actually trying to do. See [`Migrated`].
pub(crate) fn migrated_replay(
    root: &Path,
    mut object: Object,
    wanted: &Migrated,
) -> Result<Object> {
    let convert = |section: &Section| {
        wanted.includes(section.id)
            && section
                .refs
                .iter()
                .any(|reference| matches!(reference, Ref::Legacy(_)))
    };
    if !object.sections.iter().any(convert) {
        return Ok(object);
    }
    let mut closure = RefClosure::new(root);
    let mut sections = Vec::with_capacity(object.sections.len());
    for section in std::mem::take(&mut object.sections) {
        if convert(&section) {
            sections.push(closure.convert_section(section)?);
        } else {
            sections.push(section);
        }
    }
    object.sections = sections;
    Ok(object)
}

/// How much of an Object a caller needs in the migrated spelling.
///
/// Converting a Section is not free and not local: it reopens the historical Git
/// commit each of its legacy Refs pins. So a caller that converts the whole
/// Object has made every legacy Ref anywhere in it a precondition — and if one
/// of those commits is later lost, the conversion fails with `EXIT_NOT_FOUND`
/// and takes the caller down with it, however little of the Object the caller
/// was reading.
///
/// That matters most on the read path, where the failure would arrive as "this
/// Object does not exist". Retained Event history is not current-state
/// authority, and a Ref whose pinned commit is gone is `provenance unavailable`
/// where it is actually depended on — never grounds for making a sound current
/// Object unreadable because something unrelated to it moved.
pub(crate) enum Migrated {
    /// Every Section carrying a legacy Ref. For projections that read them all.
    Whole,
    /// Only these Sections, by id.
    Sections(std::collections::BTreeSet<u64>),
}

impl Migrated {
    /// Nothing yet. Widen it with the operations that will actually be proved.
    pub(crate) fn nothing() -> Self {
        Migrated::Sections(std::collections::BTreeSet::new())
    }

    /// Add whatever one operation's CandidateDigest projection reads.
    ///
    /// The arms are the arms of [`crate::proof::candidate_subject`], and they
    /// have to stay that way: this decides what gets reconstructed, and that
    /// function decides what gets hashed. `object.renamed` and the lifecycle
    /// operations project title and state and name no Section. A Section
    /// operation names its own participants. Only `section_merged` and
    /// `object.superseded` need everything, because `ObjectInvariant` carries
    /// every Section's semantics.
    ///
    /// `section.added` needs nothing: its before-state names no Section, and the
    /// Section it produces is created after the boundary, so it cannot be
    /// carrying a legacy Ref.
    pub(crate) fn widen(&mut self, action: &crate::model::Action) {
        use crate::model::Action;
        let Migrated::Sections(ids) = self else {
            return;
        };
        match action {
            // Repair joins these: its projection is `ObjectInvariant` too, so
            // every Section's semantics are one of its digest inputs.
            Action::SectionMerged { .. } | Action::ObjectSuperseded | Action::ObjectRepaired => {
                *self = Migrated::Whole
            }
            Action::SectionRevised { section } | Action::SectionDeleted { section } => {
                ids.insert(*section);
            }
            Action::SectionAdded
            | Action::ObjectCreated
            | Action::ObjectRenamed
            | Action::ObjectClosed
            | Action::ObjectReopened
            | Action::ObjectClassified { .. } => {}
        }
    }

    fn includes(&self, section: u64) -> bool {
        match self {
            Migrated::Whole => true,
            Migrated::Sections(ids) => ids.contains(&section),
        }
    }
}

/// Migrate or resume a migration while the caller holds the workspace lock.
pub(crate) fn run(root: &Path) -> Result<()> {
    let stage = stage_dir(root);
    if stage.exists() {
        store::ensure_migration_ignored(root)?;
        return commit_stage(root, &stage);
    }
    let before = store::validate_format(root)?;
    ensure!(
        before != WorkspaceFormat::Current,
        EXIT_SCHEMA,
        "workspace does not require migration"
    );
    let source_version = match before {
        WorkspaceFormat::LegacyV0 => 0,
        WorkspaceFormat::OlderVersion(version) => version,
        WorkspaceFormat::Current => unreachable!("checked above"),
    };
    let plan = preflight(root, source_version)?;
    store::ensure_migration_ignored(root)?;
    stage_plan(root, source_version, &plan)?;
    commit_stage(root, &stage_dir(root))
}

/// Everything the commit phase will publish, and the exact predecessor bytes it
/// was derived from.
///
/// `source` is not a second read of the workspace taken afterwards. Every entry
/// is the digest of the text preflight itself decoded, so the manifest names
/// what was actually validated rather than whatever happens to be on disk when
/// the manifest is written. A source file that moves in between then fails the
/// closing comparison instead of quietly becoming the new expected predecessor.
struct Plan {
    objects: BTreeMap<String, Object>,
    /// Retained resources whose persisted bytes change under v3, by their
    /// `.engr`-relative path.
    resources: BTreeMap<String, String>,
    source: BTreeMap<String, String>,
}

/// One read of a predecessor file, with its digest kept.
///
/// The capture and the validation see the same bytes because they are the same
/// bytes: the read happens once, and everything downstream works from the text
/// it returned.
fn capture(source: &mut BTreeMap<String, String>, root: &Path, path: &Path) -> Result<String> {
    let text = fs::read_to_string(path).map_err(|error| tool_error(path.display(), error))?;
    source.insert(relative_to_engr(root, path)?, sha256_of(&text));
    Ok(text)
}

fn relative_to_engr(root: &Path, path: &Path) -> Result<String> {
    let base = store::engr_dir(root);
    path.strip_prefix(&base)
        .map(|relative| {
            relative
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/")
        })
        .map_err(|_| {
            Error::new(
                EXIT_SCHEMA,
                format!("{} is outside the workspace", path.display()),
            )
        })
}

/// The predecessor projection for one Object, from the exact bytes it is built
/// out of.
///
/// Preflight passes the bytes it captures; resume passes the bytes the manifest
/// says preflight captured. Both get the same rules from here, because a second
/// copy of them kept in step by hand is how the resume path and the plan start
/// disagreeing about what the predecessor was.
fn predecessor_projection(
    root: &Path,
    id: &str,
    source_version: u32,
    stored: Option<&str>,
    history: &str,
) -> Result<Object> {
    let path = store::object_path(root, id);
    let stored = match stored {
        Some(text) => {
            let value: serde_json::Value = serde_json::from_str(text)
                .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
            stored_within_safe_integers(&value, &path.display().to_string())?;
            let object = store::decode_object_for_version(&path, id, value, source_version)?;
            check_legacy_object(&object)?;
            Some(object)
        }
        None => None,
    };

    let events_path = store::events_path(root, id);
    let events = store::decode_events(root, &events_path, id, history)?;
    for event in &events {
        ensure!(
            event.version == crate::EVENT_ENVELOPE_VERSION_V0
                && matches!(event.provenance, Provenance::Confirmed { .. }),
            EXIT_SCHEMA,
            "a predecessor workspace carries only Event generation 1"
        );
        ensure_legacy_refs(&event.payload.content.refs)?;
    }
    if stored.is_none() {
        ensure!(
            events.first().is_some_and(|event| event.rev == 1
                && matches!(event.payload.action, crate::model::Action::ObjectCreated)),
            EXIT_SCHEMA,
            "{id}: event rev 1 cannot reconstruct a missing object"
        );
    }
    let (reconciled, _) =
        crate::model::replay_recoverable_tail(Object::new(id.to_owned(), String::new())?, &events)
            .map_err(|error| {
                Error::new(
                    EXIT_SCHEMA,
                    format!(
                        "{id}: predecessor event tail cannot reconcile: {}",
                        error.message
                    ),
                )
            })?;
    if let Some(stored) = stored {
        ensure!(
            stored.title == reconciled.title
                && stored.object_type == reconciled.object_type
                && stored.state == reconciled.state
                && stored.rev == reconciled.rev
                && stored.next_section_id == reconciled.next_section_id
                && stored.sections == reconciled.sections,
            EXIT_INVARIANT,
            "{id}: predecessor Object projection is not exactly derivable from admitted history"
        );
    }
    check_legacy_object(&reconciled)?;
    Ok(reconciled)
}

/// The deterministic v3 conversion of one predecessor projection.
///
/// This is the migration's actual target for an Object. Preflight derives the
/// plan with it and resume re-derives the same answer with it, so what the
/// stage claims is never the only thing saying what should be published.
fn migrate_object(closure: &mut RefClosure, id: &str, mut object: Object) -> Result<Object> {
    object.legacy_format = None;
    object.legacy_version = None;
    object.sections.sort_by_key(|section| section.id);
    let mut sections = Vec::with_capacity(object.sections.len());
    for section in object.sections {
        let mut section = closure.convert_section(section)?;
        // The current generation persists one order for a set, so the
        // migrated bytes have to be in it. Sealing canonicalizes a clone
        // either way; what changes here is the representation on disk.
        crate::proof::canonical_set(&mut section.refs, "reference")?;
        crate::proof::canonical_set(&mut section.relations, "relation")?;
        sections.push(section);
    }
    object.sections = sections;
    object.sha256 = None;
    object.validate()?;
    // Before the seal, not after it. Sealing runs this same walk on its way
    // through `canonical_bytes`, but that one reports a *usage* fault — and
    // a number reaching here came out of a predecessor file, not off
    // somebody's command line. Checking first keeps the fault class honest
    // about where the value was found, and refuses it before a seal is
    // computed over bytes JCS would have silently rounded.
    let value = serde_json::to_value(&object)
        .map_err(|error| Error::new(EXIT_SCHEMA, format!("object {id}: {error}")))?;
    stored_within_safe_integers(&value, &format!("object {id}"))?;
    let resealed = crate::integrity::seal_migrated(object)?;
    resealed.object.validate()?;
    crate::integrity::check_stored_object_integrity(&resealed.object)?;
    Ok(resealed.object)
}

fn preflight(root: &Path, source_version: u32) -> Result<Plan> {
    ensure!(
        migratable_source_version(source_version),
        EXIT_SCHEMA,
        "workspace version {source_version} has no defined migration to version {}",
        crate::WORKSPACE_VERSION
    );
    let mut source = BTreeMap::new();
    let mut stored = BTreeMap::new();
    for id in store::object_ids(root)? {
        let path = store::object_path(root, &id);
        let text = capture(&mut source, root, &path)?;
        stored.insert(id, text);
    }

    // An Object with no Event file was never compared against admitted history
    // at all: the loop below only reaches ids the EventStore knows. Its legacy
    // Section seals say nothing about the Object level — title, type, state,
    // revision, counter, Section membership — so granting it the first v3
    // aggregate seal would launder an unverifiable projection into current
    // authority. Under the retained EventStore contract every stored Object has
    // an admitted creation, so absence here is a broken predecessor rather than
    // a shape this generation has to accommodate.
    let event_ids = store::event_ids(root)?;
    for id in stored.keys() {
        ensure!(
            event_ids.contains(id),
            EXIT_INVARIANT,
            "{id}: no admitted history, so its projection cannot be proven before it is sealed"
        );
    }

    // Validate every Event record and apply only a recoverable tail before
    // converting representation. No v1 Event is ever replayed into a v3
    // projection after the generation has advanced.
    let mut predecessor = BTreeMap::new();
    for id in &event_ids {
        // Retained Event-v1 history stays under the contract that wrote it.
        // #35 scopes the Phase-3 numeric domain to values participating in
        // *current* state — §11 fails migration on "a required current-state
        // JSON integer", and acceptance criterion 21 says the same — so the
        // bound is applied to the predecessor Object above and to the migrated
        // projection below, and never to immutable history for its own sake.
        //
        // Nothing escapes through the gap. An Event's only numbers are its
        // `rev` and the Section ids an action names, and neither can be out of
        // domain here while the migration still succeeds: `rev` is replayed
        // contiguously from 1, and a Section id is only ever handed out from
        // `next_section_id`, which the migrated Object carries into the walk
        // below. An out-of-domain number in either position fails replay or
        // fails that walk — it does not need a third check that would also
        // refuse history for numbers current state never reads.
        let history = capture(&mut source, root, &store::events_path(root, id))?;
        let projection = predecessor_projection(
            root,
            id,
            source_version,
            stored.get(id).map(String::as_str),
            &history,
        )?;
        predecessor.insert(id.clone(), projection);
    }

    let resources = validate_retained_resources(root, &predecessor, &mut source)?;

    let mut closure = RefClosure::new(root);
    let mut migrated = BTreeMap::new();
    for (id, object) in predecessor {
        let object = migrate_object(&mut closure, &id, object)?;
        migrated.insert(id, object);
    }
    // Everything preflight read is confirmed unchanged, and nothing else may be
    // in the set. The closing walk used to *become* `Plan.source`, so a file
    // that appeared after its own domain was enumerated — a new Backlog item, a
    // new Event log, a new Rule — was promoted into the manifest as an expected
    // predecessor without ever being schema, JCS, replay or Rule validated. The
    // commit phase would then find the workspace exactly as the manifest
    // described it and advance the generation over an unvalidated resource.
    //
    // So the manifest is the captured set, and the live walk is only ever asked
    // whether it still agrees.
    let live = source_fingerprint(root)?;
    for path in live.keys() {
        ensure!(
            source.contains_key(path),
            EXIT_INVARIANT,
            "{path} appeared while migration was validating the workspace"
        );
    }
    for (path, digest) in &source {
        ensure!(
            live.get(path) == Some(digest),
            EXIT_INVARIANT,
            "{path} changed while migration was validating it"
        );
    }
    Ok(Plan {
        objects: migrated,
        resources,
        source,
    })
}

/// Validate every retained resource, and say which of them need new bytes.
///
/// Adopting JCS is itself a representation migration: the predecessor build
/// wrote these files with a pretty serializer, so an ordinary v2 Backlog,
/// Collection or Work file is not the bytes v3 says a current resource has.
/// Advancing `format.json` over them unchanged would leave a workspace full of
/// resources its own reader refuses. One already byte-identical to its v3 form
/// needs no rewrite and gets none.
fn validate_retained_resources(
    root: &Path,
    objects: &BTreeMap<String, Object>,
    source: &mut BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>> {
    let mut rewrites = BTreeMap::new();
    for id in crate::backlog::ids(root)? {
        let path = crate::backlog::item_path(root, &id);
        let text = capture(source, root, &path)?;
        let mut item = crate::backlog::decode_for_migration(&path, &id, &text)?;
        crate::backlog::canonicalize_sets(&mut item)?;
        let canonical = crate::proof::canonical_bytes(&item, "backlog item")?;
        plan_rewrite(root, &path, canonical, source, &mut rewrites)?;
    }
    for id in crate::collection::ids(root)? {
        let path = crate::collection::path(root, &id);
        let text = capture(source, root, &path)?;
        let mut collection = crate::collection::decode_for_migration(&path, &id, &text)?;
        crate::collection::canonicalize_members(&mut collection)?;
        let canonical = crate::proof::canonical_bytes(&collection, "collection")?;
        plan_rewrite(root, &path, canonical, source, &mut rewrites)?;
    }
    for id in crate::work::ids(root)? {
        let path = crate::work::path(root, &id);
        let text = capture(source, root, &path)?;
        let mut work = crate::work::decode_for_migration(&path, &id, &text)?;
        ensure!(
            objects.contains_key(&id),
            EXIT_SCHEMA,
            "work sidecar {id} belongs to no Object in the migrated projection"
        );
        crate::work::canonicalize_work(&mut work)?;
        let canonical = crate::proof::canonical_bytes(&work, "work")?;
        plan_rewrite(root, &path, canonical, source, &mut rewrites)?;
    }
    // Rules are captured from the same read that validated them, exactly as
    // artifact-exact identity requires everywhere else: reopening the path to
    // fingerprint it is a second read of a file that is editable outside the
    // workspace lock, so the manifest could name bytes the loader never
    // accepted. `Rule::raw` is the text that was parsed.
    for rule in crate::rules::load_all_for_migration(root)? {
        source.insert(relative_to_engr(root, &rule.source)?, sha256_of(&rule.raw));
    }
    // Anything else living under `rules/` is captured too, and only so the
    // closing set comparison can be exact. It is not a Rule — the loader reads
    // `.md` and nothing else — so nothing here claims it was validated as one;
    // what is claimed is that it was present, and that it did not change.
    let rules_dir = crate::rules::dir(root);
    if rules_dir.is_dir() {
        for entry in
            fs::read_dir(&rules_dir).map_err(|error| tool_error(rules_dir.display(), error))?
        {
            let entry = entry.map_err(|error| tool_error(rules_dir.display(), error))?;
            let path = entry.path();
            if entry
                .file_type()
                .map_err(|error| tool_error(path.display(), error))?
                .is_file()
            {
                let relative = relative_to_engr(root, &path)?;
                if !source.contains_key(&relative) {
                    capture(source, root, &path)?;
                }
            }
        }
    }
    Ok(rewrites)
}

fn plan_rewrite(
    root: &Path,
    path: &Path,
    canonical: String,
    source: &BTreeMap<String, String>,
    rewrites: &mut BTreeMap<String, String>,
) -> Result<()> {
    let relative = relative_to_engr(root, path)?;
    if source.get(&relative) != Some(&sha256_of(&canonical)) {
        rewrites.insert(relative, canonical);
    }
    Ok(())
}

fn migratable_source_version(version: u32) -> bool {
    // Format-less legacy workspaces carry a representation explicitly handled
    // by this converter. The governed Phase-3 migration itself is frozen as
    // v2 -> v3; accepting v1 here would silently invent a cumulative contract.
    version == 0 || crate::MIGRATABLE_WORKSPACE_VERSIONS.contains(&version)
}

fn check_legacy_object(object: &Object) -> Result<()> {
    ensure!(
        object.sha256.is_none(),
        EXIT_SCHEMA,
        "a predecessor Object cannot already carry a v3 aggregate seal"
    );
    for section in &object.sections {
        check_legacy_section(section)?;
    }
    Ok(())
}

fn check_legacy_section(section: &Section) -> Result<()> {
    ensure!(
        section.admission == Admission::Human,
        EXIT_SCHEMA,
        "a predecessor Section can only reconstruct as human admission"
    );
    ensure_lower_sha256(&section.sha256, "legacy Section seal")?;
    let recomputed = section.recomputed_sha256()?;
    ensure!(
        recomputed == section.sha256,
        EXIT_INVARIANT,
        "section {} was sealed as {} and its predecessor contents seal as {}",
        section.id,
        section.sha256,
        recomputed
    );
    ensure_legacy_refs(&section.refs)
}

fn ensure_legacy_refs(refs: &[Ref]) -> Result<()> {
    for reference in refs {
        ensure!(
            matches!(reference, Ref::Legacy(_)),
            EXIT_SCHEMA,
            "a predecessor resource cannot already carry a selective reference"
        );
    }
    Ok(())
}

fn ensure_lower_sha256(value: &str, what: &str) -> Result<()> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        EXIT_SCHEMA,
        "{what} is not 64 lowercase hexadecimal characters"
    );
    Ok(())
}

/// The Object id a `.engr`-relative source path names, if it names one.
fn staged_object_id(path: &str) -> Option<&str> {
    path.strip_prefix("objects/")
        .and_then(|rest| rest.strip_suffix(".json"))
}

/// The only retained resources a v3 stage may publish. A manifest is durable
/// operational input, so its map keys are never paths by convention: they are
/// parsed capabilities before either staging or workspace paths are built.
enum RetainedResource {
    Backlog(String),
    Collection(String),
    Work(String),
}

fn retained_resource(relative: &str) -> Result<RetainedResource> {
    ensure!(
        !relative.contains('\\'),
        EXIT_SCHEMA,
        "staged retained resource {relative:?} is not an .engr-relative path"
    );
    let pieces: Vec<_> = relative.split('/').collect();
    let file_id = |name: &str| -> Result<String> {
        let id = name.strip_suffix(".json").ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!("staged retained resource {relative:?} is not a JSON resource"),
            )
        })?;
        ensure!(
            !id.is_empty() && id != "." && id != "..",
            EXIT_SCHEMA,
            "staged retained resource {relative:?} has no valid resource identity"
        );
        Ok(id.to_owned())
    };
    match pieces.as_slice() {
        ["backlog", name] => Ok(RetainedResource::Backlog(file_id(name)?)),
        ["collections", name] => Ok(RetainedResource::Collection(file_id(name)?)),
        ["work", "objects", name] => Ok(RetainedResource::Work(file_id(name)?)),
        _ => Err(Error::new(
            EXIT_SCHEMA,
            format!("staged retained resource {relative:?} is not a known .engr resource path"),
        )),
    }
}

impl RetainedResource {
    fn staged_path(&self, stage: &Path) -> PathBuf {
        match self {
            Self::Backlog(id) => stage
                .join("resources")
                .join("backlog")
                .join(format!("{id}.json")),
            Self::Collection(id) => stage
                .join("resources")
                .join("collections")
                .join(format!("{id}.json")),
            Self::Work(id) => stage
                .join("resources")
                .join("work")
                .join("objects")
                .join(format!("{id}.json")),
        }
    }

    fn destination(&self, root: &Path) -> PathBuf {
        match self {
            Self::Backlog(id) => crate::backlog::item_path(root, id),
            Self::Collection(id) => crate::collection::path(root, id),
            Self::Work(id) => crate::work::path(root, id),
        }
    }

    fn validate_staged(
        &self,
        staged: &Path,
        text: &str,
        objects: &BTreeMap<String, String>,
    ) -> Result<()> {
        match self {
            Self::Backlog(id) => {
                crate::backlog::decode_current_staged(staged, id, text)?;
            }
            Self::Collection(id) => {
                crate::collection::decode_current_staged(staged, id, text)?;
            }
            Self::Work(id) => {
                ensure!(
                    objects.contains_key(id),
                    EXIT_SCHEMA,
                    "staged work sidecar {id} belongs to no Object in the migration plan"
                );
                crate::work::decode_current_staged(staged, id, text)?;
            }
        }
        Ok(())
    }

    /// A resource digest alone proves only that staged bytes and the manifest
    /// agree. Rebuild the deterministic v3 spelling from the captured
    /// predecessor too, so editing both staged bytes and their digest cannot
    /// replace the migration plan with a different valid resource.
    fn verify_derivation(
        &self,
        root: &Path,
        relative: &str,
        staged: &str,
        manifest: &Manifest,
    ) -> Result<()> {
        let source_path = self.destination(root);
        let source = fs::read_to_string(&source_path)
            .map_err(|error| tool_error(source_path.display(), error))?;
        let expected_source = manifest.source.get(relative).ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!("staged retained resource {relative:?} was not captured at preflight"),
            )
        })?;
        let staged_digest = manifest.resources.get(relative).ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!("staged retained resource {relative:?} has no staged digest"),
            )
        })?;
        let source_digest = sha256_of(&source);
        if source_digest == *staged_digest {
            return Ok(());
        }
        ensure!(
            source_digest == *expected_source,
            EXIT_INVARIANT,
            "{relative} changed after migration preflight"
        );
        let canonical = match self {
            Self::Backlog(id) => {
                let mut item = crate::backlog::decode_for_migration(&source_path, id, &source)?;
                crate::backlog::canonicalize_sets(&mut item)?;
                crate::proof::canonical_bytes(&item, "backlog item")?
            }
            Self::Collection(id) => {
                let mut collection =
                    crate::collection::decode_for_migration(&source_path, id, &source)?;
                crate::collection::canonicalize_members(&mut collection)?;
                crate::proof::canonical_bytes(&collection, "collection")?
            }
            Self::Work(id) => {
                ensure!(
                    manifest.objects.contains_key(id),
                    EXIT_SCHEMA,
                    "staged work sidecar {id} belongs to no Object in the migration plan"
                );
                let mut work = crate::work::decode_for_migration(&source_path, id, &source)?;
                crate::work::canonicalize_work(&mut work)?;
                crate::proof::canonical_bytes(&work, "work")?
            }
        };
        ensure!(
            canonical == staged,
            EXIT_INVARIANT,
            "{relative} staged bytes are not the canonical migration of the captured predecessor"
        );
        Ok(())
    }
}

/// A staged Object digest proves only that the stage agrees with itself.
///
/// Retained resources are re-derived from the captured predecessor on resume,
/// so editing staged bytes *and* their manifest digest cannot quietly swap in a
/// different resource. Objects carry more authority than any of them and had no
/// equivalent: a crash leaves a stage whose Object can be rewritten, resealed —
/// Section and aggregate seals are unkeyed, so whoever can edit the file can
/// compute valid ones — and re-digested into a plan that agrees with itself
/// everywhere resume looked, while the predecessor bytes it supposedly came
/// from sit unchanged on disk. That makes operational staging a way to write
/// authority. #31 requires each required conversion to be deterministic and says
/// migration must not legitimize state merely by resealing it, so the target has
/// to be the derivation rather than whatever the manifest currently claims.
fn verify_object_derivation(
    root: &Path,
    closure: &mut RefClosure,
    id: &str,
    staged: &str,
    manifest: &Manifest,
) -> Result<()> {
    let path = store::object_path(root, id);
    let relative = relative_to_engr(root, &path)?;
    let staged_digest = manifest.objects.get(id).ok_or_else(|| {
        Error::new(
            EXIT_SCHEMA,
            format!("staged object {id} has no staged digest"),
        )
    })?;
    let live = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(tool_error(path.display(), error)),
    };
    // An interrupted commit already published this one. The live bytes are the
    // target, and the predecessor they were derived from is no longer on disk
    // to derive from again.
    if live
        .as_deref()
        .is_some_and(|text| sha256_of(text) == *staged_digest)
    {
        return Ok(());
    }
    let stored = match manifest.source.get(&relative) {
        Some(expected) => {
            let text = live.ok_or_else(|| {
                Error::new(
                    EXIT_INVARIANT,
                    format!("{relative} disappeared after migration preflight"),
                )
            })?;
            ensure!(
                sha256_of(&text) == *expected,
                EXIT_INVARIANT,
                "{relative} changed after migration preflight"
            );
            Some(text)
        }
        // Preflight captured no projection for it, so it is the recovery case:
        // an Object whose admitted history reconstructs it. Anything on disk
        // here is neither the predecessor nor the published target.
        None => {
            ensure!(
                live.is_none(),
                EXIT_INVARIANT,
                "{relative} appeared after migration preflight"
            );
            None
        }
    };

    let events_path = store::events_path(root, id);
    let events_relative = relative_to_engr(root, &events_path)?;
    let expected = manifest.source.get(&events_relative).ok_or_else(|| {
        Error::new(
            EXIT_INVARIANT,
            format!("{id}: no admitted history was captured to derive its migration from"),
        )
    })?;
    let history = fs::read_to_string(&events_path)
        .map_err(|error| tool_error(events_path.display(), error))?;
    ensure!(
        sha256_of(&history) == *expected,
        EXIT_INVARIANT,
        "{events_relative} changed after migration preflight"
    );

    let projection = predecessor_projection(
        root,
        id,
        manifest.source_version,
        stored.as_deref(),
        &history,
    )?;
    let canonical = crate::proof::canonical_bytes(
        &migrate_object(closure, id, projection)?,
        &format!("object {id}"),
    )?;
    ensure!(
        canonical == staged,
        EXIT_INVARIANT,
        "{relative} staged bytes are not the canonical migration of the captured predecessor"
    );
    Ok(())
}

fn validate_manifest(manifest: &Manifest) -> Result<()> {
    ensure!(
        migratable_source_version(manifest.source_version),
        EXIT_SCHEMA,
        "staged migration source version {} has no defined migration to version {}",
        manifest.source_version,
        crate::WORKSPACE_VERSION
    );
    for id in manifest.objects.keys() {
        crate::model::validate_object_id(id).map_err(|error| {
            Error::new(
                EXIT_SCHEMA,
                format!(
                    "staged migration object id {id:?} is invalid: {}",
                    error.message
                ),
            )
        })?;
    }
    for (relative, digest) in &manifest.resources {
        retained_resource(relative)?;
        let source = manifest.source.get(relative).ok_or_else(|| {
            Error::new(
                EXIT_SCHEMA,
                format!("staged retained resource {relative:?} was not captured at preflight"),
            )
        })?;
        ensure!(
            source != digest,
            EXIT_SCHEMA,
            "staged retained resource {relative:?} does not rewrite its captured predecessor"
        );
    }
    Ok(())
}
fn stage_dir(root: &Path) -> PathBuf {
    store::engr_dir(root).join(STAGE)
}

fn stage_plan(root: &Path, source_version: u32, plan: &Plan) -> Result<()> {
    let temporary = store::engr_dir(root).join(STAGE_TEMP);
    match fs::symlink_metadata(&temporary) {
        Ok(metadata) => {
            ensure!(
                metadata.file_type().is_dir(),
                EXIT_SCHEMA,
                "{} is not a migration staging directory",
                temporary.display()
            );
            fs::remove_dir_all(&temporary)
                .map_err(|error| tool_error(temporary.display(), error))?;
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(tool_error(temporary.display(), error)),
    }
    let object_dir = temporary.join("objects");
    fs::create_dir_all(&object_dir).map_err(|error| tool_error(object_dir.display(), error))?;
    let mut digests = BTreeMap::new();
    for (id, object) in &plan.objects {
        let path = object_dir.join(format!("{id}.json"));
        store::write_json(&path, object)?;
        let bytes = fs::read_to_string(&path).map_err(|error| tool_error(path.display(), error))?;
        digests.insert(id.clone(), sha256_of(&bytes));
    }
    let mut resources = BTreeMap::new();
    for (relative, canonical) in &plan.resources {
        let path = temporary.join("resources").join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| tool_error(parent.display(), error))?;
        }
        store::write_text(&path, canonical)?;
        resources.insert(relative.clone(), sha256_of(canonical));
    }
    store::write_json(
        &temporary.join(MANIFEST),
        &Manifest {
            source_version,
            target_version: crate::WORKSPACE_VERSION,
            objects: digests,
            resources,
            source: plan.source.clone(),
        },
    )?;
    let stage = stage_dir(root);
    match fs::symlink_metadata(&stage) {
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Ok(_) => {
            return Err(Error::new(
                EXIT_SCHEMA,
                format!("{} already exists", stage.display()),
            ));
        }
        Err(error) => return Err(tool_error(stage.display(), error)),
    }
    fs::rename(&temporary, &stage).map_err(|error| tool_error(stage.display(), error))
}

/// A target-version workspace may retain its stage only in the narrow crash
/// window after every staged artifact was published. The version scalar alone
/// cannot establish that ordering: it can also have been edited or merged.
fn verify_published_stage(root: &Path, manifest: &Manifest) -> Result<()> {
    let current = source_fingerprint(root)?;
    for path in current.keys() {
        let reconstructed = staged_object_id(path).is_some_and(|id| {
            manifest.objects.contains_key(id) && !manifest.source.contains_key(path)
        });
        ensure!(
            manifest.source.contains_key(path) || reconstructed,
            EXIT_INVARIANT,
            "{path} appeared after migration preflight"
        );
    }
    for (path, source_digest) in &manifest.source {
        let actual = current.get(path).ok_or_else(|| {
            Error::new(
                EXIT_INVARIANT,
                format!("{path} disappeared after migration preflight"),
            )
        })?;
        let published_object = staged_object_id(path).and_then(|id| manifest.objects.get(id));
        let published_resource = manifest.resources.get(path);
        ensure!(
            actual == source_digest
                || published_object == Some(actual)
                || published_resource == Some(actual),
            EXIT_INVARIANT,
            "{path} does not match either its predecessor or staged target"
        );
    }

    let current_ids = store::object_ids(root)?;
    ensure!(
        current_ids.len() == manifest.objects.len()
            && current_ids
                .iter()
                .all(|id| manifest.objects.contains_key(id)),
        EXIT_INVARIANT,
        "the target workspace does not contain exactly the staged Object set"
    );
    for (id, expected) in &manifest.objects {
        let path = store::object_path(root, id);
        let bytes = fs::read_to_string(&path).map_err(|error| tool_error(path.display(), error))?;
        ensure!(
            sha256_of(&bytes) == *expected,
            EXIT_INVARIANT,
            "{} is not the staged target Object",
            path.display()
        );
        let value: serde_json::Value = serde_json::from_str(&bytes)
            .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", path.display())))?;
        let object = store::decode_object(&path, id, value)?;
        crate::integrity::check_stored_object_integrity(&object)?;
    }
    for (relative, expected) in &manifest.resources {
        let resource = retained_resource(relative)?;
        let path = resource.destination(root);
        let bytes = fs::read_to_string(&path).map_err(|error| tool_error(path.display(), error))?;
        ensure!(
            sha256_of(&bytes) == *expected,
            EXIT_INVARIANT,
            "{} is not the staged target resource",
            path.display()
        );
        resource.validate_staged(&path, &bytes, &manifest.objects)?;
    }
    Ok(())
}

fn commit_stage(root: &Path, stage: &Path) -> Result<()> {
    let metadata =
        fs::symlink_metadata(stage).map_err(|error| tool_error(stage.display(), error))?;
    ensure!(
        metadata.file_type().is_dir(),
        EXIT_SCHEMA,
        "{} is not a migration staging directory",
        stage.display()
    );
    let manifest: Manifest = store::read_current_json(&stage.join(MANIFEST))?;
    ensure!(
        manifest.target_version == crate::WORKSPACE_VERSION,
        EXIT_SCHEMA,
        "staged migration targets workspace version {}, not {}",
        manifest.target_version,
        crate::WORKSPACE_VERSION
    );
    validate_manifest(&manifest)?;
    let workspace_version = store::declared_workspace_version(root)?.unwrap_or(0);
    ensure!(
        workspace_version == manifest.source_version
            || workspace_version == manifest.target_version,
        EXIT_INVARIANT,
        "staged migration was prepared from workspace version {}, but the workspace is version {}",
        manifest.source_version,
        workspace_version
    );
    if workspace_version == manifest.target_version {
        verify_published_stage(root, &manifest)?;
        fs::remove_dir_all(stage).map_err(|error| tool_error(stage.display(), error))?;
        return Ok(());
    }
    // The Object set the plan was built from is the set of *predecessor
    // projections*, which the manifest names in `source`. It is not the set the
    // plan publishes: an Object whose projection was missing and whose admitted
    // history reconstructs it is legitimately in `objects` and legitimately not
    // on disk yet. Comparing against `objects` made the recovery case preflight
    // explicitly admits impossible to commit.
    //
    // An id may therefore be present now for exactly two reasons: it had a
    // predecessor projection, or an interrupted commit already published it.
    let current_ids = store::object_ids(root)?;
    for id in &current_ids {
        ensure!(
            manifest.objects.contains_key(id),
            EXIT_INVARIANT,
            "{id} appeared after migration preflight"
        );
    }
    for id in manifest
        .source
        .keys()
        .filter_map(|path| staged_object_id(path))
    {
        crate::model::validate_object_id(id).map_err(|error| {
            Error::new(
                EXIT_SCHEMA,
                format!(
                    "staged migration source object id {id:?} is invalid: {}",
                    error.message
                ),
            )
        })?;
        ensure!(
            current_ids.iter().any(|current| current == id),
            EXIT_INVARIANT,
            "{id} disappeared after migration preflight"
        );
    }
    if workspace_version == manifest.source_version {
        let current = source_fingerprint(root)?;
        for path in current.keys() {
            // A file that is in the workspace but not in the plan is either a
            // reconstructed Object an interrupted commit already published, or
            // something that arrived after preflight validated the workspace.
            let published = staged_object_id(path).is_some_and(|id| {
                manifest.objects.contains_key(id) && !manifest.source.contains_key(path)
            });
            ensure!(
                manifest.source.contains_key(path) || published,
                EXIT_INVARIANT,
                "{path} appeared after migration preflight"
            );
        }
        for (path, expected) in &manifest.source {
            let Some(actual) = current.get(path) else {
                return Err(Error::new(
                    EXIT_INVARIANT,
                    format!("{path} disappeared after migration preflight"),
                ));
            };
            let staged_ok =
                staged_object_id(path).and_then(|id| manifest.objects.get(id)) == Some(actual);
            let rewritten = manifest
                .resources
                .get(path)
                .is_some_and(|digest| digest == actual);
            ensure!(
                actual == expected || staged_ok || rewritten,
                EXIT_INVARIANT,
                "{path} changed after migration preflight"
            );
        }
    }
    // One pass, and it keeps what it validated. Reading the staged file a second
    // time to publish it would mean the bytes that were checked and the bytes
    // that get written are two different reads of a file the workspace lock does
    // not make immutable — so the validated artifact and the published artifact
    // are the same value here, held in memory between the two.
    let mut publish: Vec<(PathBuf, String)> = Vec::new();
    let mut closure = RefClosure::new(root);
    for (id, expected) in &manifest.objects {
        let staged = stage.join("objects").join(format!("{id}.json"));
        ensure_regular_file(&staged)?;
        let bytes =
            fs::read_to_string(&staged).map_err(|error| tool_error(staged.display(), error))?;
        ensure!(
            sha256_of(&bytes) == *expected,
            EXIT_INVARIANT,
            "{} changed after migration preflight",
            staged.display()
        );
        let value: serde_json::Value = serde_json::from_str(&bytes)
            .map_err(|error| Error::new(EXIT_SCHEMA, format!("{}: {error}", staged.display())))?;
        let object = store::decode_object(&staged, id, value)?;
        crate::integrity::check_stored_object_integrity(&object)?;
        if workspace_version == manifest.source_version {
            verify_object_derivation(root, &mut closure, id, &bytes, &manifest)?;
        }
        publish.push((store::object_path(root, id), bytes));
    }
    for (relative, expected) in &manifest.resources {
        let resource = retained_resource(relative)?;
        let staged = resource.staged_path(stage);
        ensure_regular_file(&staged)?;
        let bytes =
            fs::read_to_string(&staged).map_err(|error| tool_error(staged.display(), error))?;
        ensure!(
            sha256_of(&bytes) == *expected,
            EXIT_INVARIANT,
            "{} changed after migration preflight",
            staged.display()
        );
        resource.validate_staged(&staged, &bytes, &manifest.objects)?;
        if workspace_version == manifest.source_version {
            resource.verify_derivation(root, relative, &bytes, &manifest)?;
        }
        publish.push((resource.destination(root), bytes));
    }
    for (path, bytes) in publish {
        store::write_text(&path, &bytes)?;
    }
    store::write_workspace_format(root, crate::WORKSPACE_VERSION)?;
    fs::remove_dir_all(stage).map_err(|error| tool_error(stage.display(), error))?;
    ensure!(
        store::validate_format(root)? == WorkspaceFormat::Current,
        EXIT_SCHEMA,
        "workspace migration did not produce the current format"
    );
    Ok(())
}

/// A staged entry has to be a real file, not a link to one.
///
/// The staging directory is inside the repository like everything else, so a
/// symlink placed there would make the digest check read one path and the
/// publication read another.
fn ensure_regular_file(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| tool_error(path.display(), error))?;
    ensure!(
        metadata.file_type().is_file(),
        EXIT_SCHEMA,
        "{} is not a staged file",
        path.display()
    );
    Ok(())
}

fn source_fingerprint(root: &Path) -> Result<BTreeMap<String, String>> {
    let base = store::engr_dir(root);
    let mut result = BTreeMap::new();
    for directory in [
        "objects",
        "events",
        "backlog",
        "collections",
        "work",
        "rules",
    ] {
        let start = base.join(directory);
        if !start.exists() {
            continue;
        }
        let mut pending = vec![start];
        while let Some(path) = pending.pop() {
            for entry in fs::read_dir(&path).map_err(|error| tool_error(path.display(), error))? {
                let entry = entry.map_err(|error| tool_error(path.display(), error))?;
                let file = entry.path();
                if entry
                    .file_type()
                    .map_err(|error| tool_error(file.display(), error))?
                    .is_dir()
                {
                    pending.push(file);
                } else {
                    let relative = relative_to_engr(root, &file)?;
                    let bytes = fs::read_to_string(&file)
                        .map_err(|error| tool_error(file.display(), error))?;
                    result.insert(relative, sha256_of(&bytes));
                }
            }
        }
    }
    Ok(result)
}

// Keep migration-only validation out of the public Content API while still
// applying the same schema rules after replacing legacy refs.
trait MigrationContentValidation {
    fn validate_for_migration(&self) -> Result<()>;
}

impl MigrationContentValidation for crate::model::Content {
    fn validate_for_migration(&self) -> Result<()> {
        let value = serde_json::to_value(self)
            .map_err(|error| Error::new(EXIT_SCHEMA, format!("section content: {error}")))?;
        stored_within_safe_integers(&value, "section content")
    }
}

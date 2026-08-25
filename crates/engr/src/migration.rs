//! Coordinated workspace-generation migration.
//!
//! The predecessor is validated as a whole before one authoritative Object is
//! replaced. A durable staging marker then makes the commit phase resumable:
//! Objects may be copied from the already-validated plan more than once, and
//! `format.json` advances only after every copy succeeds.

use crate::dependency::{self, SemanticField};
use crate::model::{LegacyRef, Object, Provenance, Ref, Section};
use crate::proof::{sha256_of, within_safe_integers};
use crate::semantics::Admission;
use crate::store::{self, WorkspaceFormat};
use crate::{ensure, tool_error, Error, Result, EXIT_INVARIANT, EXIT_SCHEMA, EXIT_USAGE};
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
            EXIT_USAGE,
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

/// Migrate or resume a migration while the caller holds the workspace lock.
pub(crate) fn run(root: &Path) -> Result<()> {
    let stage = stage_dir(root);
    if stage.exists() {
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
    stage_plan(root, source_version, &plan)?;
    commit_stage(root, &stage_dir(root))
}

fn preflight(root: &Path, source_version: u32) -> Result<BTreeMap<String, Object>> {
    let mut predecessor = BTreeMap::new();
    for id in store::object_ids(root)? {
        let path = store::object_path(root, &id);
        let value: serde_json::Value = store::read_json(&path)?;
        within_safe_integers(&value, &path.display().to_string())?;
        let object = store::decode_object_for_version(&path, &id, value, source_version)?;
        check_legacy_object(&object)?;
        predecessor.insert(id, object);
    }

    // Validate every Event record and apply only a recoverable tail before
    // converting representation. No v1 Event is ever replayed into a v3
    // projection after the generation has advanced.
    for id in store::event_ids(root)? {
        let events = store::load_events(root, &id)?;
        for event in &events {
            ensure!(
                event.version == crate::EVENT_ENVELOPE_VERSION_V0
                    && matches!(event.provenance, Provenance::Confirmed { .. }),
                EXIT_SCHEMA,
                "a predecessor workspace carries only Event generation 1"
            );
            ensure_legacy_refs(&event.payload.content.refs)?;
        }
        let stored = predecessor.remove(&id);
        if stored.is_none() {
            ensure!(
                events.first().is_some_and(|event| event.rev == 1
                    && matches!(event.payload.action, crate::model::Action::ObjectCreated)),
                EXIT_SCHEMA,
                "{id}: event rev 1 cannot reconstruct a missing object"
            );
        }
        let (reconciled, _) =
            crate::model::replay_recoverable_tail(Object::new(id.clone(), String::new())?, &events)
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
        predecessor.insert(id, reconciled);
    }

    validate_retained_resources(root, &predecessor)?;

    let mut closure = RefClosure::new(root);
    let mut migrated = BTreeMap::new();
    for (id, mut object) in predecessor {
        object.legacy_format = None;
        object.legacy_version = None;
        object.sections.sort_by_key(|section| section.id);
        let mut sections = Vec::with_capacity(object.sections.len());
        for section in object.sections {
            sections.push(closure.convert_section(section)?);
        }
        object.sections = sections;
        object.sha256 = None;
        object.validate()?;
        let resealed = crate::integrity::seal_migrated(object)?;
        resealed.object.validate()?;
        crate::integrity::check_stored_object_integrity(&resealed.object)?;
        let value = serde_json::to_value(&resealed.object)
            .map_err(|error| Error::new(EXIT_SCHEMA, format!("object {id}: {error}")))?;
        within_safe_integers(&value, &format!("object {id}"))?;
        migrated.insert(id, resealed.object);
    }
    Ok(migrated)
}

fn validate_retained_resources(root: &Path, objects: &BTreeMap<String, Object>) -> Result<()> {
    for id in crate::backlog::ids(root)? {
        validate_json_file(&crate::backlog::item_path(root, &id))?;
        crate::backlog::load(root, &id)?;
    }
    for id in crate::collection::ids(root)? {
        validate_json_file(&crate::collection::path(root, &id))?;
        crate::collection::load(root, &id)?;
    }
    for id in crate::work::ids(root)? {
        validate_json_file(&crate::work::path(root, &id))?;
        crate::work::load_for_migration(root, &id)?;
        ensure!(
            objects.contains_key(&id),
            EXIT_SCHEMA,
            "work sidecar {id} belongs to no Object in the migrated projection"
        );
    }
    crate::rules::load_all_for_migration(root)?;
    Ok(())
}

fn validate_json_file(path: &Path) -> Result<()> {
    let value: serde_json::Value = store::read_json(path)?;
    within_safe_integers(&value, &path.display().to_string())
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

fn stage_dir(root: &Path) -> PathBuf {
    store::engr_dir(root).join(STAGE)
}

fn stage_plan(root: &Path, source_version: u32, objects: &BTreeMap<String, Object>) -> Result<()> {
    let source = source_fingerprint(root)?;
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
    for (id, object) in objects {
        let path = object_dir.join(format!("{id}.json"));
        store::write_json(&path, object)?;
        let bytes = fs::read_to_string(&path).map_err(|error| tool_error(path.display(), error))?;
        digests.insert(id.clone(), sha256_of(&bytes));
    }
    store::write_json(
        &temporary.join(MANIFEST),
        &Manifest {
            source_version,
            target_version: crate::WORKSPACE_VERSION,
            objects: digests,
            source,
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

fn commit_stage(root: &Path, stage: &Path) -> Result<()> {
    let metadata =
        fs::symlink_metadata(stage).map_err(|error| tool_error(stage.display(), error))?;
    ensure!(
        metadata.file_type().is_dir(),
        EXIT_SCHEMA,
        "{} is not a migration staging directory",
        stage.display()
    );
    let manifest: Manifest = store::read_json(&stage.join(MANIFEST))?;
    ensure!(
        manifest.target_version == crate::WORKSPACE_VERSION,
        EXIT_SCHEMA,
        "staged migration targets workspace version {}, not {}",
        manifest.target_version,
        crate::WORKSPACE_VERSION
    );
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
        fs::remove_dir_all(stage).map_err(|error| tool_error(stage.display(), error))?;
        return Ok(());
    }
    let current_ids = store::object_ids(root)?;
    ensure!(
        current_ids.len() == manifest.objects.len()
            && current_ids
                .iter()
                .all(|id| manifest.objects.contains_key(id)),
        EXIT_INVARIANT,
        "the Object set changed after migration preflight"
    );
    if workspace_version == manifest.source_version {
        let current = source_fingerprint(root)?;
        ensure!(
            current.keys().eq(manifest.source.keys()),
            EXIT_INVARIANT,
            "the predecessor workspace resource set changed after migration preflight"
        );
        for (path, expected) in &manifest.source {
            let actual = current.get(path).expect("key sets checked");
            let object_id = path
                .strip_prefix("objects/")
                .and_then(|p| p.strip_suffix(".json"));
            let staged_ok = object_id.and_then(|id| manifest.objects.get(id)) == Some(actual);
            ensure!(
                actual == expected || staged_ok,
                EXIT_INVARIANT,
                "{path} changed after migration preflight"
            );
        }
    }
    for (id, expected) in &manifest.objects {
        let staged = stage.join("objects").join(format!("{id}.json"));
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
    }
    for id in manifest.objects.keys() {
        let staged = stage.join("objects").join(format!("{id}.json"));
        let value: serde_json::Value = store::read_json(&staged)?;
        store::write_json(&store::object_path(root, id), &value)?;
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
                    let relative = file
                        .strip_prefix(&base)
                        .expect("walk starts below .engr")
                        .to_string_lossy()
                        .replace('\\', "/");
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
        within_safe_integers(&value, "section content")
    }
}

use crate::{
    classic_persistence::{self, CacheIdentity, PersistenceError, boundary, list_rows},
    core::ErlangTerm,
    external_term::{self, Limits},
    form_error::ReasonBuilder,
    incremental_warning_cache::CachedWarnings,
    module_dependencies::ModuleDependencies,
    native_form_environment::RawFormTerm,
    plt_tables::CompletedPlt,
    snapshot_file::{Field, Kind, SnapshotFile},
};
use std::path::Path;
type Result<T> = std::result::Result<T, PersistenceError>;
pub struct IncrementalHeader<'a> {
    pub version: &'a str,
    pub module_fingerprints: &'a RawFormTerm,
    pub implementation_fingerprints: &'a RawFormTerm,
    pub repaired_dependencies: &'a ModuleDependencies,
    pub legal_warnings: &'a RawFormTerm,
}
pub struct StoredIncrementalHeader {
    pub version: RawFormTerm,
    pub module_fingerprints: RawFormTerm,
    pub implementation_fingerprints: RawFormTerm,
    pub dependency_record: RawFormTerm,
    pub warning_cache: RawFormTerm,
    pub legal_warnings: RawFormTerm,
}
impl StoredIncrementalHeader {
    pub fn read(file: &SnapshotFile, limits: Limits) -> Result<Self> {
        if file.kind() != Kind::Incremental {
            return Err(boundary("incremental header requires IPLT container"));
        }
        let data = file
            .incremental_data(limits)
            .map_err(|e| boundary(format!("{e:?}")))?
            .raw();
        let ErlangTerm::Tuple(fields) = data.term() else {
            return Err(boundary("incremental_data record expected"));
        };
        if fields.len() != 4 {
            return Err(boundary("incremental_data arity differs"));
        }
        let deps = RawFormTerm::new(data.arena().clone(), fields[1]).map_err(boundary)?;

        let cache = RawFormTerm::new(data.arena().clone(), fields[2]).map_err(boundary)?;
        let legal_warnings = RawFormTerm::new(data.arena().clone(), fields[3]).map_err(boundary)?;
        Ok(Self {
            version: file.field(Field::Version).raw(),
            module_fingerprints: file.field(Field::Fingerprints).raw(),
            implementation_fingerprints: file.field(Field::ImplementationFingerprints).raw(),
            dependency_record: deps,
            warning_cache: cache,
            legal_warnings,
        })
    }
}
fn map(rows: &RawFormTerm) -> Result<RawFormTerm> {
    let mut builder = ReasonBuilder::new();
    let rows = list_rows(rows)?
        .into_iter()
        .map(|row| {
            let ErlangTerm::Tuple(fields) = row.term() else {
                unreachable!()
            };
            (
                builder.copy(
                    &RawFormTerm::new(row.arena().clone(), fields[0]).expect("completed key"),
                ),
                builder.copy(
                    &RawFormTerm::new(row.arena().clone(), fields[1]).expect("completed value"),
                ),
            )
        })
        .collect();
    let root = builder.push(ErlangTerm::Map(rows));
    Ok(builder.finish(root))
}
fn encoded(raw: &RawFormTerm, limits: Limits) -> Result<Vec<u8>> {
    external_term::encode_at(
        raw.arena(),
        raw.id(),
        limits,
        true,
        external_term::MapEncoding::Canonical,
    )
    .map_err(PersistenceError::Transport)
}
pub fn compose(
    completed: CompletedPlt,
    cache: &CachedWarnings,
    header: IncrementalHeader<'_>,
    limits: Limits,
) -> Result<RawFormTerm> {
    if completed.version != header.version {
        return Err(boundary(
            "incremental header differs from compiler Dialyzer ABI",
        ));
    }
    let mut builder = ReasonBuilder::new();
    let mut tables = Vec::new();
    for index in 0..5 {
        let field = completed.field(index);
        let field = if index < 3 { map(&field)? } else { field };
        let bytes = encoded(&field, limits)?;
        tables.push(builder.push(ErlangTerm::Binary(bytes)));
    }
    let deps = header
        .repaired_dependencies
        .union(&completed.dependencies)
        .map_err(PersistenceError::Dependencies)?;
    let dep_dict = deps.to_raw_dict();
    let mut data = ReasonBuilder::new();
    let tag = data.atom("incremental_data");
    let deps = data.copy(&dep_dict);
    let warnings = data.copy(&cache.raw());
    let legal = data.copy(header.legal_warnings);
    let root = data.tuple(vec![tag, deps, warnings, legal]);
    let bytes = encoded(&data.finish(root), limits)?;
    let data = builder.push(ErlangTerm::Binary(bytes));
    let tag = builder.atom("ifile_plt");
    let version = builder.string(header.version);
    let modules = builder.copy(header.module_fingerprints);
    let implementation = builder.copy(header.implementation_fingerprints);
    let root = builder.tuple(vec![
        tag,
        version,
        modules,
        tables[0],
        tables[1],
        tables[2],
        tables[3],
        tables[4],
        data,
        implementation,
    ]);
    Ok(builder.finish(root))
}
pub fn write(
    completed: CompletedPlt,
    cache: &CachedWarnings,
    path: &Path,
    header: IncrementalHeader<'_>,
    limits: Limits,
    identity: &CacheIdentity,
) -> Result<()> {
    let raw = compose(completed, cache, header, limits)?;
    classic_persistence::write_artifact(path, &raw, limits, identity)
}

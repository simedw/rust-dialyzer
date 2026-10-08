use crate::{
    bifs::registry::Mfa,
    core::{ErlangTerm, TermId},
    external_term::{self, Limits},
    form_error::ReasonBuilder,
    module_dependencies::ModuleDependencies,
    native_dict::{NativeDictOrder, PinnedPhashKey, pinned_hash_key},
    native_form_environment::RawFormTerm,
    native_runtime_hash::NativeHashKey,
    snapshot_file,
};
use std::{collections::BTreeMap, path::Path};

#[derive(Debug)]
pub enum PersistenceError {
    Boundary(String),
    Dependencies(crate::module_dependencies::DependencyError),
    Transport(external_term::Error),
    Io(std::io::Error),
}
type Result<T> = std::result::Result<T, PersistenceError>;
pub(crate) fn boundary(message: impl Into<String>) -> PersistenceError {
    PersistenceError::Boundary(message.into())
}
#[derive(Clone, PartialEq, Eq)]
struct DictionaryKey {
    value: NativeHashKey,
    hash: u32,
}
impl PinnedPhashKey for DictionaryKey {
    fn pinned_phash_hash(&self) -> u32 {
        self.hash
    }
}
pub(crate) fn raw_key(raw: &RawFormTerm) -> Result<NativeHashKey> {
    NativeHashKey::from_raw(raw).map_err(|e| boundary(e.to_string()))
}

#[derive(Default)]
struct DictionaryContext {
    arenas: BTreeMap<
        usize,
        (
            std::sync::Arc<crate::core::ErlangTerms>,
            crate::term_order::TermOrderArena,
        ),
    >,
}
impl DictionaryContext {
    fn key(&mut self, raw: &RawFormTerm) -> Result<NativeHashKey> {
        let identity = std::sync::Arc::as_ptr(raw.arena()) as usize;
        if let std::collections::btree_map::Entry::Vacant(entry) = self.arenas.entry(identity) {
            entry.insert((
                raw.arena().clone(),
                crate::term_order::TermOrderArena::new(raw.arena()).map_err(boundary)?,
            ));
        }
        NativeHashKey::from_raw_with_order(raw, &self.arenas[&identity].1)
            .map_err(|e| boundary(e.to_string()))
    }
}
fn dict(
    builder: &mut ReasonBuilder,
    rows: Vec<(RawFormTerm, TermId)>,
    context: &mut DictionaryContext,
) -> Result<TermId> {
    let mut order = NativeDictOrder::default();
    let mut entries = BTreeMap::new();
    for (raw, value) in rows {
        let value_key = context.key(&raw)?;
        let key = DictionaryKey {
            hash: pinned_hash_key(&value_key).map_err(|e| boundary(e.to_string()))?,
            value: value_key.clone(),
        };
        order.store(key);
        entries.insert(value_key, (builder.copy(&raw), value));
    }
    Ok(crate::native_dict::record_term(
        &order,
        builder,
        |key, _| entries[&key.value],
    ))
}
pub fn dictionary_record(rows: Vec<(RawFormTerm, RawFormTerm)>) -> Result<RawFormTerm> {
    let mut builder = ReasonBuilder::new();
    let rows = rows
        .into_iter()
        .map(|(key, value)| (key, builder.copy(&value)))
        .collect();
    let id = dict(&mut builder, rows, &mut DictionaryContext::default())?;
    Ok(builder.finish(id))
}
pub(crate) fn atom_key(name: &str) -> RawFormTerm {
    let mut b = ReasonBuilder::new();
    let id = b.atom(name);
    b.finish(id)
}
pub(crate) fn mfa_key(name: &Mfa) -> RawFormTerm {
    let mut b = ReasonBuilder::new();
    let m = b.atom(&name.module);
    let f = b.atom(&name.function);
    let a = b.integer(name.arity);
    let id = b.tuple(vec![m, f, a]);
    b.finish(id)
}
pub(crate) fn map_rows(raw: &RawFormTerm) -> Result<Vec<(RawFormTerm, RawFormTerm)>> {
    let ErlangTerm::Map(rows) = raw.term() else {
        return Err(boundary("native module tables expected map"));
    };
    rows.iter()
        .map(|(k, v)| {
            Ok((
                RawFormTerm::new(raw.arena().clone(), *k).map_err(boundary)?,
                RawFormTerm::new(raw.arena().clone(), *v).map_err(boundary)?,
            ))
        })
        .collect()
}
pub(crate) fn list_rows(raw: &RawFormTerm) -> Result<Vec<RawFormTerm>> {
    let mut out = Vec::new();
    let mut id = raw.id();
    loop {
        match &raw.arena().terms[id.0] {
            ErlangTerm::Nil => return Ok(out),
            ErlangTerm::List { items, tail } => {
                for x in items {
                    out.push(RawFormTerm::new(raw.arena().clone(), *x).map_err(boundary)?);
                }
                id = *tail;
            }
            _ => return Err(boundary("native collection expected proper list")),
        }
    }
}
fn validate_fingerprints(raw: &RawFormTerm) -> Result<()> {
    for row in list_rows(raw)? {
        let ErlangTerm::Tuple(fields) = row.term() else {
            return Err(boundary("fingerprint row is not a pair"));
        };
        if fields.len() != 2 {
            return Err(boundary("fingerprint row is not a pair"));
        }
        let name = RawFormTerm::new(row.arena().clone(), fields[0]).map_err(boundary)?;
        for codepoint in list_rows(&name)? {
            let ErlangTerm::Integer(value) = codepoint.term() else {
                return Err(boundary("fingerprint filename is not a character list"));
            };
            let value = value.parse::<u32>().ok().and_then(char::from_u32);
            if value.is_none() {
                return Err(boundary(
                    "fingerprint filename outside compiler Unicode schema",
                ));
            }
        }
        if !matches!(&row.arena().terms[fields[1].0],ErlangTerm::Binary(bytes) if bytes.len()==16) {
            return Err(boundary("fingerprint digest is not MD5"));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheIdentity(String);
impl CacheIdentity {
    pub fn from_implementation_sha256(digest: &str) -> Result<Self> {
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(boundary("implementation identity must be lowercase SHA256"));
        }
        Ok(Self(digest.into()))
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
struct CacheManifest {
    schema: u32,
    implementation_sha256: String,
    plt_md5: [u8; 16],
}
fn manifest_path(path: &Path) -> std::path::PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".rust-cache.json");
    name.into()
}
pub fn read_owned_bytes(path: &Path, identity: &CacheIdentity) -> Result<Vec<u8>> {
    let bytes = std::fs::read(path).map_err(PersistenceError::Io)?;
    let manifest = std::fs::read(manifest_path(path)).map_err(PersistenceError::Io)?;
    let manifest: CacheManifest =
        serde_json::from_slice(&manifest).map_err(|_| boundary("invalid Rust cache manifest"))?;
    if manifest.schema != 1
        || manifest.implementation_sha256 != identity.0
        || manifest.plt_md5 != crate::fingerprint::md5(&bytes)
    {
        return Err(boundary("Rust cache identity or artifact digest mismatch"));
    }
    Ok(bytes)
}
pub fn file_fingerprints(paths: &[std::path::PathBuf], limits: Limits) -> Result<RawFormTerm> {
    let mut rows = Vec::new();
    for path in paths {
        let name = path
            .to_str()
            .ok_or_else(|| boundary("compiler BEAM path is not Unicode"))?;
        let bytes = std::fs::read(path).map_err(PersistenceError::Io)?;
        let digest = crate::fingerprint::classic_beam_fingerprint(&bytes, limits.bytes)
            .map_err(|e| boundary(format!("{e:?}")))?;
        rows.push((name, digest));
    }
    rows.sort_unstable_by(|a, b| a.0.cmp(b.0));
    let mut builder = ReasonBuilder::new();
    let rows = rows
        .into_iter()
        .map(|(name, digest)| {
            let name = builder.string(name);
            let digest = builder.push(ErlangTerm::Binary(digest.to_vec()));
            builder.tuple(vec![name, digest])
        })
        .collect();
    let id = builder.list(rows);
    Ok(builder.finish(id))
}
/// Native-compatible classic header, supplied by the compiler/check lifecycle.
pub struct ClassicHeader<'a> {
    pub version: &'a str,
    pub file_fingerprints: &'a RawFormTerm,
    pub implementation_fingerprints: &'a RawFormTerm,
    pub repaired_dependencies: &'a ModuleDependencies,
}

pub fn compose(
    completed: crate::plt_tables::CompletedPlt,
    header: ClassicHeader<'_>,
) -> Result<RawFormTerm> {
    if header.version != completed.version {
        return Err(boundary(
            "classic header differs from compiler Dialyzer ABI",
        ));
    }
    validate_fingerprints(header.file_fingerprints)?;
    validate_fingerprints(header.implementation_fingerprints)?;
    let mut builder = ReasonBuilder::new();
    let mut projected = Vec::new();
    let mut context = DictionaryContext::default();
    for index in 0..4 {
        let mut entries = Vec::new();
        for row in list_rows(&completed.field(index))? {
            let ErlangTerm::Tuple(pair) = row.term() else {
                unreachable!()
            };
            let key = RawFormTerm::new(row.arena().clone(), pair[0]).map_err(boundary)?;
            let value = RawFormTerm::new(row.arena().clone(), pair[1]).map_err(boundary)?;
            let value = if index == 3 {
                let rows = map_rows(&value)?
                    .into_iter()
                    .map(|(key, value)| (key, builder.copy(&value)))
                    .collect();
                dict(&mut builder, rows, &mut context)?
            } else {
                builder.copy(&value)
            };
            entries.push((key, value));
        }
        projected.push(dict(&mut builder, entries, &mut context)?);
    }
    let exports = builder.copy(&completed.field(4));
    let dependencies = header
        .repaired_dependencies
        .union(&completed.dependencies)
        .map_err(PersistenceError::Dependencies)?;
    let deps = builder.copy(&dependencies.to_raw_dict());
    let tag = builder.atom("file_plt");
    let version = builder.string(header.version);
    let files = builder.copy(header.file_fingerprints);
    let implementation = builder.copy(header.implementation_fingerprints);
    let root = builder.tuple(vec![
        tag,
        version,
        files,
        projected[0],
        projected[1],
        projected[2],
        projected[3],
        exports,
        deps,
        implementation,
    ]);
    Ok(builder.finish(root))
}
pub fn write(
    completed: crate::plt_tables::CompletedPlt,
    path: &Path,
    header: ClassicHeader<'_>,
    limits: Limits,
    identity: &CacheIdentity,
) -> Result<()> {
    let raw = compose(completed, header)?;
    write_artifact(path, &raw, limits, identity)
}
pub(crate) fn write_artifact(
    path: &Path,
    raw: &RawFormTerm,
    limits: Limits,
    identity: &CacheIdentity,
) -> Result<()> {
    let bytes =
        external_term::encode(raw.arena(), limits, true).map_err(PersistenceError::Transport)?;
    let manifest = CacheManifest {
        schema: 1,
        implementation_sha256: identity.0.clone(),
        plt_md5: crate::fingerprint::md5(&bytes),
    };
    let manifest = serde_json::to_vec(&manifest).map_err(|e| boundary(e.to_string()))?;
    snapshot_file::atomic_write(path, &bytes).map_err(PersistenceError::Io)?;
    snapshot_file::atomic_write(&manifest_path(path), &manifest).map_err(PersistenceError::Io)
}

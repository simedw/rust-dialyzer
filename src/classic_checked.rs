use crate::{
    core::ErlangTerm,
    form_error::ReasonBuilder,
    module_dependencies::ModuleDependencies,
    native_error::NativeTransferError,
    native_form_environment::RawFormTerm,
    native_map_order,
    native_runtime_hash::{NativeAtomLayout, NativeHashKey, RuntimeHashError},
    raw_literal::{RawLiteral, RawLiteralArena},
};
use std::{cmp::Ordering, collections::BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckError {
    Native(NativeTransferError),
    Dependency(crate::module_dependencies::DependencyError),
    NoFileToRemove(RawLiteral),
    DialyzerError(String),
    RuntimeOrder(RuntimeHashError),
    Boundary(String),
    ResourceLimit,
}
type Result<T> = std::result::Result<T, CheckError>;
fn native(error: NativeTransferError) -> CheckError {
    CheckError::Native(error)
}
fn literal(raw: RawFormTerm) -> RawLiteral {
    RawLiteral::new(raw).expect("constructed literal")
}
fn binary(bytes: &[u8]) -> RawLiteral {
    let mut b = ReasonBuilder::new();
    let id = b.push(ErlangTerm::Binary(bytes.to_vec()));
    literal(b.finish(id))
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FingerprintRow {
    pub file: RawLiteral,
    pub digest: RawLiteral,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FingerprintTable(Vec<FingerprintRow>);
impl FingerprintTable {
    pub fn from_raw(raw: &RawFormTerm) -> Result<Self> {
        let literals = RawLiteralArena::new(raw.arena().clone()).map_err(CheckError::Boundary)?;
        let mut rows = Vec::new();
        let mut cursor = raw.id();
        loop {
            match &raw.arena().terms[cursor.0] {
                ErlangTerm::Nil => return Ok(Self(rows)),
                ErlangTerm::List { items, tail } => {
                    for &id in items {
                        let ErlangTerm::Tuple(fields) = &raw.arena().terms[id.0] else {
                            return Err(native(NativeTransferError::FunctionClause));
                        };
                        if fields.len() != 2 {
                            return Err(native(NativeTransferError::FunctionClause));
                        }
                        rows.push(FingerprintRow {
                            file: literals.at(fields[0]).map_err(CheckError::Boundary)?,
                            digest: literals.at(fields[1]).map_err(CheckError::Boundary)?,
                        });
                    }
                    cursor = *tail;
                }
                _ => return Err(native(NativeTransferError::FunctionClause)),
            }
        }
    }
    pub fn rows(&self) -> &[FingerprintRow] {
        &self.0
    }
    pub fn to_raw(&self) -> RawFormTerm {
        let mut b = ReasonBuilder::new();
        let rows = self
            .0
            .iter()
            .map(|row| {
                let file = b.copy(row.file.raw());
                let digest = b.copy(row.digest.raw());
                b.tuple(vec![file, digest])
            })
            .collect();
        let id = b.list(rows);
        b.finish(id)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Differ(RawLiteral),
    Removed(RawLiteral),
}
impl Change {
    pub fn module(&self) -> &RawLiteral {
        match self {
            Self::Differ(m) | Self::Removed(m) => m,
        }
    }
}
pub struct CheckedHeader<'a> {
    pub version: &'a RawLiteral,
    pub files: &'a RawFormTerm,
    pub implementation: &'a RawFormTerm,
    pub dependencies: &'a RawFormTerm,
}
#[derive(Clone, Debug)]
pub enum CheckOutcome {
    UpToDate {
        files: FingerprintTable,
        dependencies: RawFormTerm,
    },
    OldVersion {
        files: FingerprintTable,
    },
    Differ {
        files: FingerprintTable,
        changes: Vec<Change>,
        dependencies: RawFormTerm,
    },
}
#[derive(Clone, Debug)]
pub enum RepairPlan {
    Reuse {
        files: FingerprintTable,
        dependencies: RawFormTerm,
    },
    Rebuild {
        files: FingerprintTable,
        analyze_files: Vec<RawLiteral>,
    },
    Repair {
        files: FingerprintTable,
        analyze_files: Vec<RawLiteral>,
        purge_modules: BTreeSet<RawLiteral>,
        dependencies: ModuleDependencies,
    },
}
impl CheckOutcome {
    pub fn into_plan(self) -> Result<RepairPlan> {
        Ok(match self {
            Self::UpToDate {
                files,
                dependencies,
            } => RepairPlan::Reuse {
                files,
                dependencies,
            },
            Self::OldVersion { files } => {
                let analyze_files = files.0.iter().map(|row| row.file.clone()).collect();
                RepairPlan::Rebuild {
                    files,
                    analyze_files,
                }
            }
            Self::Differ {
                files,
                changes,
                dependencies,
            } => {
                let dependencies = ModuleDependencies::from_raw_dict(&dependencies)
                    .map_err(CheckError::Dependency)?;
                let (analyze_files, purge_modules, dependencies) =
                    expand(&files, &changes, &dependencies)?;
                RepairPlan::Repair {
                    files,
                    analyze_files,
                    purge_modules,
                    dependencies,
                }
            }
        })
    }
}
pub fn check(
    header: CheckedHeader<'_>,
    current_version: &RawLiteral,
    remove: &[RawLiteral],
    add: &[RawLiteral],
    layout: Option<&NativeAtomLayout>,
    mut fingerprint: impl FnMut(&RawLiteral) -> Result<[u8; 16]>,
) -> Result<CheckOutcome> {
    let current = if header.version == current_version {
        compute_new(
            &FingerprintTable::from_raw(header.implementation)?,
            &[],
            &[],
            layout,
            &mut fingerprint,
        )?
        .is_none()
    } else {
        false
    };
    let files = FingerprintTable::from_raw(header.files)?;
    match compute_new(&files, remove, add, layout, &mut fingerprint)? {
        None if current => Ok(CheckOutcome::UpToDate {
            files: files.clone(),
            dependencies: header.dependencies.clone(),
        }),
        None => Ok(CheckOutcome::OldVersion {
            files: files.clone(),
        }),
        Some((files, _changes)) if !current => Ok(CheckOutcome::OldVersion { files }),
        Some((files, changes)) => Ok(CheckOutcome::Differ {
            files,
            changes,
            dependencies: header.dependencies.clone(),
        }),
    }
}
fn subtract(left: &[RawLiteral], right: &[RawLiteral]) -> Vec<RawLiteral> {
    let mut left = left.to_vec();
    for item in right {
        if let Some(index) = left.iter().position(|value| value == item) {
            left.remove(index);
        }
    }
    left
}
fn removed_order(
    modules: BTreeSet<RawLiteral>,
    layout: Option<&NativeAtomLayout>,
) -> Result<Vec<RawLiteral>> {
    if modules.len() <= 1 {
        return Ok(modules.into_iter().collect());
    }
    let layout = layout.ok_or(CheckError::RuntimeOrder(
        RuntimeHashError::UnavailableAtomLayout,
    ))?;
    let keys = modules
        .iter()
        .map(|m| NativeHashKey::from_raw(m.raw()))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(CheckError::RuntimeOrder)?;
    native_map_order::ordered_keys(layout, keys)
        .map_err(CheckError::RuntimeOrder)?
        .into_iter()
        .map(|key| {
            let raw = key.literal();
            RawLiteral::from_arena(std::sync::Arc::new(raw.clone()), raw.root)
                .map_err(CheckError::Boundary)
        })
        .collect()
}
fn compute_new(
    old: &FingerprintTable,
    remove: &[RawLiteral],
    add: &[RawLiteral],
    layout: Option<&NativeAtomLayout>,
    fingerprint: &mut impl FnMut(&RawLiteral) -> Result<[u8; 16]>,
) -> Result<Option<(FingerprintTable, Vec<Change>)>> {
    let (mut files, mut changes) =
        if remove.is_empty() && add.is_empty() {
            (old.0.clone(), Vec::new())
        } else {
            let remove_only = subtract(remove, add);
            let add_only = subtract(add, remove);
            let remove_modules = remove_only
                .iter()
                .map(path_to_mod)
                .collect::<Result<BTreeSet<_>>>()?;
            let add_modules = add_only
                .iter()
                .map(path_to_mod)
                .collect::<Result<BTreeSet<_>>>()?;
            let removed = remove_modules.difference(&add_modules).cloned().collect();
            let changes = removed_order(removed, layout)?
                .into_iter()
                .map(Change::Removed)
                .collect();
            let mut edits = remove_only
                .into_iter()
                .map(|file| (false, file))
                .chain(add_only.into_iter().map(|file| (true, file)))
                .collect::<Vec<_>>();
            edits.sort_by(|a, b| a.1.normal_cmp(&b.1));
            let mut sorted = old.0.clone();
            sorted.sort_by(|a, b| a.file.normal_cmp(&b.file));
            let mut result = Vec::new();
            let (mut i, mut j) = (0, 0);
            while i < sorted.len() && j < edits.len() {
                let (add, file) = &edits[j];
                if sorted[i].file == *file {
                    if *add {
                        result.push(sorted[i].clone());
                    }
                    i += 1;
                    j += 1;
                } else if sorted[i].file.normal_cmp(file) == Ordering::Less {
                    result.push(sorted[i].clone());
                    i += 1;
                } else {
                    if sorted[i].file.normal_cmp(file) != Ordering::Greater {
                        return Err(native(NativeTransferError::BadMatchBoolean(false)));
                    }
                    if *add {
                        result.push(FingerprintRow {
                            file: file.clone(),
                            digest: binary(&[]),
                        });
                    } else {
                        return Err(CheckError::NoFileToRemove(file.clone()));
                    }
                    j += 1;
                }
            }
            result.extend(sorted[i..].iter().cloned());
            result.extend(edits[j..].iter().filter(|(add, _)| *add).map(|(_, file)| {
                FingerprintRow {
                    file: file.clone(),
                    digest: binary(&[]),
                }
            }));
            (result, changes)
        };
    for row in &mut files {
        let current = binary(&fingerprint(&row.file)?);
        if current != row.digest {
            row.digest = current;
            changes.insert(0, Change::Differ(path_to_mod(&row.file)?));
        }
    }
    if changes.is_empty() {
        Ok(None)
    } else {
        // compute_new_md5_1 prepends entries before its stable keysort.
        files.reverse();
        files.sort_by(|a, b| a.file.normal_cmp(&b.file));
        Ok(Some((FingerprintTable(files), changes)))
    }
}
pub fn expand(
    files: &FingerprintTable,
    changes: &[Change],
    dependencies: &ModuleDependencies,
) -> Result<(Vec<RawLiteral>, BTreeSet<RawLiteral>, ModuleDependencies)> {
    let purge = changes
        .iter()
        .map(|c| c.module().clone())
        .collect::<BTreeSet<_>>();
    let removed = changes
        .iter()
        .filter_map(|c| {
            if let Change::Removed(m) = c {
                Some(m.clone())
            } else {
                None
            }
        })
        .collect::<BTreeSet<_>>();
    let mut included = purge.clone();
    let mut pending = purge.iter().cloned().collect::<Vec<_>>();
    while let Some(module) = pending.pop() {
        if let Some(dependents) = dependencies
            .set_members(&module)
            .map_err(CheckError::Dependency)?
        {
            for dependent in dependents {
                if included.insert(dependent.clone()) {
                    pending.push(dependent.clone());
                }
            }
        }
    }
    let repaired = dependencies
        .retaining_values(|module| !purge.contains(module))
        .map_err(CheckError::Dependency)?;
    let mut analyze = Vec::new();
    for row in &files.0 {
        let module = path_to_mod(&row.file)?;
        if included.contains(&module) && !removed.contains(&module) {
            analyze.push(row.file.clone());
        }
    }
    Ok((analyze, purge, repaired))
}
fn flatten(value: &RawLiteral, out: &mut Vec<RawLiteral>) -> Result<()> {
    match value.raw().term() {
        ErlangTerm::Atom(name) => {
            for c in name.chars() {
                let mut b = ReasonBuilder::new();
                let id = b.integer(c as u32);
                out.push(literal(b.finish(id)));
            }
            Ok(())
        }
        ErlangTerm::Nil => Ok(()),
        ErlangTerm::List { items, tail } => {
            for &id in items {
                let item = value.at(id).map_err(CheckError::Boundary)?;
                match item.raw().term() {
                    ErlangTerm::List { .. } | ErlangTerm::Nil | ErlangTerm::Atom(_) => {
                        flatten(&item, out)?
                    }
                    _ => out.push(item),
                }
            }
            flatten(&value.at(*tail).map_err(CheckError::Boundary)?, out)
        }
        _ => Err(native(NativeTransferError::FunctionClause)),
    }
}
pub fn path_to_mod(file: &RawLiteral) -> Result<RawLiteral> {
    if matches!(file.raw().term(), ErlangTerm::Binary(_))
        || matches!(file.raw().term(), ErlangTerm::Bitstring { bits, .. } if bits % 8 == 0)
    {
        return Err(native(NativeTransferError::Badarg));
    }
    let mut chars = Vec::new();
    flatten(file, &mut chars)?;
    let extension = [46, 98, 101, 97, 109];
    let mut name = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let suffix = &chars[index..];
        if suffix.len() == extension.len()
            && suffix
                .iter()
                .zip(extension)
                .all(|(v, c)| matches!(v.raw().term(),ErlangTerm::Integer(x) if x==&c.to_string()))
        {
            break;
        }
        match chars[index].raw().term() {
            ErlangTerm::Integer(x) if x == "47" => {
                if index + 1 != chars.len() {
                    name.clear();
                }
            }
            ErlangTerm::Integer(x) => name.push(x.clone()),
            _ => return Err(native(NativeTransferError::FunctionClause)),
        }
        index += 1;
    }
    if name.len() > 255 {
        return Err(native(NativeTransferError::SystemLimit));
    }
    let name = name
        .iter()
        .map(|x| {
            x.parse::<u32>()
                .ok()
                .and_then(char::from_u32)
                .ok_or_else(|| native(NativeTransferError::Badarg))
        })
        .collect::<Result<String>>()?;
    Ok(RawLiteral::atom(&name))
}
pub fn candidate_layout_atoms(
    files: &RawFormTerm,
    implementation: &RawFormTerm,
    remove: &[RawLiteral],
    add: &[RawLiteral],
) -> Result<BTreeSet<String>> {
    let mut candidates = remove.iter().chain(add).cloned().collect::<Vec<_>>();
    for table in [files, implementation] {
        let literals = RawLiteralArena::new(table.arena().clone()).map_err(CheckError::Boundary)?;
        let mut cursor = table.id();
        while let ErlangTerm::List { items, tail } = &table.arena().terms[cursor.0] {
            for &id in items {
                if let ErlangTerm::Tuple(fields) = &table.arena().terms[id.0]
                    && let Some(&file) = fields.first()
                {
                    candidates.push(literals.at(file).map_err(CheckError::Boundary)?);
                }
            }
            cursor = *tail;
        }
    }
    Ok(candidates
        .iter()
        .filter_map(|file| path_to_mod(file).ok())
        .filter_map(|module| module.as_atom().map(str::to_owned))
        .collect())
}

pub fn fingerprint_file(file: &RawLiteral, max_expanded_bytes: usize) -> Result<[u8; 16]> {
    let binary_input = match file.raw().term() {
        ErlangTerm::Binary(bytes) => Some(bytes.clone()),
        ErlangTerm::Bitstring { bytes, bits } if bits % 8 == 0 => {
            Some(bytes[..(*bits as usize / 8)].to_vec())
        }
        _ => None,
    };
    let (bytes, display) = if let Some(bytes) = binary_input {
        (bytes.clone(), String::from_utf8(bytes).ok())
    } else {
        let mut chars = Vec::new();
        flatten(file, &mut chars)?;
        let display = chars
            .iter()
            .map(|c| match c.raw().term() {
                ErlangTerm::Integer(x) => x.parse::<u32>().ok().and_then(char::from_u32),
                _ => None,
            })
            .collect::<Option<String>>()
            .ok_or_else(|| native(NativeTransferError::Badarg))?;
        let path = format!("{}.beam", display.strip_suffix(".beam").unwrap_or(&display));
        match std::fs::read(path) {
            Ok(bytes) => (bytes, Some(display)),
            Err(error) => {
                let message = if error.kind() == std::io::ErrorKind::NotFound {
                    format!("File not found: {display}\n")
                } else {
                    format!("Could not compute MD5 for .beam: {display}\n")
                };
                return Err(CheckError::DialyzerError(message));
            }
        }
    };
    crate::fingerprint::classic_beam_fingerprint(&bytes, max_expanded_bytes).map_err(|error| {
        if matches!(
            error,
            crate::fingerprint::ClassicFingerprintError::Transport(
                crate::external_term::Error::ResourceLimit
            )
        ) {
            return CheckError::ResourceLimit;
        }
        match display {
            Some(display) => {
                CheckError::DialyzerError(format!("Could not compute MD5 for .beam: {display}\n"))
            }
            None => native(NativeTransferError::Badarg),
        }
    })
}

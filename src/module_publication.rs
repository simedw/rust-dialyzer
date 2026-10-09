pub use crate::module_dependencies::ModuleDependencies;
use crate::{
    compiled_definitions::{PreparedContract, PreparedDefinitions},
    core::{CoreProgram, ErlangTerm, TermId},
    form_error::FormError,
    native_error::NativeTransferError,
    program::{ModuleId, Program},
    type_exports::{self, ExportError},
};
use crate::{
    native_map_order,
    native_runtime_hash::NativeHashKey,
    raw_literal::{RawLiteral, normal_usort},
};

#[derive(Debug)]
pub enum PublicationError {
    Boundary(String),
    Native(NativeTransferError),
    Export(ExportError),
    Form(FormError),
    Dependencies(crate::module_dependencies::DependencyError),
}
impl From<ExportError> for PublicationError {
    fn from(e: ExportError) -> Self {
        Self::Export(e)
    }
}
type Result<T> = std::result::Result<T, PublicationError>;
pub struct PublicationInput<'a> {
    pub core: &'a CoreProgram,
    pub program: &'a Program,
    pub definitions: &'a PreparedDefinitions,
    pub targets: &'a [ModuleId],
    pub use_contracts: bool,
}
fn tuple(terms: &[ErlangTerm], id: TermId) -> Result<&[TermId]> {
    match &terms[id.0] {
        ErlangTerm::Tuple(xs) => Ok(xs),
        _ => Err(PublicationError::Boundary(
            "prepared contract tuple missing".into(),
        )),
    }
}
fn list(terms: &[ErlangTerm], mut id: TermId) -> Result<Vec<TermId>> {
    let mut out = Vec::new();
    loop {
        match &terms[id.0] {
            ErlangTerm::Nil => return Ok(out),
            ErlangTerm::List { items, tail } => {
                out.extend(items);
                id = *tail;
            }
            _ => {
                return Err(PublicationError::Boundary(
                    "prepared form list missing".into(),
                ));
            }
        }
    }
}
fn contract_dependencies(
    contract: &PreparedContract,
    arenas: &mut type_exports::LiteralArenas,
) -> Result<Vec<RawLiteral>> {
    let raw = &contract.capture;
    let terms = &raw.arena().terms;
    let fields = tuple(terms, raw.id())?;
    if fields.len() != 4 || !matches!(&terms[fields[0].0],ErlangTerm::Atom(a) if a=="contract") {
        return Err(PublicationError::Boundary(
            "prepared contract record missing".into(),
        ));
    }
    let mut pairs = Vec::new();
    for id in list(terms, fields[3])? {
        let pair = tuple(terms, id)?;
        if pair.len() != 2 {
            return Err(PublicationError::Boundary(
                "prepared contract form pair missing".into(),
            ));
        }
        pairs.push((pair[0], pair[1]));
    }
    // Source constructs both comprehensions before traversing any type forms.
    for (_, constraints) in &pairs {
        list(terms, *constraints)?;
    }
    let forms = pairs.iter().map(|(form, _)| *form).collect::<Vec<_>>();
    let mut dependencies =
        type_exports::type_forms_to_remote_modules_in(arenas, raw.arena(), &forms)?;
    if crate::lattice::verify_fast_paths() {
        assert_eq!(
            dependencies,
            type_exports::type_forms_to_remote_modules(raw.arena(), &forms)?,
            "shared literal arena changed contract form dependencies"
        );
    }
    for (_, constraints) in pairs {
        dependencies.extend(type_exports::constraint_form_to_remote_modules_in(
            arenas,
            raw.arena(),
            constraints,
        )?);
    }
    Ok(normal_usort(dependencies))
}
impl PublicationInput<'_> {
    fn ordered_keys(&self, keys: Vec<NativeHashKey>) -> Result<Vec<NativeHashKey>> {
        if keys.len() <= 1 {
            return Ok(keys);
        }
        let layout = self.core.atom_layout.as_ref().ok_or_else(|| {
            PublicationError::Boundary("compiler atom layout missing for map traversal".into())
        })?;
        native_map_order::ordered_keys(layout, keys)
            .map_err(|e| PublicationError::Boundary(e.to_string()))
    }
    pub fn type_dependencies(&self) -> Result<ModuleDependencies> {
        // Original module_type_deps obtains every behaviour list before its collector runs.
        let mut behaviour_dependencies = Vec::new();
        // Behaviour metadata grouped once instead of scanning all metadata per module.
        let mut behaviours_by_module: std::collections::BTreeMap<_, Vec<_>> =
            std::collections::BTreeMap::new();
        for metadata in &self.program.metadata {
            if let Some(behaviour) = metadata.behaviour.as_ref() {
                behaviours_by_module.entry(metadata.module).or_default().push(behaviour);
            }
        }
        for &target in self.targets {
            let module = self.core.modules.get(target.0).ok_or_else(|| {
                PublicationError::Boundary("target outside compiler input".into())
            })?;
            if self.definitions.module_names.get(target.0) != Some(&module.name) {
                return Err(PublicationError::Boundary(
                    "prepared module identity differs from compiler input".into(),
                ));
            }
            behaviour_dependencies.push((
                module.name.clone(),
                crate::behaviour_metadata::collect(
                    behaviours_by_module
                        .get(&target)
                        .into_iter()
                        .flatten()
                        .copied(),
                )
                .map_err(|error| match error {
                    FormError::Native(error) => PublicationError::Native(error),
                    other => PublicationError::Form(other),
                })?
                .into_iter()
                .map(|occurrence| occurrence.name)
                .collect::<Vec<_>>(),
            ));
        }
        let mut rows = Vec::new();
        // Contract keys and forms of a module share its arena; its term order
        // is built once instead of for every key and form read from it.
        let mut arenas = type_exports::LiteralArenas::default();
        for (enabled, table) in [
            (self.use_contracts, &self.definitions.contracts),
            (true, &self.definitions.callbacks),
        ] {
            if !enabled {
                continue;
            }
            let entries = table
                .raw_entries()
                .map(|e| {
                    let raw = e.key.raw();
                    Ok((
                        arenas
                            .arena(raw.arena())
                            .and_then(|literals| literals.at(raw.id()))
                            .map_err(PublicationError::Boundary)?,
                        e,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            let keys = entries
                .iter()
                .map(|(_, e)| {
                    let raw = e.key.raw();
                    arenas
                        .order(raw.arena())
                        .map_err(crate::native_runtime_hash::RuntimeHashError::Boundary)
                        .and_then(|order| NativeHashKey::from_raw_with_order(raw, order))
                        .map_err(|e| PublicationError::Boundary(e.to_string()))
                })
                .collect::<Result<Vec<_>>>()?;
            if crate::lattice::verify_fast_paths() {
                for ((literal, e), key) in entries.iter().zip(&keys) {
                    let raw = e.key.raw();
                    assert!(
                        RawLiteral::new(raw.clone()).is_ok_and(|fresh| &fresh == literal)
                            && NativeHashKey::from_raw(raw).is_ok_and(|fresh| &fresh == key),
                        "shared literal arena changed a declaration key"
                    );
                }
            }
            // First entry per key, as the linear search over entries returned.
            let mut first_index = std::collections::BTreeMap::new();
            for (index, key) in keys.iter().enumerate() {
                first_index.entry(key.clone()).or_insert(index);
            }
            let ordered = self.ordered_keys(keys)?;
            for key in ordered {
                let entry = first_index
                    .get(&key)
                    .map(|index| entries[*index].1)
                    .ok_or_else(|| {
                        PublicationError::Boundary("ordered declaration row missing".into())
                    })?;
                if crate::lattice::verify_fast_paths() {
                    let literal = key.literal();
                    let root = literal.root;
                    let raw = RawLiteral::from_arena(std::sync::Arc::new(literal), root)
                        .map_err(PublicationError::Boundary)?;
                    let linear = entries.iter().find(|(candidate, _)| candidate == &raw).map(|(_, e)| *e);
                    assert!(linear.is_some_and(|e| std::ptr::eq(e, entry)), "publication index differs from linear search");
                }
                let raw = entry.key.raw();
                let ErlangTerm::Tuple(fields) = raw.term() else {
                    unreachable!()
                };
                rows.push((
                    arenas
                        .arena(raw.arena())
                        .and_then(|literals| literals.at(fields[0]))
                        .map_err(PublicationError::Boundary)?,
                    contract_dependencies(&entry.value, &mut arenas)?,
                ));
            }
        }
        // Each module table is read once; release the contract arenas.
        drop(arenas);
        let ordered_modules = self.ordered_keys(
            self.targets
                .iter()
                .map(|target| NativeHashKey::Atom(self.core.modules[target.0].name.clone()))
                .collect(),
        )?;
        for key in &ordered_modules {
            let NativeHashKey::Atom(name) = key else {
                unreachable!()
            };
            let target = *self
                .targets
                .iter()
                .rev()
                .find(|target| &self.core.modules[target.0].name == name)
                .ok_or_else(|| {
                    PublicationError::Boundary("ordered target module missing".into())
                })?;
            if let Some(table) = self.definitions.module_table(target) {
                rows.push((
                    RawLiteral::atom(name),
                    type_exports::module_type_deps_of_type_defs(
                        table.shared_arena(),
                        table.root(),
                    )?,
                ));
            }
        }
        for key in ordered_modules {
            let NativeHashKey::Atom(name) = key else {
                unreachable!()
            };
            let (_, deps) = behaviour_dependencies
                .iter()
                .rev()
                .find(|(module, _)| module == &name)
                .ok_or_else(|| {
                    PublicationError::Boundary("ordered behaviour module missing".into())
                })?;
            rows.push((RawLiteral::atom(&name), deps.clone()));
        }
        Ok(ModuleDependencies::collect_type_rows(rows))
    }
    pub fn publish(
        &self,
        call_dependencies: &std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
    ) -> Result<ModuleDependencies> {
        ModuleDependencies::from_named_call_dependencies(call_dependencies)
            .union(&self.type_dependencies()?)
            .map_err(PublicationError::Dependencies)
    }
}

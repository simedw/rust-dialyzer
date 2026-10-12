use crate::{
    bifs::registry::{Mfa, Registry},
    binder::{BindError, Binder, BinderState, OpaqueMatch, WarningLocation},
    call_transfer::Signature,
    callback_warnings::{
        self, BehaviourOccurrence, CallbackDeclaration, CallbackWarningModule, DeclaredCallback,
    },
    compiled_definitions::{PreparedContract, PreparedDefinitions},
    contract_warnings::{self, ContractWarningEntry, ContractWarningModule},
    core::{ErlangTerm, TermId},
    dataflow_nodes::DataflowNodes,
    dependencies::{CallbackLookupError, Store},
    form_context::FormContext,
    form_error::{FormError, ReasonBuilder},
    lattice::{PresentationContext, PresentationRecordField},
    native_ets_set::NativeEtsSet,
    native_form_environment::RawFormTerm,
    native_map_order,
    native_runtime_hash::{NativeAtomLayout, NativeHashKey, RuntimeHashError},
    patterns::{PatternRef, Patterns},
    program::{FunctionIdentity, ModuleId, NodeRef, Program},
    source_callgraph::SourceCallGraph,
    success_phase::FunctionTypes,
    warning_payload::NativeWarning,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
#[derive(Debug)]
pub enum DiagnosticInputError {
    Boundary(String),
    Ordering(RuntimeHashError),
    LibraryCallbacks(CallbackLookupError),
    Form(FormError),
    Bind(BindError),
    CallbackPreparationMissing(ModuleId),
}
impl From<FormError> for DiagnosticInputError {
    fn from(e: FormError) -> Self {
        Self::Form(e)
    }
}
impl From<RuntimeHashError> for DiagnosticInputError {
    fn from(e: RuntimeHashError) -> Self {
        Self::Ordering(e)
    }
}
type Result<T> = std::result::Result<T, DiagnosticInputError>;
fn ordered_raw<'a>(
    entries: impl Iterator<Item = &'a crate::compiled_definitions::PreparedDeclaration>,
    layout: &NativeAtomLayout,
) -> Result<Vec<&'a crate::compiled_definitions::PreparedDeclaration>> {
    let entries = entries
        .map(|entry| Ok((NativeHashKey::from_raw(entry.key.raw())?, entry)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    native_map_order::ordered_keys(layout, entries.keys().cloned())?
        .into_iter()
        .map(|key| {
            entries.get(&key).copied().ok_or_else(|| {
                DiagnosticInputError::Boundary("unknown ordered declaration key".into())
            })
        })
        .collect()
}
pub struct PreparedCallbackGroup {
    pub module: RawFormTerm,
    pub callbacks: RawFormTerm,
}
pub struct PreparedCallbackGroups {
    pub groups: Vec<PreparedCallbackGroup>,
}
impl PreparedCallbackGroups {
    pub fn raw(&self) -> RawFormTerm {
        let mut b = ReasonBuilder::new();
        let mut groups = Vec::new();
        for group in &self.groups {
            let module = b.copy(&group.module);
            let callbacks = b.copy(&group.callbacks);
            groups.push(b.tuple(vec![module, callbacks]));
        }
        let root = b.list(groups);
        b.finish(root)
    }
}
pub struct TargetCallbackState<'a> {
    definitions: &'a PreparedDefinitions,
    scope: Option<String>,
    rows: NativeEtsSet<&'a crate::compiled_definitions::PreparedDeclaration>,
    completed: usize,
}
impl<'a> TargetCallbackState<'a> {
    pub fn new(definitions: &'a PreparedDefinitions) -> Self {
        Self {
            definitions,
            scope: None,
            rows: NativeEtsSet::default(),
            completed: 0,
        }
    }
    pub fn from_definitions(
        definitions: &'a PreparedDefinitions,
        layout: &NativeAtomLayout,
    ) -> Result<Self> {
        let mut state = Self::new(definitions);
        for _ in definitions.callbacks.write_events() {
            state.store_next(layout)?;
        }
        state.scope = Some(layout.scope.clone());
        Ok(state)
    }
    pub fn store_next(&mut self, layout: &NativeAtomLayout) -> Result<ModuleId> {
        if self
            .scope
            .as_ref()
            .is_some_and(|scope| scope != &layout.scope)
        {
            return Err(DiagnosticInputError::Boundary(
                "callback preparation runtime scope changed".into(),
            ));
        }
        let (module, rows) = self
            .definitions
            .callbacks
            .write_events()
            .nth(self.completed)
            .ok_or_else(|| {
                DiagnosticInputError::Boundary("callback preparation already complete".into())
            })?;
        for entry in ordered_raw(rows.iter(), layout)? {
            self.rows
                .insert(NativeHashKey::from_raw(entry.key.raw())?, entry, layout)?;
        }
        self.scope = Some(layout.scope.clone());
        self.completed += 1;
        Ok(module)
    }
    pub fn persistence_groups(&self) -> Result<PreparedCallbackGroups> {
        self.require_complete(self.definitions.module_names.len())?;
        let rows = self.rows();
        let mut modules: Vec<RawFormTerm> = Vec::new();
        for row in &rows {
            let module = row.key.fields()[0].clone();
            let mut at = modules.len();
            while at > 0
                && crate::term_order::compare_terms(
                    module.arena(),
                    module.id(),
                    modules[at - 1].arena(),
                    modules[at - 1].id(),
                )
                .map_err(DiagnosticInputError::Boundary)?
                .is_lt()
            {
                at -= 1;
            }
            if at > 0
                && crate::term_order::compare_terms(
                    module.arena(),
                    module.id(),
                    modules[at - 1].arena(),
                    modules[at - 1].id(),
                )
                .map_err(DiagnosticInputError::Boundary)?
                .is_eq()
            {
                continue;
            }
            modules.insert(at, module);
        }
        let mut groups = Vec::new();
        for module in modules {
            let mut b = ReasonBuilder::new();
            let mut callbacks = Vec::new();
            for row in &rows {
                let actual = row.key.fields()[0].clone();
                if crate::term_order::compare_map_keys(
                    module.arena(),
                    module.id(),
                    actual.arena(),
                    actual.id(),
                )
                .map_err(DiagnosticInputError::Boundary)?
                .is_eq()
                {
                    callbacks.push(b.copy(&row.callback_row()));
                }
            }
            let root = b.list(callbacks);
            groups.push(PreparedCallbackGroup {
                module,
                callbacks: b.finish(root),
            });
        }
        Ok(PreparedCallbackGroups { groups })
    }
    fn require_complete(&self, _core_modules: usize) -> Result<()> {
        for (module, rows) in self
            .definitions
            .callbacks
            .write_events()
            .skip(self.completed)
        {
            if !rows.is_empty() {
                return Err(DiagnosticInputError::CallbackPreparationMissing(module));
            }
        }
        Ok(())
    }
    pub fn rows(&self) -> Vec<&crate::compiled_definitions::PreparedDeclaration> {
        self.rows
            .rows()
            .into_iter()
            .map(|(_, value)| *value)
            .collect()
    }
}
pub struct DiagnosticInputs<'a, 'b> {
    pub nodes: &'a DataflowNodes<'b>,
    pub program: &'a Program,
    pub callgraph: &'a SourceCallGraph,
    pub library: &'a Store,
    pub signatures: &'a FunctionTypes,
    pub atom_layout: &'a NativeAtomLayout,
    pub target_callbacks: &'a TargetCallbackState<'b>,
}
pub struct ModuleDiagnosticWarnings {
    pub module: ModuleId,
    pub missing_contract_functions: Vec<NativeWarning>,
    pub callbacks: Vec<NativeWarning>,
}
pub struct DiagnosticWarnings {
    pub contracts: Vec<NativeWarning>,
    pub modules: Vec<ModuleDiagnosticWarnings>,
}
pub(crate) struct Origin {
    pub(crate) file: RawFormTerm,
    pub(crate) location: RawFormTerm,
}
pub(crate) fn prepared_origin(contract: &PreparedContract) -> Origin {
    let raw = contract.file_location();
    let ErlangTerm::Tuple(fields) = raw.term() else {
        unreachable!("prepared source origin")
    };
    Origin {
        file: RawFormTerm::new(raw.arena().clone(), fields[0]).expect("prepared file"),
        location: RawFormTerm::new(raw.arena().clone(), fields[1]).expect("prepared location"),
    }
}
fn list_ids(terms: &crate::shared_vec::SharedVec<ErlangTerm>, mut id: TermId) -> Result<Vec<TermId>> {
    let mut out = Vec::new();
    loop {
        match &terms[id.0] {
            ErlangTerm::Nil => return Ok(out),
            ErlangTerm::List { items, tail } => {
                out.extend(items);
                id = *tail
            }
            _ => {
                return Err(DiagnosticInputError::Boundary(
                    "compiler annotation expected proper list".into(),
                ));
            }
        }
    }
}
fn origin_raw(file: &RawFormTerm, location: &RawFormTerm, current: &Mfa) -> RawFormTerm {
    let mut b = ReasonBuilder::new();
    let file = b.copy(file);
    let loc = b.copy(location);
    let m = b.atom(&current.module);
    let f = b.atom(&current.function);
    let a = b.integer(current.arity);
    let mfa = b.tuple(vec![m, f, a]);
    let root = b.tuple(vec![file, loc, mfa]);
    b.finish(root)
}
fn location_raw(loc: WarningLocation) -> RawFormTerm {
    let mut b = ReasonBuilder::new();
    let id = match loc {
        WarningLocation::Line(line) => b.integer(line),
        WarningLocation::Position { line, column } => {
            let l = b.integer(line);
            let c = b.integer(column);
            b.tuple(vec![l, c])
        }
    };
    b.finish(id)
}
impl<'a, 'b> DiagnosticInputs<'a, 'b> {
    pub fn named_signatures(&self) -> Result<BTreeMap<Mfa, Signature>> {
        let mut named = BTreeMap::new();
        for (label, sig) in self.signatures {
            if let Some(name) = self.callgraph.function_name(*label) {
                named.insert(name, sig.clone());
            }
        }

        Ok(named)
    }
    pub fn prepare(self, modules: &[ModuleId]) -> Result<DiagnosticSession<'a, 'b>> {
        self.target_callbacks
            .require_complete(self.nodes.core().modules.len())?;
        if self
            .target_callbacks
            .scope
            .as_ref()
            .is_some_and(|scope| scope != &self.atom_layout.scope)
        {
            return Err(DiagnosticInputError::Boundary(
                "diagnostic and callback runtime scopes differ".into(),
            ));
        }
        let named = self.named_signatures()?;
        let forms = FormContext::new(
            self.target_callbacks.definitions.tables.clone(),
            Arc::new(self.library.exported_types.clone()),
        );
        let origins = self
            .target_callbacks
            .definitions
            .contracts
            .raw_entries()
            .map(|entry| (entry.key.clone(), prepared_origin(&entry.value)))
            .collect::<BTreeMap<_, _>>();
        let mut presentations = BTreeMap::new();
        for module in modules {
            let mut ctx = PresentationContext::default();
            for ((owner, tag, _), record) in &self.target_callbacks.definitions.records {
                if owner == module {
                    ctx.insert_record(
                        tag.clone(),
                        record
                            .field_names
                            .iter()
                            .zip(&record.field_types)
                            .map(|(name, ty)| PresentationRecordField {
                                name: name.clone(),
                                declared_type: ty.clone(),
                            })
                            .collect(),
                    );
                }
            }
            presentations.insert(*module, ctx);
        }
        // Per-module positions in program order; diagnostics run once per
        // module, and scanning the whole program each time is quadratic.
        let mut module_functions: Vec<Vec<usize>> = Vec::new();
        for (i, f) in self.program.functions.iter().enumerate() {
            if module_functions.len() <= f.module.0 {
                module_functions.resize_with(f.module.0 + 1, Vec::new);
            }
            module_functions[f.module.0].push(i);
        }
        let mut module_metadata: Vec<Vec<usize>> = Vec::new();
        for (i, m) in self.program.metadata.iter().enumerate() {
            if module_metadata.len() <= m.module.0 {
                module_metadata.resize_with(m.module.0 + 1, Vec::new);
            }
            module_metadata[m.module.0].push(i);
        }
        Ok(DiagnosticSession {
            inputs: self,
            named,
            forms,
            origins,
            presentations,
            modules: modules.to_vec(),
            module_functions,
            module_metadata,
        })
    }
}
pub struct DiagnosticSession<'a, 'b> {
    inputs: DiagnosticInputs<'a, 'b>,
    named: BTreeMap<Mfa, Signature>,
    forms: FormContext,
    origins: BTreeMap<crate::declaration_key::DeclarationKey, Origin>,
    presentations: BTreeMap<ModuleId, PresentationContext>,
    modules: Vec<ModuleId>,
    module_functions: Vec<Vec<usize>>,
    module_metadata: Vec<Vec<usize>>,
}
impl DiagnosticSession<'_, '_> {
    fn functions_of(&self, module: ModuleId) -> impl Iterator<Item = &crate::program::Function> {
        let functions = &self.inputs.program.functions;
        self.module_functions.get(module.0).into_iter().flatten().map(move |&i| &functions[i])
    }
    fn metadata_of(&self, module: ModuleId) -> impl Iterator<Item = &crate::program::Metadata> {
        let metadata = &self.inputs.program.metadata;
        self.module_metadata.get(module.0).into_iter().flatten().map(move |&i| &metadata[i])
    }
    fn contract_entries(&self, module: ModuleId) -> Result<Vec<ContractWarningEntry<'_>>> {
        let entries = ordered_raw(
            self.inputs
                .target_callbacks
                .definitions
                .contracts
                .owner_entries(module),
            self.inputs.atom_layout,
        )?
        .into_iter()
        .map(|entry| {
            let c = &entry.value;
            let origin = &self.origins[&entry.key];
            ContractWarningEntry {
                name: &entry.key,
                summary: &c.summary,
                capture: &c.capture,
                file: &origin.file,
                location: &origin.location,
            }
        })
        .collect::<Vec<_>>();
        Ok(entries)
    }
    pub fn contracts(&self, bifs: &Registry) -> Result<Vec<NativeWarning>> {
        let mut timings = crate::phase_timings::PhaseTimings::new("contract_warnings");
        // Modules order their entries independently; the first error in
        // module order is the sequential result.
        let entries = crate::ordered_parallel::setup_map(&self.modules, |module| self.contract_entries(*module))
            .into_iter()
            .collect::<Result<Vec<_>>>()?;
        timings.mark("entries");
        let modules = self
            .modules
            .iter()
            .zip(&entries)
            .map(|(module, entries)| ContractWarningModule {
                ordered_contracts: entries,
                presentation: &self.presentations[module],
                forms: &self.forms,
            })
            .collect::<Vec<_>>();
        let warnings = contract_warnings::collect(&modules, &self.named, bifs)?;
        timings.mark("collect");
        Ok(warnings)
    }
    pub fn missing_contract_functions(&self, module: ModuleId) -> Result<Vec<NativeWarning>> {
        let entries = self.contract_entries(module)?;
        let implemented = self
            .functions_of(module)
            .filter_map(|f| {
                self.inputs
                    .nodes
                    .labels()
                    .label(NodeRef {
                        module,
                        node: f.node.0,
                    })
                    .and_then(|label| self.inputs.callgraph.function_name(label))
                    .map(|mut name| {
                        name.arity = f.arity;
                        name
                    })
            })
            .collect::<Vec<_>>();
        let missing_contract_functions =
            contract_warnings::without_functions(&entries, &implemented);
        Ok(missing_contract_functions)
    }
    pub fn callbacks(&self, module: ModuleId) -> Result<Vec<NativeWarning>> {
        let target_rows = self.inputs.target_callbacks.rows();
        let presentation = &self.presentations[&module];
        let source = self
            .inputs
            .nodes
            .module(module)
            .map_err(DiagnosticInputError::Boundary)?;
        let occurrences = crate::behaviour_metadata::collect(
            self.metadata_of(module)
                .filter_map(|metadata| metadata.behaviour.as_ref()),
        )?;
        if occurrences.is_empty() {
            return Ok(Vec::new());
        }
        let mut books =
            BTreeMap::<crate::raw_literal::RawLiteral, Option<Vec<CallbackDeclaration<'_>>>>::new();
        for occurrence in &occurrences {
            let behaviour = &occurrence.name;
            if books.contains_key(behaviour) {
                continue;
            }
            let target = target_rows
                .iter()
                .filter(|entry| {
                    let ErlangTerm::Tuple(fields) = entry.key.raw().term() else {
                        unreachable!()
                    };
                    crate::raw_literal::RawLiteral::from_arena(
                        entry.key.raw().arena().clone(),
                        fields[0],
                    )
                    .expect("validated declaration module")
                        == *behaviour
                })
                .map(|entry| CallbackDeclaration {
                    name: entry.key.clone(),
                    summary: &entry.value.summary,
                    optional: entry.value.optional,
                })
                .collect::<Vec<_>>();
            let entries = if !target.is_empty() {
                Some(target)
            } else {
                match behaviour.as_atom() {
                    None => None,
                    Some(name) => self
                        .inputs
                        .library
                        .callback_rows(name)
                        .map_err(DiagnosticInputError::LibraryCallbacks)?
                        .map(|order| {
                            order
                                .iter()
                                .map(|name| {
                                    let c =
                                        name.contract().map_err(DiagnosticInputError::Boundary)?;
                                    Ok(CallbackDeclaration {
                                        name: name.key.clone(),
                                        summary: c
                                            .summary()
                                            .map_err(DiagnosticInputError::Boundary)?,
                                        optional: optional(c)?,
                                    })
                                })
                                .collect::<Result<Vec<_>>>()
                        })
                        .transpose()?,
                }
            };
            books.insert(behaviour.clone(), entries);
        }
        let behaviours = occurrences
            .iter()
            .map(|occurrence| BehaviourOccurrence {
                name: &occurrence.name,
                location: occurrence.location.raw(),
                callbacks: books[&occurrence.name].as_deref(),
            })
            .collect::<Vec<_>>();
        let mut patterns = Patterns::new(self.inputs.nodes);
        let binder = Binder {
            patterns: &mut patterns,
        };
        let state = BinderState::<OpaqueMatch>::for_module(
            self.inputs.nodes.core(),
            self.inputs.target_callbacks.definitions,
            module,
            false,
        )
        .map_err(|e| DiagnosticInputError::Boundary(e.to_string()))?;
        let mut function_origins = BTreeMap::new();
        let mut exported = BTreeSet::new();
        let mut filename = None;
        for function in self.functions_of(module) {
            let FunctionIdentity::Named(name) = &function.identity else {
                continue;
            };
            let mfa = Mfa {
                module: name.module.clone().into(),
                function: name.name.clone().into(),
                arity: name.arity,
            };
            let node = NodeRef {
                module,
                node: function.node.0,
            };
            let info = binder
                .warning_info(PatternRef::Core(node), &state)
                .map_err(DiagnosticInputError::Bind)?;
            let mut b = ReasonBuilder::new();
            let id = b.string(&info.filename);
            let file = b.finish(id);
            if name.name == "module_info" && name.arity == 0 {
                filename = Some(file.clone())
            }
            let loc = binder
                .warning_location(PatternRef::Core(node), WarningLocation::Line(-1))
                .map_err(DiagnosticInputError::Bind)?;
            function_origins.insert(mfa.clone(), origin_raw(&file, &location_raw(loc), &mfa));
            if function.exported {
                exported.insert(mfa);
            }
        }
        let filename = filename.ok_or_else(|| {
            DiagnosticInputError::Boundary("behaviour module_info/0 missing".into())
        })?;
        let declared = self
            .inputs
            .target_callbacks
            .definitions
            .contracts
            .callables()
            .filter(|(n, _)| n.module == source.name)
            .map(|(name, c)| {
                let entry = self
                    .inputs
                    .target_callbacks
                    .definitions
                    .contracts
                    .raw_entries()
                    .find(|entry| entry.key.callable_mfa().as_ref() == Some(name))
                    .expect("callable origin");
                let origin = &self.origins[&entry.key];
                (
                    name.clone(),
                    DeclaredCallback {
                        summary: &c.summary,
                        file: &origin.file,
                        location: &origin.location,
                    },
                )
            })
            .collect();
        let callbacks = callback_warnings::collect(&CallbackWarningModule {
            module: &source.name,
            filename: &filename,
            behaviours: &behaviours,
            signatures: &self.named,
            exported: &exported,
            declared: &declared,
            function_origins: &function_origins,
            presentation,
        })?;
        Ok(callbacks)
    }
}

fn optional(contract: &crate::dependencies::Contract) -> Result<bool> {
    let Some(flags) = &contract.flags else {
        return Err(DiagnosticInputError::Boundary(
            "imported callback flags missing".into(),
        ));
    };
    let mut b = ReasonBuilder::new();
    let id = flags.append_raw(&mut b);
    let raw = b.finish(id);
    Ok(list_ids(&raw.arena().terms, raw.id())?
        .iter()
        .any(|id| matches!(&raw.arena().terms[id.0],ErlangTerm::Atom(s)if s=="optional_callback")))
}

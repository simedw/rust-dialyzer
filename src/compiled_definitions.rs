use crate::core::{ErlangTerms, TermId};
use std::{collections::BTreeMap, sync::Arc};
/// Original module-scoped source IDs and native map iteration provenance.
#[derive(Clone)]
pub struct FormSourceArena {
    pub terms: Arc<ErlangTerms>,
    pub iterator_orders: BTreeMap<TermId, Vec<usize>>,
}

use crate::{
    bifs::registry::Mfa,
    contract_runtime::ContractSummary,
    core::{CoreProgram, ErlangTerm},
    dependencies::Store,
    environment,
    form_error::{FormError, ReasonBuilder},
    forms::{Contract, DeclarationKind, Forms, TypeForm},
    lattice::{Type, VariableId},
    native_form_environment::{
        ExternalTypeEvent, LazyFormEnvironment, NativeFormSite, NativeSiteKind, RawFormTerm,
    },
    program::{ModuleId, Program, SourceOrigin, TargetContracts, TypeName},
    resolver::{FormCache, ModuleTableHandle, Resolver, Site},
};
use std::collections::BTreeSet;
#[derive(Clone, Debug)]
pub enum PreparationStage {
    Import,
    Opaque(u8),
    Records,
    RecordChecks,
    Contracts,
}
#[derive(Debug)]
pub struct PreparationFailure {
    pub stage: PreparationStage,
    pub site: Option<Box<NativeFormSite>>,
    pub error: FormError,
    pub external_types: Vec<ExternalTypeEvent>,
}
impl std::fmt::Display for PreparationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.stage, self.error)
    }
}
impl std::error::Error for PreparationFailure {}
#[derive(Clone)]
pub struct PreparedRecord {
    pub module: ModuleId,
    pub tag: String,
    pub field_names: Vec<String>,
    pub field_types: Vec<Type>,
    pub prototype: Type,
    pub origin: SourceOrigin,
}
#[derive(Clone)]
pub struct PreparedContract {
    file_location: RawFormTerm,
    pub summary: Arc<ContractSummary>,
    pub capture: RawFormTerm,
    pub origin: SourceOrigin,
    pub callback: bool,
    pub optional: bool,
}
impl PreparedContract {
    pub fn file_location(&self) -> RawFormTerm {
        self.file_location.clone()
    }
}
pub struct PreparedDeclaration {
    pub owner: ModuleId,
    pub key: crate::declaration_key::DeclarationKey,
    pub value: PreparedContract,
}
impl PreparedDeclaration {
    pub fn callback_row(&self) -> RawFormTerm {
        let mut b = ReasonBuilder::new();
        let key = b.copy(self.key.raw());
        let origin = b.copy(&self.value.file_location());
        let contract = b.copy(&self.value.capture);
        let optional = if self.value.optional {
            vec![b.atom("optional_callback")]
        } else {
            Vec::new()
        };
        let flags = b.list(optional);
        let value = b.tuple(vec![origin, contract, flags]);
        let row = b.tuple(vec![key, value]);
        b.finish(row)
    }
}
#[derive(Clone, Debug)]
pub struct ModuleCompletionOrder {
    modules: Vec<ModuleId>,
}
impl ModuleCompletionOrder {
    pub fn compiler_order(core: &CoreProgram) -> Self {
        Self {
            modules: (0..core.modules.len()).map(ModuleId).collect(),
        }
    }
    pub fn checked(core: &CoreProgram, modules: Vec<ModuleId>) -> Result<Self, String> {
        let expected = (0..core.modules.len())
            .map(ModuleId)
            .collect::<BTreeSet<_>>();
        if modules.len() != expected.len()
            || modules.iter().copied().collect::<BTreeSet<_>>() != expected
        {
            return Err(
                "contract completion order must contain each compiler module exactly once".into(),
            );
        }
        Ok(Self { modules })
    }
}
#[derive(Default)]
pub struct PreparedContractTable {
    rows: Vec<PreparedDeclaration>,
    final_index: BTreeMap<crate::declaration_key::DeclarationKey, usize>,
    owner_keys: BTreeMap<ModuleId, Vec<crate::declaration_key::DeclarationKey>>,
    write_events: Vec<(ModuleId, std::ops::Range<usize>)>,
    callable_index: BTreeMap<Mfa, usize>,
}
impl PreparedContractTable {
    fn add_module(&mut self, owner: ModuleId, entries: Vec<PreparedDeclaration>) {
        let start = self.rows.len();
        let mut keys = Vec::with_capacity(entries.len());
        for entry in entries {
            let index = self.rows.len();
            keys.push(entry.key.clone());
            self.final_index.insert(entry.key.clone(), index);
            if let Some(name) = entry.key.callable_mfa() {
                self.callable_index.insert(name, index);
            }
            self.rows.push(entry);
        }
        self.owner_keys.insert(owner, keys);
        self.write_events.push((owner, start..self.rows.len()));
    }
    pub fn raw_entries(&self) -> impl Iterator<Item = &PreparedDeclaration> {
        self.final_index.values().map(|&index| &self.rows[index])
    }
    pub fn owner_entries(&self, owner: ModuleId) -> impl Iterator<Item = &PreparedDeclaration> {
        self.owner_keys
            .get(&owner)
            .into_iter()
            .flatten()
            .map(|key| &self.rows[self.final_index[key]])
    }
    pub fn write_events(&self) -> impl Iterator<Item = (ModuleId, &[PreparedDeclaration])> {
        self.write_events
            .iter()
            .map(|(owner, range)| (*owner, &self.rows[range.clone()]))
    }
    pub fn callables(&self) -> impl Iterator<Item = (&Mfa, &PreparedContract)> {
        self.callable_index
            .iter()
            .map(|(name, index)| (name, &self.rows[*index].value))
    }
    pub fn callable_keys(&self) -> impl Iterator<Item = &Mfa> {
        self.callable_index.keys()
    }
    pub fn get_callable(&self, name: &Mfa) -> Option<&PreparedContract> {
        self.callable_index.get(name).map(|i| &self.rows[*i].value)
    }
    pub fn len(&self) -> usize {
        self.final_index.len()
    }
    pub fn is_empty(&self) -> bool {
        self.final_index.is_empty()
    }
}
pub struct PreparedDefinitions {
    pub forms: Arc<Forms>,
    pub module_names: Arc<[String]>,
    pub sources: Arc<BTreeMap<ModuleId, FormSourceArena>>,
    pub records: BTreeMap<(ModuleId, String, usize), PreparedRecord>,
    pub contracts: PreparedContractTable,
    pub callbacks: PreparedContractTable,
    pub external_types: Vec<ExternalTypeEvent>,
    pub external_type_occurrences: Vec<ExternalTypeEvent>,
    pub tables: BTreeMap<String, ModuleTableHandle>,
    pub exported_types: BTreeSet<TypeName>,
}
impl PreparedDefinitions {
    pub fn module_table(&self, module: ModuleId) -> Option<&ModuleTableHandle> {
        self.module_names
            .get(module.0)
            .and_then(|name| self.tables.get(name))
    }
    pub fn prepare(
        core: &CoreProgram,
        program: &Program,
        library: &Store,
    ) -> Result<Self, PreparationFailure> {
        Self::prepare_with_completion(
            core,
            program,
            library,
            &ModuleCompletionOrder::compiler_order(core),
        )
    }
    pub fn prepare_with_completion(
        core: &CoreProgram,
        program: &Program,
        library: &Store,
        completion: &ModuleCompletionOrder,
    ) -> Result<Self, PreparationFailure> {
        let mut timings = crate::phase_timings::PhaseTimings::new("prepare_definitions");
        let fail = |e: String| PreparationFailure {
            stage: PreparationStage::Import,
            site: None,
            error: e.into(),
            external_types: Vec::new(),
        };
        ModuleCompletionOrder::checked(core, completion.modules.clone()).map_err(&fail)?;
        // store_temp_records guards precede all target definition preparation.
        let targets_for_import = core
            .modules
            .iter()
            .map(|m| m.name.as_str())
            .collect::<BTreeSet<_>>();
        let arena = Arc::new(library.type_tables.clone());
        let ErlangTerm::Map(entries) = &arena.terms[arena.root.0] else {
            let raw = RawFormTerm::new(arena.clone(), arena.root).expect("library root");
            let mut b = ReasonBuilder::new();
            let value = b.copy(&raw);
            let tag = b.atom("badmap");
            let root = b.tuple(vec![tag, value]);
            return Err(import_failure(FormError::ErrorReason(b.finish(root))));
        };
        for (key, value) in entries {
            if program.target_contracts() == TargetContracts::Use
                && matches!(&arena.terms[key.0],ErlangTerm::Atom(name) if targets_for_import.contains(name.as_str()))
            {
                continue;
            }
            if !matches!(&arena.terms[key.0], ErlangTerm::Atom(_)) {
                return Err(import_failure(FormError::Native(
                    crate::native_error::NativeTransferError::FunctionClause,
                )));
            }
            if !matches!(&arena.terms[value.0], ErlangTerm::Map(_)) {
                let raw = RawFormTerm::new(arena.clone(), *value).expect("library table");
                let mut b = ReasonBuilder::new();
                let value = b.copy(&raw);
                let tag = b.atom("badmap");
                let root = b.tuple(vec![tag, value]);
                return Err(import_failure(FormError::ErrorReason(b.finish(root))));
            }
        }
        let source_locations =
            crate::temporary_contracts::locations(core, program).map_err(import_failure)?;
        let mut forms =
            Forms::build_deferred(core, program, &source_locations).map_err(import_failure)?;
        timings.mark("forms");
        if program.target_contracts() == TargetContracts::Ignore {
            let names = core
                .modules
                .iter()
                .map(|m| m.name.clone())
                .collect::<Vec<_>>();
            let targets = names.iter().cloned().collect::<BTreeSet<_>>();
            let mut exports =
                environment::exported_type_names(&library.exported_types).map_err(fail)?;
            exports.retain(|name| !targets.contains(&name.module));
            exports.extend(
                forms
                    .declarations
                    .values()
                    .filter(|d| d.exported)
                    .map(|d| d.name.clone()),
            );
            let mut sources = BTreeMap::new();
            let mut tables = BTreeMap::new();
            for (index, module) in core.modules.iter().enumerate() {
                let mut builder = ReasonBuilder::from_terms(module.terms.clone());
                let root = builder.push(ErlangTerm::Map(Vec::new()));
                let raw = builder.finish(root);
                tables.insert(
                    module.name.clone(),
                    ModuleTableHandle::at(raw.arena().clone(), root).map_err(import_failure)?,
                );
                sources.insert(
                    ModuleId(index),
                    FormSourceArena {
                        terms: raw.arena().clone(),
                        iterator_orders: module.map_iterator_orders.clone(),
                    },
                );
            }
            return Ok(Self {
                forms: Arc::new(forms),
                module_names: names.into(),
                sources: Arc::new(sources),
                records: BTreeMap::new(),
                contracts: PreparedContractTable::default(),
                callbacks: PreparedContractTable::default(),
                external_types: Vec::new(),
                external_type_occurrences: Vec::new(),
                tables,
                exported_types: exports,
            });
        }

        let mut names = core
            .modules
            .iter()
            .map(|m| m.name.clone())
            .collect::<Vec<_>>();
        let targets = names.iter().cloned().collect::<BTreeSet<_>>();
        let imported = environment::import_deferred(
            &mut forms,
            &mut names,
            &library.type_tables,
            &library.exported_types,
            &targets,
        )
        .map_err(fail)?;
        let mut exports =
            environment::exported_type_names(&library.exported_types).map_err(fail)?;
        exports.retain(|n| !targets.contains(&n.module));
        exports.extend(
            forms
                .declarations
                .values()
                .filter(|d| d.exported && targets.contains(&d.name.module))
                .map(|d| d.name.clone()),
        );
        timings.mark("import");
        let library_arena = arena;
        let ErlangTerm::Map(entries) = &library_arena.terms[library_arena.root.0] else {
            return Err(fail("library type tables expected module map".into()));
        };
        let mut tables = BTreeMap::new();
        let mut definitions = BTreeMap::new();
        let mut sources = BTreeMap::new();
        let mut lazy_tables = BTreeMap::new();
        for (name, table) in entries {
            let ErlangTerm::Atom(name) = &library_arena.terms[name.0] else {
                return Err(fail("library table key expected atom".into()));
            };
            if targets.contains(name) {
                continue;
            }
            tables.insert(
                name.clone(),
                ModuleTableHandle::at(library_arena.clone(), *table).map_err(import_failure)?,
            );
            lazy_tables.insert(name.clone(), *table);
        }
        let lazy = LazyFormEnvironment {
            arena: library_arena.clone(),
            tables: lazy_tables,
        };
        for d in forms
            .declarations
            .values()
            .filter(|d| !targets.contains(&d.name.module))
        {
            let raw = lazy
                .declaration(&d.name)
                .ok_or_else(|| fail("imported declaration lacks raw source".into()))?;
            definitions.insert(d.name.clone(), raw.definition().map_err(fail)?);
        }
        for (i, name) in names.iter().enumerate().skip(core.modules.len()) {
            if tables.contains_key(name) {
                sources.insert(
                    ModuleId(i),
                    FormSourceArena {
                        terms: library_arena.clone(),
                        iterator_orders: BTreeMap::new(),
                    },
                );
            }
        }
        timings.mark("library_tables");
        let mut sites = BTreeMap::<usize, NativeFormSite>::new();
        // Each module's table, contract forms and definitions only read the
        // forms built so far, so modules are prepared on all cores. Changes to
        // `Forms` (deferred contract alternatives and the contracts) and the
        // tables are applied in module order afterwards, which is the order
        // the sequential loop made them in; the first failing module in
        // module order is reported.
        let mut module_metadata = vec![Vec::new(); core.modules.len()];
        for (index, meta) in program.metadata.iter().enumerate() {
            if let Some(entries) = module_metadata.get_mut(meta.module.0) {
                entries.push(index);
            }
        }
        struct ModuleTables {
            table: ModuleTableHandle,
            source: FormSourceArena,
            contracts: Vec<crate::temporary_contracts::PendingContract>,
            definitions: Vec<(TypeName, crate::native_form_environment::RawDefinition)>,
            sites: Vec<(usize, NativeFormSite)>,
        }
        let shared_forms = &forms;
        let indices = (0..core.modules.len()).collect::<Vec<_>>();
        let prepared = crate::ordered_parallel::setup_map(
            &indices,
            |&i| -> Result<ModuleTables, PreparationFailure> {
                let forms = shared_forms;
                let module = &core.modules[i];
                let mid = ModuleId(i);
                let mut b = ReasonBuilder::from_terms(module.terms.clone());
                let mut own = Vec::new();
                // One shared copy of the module's terms for all its aliases (this
                // was copied per declaration).
                let source_arena = Arc::new(crate::core::ErlangTerms {
                    root: module.source_forms,
                    terms: module.terms.clone(),
                });
                let declarations = ordered_declarations(forms, &module.name);
                // This module's aliases by position; prior_aliases scanned every
                // declaration of every module per alias.
                let mut module_aliases: Vec<(usize, (String, usize))> = declarations
                    .iter()
                    .filter(|d| d.kind == DeclarationKind::Alias)
                    .map(|d| (d.metadata, (d.name.name.clone(), d.name.arity)))
                    .collect();
                module_aliases.sort();
                for d in &declarations {
                    let meta = &program.metadata[d.metadata];
                    let file = b.string(meta.origin.file.as_deref().unwrap_or("nofile"));
                    let loc = source_location(&mut b, &meta.origin);
                    let fl = b.tuple(vec![file, loc]);
                    let m = b.atom(&module.name);
                    let body = if d.kind == DeclarationKind::Alias && module.name != "erlang" {
                        let raw = RawFormTerm::new(source_arena.clone(), forms.nodes[d.body.0].raw)
                            .expect("source declaration");
                        let defs: BTreeSet<(String, usize)> = module_aliases
                            .iter()
                            .take_while(|(metadata, _)| *metadata < d.metadata)
                            .map(|(_, name)| name.clone())
                            .collect();
                        b.copy(&crate::native_form_preprocessing::massage(&raw, &defs))
                    } else {
                        forms.nodes[d.body.0].raw
                    };
                    let params = d
                        .parameters
                        .iter()
                        .map(|v| match v {
                            VariableId::Atom(v) => b.atom(v),
                            VariableId::Integer(v) => b.integer(v),
                        })
                        .collect();
                    let params = b.list(params);
                    let metadata = b.tuple(vec![m, fl, body, params]);
                    let any = b.atom("any");
                    let value = b.tuple(vec![metadata, any]);
                    let tag = b.atom(match d.kind {
                        DeclarationKind::Alias => "type",
                        DeclarationKind::Opaque => "opaque",
                        DeclarationKind::Nominal => "nominal",
                    });
                    let n = b.atom(&d.name.name);
                    let arity = b.integer(d.name.arity);
                    let key = b.tuple(vec![tag, n, arity]);
                    own.push((key, value));
                    let _ = file;
                }
                for record in module_records(forms, mid) {
                    let meta = &program.metadata[record.metadata];
                    let file = b.string(meta.origin.file.as_deref().unwrap_or("nofile"));
                    let loc = source_location(&mut b, &meta.origin);
                    let fl = b.tuple(vec![file, loc]);
                    let mut fields = Vec::new();
                    for field in &record.fields {
                        let name = b.atom(&field.name);
                        let form = match field.declared_type {
                            Some(id) => forms.nodes[id.0].raw,
                            None => generated_variable(&mut b),
                        };
                        let any = b.atom("any");
                        fields.push(b.tuple(vec![name, form, any]));
                    }
                    let fields = b.list(fields);
                    let arity = b.integer(record.fields.len());
                    let variant = b.tuple(vec![arity, fields]);
                    let variants = b.list(vec![variant]);
                    let value = b.tuple(vec![fl, variants]);
                    let tag = b.atom("record");
                    let name = b.atom(&record.name);
                    let key = b.tuple(vec![tag, name]);
                    own.push((key, value));
                }
                let table = b.push(ErlangTerm::Map(own));
                let raw = b.finish(table);
                let table =
                    ModuleTableHandle::at(raw.arena().clone(), table).map_err(import_failure)?;
                let contracts = crate::temporary_contracts::prepare_module(
                    core,
                    program,
                    mid,
                    &module_metadata[i],
                    &raw,
                    &source_locations,
                    None,
                )
                .map_err(import_failure)?;
                let lazy = LazyFormEnvironment {
                    arena: raw.arena().clone(),
                    tables: BTreeMap::from([(module.name.clone(), raw.id())]),
                };
                let mut definitions = Vec::with_capacity(declarations.len());
                for d in &declarations {
                    definitions.push((
                        d.name.clone(),
                        lazy.declaration(&d.name)
                            .expect("constructed declaration")
                            .definition()
                            .map_err(fail)?,
                    ));
                }
                let sites = module_metadata[i]
                    .iter()
                    .map(|&index| {
                        let meta = &program.metadata[index];
                        let mut b = ReasonBuilder::new();
                        let file = b.string(meta.origin.file.as_deref().unwrap_or("nofile"));
                        let file = b.finish(file);
                        (
                            index,
                            NativeFormSite {
                                kind: NativeSiteKind::Spec,
                                scope: TypeName {
                                    module: module.name.clone(),
                                    name: String::new(),
                                    arity: 0,
                                }
                                .into(),
                                file,
                            },
                        )
                    })
                    .collect();
                Ok(ModuleTables {
                    table,
                    source: FormSourceArena {
                        terms: raw.arena().clone(),
                        iterator_orders: module.map_iterator_orders.clone(),
                    },
                    contracts,
                    definitions,
                    sites,
                })
            },
        );
        for (i, prepared) in prepared.into_iter().enumerate() {
            let prepared = prepared?;
            let module = &core.modules[i];
            crate::temporary_contracts::add(&mut forms, ModuleId(i), prepared.contracts);
            tables.insert(module.name.clone(), prepared.table);
            sources.insert(ModuleId(i), prepared.source);
            definitions.extend(prepared.definitions);
            sites.extend(prepared.sites);
        }
        timings.mark("forms_and_module_tables");
        let mut resolver = Resolver::new(&forms, &names);
        resolver.set_compiled_sources(sources.clone(), definitions);
        resolver.set_source_tables(tables.iter().filter(|(_,t)|!matches!(&t.arena().terms[t.root().0],ErlangTerm::Map(xs) if xs.is_empty())).map(|(n,t)|(n.clone(),t.clone())).collect());
        resolver.set_exported_types(exports.clone());
        let imported_types = imported
            .record_prototypes
            .into_iter()
            .map(|r| {
                let mid = ModuleId(
                    names
                        .iter()
                        .position(|n| n == &r.module)
                        .expect("imported module"),
                );
                ((mid, r.name), r.fields)
            })
            .collect();
        resolver
            .set_record_types(imported_types)
            .map_err(import_failure)?;
        let failure = |stage: PreparationStage,
                       site: NativeFormSite,
                       error: FormError,
                       resolver: &Resolver<'_>| PreparationFailure {
            stage,
            site: Some(Box::new(site)),
            error,
            external_types: resolver.external_types.clone(),
        };
        let mut opaque_values = BTreeMap::new();
        // Group once: scanning every declaration per module is quadratic in
        // program size. Each group keeps declaration order.
        let mut opaque_by_module: crate::fast_hash::FastHashMap<&str, Vec<_>> = Default::default();
        for d in forms.declarations.values().filter(|d| d.kind == DeclarationKind::Opaque) {
            opaque_by_module.entry(d.name.module.as_str()).or_default().push(d);
        }
        for pass in 0..2 {
            for (index, module) in core.modules.iter().enumerate() {
                resolver.replace_cache(FormCache::new());
                for d in opaque_by_module.get(module.name.as_str()).into_iter().flatten().copied() {
                    let mut site = sites[&d.metadata].clone();
                    site.kind = NativeSiteKind::Type;
                    site.scope = d.name.clone().into();
                    resolver.set_native_site(site.clone());
                    let value = resolver
                        .resolve(d.body, Site::Type(d.name.clone()), BTreeMap::new())
                        .map_err(|e| failure(PreparationStage::Opaque(pass), site, e, &resolver))?;
                    opaque_values.insert(d.name.clone(), value);
                }
                let _ = index;
            }
        }
        timings.mark("opaque_types");
        // Parallel record and contract steps read the forms from every worker;
        // parse them all up front so workers only read. Failures stay
        // deferred and surface where they did before.
        let parallel = !crate::ordered_parallel::sequential();
        if parallel {
            let mut id = 0;
            while id < resolver.forms.nodes.len() {
                let _ = resolver.materialize(crate::forms::FormId(id));
                id += 1;
            }
        }
        enum RecordFailure {
            /// Completed with the external type events seen before the failure.
            WithEvents(PreparationStage, NativeFormSite, FormError),
        }
        type RecordEntries = (
            Vec<((ModuleId, String), Vec<Type>)>,
            Vec<((ModuleId, String, usize), PreparedRecord)>,
        );
        let record_module = |resolver: &mut Resolver<'_>,
                             index: usize|
         -> std::result::Result<RecordEntries, RecordFailure> {
            let module = &core.modules[index];
            let mid = ModuleId(index);
            let mut own_types = Vec::new();
            let mut records = Vec::new();
            resolver.replace_cache(FormCache::new());
            for record in module_records(&forms, mid) {
                let mut site = sites[&record.metadata].clone();
                site.kind = NativeSiteKind::Record;
                site.scope = TypeName {
                    module: module.name.clone(),
                    name: record.name.clone(),
                    arity: record.fields.len(),
                }
                .into();
                let mut values = Vec::new();
                for field in &record.fields {
                    if let Some(id) = field.declared_type {
                        let mut check = site.clone();
                        check.kind = NativeSiteKind::Check;
                        resolver.set_native_site(check.clone());
                        resolver.check_remote_at(id, Site::Check).map_err(|e| {
                            RecordFailure::WithEvents(PreparationStage::Records, check, e)
                        })?;
                        resolver.set_native_site(site.clone());
                        values.push(
                            resolver
                                .resolve(id, Site::Record, BTreeMap::new())
                                .map_err(|e| {
                                    RecordFailure::WithEvents(
                                        PreparationStage::Records,
                                        site.clone(),
                                        e,
                                    )
                                })?,
                        );
                    } else {
                        values.push(Type::any());
                    }
                }
                own_types.push(((mid, record.name.clone()), values.clone()));
                let mut tagged = vec![Type::atom(&record.name)];
                tagged.extend(values.clone());
                let prototype = Type::tuple(tagged).map_err(|e| {
                    RecordFailure::WithEvents(PreparationStage::Records, site.clone(), e.into())
                })?;
                records.push((
                    (mid, record.name.clone(), values.len()),
                    PreparedRecord {
                        module: mid,
                        tag: record.name.clone(),
                        field_names: record.fields.iter().map(|f| f.name.clone()).collect(),
                        field_types: values,
                        prototype,
                        origin: program.metadata[record.metadata].origin.clone(),
                    },
                ));
            }
            for d in ordered_declarations(&forms, &module.name) {
                let mut site = sites[&d.metadata].clone();
                site.kind = NativeSiteKind::Check;
                site.scope = d.name.clone().into();
                resolver.set_native_site(site.clone());
                resolver.check_remote_at(d.body, Site::Check).map_err(|e| {
                    RecordFailure::WithEvents(PreparationStage::Records, site, e)
                })?;
            }
            Ok((own_types, records))
        };
        let record_outcomes = if parallel {
            per_module_workers(&resolver, core.modules.len(), &record_module)
        } else {
            None
        };
        let mut records = BTreeMap::new();
        let mut own_types = BTreeMap::new();
        let mut add_records = |(types, entries): RecordEntries| {
            own_types.extend(types);
            records.extend(entries);
        };
        let record_failure = |f: RecordFailure, resolver: &Resolver<'_>| match f {
            RecordFailure::WithEvents(stage, site, error) => failure(stage, site, error, resolver),
        };
        let records_mode = if record_outcomes.is_some() { "parallel" } else { "sequential" };
        match record_outcomes {
            Some(outcomes) => {
                for (result, events, missing) in outcomes {
                    resolver.external_types.extend(events);
                    resolver.missing.extend(missing);
                    match result {
                        Ok(entries) => add_records(entries),
                        Err(f) => return Err(record_failure(f, &resolver)),
                    }
                }
            }
            None => {
                for index in 0..core.modules.len() {
                    match record_module(&mut resolver, index) {
                        Ok(entries) => add_records(entries),
                        Err(f) => return Err(record_failure(f, &resolver)),
                    }
                }
            }
        }
        let mut all_types = resolver
            .forms
            .records
            .values()
            .filter_map(|r| {
                resolver
                    .record_prototype(r.module, &r.name)
                    .map(|v| ((r.module, r.name.clone()), v.to_vec()))
            })
            .collect::<BTreeMap<_, _>>();
        all_types.extend(own_types);
        resolver
            .set_record_types(all_types)
            .map_err(import_failure)?;
        timings.mark("records");
        let alias_index = AliasIndex::new(&resolver.forms);
        for (index, module) in core.modules.iter().enumerate() {
            let mid = ModuleId(index);
            resolver.replace_cache(FormCache::new());
            for r in module_records(&forms, mid) {
                let mut site = sites[&r.metadata].clone();
                site.kind = NativeSiteKind::Record;
                site.scope = TypeName {
                    module: module.name.clone(),
                    name: r.name.clone(),
                    arity: r.fields.len(),
                }
                .into();
                resolver.set_native_site(site.clone());
                for field in &r.fields {
                    if let Some(id) = field.declared_type {
                        resolver
                            .check_record_fields(id, &BTreeMap::new())
                            .map_err(|e| {
                                failure(
                                    PreparationStage::RecordChecks,
                                    site.clone(),
                                    e.at_position(&program.metadata[r.metadata].origin),
                                    &resolver,
                                )
                            })?;
                    }
                }
            }
            for d in ordered_declarations(&forms, &module.name) {
                let mut site = sites[&d.metadata].clone();
                site.kind = NativeSiteKind::Type;
                site.scope = d.name.clone().into();
                resolver.set_native_site(site.clone());
                resolver
                    .check_record_fields(d.body, &BTreeMap::new())
                    .map_err(|e| {
                        failure(
                            PreparationStage::RecordChecks,
                            site,
                            e.at_position(&program.metadata[d.metadata].origin),
                            &resolver,
                        )
                    })?;
            }
        }
        timings.mark("record_checks");
        // Contracts are compiled per module with a fresh resolver cache, so
        // modules are independent; OTP runs this as one parallel job per
        // module (dialyzer_contracts:process_contract_remote_types).
        let mut by_target: BTreeMap<usize, Vec<&Contract>> = BTreeMap::new();
        for contract in &forms.contracts {
            by_target
                .entry(program.metadata[contract.metadata].module.0)
                .or_default()
                .push(contract);
        }
        let module_contracts: Vec<Vec<&Contract>> = core
            .modules
            .iter()
            .map(|module| {
                let target = names
                    .iter()
                    .position(|n| n == &module.name)
                    .expect("target module");
                let mut entries = by_target.get(&target).cloned().unwrap_or_default();
                entries.sort_by_key(|c| (c.callback, c.name.clone()));
                entries
            })
            .collect();
        enum ContractFailure {
            Ready(PreparationFailure),
            /// Completed with the external type events seen before the failure.
            WithEvents(PreparationStage, NativeFormSite, FormError),
        }
        let compile_module = |resolver: &mut Resolver<'_>,
                              index: usize|
         -> std::result::Result<Vec<PreparedDeclaration>, ContractFailure> {
            let module = &core.modules[index];
            resolver.replace_cache(FormCache::new());
            let mut out = Vec::new();
            for contract in &module_contracts[index] {
                let contract: &Contract = contract;
                let mut site = sites[&contract.metadata].clone();
                site.kind = NativeSiteKind::Spec;
                let ErlangTerm::Tuple(file_location) = contract.file_location.term() else {
                    unreachable!()
                };
                site.file =
                    RawFormTerm::new(contract.file_location.arena().clone(), file_location[0])
                        .map_err(|e| ContractFailure::Ready(import_failure(e)))?;
                site.scope = crate::native_form_environment::NativeFormScope::Contract(
                    contract.name.clone(),
                );
                let mut origin = program.metadata[contract.metadata].origin.clone();
                origin.file = Some(
                    crate::program::characters_from_terms(
                        &site.file.arena().terms,
                        site.file.id(),
                        &module.name,
                    )
                    .map_err(|e| ContractFailure::Ready(fail(e)))?,
                );
                resolver.set_native_site(site.clone());
                let summary = crate::contracts::compile_at(resolver, contract, Some(&origin))
                    .map_err(|e| ContractFailure::WithEvents(PreparationStage::Contracts, site, e))?;
                let capture = contract_capture(core, &resolver.forms, &alias_index, &sources, contract, &summary)
                    .map_err(|e| {
                        ContractFailure::WithEvents(
                            PreparationStage::Contracts,
                            sites[&contract.metadata].clone(),
                            e,
                        )
                    })?;
                out.push(PreparedDeclaration {
                    owner: program.metadata[contract.metadata].module,
                    key: contract.name.clone(),
                    value: PreparedContract {
                        file_location: contract.file_location.clone(),
                        summary: Arc::new(summary),
                        capture,
                        origin,
                        callback: contract.callback,
                        optional: contract.optional,
                    },
                });
            }
            Ok(out)
        };
        // Parallel: one resolver per worker; each module's events are drained
        // and merged in module order, which reproduces the sequential order.
        // If any worker had to modify the shared forms, redo sequentially.
        let outcomes = if parallel {
            per_module_workers(&resolver, core.modules.len(), &compile_module)
        } else {
            None
        };
        timings.label(|| {
            format!(
                "records {records_mode} contracts {}",
                if outcomes.is_some() { "parallel" } else { "sequential" }
            )
        });
        let mut staged_contracts = BTreeMap::<ModuleId, Vec<PreparedDeclaration>>::new();
        let mut staged_callbacks = BTreeMap::<ModuleId, Vec<PreparedDeclaration>>::new();
        let mut stage = |entries: Vec<PreparedDeclaration>| {
            for entry in entries {
                if entry.value.callback {
                    staged_callbacks.entry(entry.owner).or_default().push(entry);
                } else {
                    staged_contracts.entry(entry.owner).or_default().push(entry);
                }
            }
        };
        let to_failure = |f: ContractFailure, resolver: &Resolver<'_>| match f {
            ContractFailure::Ready(f) => f,
            ContractFailure::WithEvents(stage, site, error) => failure(stage, site, error, resolver),
        };
        match outcomes {
            Some(outcomes) => {
                for (result, events, missing) in outcomes {
                    resolver.external_types.extend(events);
                    resolver.missing.extend(missing);
                    match result {
                        Ok(entries) => stage(entries),
                        Err(f) => return Err(to_failure(f, &resolver)),
                    }
                }
            }
            None => {
                for index in 0..core.modules.len() {
                    match compile_module(&mut resolver, index) {
                        Ok(entries) => stage(entries),
                        Err(f) => return Err(to_failure(f, &resolver)),
                    }
                }
            }
        }
        timings.mark("contracts");
        let mut contracts = PreparedContractTable::default();
        let mut callbacks = PreparedContractTable::default();
        for &owner in &completion.modules {
            contracts.add_module(owner, staged_contracts.remove(&owner).unwrap_or_default());
            callbacks.add_module(owner, staged_callbacks.remove(&owner).unwrap_or_default());
        }
        let external_type_occurrences = resolver.external_types.clone();
        let external_types = reported_events(&external_type_occurrences)?;
        let forms = resolver.forms.into_owned();
        finalize_tables(core, &records, &opaque_values, &mut tables).map_err(import_failure)?;
        Ok(Self {
            forms: Arc::new(forms),
            module_names: names.into(),
            sources: Arc::new(sources),
            records,
            contracts,
            callbacks,
            external_types,
            external_type_occurrences,
            tables,
            exported_types: exports,
        })
    }
}
/// Runs `run` for every module on all cores, each worker with a resolver that
/// borrows `base`'s forms. Results come with the external type events and
/// missing names each module added, in module order; `None` if any worker had
/// to modify the forms, in which case the caller redoes the work sequentially.
fn per_module_workers<R: Send>(
    base: &Resolver<'_>,
    count: usize,
    run: &(impl Fn(&mut Resolver<'_>, usize) -> R + Sync),
) -> Option<Vec<(R, Vec<ExternalTypeEvent>, Vec<TypeName>)>> {
    type Slot<R> = (R, Vec<ExternalTypeEvent>, Vec<TypeName>, bool);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(1, usize::from).min(count.max(1));
    let mut slots: Vec<Option<Slot<R>>> = (0..count).map(|_| None).collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                let next = &next;
                std::thread::Builder::new()
                    .stack_size(1 << 30)
                    .spawn_scoped(scope, move || {
                        let mut worker = base.borrowing_worker();
                        let mut done = Vec::new();
                        loop {
                            let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            if i >= count {
                                break;
                            }
                            let result = run(&mut worker, i);
                            let mutated = matches!(worker.forms, std::borrow::Cow::Owned(_));
                            done.push((i, (result, std::mem::take(&mut worker.external_types), std::mem::take(&mut worker.missing), mutated)));
                            if mutated {
                                break;
                            }
                        }
                        done
                    })
                    .expect("spawn definition worker")
            })
            .collect();
        for handle in handles {
            for (i, r) in handle.join().expect("definition worker panicked") {
                slots[i] = Some(r);
            }
        }
    });
    slots
        .into_iter()
        .map(|slot| match slot {
            Some((result, events, missing, false)) => Some((result, events, missing)),
            _ => None,
        })
        .collect()
}
fn source_location(b: &mut ReasonBuilder, source: &SourceOrigin) -> TermId {
    match (source.line, source.column) {
        (Some(line), Some(column)) => {
            let l = b.integer(line);
            let c = b.integer(column);
            b.tuple(vec![l, c])
        }
        (Some(line), None) => b.integer(line),
        _ => b.atom("undefined"),
    }
}
fn generated_variable(b: &mut ReasonBuilder) -> TermId {
    let var = b.atom("var");
    let generated = b.atom("generated");
    let yes = b.atom("true");
    let generated = b.tuple(vec![generated, yes]);
    let location = b.atom("location");
    let one = b.integer(1);
    let location = b.tuple(vec![location, one]);
    let ann = b.list(vec![generated, location]);
    let name = b.atom("_");
    b.tuple(vec![var, ann, name])
}
fn contract_capture(
    core: &CoreProgram,
    forms: &Forms,
    aliases: &AliasIndex,
    sources: &BTreeMap<ModuleId, FormSourceArena>,
    contract: &Contract,
    summary: &ContractSummary,
) -> Result<RawFormTerm, FormError> {
    let mut b = ReasonBuilder::new();
    let mut pairs = Vec::new();
    for (index, alternative) in summary.alternatives.iter().enumerate() {
        let signature = b.native(&alternative.signature.to_native_value());
        let mut cs = Vec::new();
        let ordered = if alternative.constraints.is_empty() {
            Vec::new()
        } else {
            let source = &forms.nodes[contract.alternatives[index].0];
            let module = &core.modules[source.module.0];
            let map = module
                .contract_variable_map(source.raw)
                .map_err(FormError::from)?;
            let mut order = module
                .map_iterator_pairs(map)
                .map_err(FormError::from)?
                .map(|(key, _)| match &module.terms[key.0] {
                    ErlangTerm::Atom(name) => Ok(VariableId::Atom(name.clone())),
                    ErlangTerm::Integer(name) => name
                        .parse()
                        .map(VariableId::Integer)
                        .map_err(|_| FormError::Transport("invalid compiler variable key".into())),
                    _ => Err(FormError::Transport(
                        "compiler variable key must be atom/integer".into(),
                    )),
                })
                .collect::<Result<Vec<_>, _>>()?;
            if order.iter().cloned().collect::<BTreeSet<_>>()
                != alternative.constraints.keys().cloned().collect()
            {
                return Err(FormError::Transport(
                    "compiler constraint key set differs from finalized binding keys".into(),
                ));
            }
            order.reverse();
            order
        };
        for id in ordered {
            let right = &alternative.constraints[&id];
            let left = match &id {
                VariableId::Atom(v) => Type::variable_atom(v),
                VariableId::Integer(v) => Type::variable_id(VariableId::Integer(v.clone())),
            };
            let left = b.native(&left.to_native_value());
            let right = b.native(&right.to_native_value());
            let subtype = b.atom("subtype");
            cs.push(b.tuple(vec![subtype, left, right]));
        }
        let cs = b.list(cs);
        pairs.push(b.tuple(vec![signature, cs]));
    }
    let pairs = b.list(pairs);
    let args = summary
        .arguments
        .iter()
        .map(|t| b.native(&t.to_native_value()))
        .collect();
    let args = b.list(args);
    let mut raw_forms = Vec::new();
    for id in &contract.alternatives {
        let (id, constraints) = match &forms.nodes[id.0].value {
            TypeForm::BoundedFunction {
                function,
                constraints,
            } => (*function, constraints.as_slice()),
            _ => (*id, &[][..]),
        };
        let form = &forms.nodes[id.0];
        let raw = RawFormTerm::new(sources[&form.module].terms.clone(), form.raw)
            .expect("validated form source");
        let raw = if contract.alternatives.len() == 1
            && !contract.callback
            && core.modules[form.module.0].name != "erlang"
        {
            let defs = aliases.prior(forms, &core.modules[form.module.0].name, contract.metadata);
            crate::native_form_preprocessing::massage(&raw, &defs)
        } else {
            raw
        };
        let form = b.copy(&raw);
        let constraints = constraints
            .iter()
            .map(|id| {
                let f = &forms.nodes[id.0];
                let r = RawFormTerm::new(sources[&f.module].terms.clone(), f.raw)
                    .expect("constraint source");
                b.copy(&r)
            })
            .collect();
        let constraints = b.list(constraints);
        raw_forms.push(b.tuple(vec![form, constraints]));
    }
    let raw_forms = b.list(raw_forms);
    let tag = b.atom("contract");
    let root = b.tuple(vec![tag, pairs, args, raw_forms]);
    Ok(b.finish(root))
}

fn import_failure(error: impl Into<FormError>) -> PreparationFailure {
    PreparationFailure {
        stage: PreparationStage::Import,
        site: None,
        error: error.into(),
        external_types: Vec::new(),
    }
}

/// `module`'s records in key order. Records are keyed by module first, so
/// they are one range of the map.
fn module_records(forms: &Forms, module: ModuleId) -> Vec<&crate::forms::Record> {
    let records = forms
        .records
        .range((module, String::new())..)
        .take_while(|((owner, _), _)| *owner == module)
        .map(|(_, record)| record)
        .filter(|record| record.module == module)
        .collect::<Vec<_>>();
    if crate::lattice::verify_fast_paths() {
        let scanned = forms.records.values().filter(|r| r.module == module).collect::<Vec<_>>();
        assert!(
            records.len() == scanned.len()
                && records.iter().zip(&scanned).all(|(a, b)| std::ptr::eq(*a, *b)),
            "module record range for {module:?}"
        );
    }
    records
}
fn ordered_declarations<'a>(forms: &'a Forms, module: &str) -> Vec<&'a crate::forms::Declaration> {
    // Declarations are keyed by module first: one range of the map.
    let start = TypeName {
        module: module.to_owned(),
        name: String::new(),
        arity: 0,
    };
    let mut entries = forms
        .declarations
        .range(start..)
        .take_while(|(name, _)| name.module == module)
        .map(|(_, d)| d)
        .filter(|d| d.name.module == module)
        .collect::<Vec<_>>();
    if crate::lattice::verify_fast_paths() {
        let scanned = forms
            .declarations
            .values()
            .filter(|d| d.name.module == module)
            .collect::<Vec<_>>();
        assert!(
            entries.len() == scanned.len()
                && entries.iter().zip(&scanned).all(|(a, b)| std::ptr::eq(*a, *b)),
            "module declaration range for {module}"
        );
    }
    entries.sort_by_key(|d| {
        (
            match d.kind {
                DeclarationKind::Nominal => 0,
                DeclarationKind::Opaque => 1,
                DeclarationKind::Alias => 2,
            },
            &d.name.name,
            d.name.arity,
        )
    });
    entries
}
fn reported_events(
    events: &[ExternalTypeEvent],
) -> Result<Vec<ExternalTypeEvent>, PreparationFailure> {
    let mut entries = events
        .iter()
        .map(|e| {
            let raw = e.to_arena();
            let normal =
                crate::term_order::TermOrderArena::new(raw.arena()).map_err(import_failure)?;
            Ok((e.clone(), raw, normal))
        })
        .collect::<Result<Vec<_>, PreparationFailure>>()?;
    entries.sort_by(|(_, a, an), (_, b, bn)| {
        an.compare(a.id(), bn, b.id())
            .expect("validated event terms")
    });
    entries.dedup_by(|(_, a, an), (_, b, bn)| {
        an.compare(a.id(), bn, b.id())
            .expect("validated event terms")
            .is_eq()
    });
    Ok(entries.into_iter().map(|(e, _, _)| e).collect())
}

/// Rebuild final tables while historical resolver cache handles retain their original snapshots.
fn finalize_tables(
    core: &CoreProgram,
    records: &BTreeMap<(ModuleId, String, usize), PreparedRecord>,
    opaques: &BTreeMap<TypeName, Type>,
    tables: &mut BTreeMap<String, ModuleTableHandle>,
) -> Result<(), FormError> {
    fn tuple(a: &ErlangTerms, id: TermId) -> &[TermId] {
        match &a.terms[id.0] {
            ErlangTerm::Tuple(xs) => xs,
            _ => unreachable!("constructed tuple"),
        }
    }
    fn atom(a: &ErlangTerms, id: TermId) -> &str {
        match &a.terms[id.0] {
            ErlangTerm::Atom(x) => x,
            _ => unreachable!("constructed atom"),
        }
    }
    fn list(a: &ErlangTerms, id: TermId) -> &[TermId] {
        match &a.terms[id.0] {
            ErlangTerm::List { items, .. } => items,
            ErlangTerm::Nil => &[],
            _ => unreachable!("constructed list"),
        }
    }
    let finalize = |tables: &BTreeMap<String, ModuleTableHandle>,
                    index: usize|
     -> Result<ModuleTableHandle, FormError> {
        let module = &core.modules[index];
        let old = &tables[&module.name];
        let a = old.arena();
        let mut b = ReasonBuilder::from_terms(a.terms.clone());
        let ErlangTerm::Map(pairs) = &a.terms[old.root().0] else {
            unreachable!("table handle")
        };
        let mut final_pairs = Vec::with_capacity(pairs.len());
        for &(key, value) in pairs {
            let k = tuple(a, key);
            let v = tuple(a, value);
            let value = match atom(a, k[0]) {
                "opaque" => {
                    let ErlangTerm::Integer(arity) = &a.terms[k[2].0] else {
                        unreachable!("constructed arity")
                    };
                    let name = TypeName {
                        module: module.name.clone(),
                        name: atom(a, k[1]).into(),
                        arity: arity.parse().expect("constructed arity"),
                    };
                    let cached = b.native(&opaques[&name].to_native_value());
                    b.tuple(vec![v[0], cached])
                }
                "record" => {
                    let tag = atom(a, k[1]);
                    let mut variants = Vec::new();
                    for &variant in list(a, v[1]) {
                        let items = tuple(a, variant);
                        let fields = list(a, items[1]);
                        let r = &records[&(ModuleId(index), tag.into(), fields.len())];
                        let mut final_fields = Vec::with_capacity(fields.len());
                        for (&field, ty) in fields.iter().zip(&r.field_types) {
                            let f = tuple(a, field);
                            let cached = b.native(&ty.to_native_value());
                            final_fields.push(b.tuple(vec![f[0], f[1], cached]));
                        }
                        let fields = b.list(final_fields);
                        variants.push(b.tuple(vec![items[0], fields]));
                    }
                    let variants = b.list(variants);
                    b.tuple(vec![v[0], variants])
                }
                _ => value,
            };
            final_pairs.push((key, value));
        }
        let root = b.push(ErlangTerm::Map(final_pairs));
        let raw = b.finish(root);
        ModuleTableHandle::at(raw.arena().clone(), root)
    };
    // Each module reads only its own table, so with distinct names the
    // tables can be rebuilt in parallel and stored in module order.
    let distinct = core.modules.iter().map(|m| &m.name).collect::<BTreeSet<_>>().len()
        == core.modules.len();
    if distinct {
        let indices = (0..core.modules.len()).collect::<Vec<_>>();
        let finalized = crate::ordered_parallel::setup_map(&indices, |&index| finalize(tables, index));
        for (module, table) in core.modules.iter().zip(finalized) {
            tables.insert(module.name.clone(), table?);
        }
    } else {
        for (index, module) in core.modules.iter().enumerate() {
            let table = finalize(tables, index)?;
            tables.insert(module.name.clone(), table);
        }
    }
    Ok(())
}

/// Each module's alias declarations sorted by position, so prior aliases are a
/// prefix instead of a scan over every declaration of every module.
struct AliasIndex {
    declarations: usize,
    by_module: BTreeMap<String, Vec<(usize, (String, usize))>>,
}
impl AliasIndex {
    fn new(forms: &Forms) -> Self {
        let mut by_module: BTreeMap<String, Vec<(usize, (String, usize))>> = BTreeMap::new();
        for d in forms.declarations.values() {
            if d.kind == DeclarationKind::Alias {
                by_module
                    .entry(d.name.module.clone())
                    .or_default()
                    .push((d.metadata, (d.name.name.clone(), d.name.arity)));
            }
        }
        for aliases in by_module.values_mut() {
            aliases.sort();
        }
        Self {
            declarations: forms.declarations.len(),
            by_module,
        }
    }
    /// Same result as `prior_aliases`; falls back to it if declarations changed.
    fn prior(&self, forms: &Forms, module: &str, metadata: usize) -> BTreeSet<(String, usize)> {
        if forms.declarations.len() != self.declarations {
            return prior_aliases(forms, module, metadata);
        }
        self.by_module
            .get(module)
            .map(|aliases| {
                aliases
                    .iter()
                    .take_while(|(m, _)| *m < metadata)
                    .map(|(_, name)| name.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}
fn prior_aliases(forms: &Forms, module: &str, metadata: usize) -> BTreeSet<(String, usize)> {
    forms
        .declarations
        .values()
        .filter(|d| {
            d.name.module == module && d.kind == DeclarationKind::Alias && d.metadata < metadata
        })
        .map(|d| (d.name.name.clone(), d.name.arity))
        .collect()
}

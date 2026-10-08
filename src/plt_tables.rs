use crate::{
    bifs::registry::Mfa,
    classic_persistence::{
        PersistenceError, atom_key, boundary, list_rows, map_rows, mfa_key, raw_key,
    },
    core::{CompilerProfile, CoreProgram, ErlangTerm, TermId},
    dependencies::Store,
    form_error::ReasonBuilder,
    module_dependencies::ModuleDependencies,
    native_form_environment::RawFormTerm,
    native_runtime_hash::NativeAtomLayout,
    program::Program,
};
use std::collections::{BTreeMap, BTreeSet};
type Result<T> = std::result::Result<T, PersistenceError>;
struct TargetCompletion<'a> {
    core: &'a CoreProgram,
    graph: crate::source_callgraph::SourceCallGraph,
    definitions: crate::compiled_definitions::PreparedDefinitions,
    analysis: crate::core_pipeline::CollectedAnalysis,
    callback_groups: crate::diagnostic_inputs::PreparedCallbackGroups,
}
pub struct CompletedPlt {
    tables: RawFormTerm,
    pub(crate) dependencies: ModuleDependencies,
    pub(crate) version: String,
}
fn rows(builder: &mut ReasonBuilder, rows: Vec<(RawFormTerm, TermId)>) -> TermId {
    let rows = rows
        .into_iter()
        .map(|(key, value)| {
            let key = builder.copy(&key);
            builder.tuple(vec![key, value])
        })
        .collect();
    builder.list(rows)
}
impl CompletedPlt {
    pub fn from_analysis(
        core: &CoreProgram,
        output: crate::compiled_analysis::AnalysisOutput,
    ) -> Result<Self> {
        if output
            .definitions
            .module_names
            .iter()
            .take(core.modules.len())
            .map(String::as_str)
            .ne(core.modules.iter().map(|m| m.name.as_str()))
        {
            return Err(boundary("prepared/compiler module identity mismatch"));
        }
        let crate::compiled_analysis::AnalysisOutput {
            library,
            graph,
            definitions,
            analysis,
            callback_groups,
            module_dependencies,
            ..
        } = output;
        let target = TargetCompletion {
            core,
            graph,
            definitions,
            analysis,
            callback_groups,
        };
        let layout = core
            .atom_layout
            .as_ref()
            .ok_or_else(|| boundary("callback table atom layout unavailable"))?;
        Self::complete(
            library,
            Some(target),
            &core.profile,
            layout,
            module_dependencies,
        )
    }
    pub fn from_retained(
        library: Store,
        profile: &CompilerProfile,
        layout: &NativeAtomLayout,
    ) -> Result<Self> {
        Self::complete(
            library,
            None,
            profile,
            layout,
            ModuleDependencies::default(),
        )
    }
    fn complete(
        library: Store,
        target: Option<TargetCompletion<'_>>,
        profile: &CompilerProfile,
        layout: &NativeAtomLayout,
        dependencies: ModuleDependencies,
    ) -> Result<Self> {
        let mut nonexports = Vec::new();
        let mut exported = BTreeSet::new();
        if let Some(target) = &target {
            exported = Program::compiler_exports(target.core)
                .map_err(boundary)?
                .into_iter()
                .map(|n| Mfa {
                    module: n.module,
                    function: n.name,
                    arity: n.arity,
                })
                .collect::<BTreeSet<_>>();

            for vertex in target
                .graph
                .source_graph()
                .ordered_vertices()
                .map_err(|e| boundary(format!("{e:?}")))?
            {
                if let crate::source_callgraph::GraphVertex::Named {
                    module,
                    function,
                    arity,
                } = vertex
                {
                    use num_traits::ToPrimitive;
                    let name = Mfa {
                        module,
                        function,
                        arity: arity
                            .to_usize()
                            .ok_or_else(|| boundary("nonexport arity outside schema"))?,
                    };
                    if !exported.contains(&name) {
                        nonexports.push(name);
                    }
                }
            }
        }
        let mut builder = ReasonBuilder::new();
        let mut info = BTreeMap::new();
        for row in library.prior_info_rows() {
            let result = row.result.append_raw(&mut builder);
            let arguments = row
                .arguments
                .iter()
                .map(|x| x.append_raw(&mut builder))
                .collect();
            let arguments = builder.list(arguments);
            let value = builder.tuple(vec![result, arguments]);
            info.insert(row.mfa.clone(), value);
        }
        if let Some(target) = &target {
            for (label, signature) in &target.analysis.state.signatures {
                if let Some(name) = target.graph.function_name(*label) {
                    if exported.contains(&name) {
                        let result = builder.native(&signature.result.to_native_value());
                        let args = signature
                            .arguments
                            .iter()
                            .map(|t| builder.native(&t.to_native_value()))
                            .collect();
                        let args = builder.list(args);
                        let value = builder.tuple(vec![result, args]);
                        info.insert(name, value);
                    } else {
                        info.remove(&name);
                    }
                }
            }
        }
        for name in &nonexports {
            info.remove(name);
        }
        let info_rows = info
            .into_iter()
            .map(|(name, value)| (mfa_key(&name), value))
            .collect();
        let info = rows(&mut builder, info_rows);
        let mut contracts = BTreeMap::new();
        for (name, contract) in &library.contracts {
            let value = builder.copy(&contract.captured_record());
            contracts.insert(name.clone(), value);
        }
        if let Some(target) = &target {
            for name in target.analysis.state.contracts.active_names() {
                let contract = target
                    .definitions
                    .contracts
                    .get_callable(name)
                    .ok_or_else(|| boundary("active contract has no prepared capture"))?;
                contracts.insert(name.clone(), builder.copy(&contract.capture));
            }
        }
        for name in &nonexports {
            contracts.remove(name);
        }
        let contracts = rows(
            &mut builder,
            contracts
                .into_iter()
                .map(|(name, value)| (mfa_key(&name), value))
                .collect(),
        );
        let mut groups = crate::native_ets_set::NativeEtsSet::default();
        let prior_callbacks = library
            .callbacks
            .original()
            .map_err(|e| boundary(format!("{e:?}")))?;
        let mut original_groups = list_rows(prior_callbacks.arena())?
            .into_iter()
            .map(|row| {
                let ErlangTerm::Tuple(fields) = row.term() else {
                    return Err(boundary("prior callback group expected"));
                };
                if fields.len() != 2 {
                    return Err(boundary("prior callback group arity differs"));
                }
                Ok((
                    RawFormTerm::new(row.arena().clone(), fields[0]).map_err(boundary)?,
                    RawFormTerm::new(row.arena().clone(), fields[1]).map_err(boundary)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut ordering_error = None;
        original_groups.sort_by(|(a, _), (b, _)| {
            match crate::term_order::compare_terms(a.arena(), a.id(), b.arena(), b.id()) {
                Ok(order) => order,
                Err(error) => {
                    ordering_error = Some(error);
                    std::cmp::Ordering::Equal
                }
            }
        });
        if let Some(error) = ordering_error {
            return Err(boundary(error));
        }
        for (module, callbacks) in original_groups {
            let key = raw_key(&module)?;
            groups
                .insert(key, callbacks, layout)
                .map_err(|e| boundary(e.to_string()))?;
        }
        for row in list_rows(prior_callbacks.arena())? {
            let ErlangTerm::Tuple(fields) = row.term() else {
                unreachable!()
            };
            let module = RawFormTerm::new(row.arena().clone(), fields[0]).map_err(boundary)?;
            let mut retained = false;
            for group in prior_callbacks.groups() {
                if crate::term_order::compare_map_keys(
                    module.arena(),
                    module.id(),
                    group.module.arena(),
                    group.module.id(),
                )
                .map_err(boundary)?
                .is_eq()
                {
                    retained = true;
                    break;
                }
            }
            if !retained {
                groups
                    .remove(&raw_key(&module)?, layout)
                    .map_err(|e| boundary(e.to_string()))?;
            }
        }
        if let Some(target) = &target {
            for group in &target.callback_groups.groups {
                if !list_rows(&group.callbacks)?.is_empty() {
                    groups
                        .insert(raw_key(&group.module)?, group.callbacks.clone(), layout)
                        .map_err(|e| boundary(e.to_string()))?;
                }
            }
        }
        let mut callback_rows = Vec::new();
        for (_, callbacks) in groups.rows() {
            for row in list_rows(callbacks)? {
                let ErlangTerm::Tuple(fields) = row.term() else {
                    return Err(boundary("callback row expected"));
                };
                if fields.len() != 2 {
                    return Err(boundary("callback row arity differs"));
                }
                let key = RawFormTerm::new(row.arena().clone(), fields[0]).map_err(boundary)?;
                let value = RawFormTerm::new(row.arena().clone(), fields[1]).map_err(boundary)?;
                callback_rows.push((key, builder.copy(&value)));
            }
        }
        let callbacks = rows(&mut builder, callback_rows);
        let mut tables = BTreeMap::new();
        let prior_type_root = library.type_tables.root;
        let prior_tables =
            RawFormTerm::new(std::sync::Arc::new(library.type_tables), prior_type_root)
                .map_err(boundary)?;
        for (name, table) in map_rows(&prior_tables)? {
            let ErlangTerm::Atom(name) = name.term() else {
                return Err(boundary("prior type module name is not atom"));
            };
            tables.insert(name.clone(), table);
        }
        if let Some(target) = &target {
            for (name, table) in &target.definitions.tables {
                if matches!(&table.arena().terms[table.root().0], ErlangTerm::Map(rows) if rows.is_empty())
                {
                    continue;
                }
                tables.insert(name.clone(), table.raw());
            }
        }
        let type_rows = tables
            .into_iter()
            .map(|(name, table)| (atom_key(&name), builder.copy(&table)))
            .collect();
        let types = rows(&mut builder, type_rows);
        let prior_export_root = library.exported_types.root;
        let prior_exports = RawFormTerm::new(
            std::sync::Arc::new(library.exported_types),
            prior_export_root,
        )
        .map_err(boundary)?;
        let mut exp_keys = BTreeMap::new();
        for raw in list_rows(&prior_exports)? {
            exp_keys.insert(raw_key(&raw)?, raw);
        }
        if let Some(target) = &target {
            for name in &target.definitions.exported_types {
                let mfa = Mfa {
                    module: name.module.clone(),
                    function: name.name.clone(),
                    arity: name.arity,
                };
                let key = mfa_key(&mfa);
                exp_keys.insert(raw_key(&key)?, key);
            }
        }
        let entries = exp_keys
            .into_values()
            .map(|key| (builder.copy(&key), builder.nil()))
            .collect();
        let exports = builder.push(ErlangTerm::Map(entries));
        let root = builder.tuple(vec![info, contracts, callbacks, types, exports]);
        Ok(Self {
            tables: builder.finish(root),
            dependencies,
            version: format!("v{}", profile.dialyzer),
        })
    }
    pub(crate) fn field(&self, index: usize) -> RawFormTerm {
        let ErlangTerm::Tuple(fields) = self.tables.term() else {
            unreachable!()
        };
        RawFormTerm::new(self.tables.arena().clone(), fields[index])
            .expect("completed table handle")
    }
}

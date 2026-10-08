use crate::{
    bifs::registry::{Mfa, Registry},
    call_transfer::Signature,
    constraints::generation_capture_headers::GenerationRuntimeCaptures,
    contract_book::ContractBook,
    core::CoreExpr,
    dataflow_nodes::DataflowNodes,
    labels::CoreLabel,
    lattice::{LatticeError, Type},
    program::{FunctionName, ModuleId, NodeRef, Program},
    source_callgraph::{GraphVertex, SourceCallGraph},
    success_phase::{FunctionTypes, persisted_signature, reached_fixpoint_strict, updated_types},
};
use std::collections::BTreeMap;

pub struct AnalysisState {
    pub signatures: FunctionTypes,
    pub contracts: ContractBook,
}
pub use crate::source_schedule::OrderedScc;
pub trait PhaseSchedule {
    type Error;
    fn runtime(&self) -> &GenerationRuntimeCaptures;
    fn finalize(&mut self, graph: &SourceCallGraph) -> Result<Vec<OrderedScc>, Self::Error>;
    fn modules_from_functions(
        &mut self,
        graph: &SourceCallGraph,
        changed: &[CoreLabel],
    ) -> Result<Vec<ModuleId>, Self::Error>;
    fn reset_from_functions(
        &mut self,
        graph: &SourceCallGraph,
        changed: &[CoreLabel],
    ) -> Result<Vec<OrderedScc>, Self::Error>;
}
pub struct SccRequest<'a> {
    pub runtime: &'a GenerationRuntimeCaptures,
    pub ordered_mfas: &'a [Mfa],
    pub next_core_label: CoreLabel,
    pub previous: &'a FunctionTypes,
    pub state: &'a AnalysisState,
}
pub struct ModuleRequest<'a> {
    pub runtime: &'a GenerationRuntimeCaptures,
    pub module: ModuleId,
    pub previous: &'a FunctionTypes,
    pub state: &'a AnalysisState,
}
pub trait AnalysisPhases {
    type Error;
    fn publish<'a>(
        &mut self,
        signatures: impl Iterator<Item = (CoreLabel, &'a Signature)>,
        contracts: impl Iterator<Item = &'a Mfa>,
    );
    fn analyze_scc(&mut self, request: SccRequest<'_>) -> Result<FunctionTypes, Self::Error>;
    fn refine_module(&mut self, request: ModuleRequest<'_>) -> Result<FunctionTypes, Self::Error>;
}
#[derive(Debug)]
pub enum AnalysisError<P, S> {
    Boundary(String),
    Lattice(LatticeError),
    Phase(P),
    Schedule(S),
}
pub struct CoreAnalyzer<'a> {
    pub nodes: &'a DataflowNodes<'a>,
    pub program: &'a Program,
    pub graph: &'a SourceCallGraph,
    pub bifs: &'a Registry,
    pub next_core_label: CoreLabel,
}
#[derive(Clone, Copy)]
struct FunctionInfo {
    label: CoreLabel,
    arity: usize,
}
impl CoreAnalyzer<'_> {
    fn collect(&self, root: NodeRef) -> Result<Vec<FunctionInfo>, String> {
        let mut pending = vec![(root, false)];
        let mut functions = Vec::new();
        while let Some((node, finish)) = pending.pop() {
            if !finish {
                pending.push((node, true));
                pending.extend(
                    self.nodes
                        .native_subtrees(node)?
                        .into_iter()
                        .rev()
                        .map(|node| (node, false)),
                );
            } else if let CoreExpr::Fun { params, .. } = self.nodes.expression(node)? {
                functions.push(FunctionInfo {
                    label: self.nodes.label(node)?,
                    arity: params.len(),
                });
            }
        }
        functions.reverse();
        Ok(functions)
    }
    fn scc_functions(&self, names: &[Mfa]) -> Result<Vec<FunctionInfo>, String> {
        let mut functions = Vec::new();
        for mfa in names {
            let name = FunctionName {
                module: mfa.module.clone(),
                name: mfa.function.clone(),
                arity: mfa.arity,
            };
            let id = self
                .program
                .named
                .get(&name)
                .ok_or("SCC named function absent")?;
            let function = &self.program.functions[id.0];
            functions.extend(self.collect(NodeRef {
                module: function.module,
                node: function.node.0,
            })?);
        }
        Ok(functions)
    }
    fn previous(&self, info: &[FunctionInfo], state: &AnalysisState) -> FunctionTypes {
        info.iter()
            .map(|function| {
                (
                    function.label,
                    state
                        .signatures
                        .get(&function.label)
                        .cloned()
                        .unwrap_or_else(|| Signature {
                            arguments: vec![Type::any(); function.arity],
                            result: Type::any(),
                        }),
                )
            })
            .collect()
    }
    pub fn module_previous(
        &self,
        module: ModuleId,
        state: &AnalysisState,
    ) -> Result<FunctionTypes, String> {
        let core = self.nodes.module(module)?;
        let functions = self.collect(NodeRef {
            module,
            node: core.core.0,
        })?;
        Ok(self.previous(&functions, state))
    }
    pub fn run<P: AnalysisPhases, S: PhaseSchedule>(
        &self,
        mut state: AnalysisState,
        phases: &mut P,
        schedule: &mut S,
    ) -> Result<AnalysisState, AnalysisError<P::Error, S::Error>> {
        let mut sccs = schedule
            .finalize(self.graph)
            .map_err(AnalysisError::Schedule)?;
        phases.publish(
            state.signatures.iter().map(|(label, signature)| (*label, signature)),
            state.contracts.active_names(),
        );
        loop {
            let mut scc_changed = Vec::new();
            for scc in sccs {
                let mut timings = crate::phase_timings::PhaseTimings::new("scc_orchestration");
                let mfas = scc
                    .members
                    .iter()
                    .filter_map(|vertex| match vertex {
                        GraphVertex::Named {
                            module,
                            function,
                            arity,
                        } => Some(
                            arity
                                .to_string()
                                .parse::<usize>()
                                .map(|arity| Mfa {
                                    module: module.clone(),
                                    function: function.clone(),
                                    arity,
                                })
                                .map_err(|_| {
                                    AnalysisError::Boundary(
                                        "SCC arity outside platform range".into(),
                                    )
                                }),
                        ),
                        _ => None,
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let all = self.scc_functions(&mfas).map_err(AnalysisError::Boundary)?;
                let previous = self.previous(&all, &state);
                timings.mark("function_info_and_previous");
                let generated = phases
                    .analyze_scc(SccRequest {
                        runtime: schedule.runtime(),
                        ordered_mfas: &mfas,
                        next_core_label: self.next_core_label,
                        previous: &previous,
                        state: &state,
                    })
                    .map_err(AnalysisError::Phase)?;
                timings.mark("scc_phase");
                let filtered: FunctionTypes = generated
                    .into_iter()
                    .filter(|(label, _)| previous.contains_key(label))
                    .collect();
                timings.mark("filter_bindings");
                let named = filtered
                    .iter()
                    .filter_map(|(label, signature)| {
                        self.graph.function_name(*label).map(|name| {
                            (
                                name,
                                Type::function(
                                    Some(signature.arguments.clone()),
                                    signature.result.clone(),
                                ),
                            )
                        })
                    })
                    .collect::<BTreeMap<_, _>>();
                timings.mark("named_signatures");
                let validated = state
                    .contracts
                    .validate(&named, self.bifs)
                    .map_err(AnalysisError::Lattice)?;
                timings.mark("validate_contracts");
                for (label, signature) in &filtered {
                    state.signatures.insert(
                        *label,
                        persisted_signature(signature).map_err(AnalysisError::Lattice)?,
                    );
                }
                timings.mark("persist_signatures");
                let newly_activated = state.contracts.commit(validated);
                phases.publish(
                    filtered.keys().map(|label| (*label, &state.signatures[label])),
                    newly_activated.iter(),
                );
                timings.mark("commit_and_publish");
                let changed = !newly_activated.is_empty()
                    || !reached_fixpoint_strict(&previous, &filtered)
                        .map_err(AnalysisError::Lattice)?;
                timings.mark("fixpoint");
                if changed {
                    scc_changed.extend(all.into_iter().map(|function| function.label));
                }
            }
            if scc_changed.is_empty() {
                return Ok(state);
            }
            let modules = schedule
                .modules_from_functions(self.graph, &scc_changed)
                .map_err(AnalysisError::Schedule)?;
            let mut module_changed = Vec::new();
            for module in modules {
                let mut timings = crate::phase_timings::PhaseTimings::new("module_orchestration");
                let previous = self
                    .module_previous(module, &state)
                    .map_err(AnalysisError::Boundary)?;
                timings.mark("previous");
                let next = phases
                    .refine_module(ModuleRequest {
                        runtime: schedule.runtime(),
                        module,
                        previous: &previous,
                        state: &state,
                    })
                    .map_err(AnalysisError::Phase)?;
                timings.mark("module_phase");
                let changed_start = module_changed.len();
                for (label, signature) in
                    updated_types(&previous, &next).map_err(AnalysisError::Lattice)?
                {
                    state.signatures.insert(
                        label,
                        persisted_signature(&signature).map_err(AnalysisError::Lattice)?,
                    );
                    module_changed.push(label);
                }
                timings.mark("updated_types_and_persist");
                phases.publish(
                    module_changed[changed_start..].iter().map(|label| (*label, &state.signatures[label])),
                    std::iter::empty(),
                );
            }
            if module_changed.is_empty() {
                return Ok(state);
            }
            sccs = schedule
                .reset_from_functions(self.graph, &module_changed)
                .map_err(AnalysisError::Schedule)?;
        }
    }
}

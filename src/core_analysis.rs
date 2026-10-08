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
    /// Whether batches may hold several independent SCCs or modules. When
    /// false, every batch has exactly one element and the analysis is the
    /// original sequential traversal.
    fn parallel(&self) -> bool {
        false
    }
    /// Analyzes SCCs that do not call each other. Results are in input order.
    fn analyze_scc_batch(
        &mut self,
        requests: Vec<SccRequest<'_>>,
    ) -> Vec<Result<FunctionTypes, Self::Error>> {
        requests.into_iter().map(|r| self.analyze_scc(r)).collect()
    }
    /// Refines modules against the same state. Results are in input order.
    fn refine_module_batch(
        &mut self,
        requests: Vec<ModuleRequest<'_>>,
    ) -> Vec<Result<FunctionTypes, Self::Error>> {
        requests.into_iter().map(|r| self.refine_module(r)).collect()
    }
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
    /// SCC names, skipping anonymous vertices.
    fn scc_mfas(scc: &OrderedScc) -> Result<Vec<Mfa>, String> {
        scc.members
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
                        .map_err(|_| "SCC arity outside platform range".to_owned()),
                ),
                _ => None,
            })
            .collect()
    }
    /// Graph vertex of each function label (the first named vertex, else the label).
    fn label_vertices(&self) -> std::collections::HashMap<CoreLabel, GraphVertex> {
        let mut index = std::collections::HashMap::new();
        for (vertex, label) in &self.graph.named_labels {
            index.entry(*label).or_insert_with(|| vertex.clone());
        }
        index
    }
    /// Every vertex that calls a changed function, directly or transitively,
    /// including the changed functions. If one member of an SCC is reached,
    /// all are, so filtering the full SCC list keeps whole SCCs in a valid
    /// callees-first order.
    fn reaching_callers(
        &self,
        label_vertices: &std::collections::HashMap<CoreLabel, GraphVertex>,
        changed: &[CoreLabel],
    ) -> std::collections::BTreeSet<GraphVertex> {
        let mut seen = std::collections::BTreeSet::new();
        let mut pending: Vec<GraphVertex> = changed
            .iter()
            .map(|label| label_vertices.get(label).cloned().unwrap_or(GraphVertex::Label(*label)))
            .collect();
        while let Some(vertex) = pending.pop() {
            if let Some(callers) = self.graph.in_neighbours(&vertex) {
                pending.extend(callers.iter().filter(|c| !seen.contains(*c)).cloned());
            }
            seen.insert(vertex);
        }
        seen
    }
    /// Modules owning the reached named functions, in module order.
    fn modules_of(&self, reached: &std::collections::BTreeSet<GraphVertex>) -> Vec<ModuleId> {
        let names: std::collections::BTreeSet<&String> = reached
            .iter()
            .filter_map(|v| match v {
                GraphVertex::Named { module, .. } => Some(module),
                _ => None,
            })
            .collect();
        self.graph
            .module_names
            .iter()
            .enumerate()
            .filter(|(_, name)| names.contains(name))
            .map(|(i, _)| ModuleId(i))
            .collect()
    }
    /// Groups SCCs (callees first) into waves: no SCC calls another SCC in
    /// the same wave, so a wave can be analyzed in parallel against the state
    /// left by earlier waves. Falls back to one SCC per wave if the order is
    /// not callees-first.
    fn waves(&self, sccs: &[OrderedScc]) -> Vec<Vec<usize>> {
        let sequential = || (0..sccs.len()).map(|i| vec![i]).collect();
        let mut index = BTreeMap::new();
        for (i, scc) in sccs.iter().enumerate() {
            for member in &scc.members {
                index.insert(member, i);
            }
        }
        let mut level = vec![0usize; sccs.len()];
        for (i, scc) in sccs.iter().enumerate() {
            for member in &scc.members {
                for caller in self.graph.in_neighbours(member).unwrap_or(&[]) {
                    match index.get(caller) {
                        Some(&j) if j > i => level[j] = level[j].max(level[i] + 1),
                        Some(&j) if j < i => return sequential(),
                        _ => {}
                    }
                }
            }
        }
        let mut waves: Vec<Vec<usize>> = Vec::new();
        for (i, l) in level.into_iter().enumerate() {
            if waves.len() <= l {
                waves.resize(l + 1, Vec::new());
            }
            waves[l].push(i);
        }
        waves
    }
    pub fn run<P: AnalysisPhases, S: PhaseSchedule>(
        &self,
        mut state: AnalysisState,
        phases: &mut P,
        schedule: &mut S,
    ) -> Result<AnalysisState, AnalysisError<P::Error, S::Error>> {
        let mut schedule_timings = crate::phase_timings::PhaseTimings::new("schedule");
        schedule_timings.label(|| "finalize".into());
        let mut sccs = schedule
            .finalize(self.graph)
            .map_err(AnalysisError::Schedule)?;
        drop(schedule_timings);
        // Parallel mode schedules from the full SCC list instead of rebuilding
        // OTP's runtime-ordered digraphs each iteration (see `reaching_callers`).
        let all_sccs = sccs.clone();
        let label_vertices = self.label_vertices();
        phases.publish(
            state.signatures.iter().map(|(label, signature)| (*label, signature)),
            state.contracts.active_names(),
        );
        let mut iteration = 0usize;
        loop {
            iteration += 1;
            let mut scc_changed = Vec::new();
            let waves = if phases.parallel() {
                self.waves(&sccs)
            } else {
                (0..sccs.len()).map(|i| vec![i]).collect()
            };
            for wave in waves {
                let mut timings = crate::phase_timings::PhaseTimings::new("scc_wave");
                timings.label(|| format!("iter{iteration} wave of {}", wave.len()));
                let mut prepared = Vec::with_capacity(wave.len());
                for &i in &wave {
                    let mfas = Self::scc_mfas(&sccs[i]).map_err(AnalysisError::Boundary)?;
                    let all = self.scc_functions(&mfas).map_err(AnalysisError::Boundary)?;
                    let previous = self.previous(&all, &state);
                    prepared.push((mfas, all, previous));
                }
                timings.mark("function_info_and_previous");
                let requests = prepared
                    .iter()
                    .map(|(mfas, _, previous)| SccRequest {
                        runtime: schedule.runtime(),
                        ordered_mfas: mfas,
                        next_core_label: self.next_core_label,
                        previous,
                        state: &state,
                    })
                    .collect();
                let results = phases.analyze_scc_batch(requests);
                timings.mark("scc_phase");
                for ((_, all, previous), generated) in prepared.into_iter().zip(results) {
                    let generated = generated.map_err(AnalysisError::Phase)?;
                    let filtered: FunctionTypes = generated
                        .into_iter()
                        .filter(|(label, _)| previous.contains_key(label))
                        .collect();
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
                    let validated = state
                        .contracts
                        .validate(&named, self.bifs)
                        .map_err(AnalysisError::Lattice)?;
                    for (label, signature) in &filtered {
                        state.signatures.insert(
                            *label,
                            persisted_signature(signature).map_err(AnalysisError::Lattice)?,
                        );
                    }
                    let newly_activated = state.contracts.commit(validated);
                    phases.publish(
                        filtered.keys().map(|label| (*label, &state.signatures[label])),
                        newly_activated.iter(),
                    );
                    let changed = !newly_activated.is_empty()
                        || !reached_fixpoint_strict(&previous, &filtered)
                            .map_err(AnalysisError::Lattice)?;
                    if changed {
                        scc_changed.extend(all.into_iter().map(|function| function.label));
                    }
                }
                timings.mark("commit_and_publish");
            }
            if scc_changed.is_empty() {
                return Ok(state);
            }
            let mut schedule_timings = crate::phase_timings::PhaseTimings::new("schedule");
            schedule_timings.label(|| format!("iter{iteration} modules_from_functions"));
            let modules = if phases.parallel() {
                self.modules_of(&self.reaching_callers(&label_vertices, &scc_changed))
            } else {
                schedule
                    .modules_from_functions(self.graph, &scc_changed)
                    .map_err(AnalysisError::Schedule)?
            };
            drop(schedule_timings);
            // In parallel mode all modules are refined against the state at the
            // start of the pass (OTP's coordinator also refines concurrently);
            // the enclosing fixpoint loop absorbs the difference.
            let batches: Vec<Vec<ModuleId>> = if phases.parallel() {
                vec![modules]
            } else {
                modules.into_iter().map(|m| vec![m]).collect()
            };
            let mut module_changed = Vec::new();
            for batch in batches {
                let mut timings = crate::phase_timings::PhaseTimings::new("module_batch");
                timings.label(|| format!("iter{iteration} modules {}", batch.len()));
                let previous = batch
                    .iter()
                    .map(|module| self.module_previous(*module, &state))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(AnalysisError::Boundary)?;
                let requests = batch
                    .iter()
                    .zip(&previous)
                    .map(|(module, previous)| ModuleRequest {
                        runtime: schedule.runtime(),
                        module: *module,
                        previous,
                        state: &state,
                    })
                    .collect();
                let results = phases.refine_module_batch(requests);
                timings.mark("module_phase");
                for (previous, next) in previous.into_iter().zip(results) {
                    let next = next.map_err(AnalysisError::Phase)?;
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
                    phases.publish(
                        module_changed[changed_start..]
                            .iter()
                            .map(|label| (*label, &state.signatures[label])),
                        std::iter::empty(),
                    );
                }
                timings.mark("updated_types_and_persist");
            }
            if module_changed.is_empty() {
                return Ok(state);
            }
            let mut schedule_timings = crate::phase_timings::PhaseTimings::new("schedule");
            schedule_timings.label(|| format!("iter{iteration} reset_from_functions"));
            sccs = if phases.parallel() {
                let reached = self.reaching_callers(&label_vertices, &module_changed);
                all_sccs
                    .iter()
                    .filter(|scc| scc.members.iter().any(|m| reached.contains(m)))
                    .cloned()
                    .collect()
            } else {
                schedule
                    .reset_from_functions(self.graph, &module_changed)
                    .map_err(AnalysisError::Schedule)?
            };
            drop(schedule_timings);
        }
    }
}

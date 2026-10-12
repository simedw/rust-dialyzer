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
pub struct SccJob<'a> {
    pub ordered_mfas: &'a [Mfa],
    pub previous: &'a FunctionTypes,
}
pub struct SccGraph<'a> {
    pub runtime: &'a GenerationRuntimeCaptures,
    pub next_core_label: CoreLabel,
    pub jobs: Vec<SccJob<'a>>,
    /// Wave of each SCC: one more than the highest wave among its callees.
    pub levels: &'a [usize],
    pub schedule: crate::ordered_parallel::JobGraph<'a>,
}
/// Signatures stored by one SCC commit and the contracts it activated.
pub struct Published {
    pub signatures: Vec<(CoreLabel, Signature)>,
    pub contracts: Vec<Mfa>,
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
    /// Whether `analyze_scc_graph` may replace wave-by-wave batches.
    fn dependency_scheduled(&self) -> bool {
        false
    }
    /// Analyzes every SCC of `graph`, starting each once its callees have
    /// committed. Each SCC must see exactly the state it would see if the
    /// SCCs ran in waves of `graph.levels`: everything committed by lower
    /// levels and nothing from its own or higher levels. `commit` stores a
    /// result and returns what to publish; publication into the shared state
    /// happens here, after the last SCC.
    fn analyze_scc_graph<C>(
        &mut self,
        _graph: SccGraph<'_>,
        _failed: Option<(u64, C)>,
        _commit: impl FnMut(usize, Result<FunctionTypes, Self::Error>) -> Result<Published, C>,
    ) -> Result<(), C> {
        unreachable!("analyze_scc_graph without dependency_scheduled")
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
        self.collect_counted(root, &mut 0)
    }
    /// `collect`, adding the number of nodes visited to `nodes`.
    fn collect_counted(&self, root: NodeRef, nodes: &mut u64) -> Result<Vec<FunctionInfo>, String> {
        let mut pending = vec![(root, false)];
        let mut functions = Vec::new();
        while let Some((node, finish)) = pending.pop() {
            if !finish {
                *nodes += 1;
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
        self.scc_functions_counted(names, &mut 0)
    }
    fn scc_functions_counted(&self, names: &[Mfa], nodes: &mut u64) -> Result<Vec<FunctionInfo>, String> {
        let mut functions = Vec::new();
        for mfa in names {
            let name = FunctionName {
                module: mfa.module.clone().to_string(),
                name: mfa.function.clone().to_string(),
                arity: mfa.arity,
            };
            let id = self
                .program
                .named
                .get(&name)
                .ok_or("SCC named function absent")?;
            let function = &self.program.functions[id.0];
            functions.extend(self.collect_counted(
                NodeRef {
                    module: function.module,
                    node: function.node.0,
                },
                nodes,
            )?);
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
                            module: module.clone().into(),
                            function: function.clone().into(),
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
        let names: std::collections::BTreeSet<&str> = reached
            .iter()
            .filter_map(|v| match v {
                GraphVertex::Named { module, .. } => Some(module.as_str()),
                _ => None,
            })
            .collect();
        self.graph
            .module_names
            .iter()
            .enumerate()
            .filter(|(_, name)| names.contains(name.as_str()))
            .map(|(i, _)| ModuleId(i))
            .collect()
    }
    /// Groups SCCs (callees first) into waves: no SCC calls another SCC in
    /// the same wave, so a wave can be analyzed in parallel against the state
    /// left by earlier waves. Falls back to one SCC per wave if the order is
    /// not callees-first.
    fn waves(&self, sccs: &[OrderedScc]) -> Vec<Vec<usize>> {
        let Some((level, _)) = self.levels(sccs) else {
            return (0..sccs.len()).map(|i| vec![i]).collect();
        };
        let mut waves: Vec<Vec<usize>> = Vec::new();
        for (i, l) in level.into_iter().enumerate() {
            if waves.len() <= l {
                waves.resize(l + 1, Vec::new());
            }
            waves[l].push(i);
        }
        waves
    }
    /// The wave of each SCC and the SCCs calling it, or None if the order is
    /// not callees-first.
    fn levels(&self, sccs: &[OrderedScc]) -> Option<(Vec<usize>, Vec<Vec<usize>>)> {
        let mut index = BTreeMap::new();
        for (i, scc) in sccs.iter().enumerate() {
            for member in &scc.members {
                index.insert(member, i);
            }
        }
        let mut level = vec![0usize; sccs.len()];
        let mut callers = vec![Vec::new(); sccs.len()];
        for (i, scc) in sccs.iter().enumerate() {
            for member in &scc.members {
                for caller in self.graph.in_neighbours(member).unwrap_or_default() {
                    match index.get(caller) {
                        Some(&j) if j > i => {
                            level[j] = level[j].max(level[i] + 1);
                            callers[i].push(j);
                        }
                        Some(&j) if j < i => return None,
                        _ => {}
                    }
                }
            }
            callers[i].sort_unstable();
            callers[i].dedup();
        }
        Some((level, callers))
    }
    fn analyze_wave<P: AnalysisPhases, S: PhaseSchedule>(
        &self,
        iteration: usize,
        sccs: &[OrderedScc],
        wave: &[usize],
        state: &mut AnalysisState,
        phases: &mut P,
        schedule: &S,
        scc_changed: &mut Vec<CoreLabel>,
    ) -> Result<(), AnalysisError<P::Error, S::Error>> {
        let mut timings = crate::phase_timings::PhaseTimings::new("scc_wave");
        timings.label(|| format!("iter{iteration} wave of {}", wave.len()));
        let mut prepared = Vec::with_capacity(wave.len());
        for &i in wave {
            let mfas = Self::scc_mfas(&sccs[i]).map_err(AnalysisError::Boundary)?;
            let all = self.scc_functions(&mfas).map_err(AnalysisError::Boundary)?;
            let previous = self.previous(&all, state);
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
                state,
            })
            .collect();
        let results = phases.analyze_scc_batch(requests);
        timings.mark("scc_phase");
        for ((_, all, previous), generated) in prepared.into_iter().zip(results) {
            let generated = generated.map_err(AnalysisError::Phase)?;
            let (published, changed) = self.commit_scc(state, &previous, generated)?;
            phases.publish(
                published.signatures.iter().map(|(label, signature)| (*label, signature)),
                published.contracts.iter(),
            );
            if changed {
                scc_changed.extend(all.into_iter().map(|function| function.label));
            }
        }
        timings.mark("commit_and_publish");
        Ok(())
    }
    /// Analyzes one iteration's SCCs with `AnalysisPhases::analyze_scc_graph`.
    /// Like `analyze_wave` over all waves; fails with the error the waves
    /// would reach first.
    #[allow(clippy::too_many_arguments)]
    fn analyze_graph<P: AnalysisPhases, S: PhaseSchedule>(
        &self,
        iteration: usize,
        sccs: &[OrderedScc],
        levels: Vec<usize>,
        callers: Vec<Vec<usize>>,
        state: &mut AnalysisState,
        phases: &mut P,
        schedule: &S,
        scc_changed: &mut Vec<CoreLabel>,
    ) -> Result<(), AnalysisError<P::Error, S::Error>> {
        let mut timings = crate::phase_timings::PhaseTimings::new("scc_graph");
        timings.label(|| format!("iter{iteration} sccs {} waves {}", sccs.len(), levels.iter().max().map_or(0, |l| l + 1)));
        let prepared = {
            let state = &*state;
            let indices: Vec<usize> = (0..sccs.len()).collect();
            crate::ordered_parallel::parallel_map(&indices, |&i| {
                let mfas = Self::scc_mfas(&sccs[i])?;
                let mut nodes = 0;
                let all = self.scc_functions_counted(&mfas, &mut nodes)?;
                let previous = self.previous(&all, state);
                Ok::<_, String>((mfas, all, previous, nodes))
            })
        };
        // A wave stops at its first preparation error before analyzing.
        let rank = |i: usize| (levels[i] * sccs.len() + i) as u64;
        let failed = prepared
            .iter()
            .enumerate()
            .filter_map(|(i, prepared)| prepared.as_ref().err().map(|error| (levels[i], i, error)))
            .min_by_key(|(level, i, _)| (*level, *i))
            .map(|(level, _, error)| ((level * sccs.len()) as u64, AnalysisError::Boundary(error.clone())));
        // Longest remaining chain of callers, weighted by Core size.
        let mut priority = vec![0u64; sccs.len()];
        for i in (0..sccs.len()).rev() {
            let own = prepared[i].as_ref().map_or(0, |(_, _, _, nodes)| *nodes);
            priority[i] = own + callers[i].iter().map(|&j| priority[j]).max().unwrap_or(0);
        }
        let ranks: Vec<u64> = (0..sccs.len()).map(rank).collect();
        timings.mark("prepare");
        let empty = FunctionTypes::new();
        let jobs = prepared
            .iter()
            .map(|prepared| match prepared {
                Ok((mfas, _, previous, _)) => SccJob { ordered_mfas: mfas, previous },
                Err(_) => SccJob { ordered_mfas: &[], previous: &empty },
            })
            .collect();
        let graph = SccGraph {
            runtime: schedule.runtime(),
            next_core_label: self.next_core_label,
            jobs,
            levels: &levels,
            schedule: crate::ordered_parallel::JobGraph { callers: &callers, priority: &priority, rank: &ranks },
        };
        if crate::job_trace::enabled() {
            crate::job_trace::record(|| {
                let sccs_json: Vec<serde_json::Value> = sccs
                    .iter()
                    .map(|scc| {
                        let mut mods: Vec<&str> = scc
                            .members
                            .iter()
                            .filter_map(|v| match v {
                                GraphVertex::Named { module, .. } => Some(module.as_str()),
                                _ => None,
                            })
                            .collect();
                        mods.sort_unstable();
                        mods.dedup();
                        serde_json::json!({"id":scc.identity,"mods":mods,"n":scc.members.len()})
                    })
                    .collect();
                serde_json::json!({"ev":"pass","pass":iteration,"sccs":sccs_json,"callers":callers,"modules":self.graph.module_names})
            });
        }
        let mut changed_by_scc = vec![false; sccs.len()];
        phases.analyze_scc_graph(graph, failed, |i, generated| {
            let generated = generated.map_err(AnalysisError::Phase)?;
            let Ok((_, _, previous, _)) = &prepared[i] else {
                unreachable!("SCCs failing preparation are not analyzed")
            };
            let (published, changed) = self.commit_scc(state, previous, generated)?;
            changed_by_scc[i] = changed;
            Ok(published)
        })?;
        timings.mark("scc_phase");
        for (prepared, changed) in prepared.into_iter().zip(changed_by_scc) {
            if changed && let Ok((_, all, _, _)) = prepared {
                scc_changed.extend(all.into_iter().map(|function| function.label));
            }
        }
        Ok(())
    }
    fn commit_scc<PE, SE>(
        &self,
        state: &mut AnalysisState,
        previous: &FunctionTypes,
        generated: FunctionTypes,
    ) -> Result<(Published, bool), AnalysisError<PE, SE>> {
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
        let mut signatures = Vec::with_capacity(filtered.len());
        for (label, signature) in &filtered {
            let persisted = persisted_signature(signature).map_err(AnalysisError::Lattice)?;
            state.signatures.insert(*label, persisted.clone());
            signatures.push((*label, persisted));
        }
        let contracts = state.contracts.commit(validated);
        let changed = !contracts.is_empty()
            || !reached_fixpoint_strict(previous, &filtered).map_err(AnalysisError::Lattice)?;
        Ok((Published { signatures, contracts }, changed))
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
            crate::job_trace::PASS.store(iteration, std::sync::atomic::Ordering::Relaxed);
            let mut scc_changed = Vec::new();
            let graph = if phases.parallel() && phases.dependency_scheduled() {
                self.levels(&sccs)
            } else {
                None
            };
            if let Some((levels, callers)) = graph {
                self.analyze_graph(iteration, &sccs, levels, callers, &mut state, phases, schedule, &mut scc_changed)?;
            } else {
                let waves = if phases.parallel() {
                    self.waves(&sccs)
                } else {
                    (0..sccs.len()).map(|i| vec![i]).collect()
                };
                for wave in waves {
                    self.analyze_wave(iteration, &sccs, &wave, &mut state, phases, schedule, &mut scc_changed)?;
                }
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

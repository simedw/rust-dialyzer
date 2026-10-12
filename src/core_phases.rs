use crate::{
    analysis_inputs::PreparedRecords,
    bifs::registry::{Mfa, Registry},
    binder::{BinderState, OpaqueMatch},
    binder_module::ModuleBinderError,
    body_calls::{ActiveContract, ApplyTarget, CallLookup, CallWorld},
    body_flow::{BodyError, BodyFlow, BodyWarning, CaseWarning},
    call_transfer::Signature,
    call_warning::CallWarning,
    closure_dependencies::ClosureDependencies,
    compiled_definitions::PreparedDefinitions,
    constraints::{
        analyze_scc::{AnalyzeSccError, SccAnalyzer, SolverSelection},
        atomic::BindingKey,
        closure_order::ClosureCodeProfile,
        generation::{
            ActiveContract as GenerationActiveContract, CallSignature, CoreCaptureStore, GenerationCallGraph,
            GenerationCaptureInputs, GenerationLibraryQuery, GenerationOutput, GenerationPhase, LibraryTypes, PropagatedTypes,
        },
    },
    core_analysis::{AnalysisPhases, ModuleRequest, Published, SccGraph, SccRequest},
    dataflow_nodes::DataflowNodes,
    dependencies::Store,
    folded_core::FoldedCore,
    function_tree::build_function_state,
    guard_control::GuardControl,
    guard_literals::GuardLiterals,
    guard_producer::GuardProducer,
    guard_warning::GuardWarning,
    labels::CoreLabel,
    lattice::{Known, LatticeError, Type, TypeGetter, TypeValue, VariableId},
    patterns::Patterns,
    program::{ModuleId, NodeRef, Program},
    source_callgraph::{GraphVertex, SourceCallGraph},
    success_phase::FunctionTypes,
    warning_payload::NativeWarning,
};
use std::collections::BTreeMap;
static DIAGNOSTIC_SNAPSHOT_RESUMES: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
pub fn diagnostic_snapshot_resumes() -> u64 {
    DIAGNOSTIC_SNAPSHOT_RESUMES.load(std::sync::atomic::Ordering::Relaxed)
}
#[derive(Debug)]
pub enum CorePhaseError {
    Boundary(String),
    Lattice(LatticeError),
    Scc(Box<AnalyzeSccError>),
    Body(Box<BodyError>),
    Metadata(ModuleBinderError),
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct AnalysisWarning(NativeWarning);
macro_rules! raw_warning {
    ($($warning:ty),* $(,)?) => {$(impl From<$warning> for AnalysisWarning {
        fn from(warning: $warning) -> Self { Self(warning.native_warning()) }
    })*};
}
raw_warning!(
    OpaqueMatch,
    BodyWarning,
    GuardWarning,
    CallWarning,
    CaseWarning
);

pub struct ModuleDiagnostics {
    pub signatures: FunctionTypes,
    pub warnings: Vec<NativeWarning>,
}

pub struct CorePhaseInputs<'a> {
    pub nodes: &'a DataflowNodes<'a>,
    pub program: &'a Program,
    pub dependencies: &'a ClosureDependencies,
    pub graph: &'a SourceCallGraph,
    pub definitions: &'a PreparedDefinitions,
    pub profile: &'a ClosureCodeProfile,
    pub folded: &'a FoldedCore,
    pub bifs: &'a Registry,
}
fn index_function_names(named_labels: &BTreeMap<GraphVertex, CoreLabel>) -> BTreeMap<CoreLabel, Mfa> {
    let mut names = BTreeMap::new();
    for (vertex, label) in named_labels {
        let GraphVertex::Named { module, function, arity } = vertex else {
            continue;
        };
        let Some(arity) = num_traits::ToPrimitive::to_usize(arity) else {
            continue;
        };
        names.entry(*label).or_insert_with(|| Mfa {
            module: module.clone().into(), function: function.clone().into(), arity,
        });
    }
    names
}

/// The parts of `CorePhases` that read only the definitions, the folded Core
/// and the labels. Failures surface in `CorePhases::with_prepared`, in the
/// order `CorePhases::new` reported them.
pub struct PreparedPhaseInputs<'a> {
    records: Result<PreparedRecords, String>,
    folded_nodes: Result<DataflowNodes<'a>, String>,
    guard_literals: Result<GuardLiterals<'a>, String>,
}
impl<'a> PreparedPhaseInputs<'a> {
    pub fn build(
        definitions: &PreparedDefinitions,
        folded: &'a FoldedCore,
        labels: &'a crate::labels::Labels,
        modules: usize,
    ) -> Self {
        let mut timings = crate::phase_timings::PhaseTimings::new("prepared_phase_inputs");
        let records = PreparedRecords::for_modules(definitions, (0..modules).map(ModuleId));
        timings.mark("records");
        let folded_nodes = DataflowNodes::build(&folded.program, labels);
        timings.mark("folded_nodes");
        let guard_literals = GuardLiterals::new(folded);
        timings.mark("guard_literals");
        Self {
            records,
            folded_nodes,
            guard_literals,
        }
    }
}

pub struct CorePhases<'a> {
    pub nodes: &'a DataflowNodes<'a>,
    pub program: &'a Program,
    pub dependencies: &'a ClosureDependencies,
    pub graph: &'a SourceCallGraph,
    pub definitions: &'a PreparedDefinitions,
    pub profile: &'a ClosureCodeProfile,
    pub folded: &'a FoldedCore,
    pub bifs: &'a Registry,
    pub solvers: SolverSelection,
    pub world: CallWorld<'a>,
    pub records: PreparedRecords,
    folded_nodes: DataflowNodes<'a>,
    guard_literals: GuardLiterals<'a>,
    letrec: BTreeMap<ModuleId, crate::letrec_types::LetrecLinks>,
    /// Each module's initial binder state; it depends only on the module, so
    /// it is built on first use and shared by every refinement run.
    binder_states: Vec<std::sync::OnceLock<Result<BinderState<AnalysisWarning>, ModuleBinderError>>>,
    function_names: BTreeMap<CoreLabel, Mfa>,
    core_captures: CoreCaptureStore<'a>,
    scc_memo: std::sync::Mutex<BTreeMap<Vec<Mfa>, std::sync::Arc<SccMemo>>>,
    module_memo: std::sync::Mutex<BTreeMap<ModuleId, std::sync::Arc<ModuleMemo<'a>>>>,
    module_memo_counts: ModuleMemoCounts,
    /// Generation's view of each entry of `world.active_contracts`, converted
    /// once when the contract is activated instead of at every call site.
    generation_contracts: BTreeMap<Mfa, Result<GenerationActiveContract, String>>,
}
impl<'a> CorePhases<'a> {
    pub fn new(
        inputs: CorePhaseInputs<'a>,
        library: &'a Store,
        solvers: SolverSelection,
    ) -> Result<Self, CorePhaseError> {
        let prepared = PreparedPhaseInputs::build(
            inputs.definitions,
            inputs.folded,
            inputs.nodes.labels(),
            inputs.nodes.core().modules.len(),
        );
        Self::with_prepared(inputs, library, solvers, prepared)
    }
    /// `new` with the parts that need only definitions and the folded Core
    /// built ahead, alongside other setup.
    pub fn with_prepared(
        inputs: CorePhaseInputs<'a>,
        library: &'a Store,
        solvers: SolverSelection,
        prepared: PreparedPhaseInputs<'a>,
    ) -> Result<Self, CorePhaseError> {
        let records = prepared.records.map_err(CorePhaseError::Boundary)?;
        let folded_nodes = prepared.folded_nodes.map_err(CorePhaseError::Boundary)?;
        let guard_literals = prepared.guard_literals.map_err(CorePhaseError::Boundary)?;
        let world = CallWorld::from_library(library);
        let mut phases = Self {
            nodes: inputs.nodes,
            program: inputs.program,
            dependencies: inputs.dependencies,
            graph: inputs.graph,
            definitions: inputs.definitions,
            profile: inputs.profile,
            folded: inputs.folded,
            bifs: inputs.bifs,
            solvers,
            world,
            records,
            folded_nodes,
            guard_literals,
            letrec: BTreeMap::new(),
            binder_states: (0..inputs.nodes.core().modules.len())
                .map(|_| std::sync::OnceLock::new())
                .collect(),
            function_names: index_function_names(&inputs.graph.named_labels),
            core_captures: CoreCaptureStore::new(inputs.nodes.core(), inputs.nodes.labels()),
            scc_memo: Default::default(),
            module_memo: Default::default(),
            module_memo_counts: Default::default(),
            generation_contracts: BTreeMap::new(),
        };
        phases.initialize()?;
        Ok(phases)
    }
    fn initialize(&mut self) -> Result<(), CorePhaseError> {
        self.world
            .add_closure_sites(self.program, self.nodes.labels(), self.dependencies)
            .map_err(CorePhaseError::Boundary)?;
        let mut links: BTreeMap<ModuleId, BTreeMap<CoreLabel, CoreLabel>> = BTreeMap::new();
        for binding in &self.program.bindings {
            let Some(function) = binding.declared_function else {
                continue;
            };
            let function = &self.program.functions[function.0];
            let variable = self
                .nodes
                .label(NodeRef {
                    module: binding.module,
                    node: binding.node.0,
                })
                .map_err(CorePhaseError::Boundary)?;
            let label = self
                .nodes
                .label(NodeRef {
                    module: function.module,
                    node: function.node.0,
                })
                .map_err(CorePhaseError::Boundary)?;
            links
                .entry(binding.module)
                .or_default()
                .insert(variable, label);
        }
        self.letrec = links.into_iter().map(|(module, links)| (module, links.into())).collect();
        Ok(())
    }
    fn publish_state<'s>(
        &mut self,
        signatures: impl Iterator<Item = (CoreLabel, &'s Signature)>,
        contracts: impl Iterator<Item = &'s Mfa>,
    ) {
        let mut timings = crate::phase_timings::PhaseTimings::detail("state_publication");
        for (label, signature) in signatures {
            if let Some(name) = self.function_names.get(&label) {
                self.world.named_signatures.insert(name.clone(), signature.clone());
            } else {
                self.world
                    .anonymous_signatures
                    .insert(label, signature.clone());
            }
        }
        timings.mark("signatures");
        for name in contracts {
            if let Some(prepared) = self.definitions.contracts.get_callable(name)
                && !self.world.active_contracts.contains_key(name)
            {
                self.world.active_contracts.insert(
                    name.clone(),
                    ActiveContract {
                        summary: prepared.summary.clone(),
                        capture: prepared.capture.clone(),
                    },
                );
                self.generation_contracts
                    .insert(name.clone(), LibraryTypes::active_contract(&self.world, name).map(Option::unwrap));
            }
        }
        timings.mark("contracts");
    }
    pub fn module_bodies(&self) -> ModuleBodies<'_, 'a> {
        ModuleBodies { nodes: self.nodes, program: self.program, graph: self.graph,
            definitions: self.definitions, folded: self.folded, bifs: self.bifs,
            world: &self.world, folded_nodes: &self.folded_nodes,
            guard_literals: &self.guard_literals, letrec: &self.letrec,
            binder_states: &self.binder_states }
    }

}

pub struct ModuleBodies<'phase, 'input> {
    nodes: &'input DataflowNodes<'input>,
    program: &'input Program,
    graph: &'input SourceCallGraph,
    definitions: &'input PreparedDefinitions,
    folded: &'input FoldedCore,
    bifs: &'input Registry,
    world: &'phase CallWorld<'input>,
    folded_nodes: &'phase DataflowNodes<'input>,
    guard_literals: &'phase GuardLiterals<'input>,
    letrec: &'phase BTreeMap<ModuleId, crate::letrec_types::LetrecLinks>,
    binder_states: &'phase [std::sync::OnceLock<Result<BinderState<AnalysisWarning>, ModuleBinderError>>],
}
/// A module body analysis stopped after its inference loop: what the
/// warning loop of a diagnostic run with the same inputs continues from.
pub struct ModuleSnapshot<'input> {
    functions: crate::function_state::FunctionState,
    patterns: Patterns<'input, 'input>,
    state: BinderState<AnalysisWarning>,
}
impl<'input> ModuleBodies<'_, 'input> {
    /// Analyzes `module`'s bodies; with `warnings`, then collects warnings.
    /// `resume` continues a diagnostic run from a refinement's inference
    /// loop with identical inputs instead of repeating it. Refinement runs
    /// return their snapshot when `keep` is set.
    fn module_body(
        &self,
        module: ModuleId,
        previous: &FunctionTypes,
        calls: &dyn CallLookup,
        warnings: bool,
        resume: Option<ModuleSnapshot<'input>>,
        keep: bool,
    ) -> Result<(FunctionTypes, Vec<NativeWarning>, Option<ModuleSnapshot<'input>>), CorePhaseError> {
        let mut timings = crate::phase_timings::PhaseTimings::new(if warnings {
            "module_diagnostics"
        } else {
            "module_refinement"
        });
        let (mut functions, mut patterns, inferred) = match resume {
            Some(snapshot) => (snapshot.functions, snapshot.patterns, Some(snapshot.state)),
            None => {
                let inputs = self
                    .graph
                    .function_inputs(module, self.program, self.nodes.labels(), previous.clone())
                    .map_err(CorePhaseError::Boundary)?;
                let functions = build_function_state(self.nodes, module, &inputs)
                    .map_err(CorePhaseError::Boundary)?;
                (functions, Patterns::new(self.nodes), None)
            }
        };
        let folded_patterns = Patterns::new(&self.folded_nodes);
        let producer = GuardProducer {
            original: self.nodes,
            folded: self.folded,
            patterns: &folded_patterns,
        };
        let guards = GuardControl {
            nodes: self.nodes,
            producer: &producer,
            literals: &self.guard_literals,
            registry: self.bifs,
        };
        let build = || {
            BinderState::<AnalysisWarning>::for_module(
                self.nodes.core(),
                self.definitions,
                module,
                false,
            )
        };
        let built;
        let initial = match self.binder_states.get(module.0) {
            Some(cell) => cell.get_or_init(build).as_ref(),
            None => {
                built = build();
                built.as_ref()
            }
        }
        .map_err(|error| CorePhaseError::Metadata(error.clone()))?;
        let empty_letrec = crate::letrec_types::LetrecLinks::default();
        let letrec = self.letrec.get(&module).unwrap_or(&empty_letrec);
        let mut flow = BodyFlow {
            patterns: &mut patterns,
            functions: &mut functions,
            bifs: self.bifs,
            calls: Some((&producer, calls)),
            guards: Some(&guards),
            letrec,
        };
        timings.mark("prepare_module_body");
        let mut bound = match inferred {
            Some(state) => flow.resume_warning_loop(&state),
            None => flow.analyze_module(initial, warnings),
        }
        .map_err(|error| CorePhaseError::Body(Box::new(error)))?;
        timings.mark("analyze_module");
        if warnings {
            bound = flow
                .collect_function_warnings(self.graph, &self.world, &bound)
                .map_err(|error| CorePhaseError::Body(Box::new(error)))?;
        }
        timings.mark("function_warnings");
        let signatures = functions.function_types();
        timings.mark("signatures");
        let warnings_out = bound
            .warnings
            .warnings
            .iter()
            .map(|warning| warning.0.clone())
            .collect();
        let snapshot = (keep && !warnings).then(|| ModuleSnapshot { functions, patterns, state: bound });
        Ok((signatures, warnings_out, snapshot))
    }
    pub fn module_diagnostics(
        &self,
        module: ModuleId,
        previous: &FunctionTypes,
        resume: Option<ModuleSnapshot<'input>>,
    ) -> Result<ModuleDiagnostics, CorePhaseError> {
        let resumed = resume.is_some();
        let result = self
            .module_body(module, previous, self.world, true, resume, false)
            .map(|(signatures, warnings, _)| ModuleDiagnostics { signatures, warnings })?;
        if resumed && crate::lattice::verify_fast_paths() {
            let (signatures, warnings, _) = self.module_body(module, previous, self.world, true, None, false)?;
            assert!(signatures == result.signatures, "resumed diagnostic signatures differ: {module:?}");
            assert!(warnings == result.warnings, "resumed diagnostic warnings differ: {module:?}");
        }
        if resumed {
            DIAGNOSTIC_SNAPSHOT_RESUMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(result)
    }
}

fn signature(typ: &Type) -> Result<Signature, CorePhaseError> {
    let TypeValue::Types(Known::Known(arguments)) = typ
        .get(TypeGetter::FunctionArgs)
        .map_err(CorePhaseError::Lattice)?
    else {
        return Err(CorePhaseError::Boundary(
            "solver binding has no function domain".into(),
        ));
    };
    let TypeValue::Type(result) = typ
        .get(TypeGetter::FunctionRange)
        .map_err(CorePhaseError::Lattice)?
    else {
        return Err(CorePhaseError::Boundary(
            "solver binding has no function range".into(),
        ));
    };
    Ok(Signature { arguments, result })
}
impl AnalysisPhases for CorePhases<'_> {
    type Error = CorePhaseError;
    fn publish<'s>(
        &mut self,
        signatures: impl Iterator<Item = (CoreLabel, &'s Signature)>,
        contracts: impl Iterator<Item = &'s Mfa>,
    ) {
        self.publish_state(signatures, contracts);
    }
    fn analyze_scc(&mut self, request: SccRequest<'_>) -> Result<FunctionTypes, Self::Error> {
        self.analyze_scc_shared(&request)
    }
    fn refine_module(&mut self, request: ModuleRequest<'_>) -> Result<FunctionTypes, Self::Error> {
        self.refine_module_shared(&request)
    }
    fn parallel(&self) -> bool {
        std::env::var_os("RUST_DIALYZER_SEQUENTIAL").is_none_or(|v| v != "1")
    }
    fn analyze_scc_batch(
        &mut self,
        requests: Vec<SccRequest<'_>>,
    ) -> Vec<Result<FunctionTypes, Self::Error>> {
        let this = &*self;
        crate::ordered_parallel::parallel_map(&requests, |request| this.analyze_scc_shared(request))
    }
    /// `RUST_DIALYZER_SCC_WAVES=1` keeps the wave-by-wave schedule.
    fn dependency_scheduled(&self) -> bool {
        std::env::var_os("RUST_DIALYZER_SCC_WAVES").is_none_or(|v| v != "1")
    }
    fn analyze_scc_graph<C>(
        &mut self,
        graph: SccGraph<'_>,
        failed: Option<(u64, C)>,
        mut commit: impl FnMut(usize, Result<FunctionTypes, Self::Error>) -> Result<Published, C>,
    ) -> Result<(), C> {
        let mut timings = crate::phase_timings::PhaseTimings::new("scc_graph_phase");
        let overlay = PhaseOverlay::new(&graph, &self.function_names);
        let retries = std::sync::atomic::AtomicUsize::new(0);
        let this = &*self;
        let result = crate::ordered_parallel::dependency_map(
            graph.schedule,
            failed,
            |i| {
                let job = &graph.jobs[i];
                let library = RecordingWorld {
                    world: &this.world,
                    contracts: &this.generation_contracts,
                    reads: Default::default(),
                    overlay: Some((&overlay, graph.levels[i])),
                    frozen_owners: Default::default(),
                    early: Default::default(),
                };
                let request = SccInput {
                    runtime: graph.runtime,
                    ordered_mfas: job.ordered_mfas,
                    next_core_label: graph.next_core_label,
                    previous: job.previous,
                };
                let t0 = crate::job_trace::now();
                let result = this.analyze_scc_in(&request, &library);
                let early = library.early.into_inner().unwrap();
                if crate::job_trace::enabled() {
                    let t1 = crate::job_trace::now();
                    let retry = !early.is_empty();
                    let note = crate::job_trace::take_note();
                    crate::job_trace::record(|| serde_json::json!({"ev":"scc","pass":crate::job_trace::pass(),"i":i,"t0":t0,"t1":t1,"retry":retry,"note":note}));
                }
                if early.is_empty() {
                    crate::ordered_parallel::Attempt::Done(result)
                } else {
                    retries.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    crate::ordered_parallel::Attempt::Retry(early.into_iter().collect())
                }
            },
            |i, result| {
                let published = commit(i, result)?;
                let _ = overlay.slots[i].set(this.overlay_entry(published));
                Ok(())
            },
        );
        timings.mark("schedule");
        timings.label(|| format!("retries {}", retries.load(std::sync::atomic::Ordering::Relaxed)));
        for slot in overlay.slots {
            if let Some(entry) = slot.into_inner() {
                self.publish_state(
                    entry.published.signatures.iter().map(|(label, signature)| (*label, signature)),
                    entry.published.contracts.iter(),
                );
            }
        }
        timings.mark("publish");
        result
    }
    fn refine_module_batch(
        &mut self,
        requests: Vec<ModuleRequest<'_>>,
    ) -> Vec<Result<FunctionTypes, Self::Error>> {
        let mut timings = crate::phase_timings::PhaseTimings::new("module_memo");
        self.module_memo_counts.reset();
        let this = &*self;
        // Modules are independent within a batch, so only the start order
        // changes: largest Core first, so that the few very large modules do
        // not start last and leave the other workers idle.
        let core = self.nodes.core();
        let mut order: Vec<usize> = (0..requests.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(core.modules[requests[i].module.0].nodes.len()));
        let ordered = crate::ordered_parallel::parallel_map(&order, |&i| {
            let t0 = crate::job_trace::now();
            let result = this.refine_module_shared(&requests[i]);
            if crate::job_trace::enabled() {
                let t1 = crate::job_trace::now();
                crate::job_trace::record(|| serde_json::json!({"ev":"mod","pass":crate::job_trace::pass(),"m":requests[i].module.0,"t0":t0,"t1":t1}));
            }
            result
        });
        crate::job_trace::flush();
        timings.label(|| self.module_memo_counts.describe(requests.len()));
        let mut results: Vec<_> = (0..requests.len()).map(|_| None).collect();
        for (i, result) in order.into_iter().zip(ordered) {
            results[i] = Some(result);
        }
        results.into_iter().map(|r| r.expect("every module refined")).collect()
    }
}

impl<'a> CorePhases<'a> {
    /// Module refinement is memoized across fixpoint iterations like SCCs.
    /// The body analysis depends on the module's Core, the fixed run inputs
    /// (definitions, call sites, guards, letrec links), `previous` and the
    /// signatures and contracts it reads from the call world; with identical
    /// values for all of these it computes the same types.
    /// The inference state of `module`'s last refinement, if that run had
    /// `previous` and read exactly the call-world values a body analysis
    /// would read now: a diagnostic run's inference loop would repeat it.
    pub fn take_module_snapshot(&self, module: ModuleId, previous: &FunctionTypes) -> Option<ModuleSnapshot<'a>> {
        let memo = self.module_memo.lock().unwrap().get(&module).cloned()?;
        if memo.previous == *previous && memo.reads.still_valid(&self.world) {
            memo.snapshot.lock().unwrap().take()
        } else {
            None
        }
    }
    fn refine_module_shared(&self, request: &ModuleRequest<'_>) -> Result<FunctionTypes, CorePhaseError> {
        let memo = self.module_memo.lock().unwrap().get(&request.module).cloned();
        if let Some(memo) = memo
            && memo.previous == *request.previous
        {
            self.module_memo_counts.same_previous.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if memo.reads.still_valid(&self.world) {
                self.module_memo_counts.hits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if crate::lattice::verify_fast_paths() {
                    let (fresh, _, _) = self.refine_module_uncached(request, false)?;
                    assert_eq!(fresh, memo.result, "memoized module result differs: {:?}", request.module);
                }
                return Ok(memo.result.clone());
            }
        }
        let (result, reads, snapshot) = self.refine_module_uncached(request, true)?;
        self.module_memo.lock().unwrap().insert(
            request.module,
            std::sync::Arc::new(ModuleMemo {
                previous: request.previous.clone(),
                reads,
                result: result.clone(),
                snapshot: std::sync::Mutex::new(snapshot),
            }),
        );
        Ok(result)
    }
    fn refine_module_uncached(
        &self,
        request: &ModuleRequest<'_>,
        keep_snapshot: bool,
    ) -> Result<(FunctionTypes, CallReads, Option<ModuleSnapshot<'a>>), CorePhaseError> {
        let calls = RecordingCalls {
            world: &self.world,
            reads: Default::default(),
        };
        let (signatures, _, snapshot) =
            self.module_bodies().module_body(request.module, request.previous, &calls, false, None, keep_snapshot)?;
        Ok((signatures, calls.reads.into_inner(), snapshot))
    }
    /// SCC results are memoized across fixpoint iterations. Generation and
    /// solving depend on the Core, the fixed run inputs, `previous` (the
    /// propagated types) and the library signatures and contracts read while
    /// generating; when all of these read the same values again, the result
    /// is the same. (The schedule's runtime captures only feed capture
    /// validation, which already passed for the memoized result.) Callee
    /// types are embedded in the constraints and steer their normalization,
    /// so constraints cannot be reused once any of these inputs changes.
    fn analyze_scc_shared(&self, request: &SccRequest<'_>) -> Result<FunctionTypes, CorePhaseError> {
        let library = RecordingWorld {
            world: &self.world,
            contracts: &self.generation_contracts,
            reads: Default::default(),
            overlay: None,
            frozen_owners: Default::default(),
            early: Default::default(),
        };
        let request = SccInput {
            runtime: request.runtime,
            ordered_mfas: request.ordered_mfas,
            next_core_label: request.next_core_label,
            previous: request.previous,
        };
        self.analyze_scc_in(&request, &library)
    }
    fn analyze_scc_in(&self, request: &SccInput<'_>, library: &RecordingWorld<'_, '_>) -> Result<FunctionTypes, CorePhaseError> {
        let memo = self.scc_memo.lock().unwrap().get(request.ordered_mfas).cloned();
        if crate::job_trace::enabled() {
            let outcome = match &memo {
                None => "none",
                Some(memo) if memo.previous != *request.previous => {
                    if memo.reads.still_valid(library) { "previous" } else { "previous+reads" }
                }
                Some(memo) if !memo.reads.still_valid(library) => "reads",
                Some(_) => "hit",
            };
            let changed_previous = memo.as_ref().map_or(0, |memo| {
                request.previous.iter().filter(|(label, sig)| memo.previous.get(label) != Some(sig)).count()
            });
            crate::job_trace::set_note(serde_json::json!({"memo":outcome,"changed_previous":changed_previous,"functions":request.previous.len()}));
        }
        if let Some(memo) = memo.clone()
            && memo.next_core_label == request.next_core_label
            && memo.previous == *request.previous
            && memo.reads.still_valid(library)
        {
            if crate::lattice::verify_fast_paths() {
                let fresh = self.analyze_scc_uncached(request, library)?;
                assert_eq!(fresh, memo.result, "memoized SCC result differs: {:?}", request.ordered_mfas);
            }
            return Ok(memo.result.clone());
        }
        let result = self.analyze_scc_uncached(request, library)?;
        if crate::job_trace::enabled() {
            let same = memo.as_ref().is_some_and(|memo| memo.result == result);
            crate::job_trace::add_note("same_result", serde_json::json!(same));
        }
        self.scc_memo.lock().unwrap().insert(
            request.ordered_mfas.to_vec(),
            std::sync::Arc::new(SccMemo {
                next_core_label: request.next_core_label,
                previous: request.previous.clone(),
                reads: std::mem::take(&mut *library.reads.lock().unwrap()),
                result: result.clone(),
            }),
        );
        Ok(result)
    }
    fn overlay_entry(&self, published: Published) -> OverlayEntry {
        let mut signatures = BTreeMap::new();
        for (label, signature) in &published.signatures {
            if let Some(name) = self.function_names.get(label) {
                signatures.insert(name.clone(), signature.clone());
            }
        }
        let mut contracts = BTreeMap::new();
        for name in &published.contracts {
            if let Some(prepared) = self.definitions.contracts.get_callable(name)
                && !self.world.active_contracts.contains_key(name)
            {
                let contract = ActiveContract {
                    summary: prepared.summary.clone(),
                    capture: prepared.capture.clone(),
                };
                contracts.insert(name.clone(), crate::analysis_inputs::generation_contract(&contract));
            }
        }
        OverlayEntry { published, signatures, contracts }
    }
    fn analyze_scc_uncached(&self, request: &SccInput<'_>, library: &RecordingWorld<'_, '_>) -> Result<FunctionTypes, CorePhaseError> {
        let mut timings = crate::phase_timings::PhaseTimings::detail("scc_analysis");
        let propagated = PropagatedTypes(
            request
                .previous
                .iter()
                .map(|(label, signature)| {
                    (
                        *label,
                        Type::function(Some(signature.arguments.clone()), signature.result.clone()),
                    )
                })
                .collect(),
        );
        let solvers = Vec::new();
        let captures = GenerationCaptureInputs {
            core_captures: Some(&self.core_captures),
            runtime: request.runtime,
            records: &self.records,
            initial_record_cache: &[],
            solvers: &solvers,
        };
        timings.mark("prepare_scc");
        let analysis = SccAnalyzer {
            nodes: self.nodes,
            profile: self.profile,
            provider: self.bifs,
            graph: self.graph,
            phase: GenerationPhase {
                captures: Some(&captures),
                callgraph: GenerationCallGraph::Indexed {
                    program: self.program,
                    dependencies: self.dependencies,
                },
                propagated: &propagated,
                library,
                records: &self.records.prototypes,
                self_recursive: None,
            },
        }
        .analyze(request.ordered_mfas, request.next_core_label, &self.solvers, GenerationOutput::BindingsOnly)
        .map_err(|error| CorePhaseError::Scc(Box::new(error)))?;
        timings.mark("generate_and_solve");
        let result = analysis
            .bindings
            .iter()
            .filter_map(|(key, typ)| {
                let BindingKey::Label(VariableId::Integer(integer)) = key else {
                    return None;
                };
                let label = CoreLabel(integer.to_u64()?);
                request
                    .previous
                    .contains_key(&label)
                    .then(|| signature(typ).map(|signature| (label, signature)))
            })
            .collect::<Result<_, _>>()?;
        Ok(result)
    }
}

struct SccInput<'a> {
    runtime: &'a crate::constraints::generation_capture_headers::GenerationRuntimeCaptures,
    ordered_mfas: &'a [Mfa],
    next_core_label: CoreLabel,
    previous: &'a FunctionTypes,
}
/// SCC results committed during a dependency-scheduled phase, before they
/// are published into the call world. An SCC reads one only if the SCC
/// that wrote it is in a lower wave, so it sees what wave-by-wave analysis
/// would show it.
struct PhaseOverlay<'g> {
    /// The SCC whose commit may write each name's signature and contract.
    owner: BTreeMap<Mfa, usize>,
    levels: &'g [usize],
    slots: Vec<std::sync::OnceLock<OverlayEntry>>,
}
struct OverlayEntry {
    published: Published,
    signatures: BTreeMap<Mfa, Signature>,
    contracts: BTreeMap<Mfa, Result<GenerationActiveContract, String>>,
}
impl<'g> PhaseOverlay<'g> {
    fn new(graph: &SccGraph<'g>, function_names: &BTreeMap<CoreLabel, Mfa>) -> Self {
        let mut owner = BTreeMap::new();
        for (i, job) in graph.jobs.iter().enumerate() {
            for label in job.previous.keys() {
                if let Some(name) = function_names.get(label) {
                    owner.insert(name.clone(), i);
                }
            }
        }
        Self {
            owner,
            levels: graph.levels,
            slots: (0..graph.jobs.len()).map(|_| Default::default()).collect(),
        }
    }
}

struct SccMemo {
    next_core_label: CoreLabel,
    previous: FunctionTypes,
    reads: LibraryReads,
    result: FunctionTypes,
}
type CallSignatureValue = Option<(Vec<Type>, Type)>;
/// Where an active contract came from. Entries in `active_contracts` are
/// never replaced and the library is fixed, so the source determines the
/// contract.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ContractSource {
    Absent,
    Library,
    Active,
}
#[derive(Default)]
struct LibraryReads {
    signatures: BTreeMap<Mfa, CallSignatureValue>,
    contracts: BTreeMap<Mfa, ContractSource>,
}
impl LibraryReads {
    fn still_valid(&self, library: &RecordingWorld<'_, '_>) -> bool {
        self.signatures.iter().all(|(mfa, value)| {
            library.current_signature(mfa).is_ok_and(|current| call_signature_value(current) == *value)
        }) && self.contracts.iter().all(|(mfa, source)| library.current_contract_source(mfa) == *source)
    }
}
fn call_signature_value(signature: Option<CallSignature>) -> CallSignatureValue {
    signature.map(|signature| (signature.arguments, signature.returns))
}
fn contract_source(world: &CallWorld<'_>, mfa: &Mfa) -> ContractSource {
    if world.active_contracts.contains_key(mfa) {
        ContractSource::Active
    } else if world.library.is_some_and(|library| library.contracts.contains_key(mfa)) {
        ContractSource::Library
    } else {
        ContractSource::Absent
    }
}
/// Records every library value generation reads, for `SccMemo` validation.
struct RecordingWorld<'w, 'l> {
    world: &'w CallWorld<'l>,
    contracts: &'w BTreeMap<Mfa, Result<GenerationActiveContract, String>>,
    reads: std::sync::Mutex<LibraryReads>,
    /// Unpublished results of the current phase and the reader's wave.
    overlay: Option<(&'w PhaseOverlay<'w>, usize)>,
    frozen_owners: std::sync::Mutex<BTreeMap<usize, Option<&'w OverlayEntry>>>,
    /// Lower-wave SCCs read before they committed; the reading SCC must
    /// run again after them.
    early: std::sync::Mutex<std::collections::BTreeSet<usize>>,
}
impl<'w, 'l> RecordingWorld<'w, 'l> {
    fn frozen_visible(&self, mfa: &Mfa, observing: bool) -> Option<&'w OverlayEntry> {
        let (overlay, level) = self.overlay?;
        let &owner = overlay.owner.get(mfa)?;
        if overlay.levels[owner] >= level {
            return None;
        }
        let entry = *self.frozen_owners.lock().unwrap()
            .entry(owner).or_insert_with(|| overlay.slots[owner].get());
        if observing && entry.is_none() {
            self.early.lock().unwrap().insert(owner);
        }
        entry
    }
    fn visible(&self, mfa: &Mfa) -> Option<&'w OverlayEntry> {
        self.frozen_visible(mfa, true)
    }
    fn signature_at(&self, mfa: &Mfa, observing: bool) -> Result<Option<CallSignature>, String> {
        if let Some(signature) = self.frozen_visible(mfa, observing).and_then(|entry| entry.signatures.get(mfa)) {
            return Ok(Some(CallSignature {
                arguments: signature.arguments.clone(),
                returns: signature.result.clone(),
            }));
        }
        LibraryTypes::signature(self.world, mfa)
    }
    fn current_signature(&self, mfa: &Mfa) -> Result<Option<CallSignature>, String> {
        self.signature_at(mfa, true)
    }
    fn contract_source_at(&self, mfa: &Mfa, observing: bool) -> ContractSource {
        if self.frozen_visible(mfa, observing).is_some_and(|entry| entry.contracts.contains_key(mfa)) {
            ContractSource::Active
        } else {
            contract_source(self.world, mfa)
        }
    }
    fn current_contract_source(&self, mfa: &Mfa) -> ContractSource {
        self.contract_source_at(mfa, true)
    }
    fn contract_at(&self, mfa: &Mfa, observing: bool) -> Result<Option<GenerationActiveContract>, String> {
        let overlay = self.frozen_visible(mfa, observing).and_then(|entry| entry.contracts.get(mfa));
        match overlay.or_else(|| self.contracts.get(mfa)) {
            Some(contract) => Ok(Some(contract.clone()?)),
            None => LibraryTypes::active_contract(self.world, mfa),
        }
    }
}
struct NonobservingWorld<'s, 'w, 'l>(&'s RecordingWorld<'w, 'l>);
impl LibraryTypes for NonobservingWorld<'_, '_, '_> {
    fn signature(&self, mfa: &Mfa) -> Result<Option<CallSignature>, String> {
        self.0.signature_at(mfa, false)
    }
    fn active_contract(&self, mfa: &Mfa) -> Result<Option<GenerationActiveContract>, String> {
        self.0.contract_at(mfa, false)
    }
}
impl LibraryTypes for RecordingWorld<'_, '_> {
    fn signature(&self, mfa: &Mfa) -> Result<Option<CallSignature>, String> {
        let signature = self.current_signature(mfa)?;
        self.reads.lock().unwrap().signatures.entry(mfa.clone()).or_insert_with(|| {
            signature.as_ref().map(|signature| (signature.arguments.clone(), signature.returns.clone()))
        });
        Ok(signature)
    }
    fn active_contract(&self, mfa: &Mfa) -> Result<Option<GenerationActiveContract>, String> {
        let contract = self.contract_at(mfa, true)?;
        let source = self.current_contract_source(mfa);
        self.reads.lock().unwrap().contracts.entry(mfa.clone()).or_insert(source);
        Ok(contract)
    }
    fn parallel_generation_snapshot(&self) -> Option<Box<dyn LibraryTypes + Send + Sync + '_>> {
        Some(Box::new(NonobservingWorld(self)))
    }
    fn commit_generation_queries(&self, queries: &[GenerationLibraryQuery]) {
        for query in queries {
            match query {
                GenerationLibraryQuery::Signature { mfa, result } => {
                    self.visible(mfa);
                    if let Ok(signature) = result {
                        self.reads.lock().unwrap().signatures.entry(mfa.clone())
                            .or_insert_with(|| call_signature_value(signature.clone()));
                    }
                }
                GenerationLibraryQuery::ActiveContract { mfa, succeeded } => {
                    self.visible(mfa);
                    if *succeeded {
                        let source = self.current_contract_source(mfa);
                        self.reads.lock().unwrap().contracts.entry(mfa.clone()).or_insert(source);
                    }
                }
            }
        }
    }
}

struct ModuleMemo<'a> {
    previous: FunctionTypes,
    reads: CallReads,
    result: FunctionTypes,
    /// The run's inference state, until a diagnostic run resumes from it.
    snapshot: std::sync::Mutex<Option<ModuleSnapshot<'a>>>,
}
#[derive(Default)]
struct ModuleMemoCounts {
    same_previous: std::sync::atomic::AtomicUsize,
    hits: std::sync::atomic::AtomicUsize,
}
impl ModuleMemoCounts {
    fn reset(&self) {
        self.same_previous.store(0, std::sync::atomic::Ordering::Relaxed);
        self.hits.store(0, std::sync::atomic::Ordering::Relaxed);
    }
    fn describe(&self, requests: usize) -> String {
        format!(
            "modules {requests} same_previous {} hits {}",
            self.same_previous.load(std::sync::atomic::Ordering::Relaxed),
            self.hits.load(std::sync::atomic::Ordering::Relaxed),
        )
    }
}
/// Call-world values read by one module body analysis.
#[derive(Default)]
struct CallReads {
    named: BTreeMap<Mfa, Option<Signature>>,
    anonymous: BTreeMap<CoreLabel, Option<Signature>>,
    contracts: BTreeMap<Mfa, ContractSource>,
}
impl CallReads {
    fn still_valid(&self, world: &CallWorld<'_>) -> bool {
        self.named.iter().all(|(mfa, value)| world.named_signature(mfa).is_ok_and(|current| current == *value))
            && self.anonymous.iter().all(|(label, value)| world.anonymous_signatures.get(label) == value.as_ref())
            && self.contracts.iter().all(|(mfa, source)| contract_source(world, mfa) == *source)
    }
}
/// Records every call-world value a module body reads, for `ModuleMemo`
/// validation. Call sites are fixed when the phases are built.
struct RecordingCalls<'w, 'l> {
    world: &'w CallWorld<'l>,
    reads: std::cell::RefCell<CallReads>,
}
impl CallLookup for RecordingCalls<'_, '_> {
    fn call_sites(&self, site: &NodeRef) -> Option<&[ApplyTarget]> {
        CallLookup::call_sites(self.world, site)
    }
    fn named_signature(&self, name: &Mfa) -> Result<Option<Signature>, String> {
        let signature = self.world.named_signature(name)?;
        self.reads.borrow_mut().named.entry(name.clone()).or_insert_with(|| signature.clone());
        Ok(signature)
    }
    fn anonymous_signature(&self, label: &CoreLabel) -> Option<Signature> {
        let signature = self.world.anonymous_signatures.get(label).cloned();
        self.reads.borrow_mut().anonymous.entry(*label).or_insert_with(|| signature.clone());
        signature
    }
    fn active_contract(&self, name: &Mfa) -> Result<Option<ActiveContract>, String> {
        let contract = self.world.active_contract(name)?;
        self.reads.borrow_mut().contracts.entry(name.clone()).or_insert_with(|| contract_source(self.world, name));
        Ok(contract)
    }
}

#[cfg(test)]
mod lookup_index_tests {
    use super::*;
    use crate::{core::{CompilerProfile, CoreProgram, CoreVersion, TermMapOrder}, labels::Labels};
    fn empty_graph() -> SourceCallGraph {
        let core = CoreProgram { version: CoreVersion::V6,
            profile: CompilerProfile { otp: "28".into(), erts: "16".into(), elixir: "1.20.4".into(), dialyzer: "5".into(), core_options: vec![], term_map_order: TermMapOrder::NativeToListWithIterator },
            atom_layout: None, contract_closure_profile: None, modules: vec![], node_count: 0, term_count: 0, kinds: BTreeMap::new(),
        };
        let program = Program::build(&core).unwrap();
        let labels = Labels::build(&core, CoreLabel(0)).unwrap();
        let policy = crate::closure_dependencies::DependencyPolicy::parse(include_str!("../corpus/dependency_policy.json")).unwrap();
        let dependencies = ClosureDependencies::analyze(&core, &program, &policy).unwrap();
        let registry = Registry::load(include_str!("../corpus/bifs.json")).unwrap();
        let nodes = DataflowNodes::build(&core, &labels).unwrap();
        SourceCallGraph::build(&nodes, &program, &dependencies, &registry).unwrap()
    }
    #[test]
    fn indexed_lookup_matches_legacy_first_successful_name() {
        let mut graph = empty_graph();
        let named = |module: &str, function: &str, arity| GraphVertex::Named { module: module.into(), function: function.into(), arity };
        graph.named_labels = BTreeMap::from([
            (GraphVertex::Label(CoreLabel(42)), CoreLabel(7)),
            (GraphVertex::Top, CoreLabel(8)),
            (named("A", "0invalid", (-1).into()), CoreLabel(7)),
            (named("A", "first", 2.into()), CoreLabel(7)),
            (named("B", "alias", 1.into()), CoreLabel(7)),
            (named("A", "overflow", num_bigint::BigInt::from(1) << usize::BITS as usize), CoreLabel(8)),
            (named("Z", "fallback", 3.into()), CoreLabel(8)),
            (named("C", "zero", 0.into()), CoreLabel(9)),
            (named("D", "max", num_bigint::BigInt::from(usize::MAX)), CoreLabel(10)),
        ]);
        let names = index_function_names(&graph.named_labels);
        for label in [CoreLabel(7), CoreLabel(8), CoreLabel(9), CoreLabel(10), CoreLabel(42), CoreLabel(999)] {
            assert_eq!(names.get(&label).cloned(), graph.function_name(label), "label {label:?}");
        }
        assert_eq!(names[&CoreLabel(7)].function, "first");
        assert_eq!(names[&CoreLabel(8)].function, "fallback");
        assert_eq!(names.len(), 4);
    }
    #[test]
    fn indexed_lookup_matches_large_named_and_anonymous_population() {
        let mut graph = empty_graph();
        for i in 0..1024u64 {
            graph.named_labels.insert(GraphVertex::Named {
                module: format!("Module{i:04}"), function: "function".into(), arity: 1.into(),
            }, CoreLabel(i));
        }
        let names = index_function_names(&graph.named_labels);
        let labels = (0..2048).map(CoreLabel).collect::<Vec<_>>();
        let start = std::time::Instant::now();
        let legacy = labels.iter().map(|label| graph.function_name(*label)).collect::<Vec<_>>();
        let legacy_ns = start.elapsed().as_nanos();
        let start = std::time::Instant::now();
        let indexed = labels.iter().map(|label| names.get(label).cloned()).collect::<Vec<_>>();
        let indexed_ns = start.elapsed().as_nanos();
        assert_eq!(legacy, indexed);
        println!("lookup population: 1024 named, 1024 anonymous; legacy_ns={legacy_ns}; indexed_ns={indexed_ns}; exact_match=true");
    }

}

#[cfg(test)]
mod frozen_generation_world_tests {
    use super::*;
    fn name(function: &str) -> Mfa {
        Mfa { module: "frozen".into(), function: function.into(), arity: 0 }
    }
    fn signature(value: &str) -> Signature {
        Signature { arguments: vec![], result: Type::atom(value) }
    }
    fn overlay<'a>(levels: &'a [usize], names: &[Mfa]) -> PhaseOverlay<'a> {
        PhaseOverlay {
            owner: names.iter().cloned().map(|mfa| (mfa, 0)).collect(),
            levels,
            slots: vec![Default::default()],
        }
    }
    fn recording<'a>(world: &'a CallWorld<'a>, contracts: &'a BTreeMap<Mfa, Result<GenerationActiveContract, String>>,
                     overlay: &'a PhaseOverlay<'a>) -> RecordingWorld<'a, 'a> {
        RecordingWorld { world, contracts, reads: Default::default(), overlay: Some((overlay, 1)),
                         frozen_owners: Default::default(), early: Default::default() }
    }
    fn published(signatures: BTreeMap<Mfa, Signature>, contracts: BTreeMap<Mfa, Result<GenerationActiveContract, String>>) -> OverlayEntry {
        OverlayEntry { published: Published { signatures: vec![], contracts: vec![] }, signatures, contracts }
    }
    #[test]
    fn missing_owner_stays_frozen_after_publication_and_actual_commit_marks_retry() {
        let mfa = name("signature");
        let mut world = CallWorld::default();
        world.named_signatures.insert(mfa.clone(), signature("base"));
        let contracts = BTreeMap::new();
        let levels = [0];
        let overlay = overlay(&levels, &[mfa.clone()]);
        let owner = recording(&world, &contracts, &overlay);
        let snapshot = owner.parallel_generation_snapshot().unwrap();
        let first = snapshot.signature(&mfa).unwrap();
        assert!(overlay.slots[0].set(published(BTreeMap::from([(mfa.clone(), signature("new"))]), BTreeMap::new())).is_ok());
        assert!(call_signature_value(snapshot.signature(&mfa).unwrap()) == call_signature_value(first.clone()));
        assert!(owner.early.lock().unwrap().is_empty());
        assert!(owner.reads.lock().unwrap().signatures.is_empty());
        owner.commit_generation_queries(&[GenerationLibraryQuery::Signature { mfa: mfa.clone(), result: Ok(first) }]);
        assert!(owner.early.lock().unwrap().contains(&0));
        assert!(owner.reads.lock().unwrap().signatures[&mfa] == Some((vec![], Type::atom("base"))));
        let retry = recording(&world, &contracts, &overlay);
        assert!(call_signature_value(retry.signature(&mfa).unwrap()) == Some((vec![], Type::atom("new"))));
        assert!(retry.early.lock().unwrap().is_empty());
    }
    #[test]
    fn owner_classification_is_shared_between_names_and_contract_source() {
        let sig = name("signature");
        let contract = name("contract");
        let world = CallWorld::default();
        let contracts = BTreeMap::new();
        let levels = [0];
        let overlay = overlay(&levels, &[sig.clone(), contract.clone()]);
        let owner = recording(&world, &contracts, &overlay);
        let snapshot = owner.parallel_generation_snapshot().unwrap();
        assert!(snapshot.signature(&sig).unwrap().is_none());
        assert!(overlay.slots[0].set(published(BTreeMap::from([(sig, signature("new"))]),
             BTreeMap::from([(contract.clone(), Err("overlay contract failure".into()))]))).is_ok());
        assert!(snapshot.active_contract(&contract).unwrap().is_none());
        assert!(owner.contract_source_at(&contract, false) == ContractSource::Absent);
        assert_eq!(owner.frozen_owners.lock().unwrap().len(), 1);
        assert!(owner.early.lock().unwrap().is_empty());
        owner.commit_generation_queries(&[GenerationLibraryQuery::ActiveContract { mfa: contract.clone(), succeeded: true }]);
        assert!(owner.early.lock().unwrap().contains(&0));
        assert!(owner.reads.lock().unwrap().contracts[&contract] == ContractSource::Absent);
    }
    #[test]
    fn speculative_queries_do_not_log_and_actual_prefix_commits_only_successes() {
        let first = name("first");
        let later = name("later");
        let contract = name("contract");
        let world = CallWorld::default();
        let contracts = BTreeMap::new();
        let levels = [0];
        let overlay = overlay(&levels, &[first.clone(), later.clone(), contract.clone()]);
        assert!(overlay.slots[0].set(published(BTreeMap::from([(first.clone(), signature("first")), (later.clone(), signature("later"))]),
             BTreeMap::from([(contract.clone(), Err("native contract failure".into()))]))).is_ok());
        let owner = recording(&world, &contracts, &overlay);
        let snapshot = owner.parallel_generation_snapshot().unwrap();
        let actual_first = snapshot.signature(&first);
        let _discarded_count = snapshot.signature(&later).unwrap();
        assert!(matches!(snapshot.active_contract(&contract), Err(e) if e == "native contract failure"));
        assert!(owner.contract_source_at(&contract, false) == ContractSource::Active);
        assert!(owner.reads.lock().unwrap().signatures.is_empty());
        assert!(owner.reads.lock().unwrap().contracts.is_empty());
        assert!(owner.early.lock().unwrap().is_empty());
        owner.commit_generation_queries(&[
            GenerationLibraryQuery::Signature { mfa: first.clone(), result: actual_first },
            GenerationLibraryQuery::ActiveContract { mfa: contract, succeeded: false },
        ]);
        let reads = owner.reads.lock().unwrap();
        assert_eq!(reads.signatures.len(), 1);
        assert!(reads.signatures.contains_key(&first));
        assert!(!reads.signatures.contains_key(&later));
        assert!(reads.contracts.is_empty());
    }
    #[test]
    fn memo_validation_and_failed_actual_queries_preserve_missing_owner_retry() {
        let mfa = name("missing");
        let world = CallWorld::default();
        let contracts = BTreeMap::new();
        let levels = [0];
        let overlay = overlay(&levels, &[mfa.clone()]);
        let owner = recording(&world, &contracts, &overlay);
        let mut memo = LibraryReads::default();
        memo.signatures.insert(mfa.clone(), None);
        assert!(memo.still_valid(&owner));
        assert!(owner.early.lock().unwrap().contains(&0));
        assert!(owner.reads.lock().unwrap().signatures.is_empty());
        owner.early.lock().unwrap().clear();
        owner.commit_generation_queries(&[
            GenerationLibraryQuery::Signature { mfa: mfa.clone(), result: Err("stored generation failure".into()) },
            GenerationLibraryQuery::ActiveContract { mfa, succeeded: false },
        ]);
        assert!(owner.early.lock().unwrap().contains(&0));
        let reads = owner.reads.lock().unwrap();
        assert!(reads.signatures.is_empty() && reads.contracts.is_empty());
    }
}

#[cfg(test)]
mod diagnostic_snapshot_tests {
    use super::*;
    use crate::{core::{CompilerProfile, CoreExpr, CoreModule, CoreNode, CoreProgram, CoreVersion, ErlangTerm, NodeId, TermId, TermMapOrder},
                function_state::{FunctionDefinition, FunctionState}, program::FunctionName, worklist::FunctionKey};
    fn literal_function() -> CoreProgram {
        let terms = vec![ErlangTerm::Nil, ErlangTerm::Atom("snapshot".into()), ErlangTerm::Atom("f".into()),
                         ErlangTerm::Integer("0".into()), ErlangTerm::Tuple(vec![TermId(2), TermId(3)]), ErlangTerm::Atom("ok".into())];
        let expressions = vec![CoreExpr::Literal(TermId(1)), CoreExpr::Var(TermId(4)), CoreExpr::Literal(TermId(5)),
            CoreExpr::Fun { params: vec![], body: NodeId(2) }, CoreExpr::Module { name: NodeId(0), exports: vec![NodeId(1)],
                attributes: vec![], definitions: vec![(NodeId(1), NodeId(3))] }];
        let nodes = expressions.into_iter().enumerate().map(|(id, expr)| CoreNode { id: NodeId(id), annotations: TermId(0), expr }).collect();
        CoreProgram { version: CoreVersion::V6, profile: CompilerProfile { otp: "28".into(), erts: "16".into(),
            elixir: "1.20.4".into(), dialyzer: "5".into(), core_options: vec![], term_map_order: TermMapOrder::NativeToListWithIterator },
            atom_layout: None, contract_closure_profile: None, modules: vec![CoreModule { name: "snapshot".into(), source_forms: TermId(0),
            core: NodeId(4), terms: terms.into(), nodes, map_iterator_orders: BTreeMap::new(), contract_variable_maps: BTreeMap::new() }],
            node_count: 5, term_count: 6, kinds: BTreeMap::new() }
    }
    #[test]
    fn saved_owners_resume_warning_loop_matches_fresh_called_function() {
        let core = literal_function();
        let labels = crate::labels::Labels::build(&core, CoreLabel(0)).unwrap();
        let nodes = DataflowNodes::build(&core, &labels).unwrap();
        let function = NodeRef { module: ModuleId(0), node: 3 };
        let label = nodes.label(function).unwrap();
        let initial_functions = FunctionState::new(vec![FunctionDefinition { key: FunctionKey::Label(label), node: Some(function),
            arity: 0, escaping: true, name: Some(FunctionName { module: "snapshot".into(), name: "f".into(), arity: 0 }),
            raw_signature: None, callers: vec![] }]);
        let registry = Registry::load(include_str!("../corpus/bifs.json")).unwrap();
        let letrec = crate::letrec_types::LetrecLinks::default();
        let initial = BinderState::<AnalysisWarning>::for_compiler_module(&core, ModuleId(0), false).unwrap();
        let mut functions = initial_functions.clone();
        let mut patterns = Patterns::new(&nodes);
        let inferred = BodyFlow { patterns: &mut patterns, functions: &mut functions, bifs: &registry,
            calls: None, guards: None, letrec: &letrec }.analyze_module(&initial, false).unwrap();
        assert!(!functions.warning_mode() && !inferred.warnings.warning_mode);
        assert!(matches!(inferred.current_fun, crate::binder::WarningFunction::Undefined));
        assert!(functions.function_types()[&label].result == Type::atom("ok"));
        let saved = ModuleSnapshot { functions, patterns, state: inferred };
        let ModuleSnapshot { mut functions, mut patterns, state } = saved;
        let resumed = BodyFlow { patterns: &mut patterns, functions: &mut functions, bifs: &registry,
            calls: None, guards: None, letrec: &letrec }.resume_warning_loop(&state).unwrap();
        let mut fresh_functions = initial_functions;
        let mut fresh_patterns = Patterns::new(&nodes);
        let fresh = BodyFlow { patterns: &mut fresh_patterns, functions: &mut fresh_functions, bifs: &registry,
            calls: None, guards: None, letrec: &letrec }.analyze_module(&initial, true).unwrap();
        assert!(functions.function_types() == fresh_functions.function_types());
        assert_eq!(resumed.warnings, fresh.warnings);
        assert!(resumed.warnings.warning_mode && functions.warning_mode());
        assert!(matches!(resumed.current_fun, crate::binder::WarningFunction::Undefined));
        assert_eq!(patterns.allocation_count(), fresh_patterns.allocation_count());
    }
    #[test]
    fn recorded_call_reads_reject_changed_arguments_returns_and_presence() {
        let mfa = Mfa { module: "snapshot".into(), function: "callee".into(), arity: 1 };
        let original = Signature { arguments: vec![Type::atom("arg")], result: Type::atom("ret") };
        let mut world = CallWorld::default();
        world.named_signatures.insert(mfa.clone(), original.clone());
        let mut reads = CallReads::default();
        reads.named.insert(mfa.clone(), Some(original.clone()));
        assert!(reads.still_valid(&world));
        world.named_signatures.insert(mfa.clone(), Signature { arguments: vec![Type::any()], result: original.result.clone() });
        assert!(!reads.still_valid(&world));
        world.named_signatures.insert(mfa.clone(), Signature { arguments: original.arguments.clone(), result: Type::any() });
        assert!(!reads.still_valid(&world));
        world.named_signatures.remove(&mfa);
        assert!(!reads.still_valid(&world));
        reads.named.insert(mfa.clone(), None);
        assert!(reads.still_valid(&world));
        world.named_signatures.insert(mfa, original);
        assert!(!reads.still_valid(&world));
    }
}

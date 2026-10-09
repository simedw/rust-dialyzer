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
            GenerationCaptureInputs, GenerationOutput, GenerationPhase, LibraryTypes, PropagatedTypes,
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
    module_memo: std::sync::Mutex<BTreeMap<ModuleId, std::sync::Arc<ModuleMemo>>>,
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
        let records = PreparedRecords::for_modules(
            inputs.definitions,
            (0..inputs.nodes.core().modules.len()).map(ModuleId),
        )
        .map_err(CorePhaseError::Boundary)?;
        let folded_nodes = DataflowNodes::build(&inputs.folded.program, inputs.nodes.labels())
            .map_err(CorePhaseError::Boundary)?;
        let guard_literals = GuardLiterals::new(inputs.folded).map_err(CorePhaseError::Boundary)?;
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
impl ModuleBodies<'_, '_> {
    fn module_body(
        &self,
        module: ModuleId,
        previous: &FunctionTypes,
        calls: &dyn CallLookup,
        warnings: bool,
    ) -> Result<(FunctionTypes, Vec<NativeWarning>), CorePhaseError> {
        let mut timings = crate::phase_timings::PhaseTimings::new(if warnings {
            "module_diagnostics"
        } else {
            "module_refinement"
        });
        let inputs = self
            .graph
            .function_inputs(module, self.program, self.nodes.labels(), previous.clone())
            .map_err(CorePhaseError::Boundary)?;
        let mut functions =
            build_function_state(self.nodes, module, &inputs).map_err(CorePhaseError::Boundary)?;
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
        let mut patterns = Patterns::new(self.nodes);
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
        let mut bound = flow
            .analyze_module(initial, warnings)
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
        Ok((
            signatures,
            bound
                .warnings
                .warnings
                .into_iter()
                .map(|warning| warning.0)
                .collect(),
        ))
    }
    pub fn module_diagnostics(
        &self,
        module: ModuleId,
        previous: &FunctionTypes,
    ) -> Result<ModuleDiagnostics, CorePhaseError> {
        self.module_body(module, previous, self.world, true)
            .map(|(signatures, warnings)| ModuleDiagnostics {
                signatures,
                warnings,
            })
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
                    early: Default::default(),
                };
                let request = SccInput {
                    runtime: graph.runtime,
                    ordered_mfas: job.ordered_mfas,
                    next_core_label: graph.next_core_label,
                    previous: job.previous,
                };
                let result = this.analyze_scc_in(&request, &library);
                let early = library.early.into_inner();
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
        let ordered = crate::ordered_parallel::parallel_map(&order, |&i| this.refine_module_shared(&requests[i]));
        timings.label(|| self.module_memo_counts.describe(requests.len()));
        let mut results: Vec<_> = (0..requests.len()).map(|_| None).collect();
        for (i, result) in order.into_iter().zip(ordered) {
            results[i] = Some(result);
        }
        results.into_iter().map(|r| r.expect("every module refined")).collect()
    }
}

impl CorePhases<'_> {
    /// Module refinement is memoized across fixpoint iterations like SCCs.
    /// The body analysis depends on the module's Core, the fixed run inputs
    /// (definitions, call sites, guards, letrec links), `previous` and the
    /// signatures and contracts it reads from the call world; with identical
    /// values for all of these it computes the same types.
    fn refine_module_shared(&self, request: &ModuleRequest<'_>) -> Result<FunctionTypes, CorePhaseError> {
        let memo = self.module_memo.lock().unwrap().get(&request.module).cloned();
        if let Some(memo) = memo
            && memo.previous == *request.previous
        {
            self.module_memo_counts.same_previous.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if memo.reads.still_valid(&self.world) {
                self.module_memo_counts.hits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if crate::lattice::verify_fast_paths() {
                    let (fresh, _) = self.refine_module_uncached(request)?;
                    assert_eq!(fresh, memo.result, "memoized module result differs: {:?}", request.module);
                }
                return Ok(memo.result.clone());
            }
        }
        let (result, reads) = self.refine_module_uncached(request)?;
        self.module_memo.lock().unwrap().insert(
            request.module,
            std::sync::Arc::new(ModuleMemo {
                previous: request.previous.clone(),
                reads,
                result: result.clone(),
            }),
        );
        Ok(result)
    }
    fn refine_module_uncached(&self, request: &ModuleRequest<'_>) -> Result<(FunctionTypes, CallReads), CorePhaseError> {
        let calls = RecordingCalls {
            world: &self.world,
            reads: Default::default(),
        };
        let (signatures, _) = self.module_bodies().module_body(request.module, request.previous, &calls, false)?;
        Ok((signatures, calls.reads.into_inner()))
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
        if let Some(memo) = memo
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
        self.scc_memo.lock().unwrap().insert(
            request.ordered_mfas.to_vec(),
            std::sync::Arc::new(SccMemo {
                next_core_label: request.next_core_label,
                previous: request.previous.clone(),
                reads: library.reads.take(),
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
    reads: std::cell::RefCell<LibraryReads>,
    /// Unpublished results of the current phase and the reader's wave.
    overlay: Option<(&'w PhaseOverlay<'w>, usize)>,
    /// Lower-wave SCCs read before they committed; the reading SCC must
    /// run again after them.
    early: std::cell::RefCell<std::collections::BTreeSet<usize>>,
}
impl RecordingWorld<'_, '_> {
    /// The committed overlay entry owning `mfa`, if the reader may see it.
    fn visible(&self, mfa: &Mfa) -> Option<&OverlayEntry> {
        let (overlay, level) = self.overlay?;
        let &owner = overlay.owner.get(mfa)?;
        if overlay.levels[owner] >= level {
            return None;
        }
        let entry = overlay.slots[owner].get();
        if entry.is_none() {
            self.early.borrow_mut().insert(owner);
        }
        entry
    }
    fn current_signature(&self, mfa: &Mfa) -> Result<Option<CallSignature>, String> {
        if let Some(signature) = self.visible(mfa).and_then(|entry| entry.signatures.get(mfa)) {
            return Ok(Some(CallSignature {
                arguments: signature.arguments.clone(),
                returns: signature.result.clone(),
            }));
        }
        LibraryTypes::signature(self.world, mfa)
    }
    fn current_contract_source(&self, mfa: &Mfa) -> ContractSource {
        if self.visible(mfa).is_some_and(|entry| entry.contracts.contains_key(mfa)) {
            ContractSource::Active
        } else {
            contract_source(self.world, mfa)
        }
    }
}
impl LibraryTypes for RecordingWorld<'_, '_> {
    fn signature(&self, mfa: &Mfa) -> Result<Option<CallSignature>, String> {
        let signature = self.current_signature(mfa)?;
        self.reads.borrow_mut().signatures.entry(mfa.clone()).or_insert_with(|| {
            signature.as_ref().map(|signature| (signature.arguments.clone(), signature.returns.clone()))
        });
        Ok(signature)
    }
    fn active_contract(&self, mfa: &Mfa) -> Result<Option<GenerationActiveContract>, String> {
        let overlay = self.visible(mfa).and_then(|entry| entry.contracts.get(mfa));
        let contract = match overlay.or_else(|| self.contracts.get(mfa)) {
            Some(contract) => Some(contract.clone()?),
            None => LibraryTypes::active_contract(self.world, mfa)?,
        };
        self.reads.borrow_mut().contracts.entry(mfa.clone()).or_insert_with(|| self.current_contract_source(mfa));
        Ok(contract)
    }
}

struct ModuleMemo {
    previous: FunctionTypes,
    reads: CallReads,
    result: FunctionTypes,
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

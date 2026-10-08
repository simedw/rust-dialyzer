use crate::{
    analysis_inputs::PreparedRecords,
    bifs::registry::{Mfa, Registry},
    binder::{BinderState, OpaqueMatch},
    binder_module::ModuleBinderError,
    body_calls::{ActiveContract, CallWorld},
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
    core_analysis::{AnalysisPhases, ModuleRequest, SccRequest},
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
    function_names: BTreeMap<CoreLabel, Mfa>,
    core_captures: CoreCaptureStore<'a>,
    scc_memo: std::sync::Mutex<BTreeMap<Vec<Mfa>, std::sync::Arc<SccMemo>>>,
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
            function_names: index_function_names(&inputs.graph.named_labels),
            core_captures: CoreCaptureStore::new(inputs.nodes.core(), inputs.nodes.labels()),
            scc_memo: Default::default(),
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
            guard_literals: &self.guard_literals, letrec: &self.letrec }
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
}
impl ModuleBodies<'_, '_> {
    fn module_body(
        &self,
        module: ModuleId,
        previous: &FunctionTypes,
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
        let initial = BinderState::<AnalysisWarning>::for_module(
            self.nodes.core(),
            self.definitions,
            module,
            false,
        )
        .map_err(CorePhaseError::Metadata)?;
        let empty_letrec = crate::letrec_types::LetrecLinks::default();
        let letrec = self.letrec.get(&module).unwrap_or(&empty_letrec);
        let mut flow = BodyFlow {
            patterns: &mut patterns,
            functions: &mut functions,
            bifs: self.bifs,
            calls: Some((&producer, &self.world)),
            guards: Some(&guards),
            letrec,
        };
        timings.mark("prepare_module_body");
        let mut bound = flow
            .analyze_module(&initial, warnings)
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
        self.module_body(module, previous, true)
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
    fn refine_module_batch(
        &mut self,
        requests: Vec<ModuleRequest<'_>>,
    ) -> Vec<Result<FunctionTypes, Self::Error>> {
        let this = &*self;
        crate::ordered_parallel::parallel_map(&requests, |request| this.refine_module_shared(request))
    }
}

impl CorePhases<'_> {
    fn refine_module_shared(&self, request: &ModuleRequest<'_>) -> Result<FunctionTypes, CorePhaseError> {
        self.module_bodies().module_body(request.module, request.previous, false)
            .map(|(signatures, _)| signatures)
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
        let memo = self.scc_memo.lock().unwrap().get(request.ordered_mfas).cloned();
        if let Some(memo) = memo
            && memo.next_core_label == request.next_core_label
            && memo.previous == *request.previous
            && memo.reads.still_valid(&self.world)
        {
            if crate::lattice::verify_fast_paths() {
                let (fresh, _) = self.analyze_scc_uncached(request)?;
                assert_eq!(fresh, memo.result, "memoized SCC result differs: {:?}", request.ordered_mfas);
            }
            return Ok(memo.result.clone());
        }
        let (result, reads) = self.analyze_scc_uncached(request)?;
        self.scc_memo.lock().unwrap().insert(
            request.ordered_mfas.to_vec(),
            std::sync::Arc::new(SccMemo {
                next_core_label: request.next_core_label,
                previous: request.previous.clone(),
                reads,
                result: result.clone(),
            }),
        );
        Ok(result)
    }
    fn analyze_scc_uncached(&self, request: &SccRequest<'_>) -> Result<(FunctionTypes, LibraryReads), CorePhaseError> {
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
        let library = RecordingWorld {
            world: &self.world,
            contracts: &self.generation_contracts,
            reads: Default::default(),
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
                library: &library,
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
        Ok((result, library.reads.into_inner()))
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
    fn still_valid(&self, world: &CallWorld<'_>) -> bool {
        self.signatures.iter().all(|(mfa, value)| {
            LibraryTypes::signature(world, mfa).is_ok_and(|current| call_signature_value(current) == *value)
        }) && self.contracts.iter().all(|(mfa, source)| contract_source(world, mfa) == *source)
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
}
impl LibraryTypes for RecordingWorld<'_, '_> {
    fn signature(&self, mfa: &Mfa) -> Result<Option<CallSignature>, String> {
        let signature = LibraryTypes::signature(self.world, mfa)?;
        self.reads.borrow_mut().signatures.entry(mfa.clone()).or_insert_with(|| {
            signature.as_ref().map(|signature| (signature.arguments.clone(), signature.returns.clone()))
        });
        Ok(signature)
    }
    fn active_contract(&self, mfa: &Mfa) -> Result<Option<GenerationActiveContract>, String> {
        let contract = match self.contracts.get(mfa) {
            Some(contract) => Some(contract.clone()?),
            None => LibraryTypes::active_contract(self.world, mfa)?,
        };
        self.reads.borrow_mut().contracts.entry(mfa.clone()).or_insert_with(|| contract_source(self.world, mfa));
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

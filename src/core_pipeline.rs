use crate::{
    constraints::{
        analyze_scc::SolverSelection, generation_capture_headers::GenerationRuntimeCaptures,
    },
    contract_book::ContractBook,
    core_analysis::{AnalysisError, AnalysisState, CoreAnalyzer},
    core_phases::{CorePhaseError, CorePhaseInputs, CorePhases, PreparedPhaseInputs},
    dependencies::Store,
    diagnostic_inputs::{DiagnosticInputError, DiagnosticInputs, TargetCallbackState},
    native_runtime_hash::NativeAtomLayout,
    program::ModuleId,
    source_callgraph::{GraphVertex, SourceCallGraph},
    source_schedule::{ScheduleError, ScopedReferenceAllocator, SourceSchedule},
    success_phase::FunctionTypes,
    warning_payload::NativeWarning,
};
use std::{collections::BTreeSet, convert::Infallible};

#[derive(Debug)]
pub enum PipelineError {
    Boundary(String),
    Phase(CorePhaseError),
    Schedule(ScheduleError<Infallible>),
    Analysis(Box<AnalysisError<CorePhaseError, ScheduleError<Infallible>>>),
    Diagnostics(DiagnosticInputError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalysisMode {
    PltBuild,
    SuccessTypings,
}

pub struct CollectedAnalysis {
    pub state: AnalysisState,
    pub documentation: Option<FunctionTypes>,
    pub raw_warnings: Vec<NativeWarning>,
}

pub type PipelineSchedule = SourceSchedule<ScopedReferenceAllocator>;

/// The schedule `analyze` starts from, with its first `finalize` (the
/// condensation of the whole call graph) already done. It reads only the
/// cleaned call graph and the program, so setup builds it alongside the
/// definitions; failures surface where `analyze` reported them.
pub fn prepare_schedule(
    graph: &SourceCallGraph,
    program: &crate::program::Program,
    next_core_label: crate::labels::CoreLabel,
    scope: String,
) -> Result<PipelineSchedule, PipelineError> {
    let runtime = GenerationRuntimeCaptures::for_analysis(scope.clone(), next_core_label, program)
        .map_err(PipelineError::Boundary)?;
    let allocator = ScopedReferenceAllocator::new(scope)
        .map_err(|error| PipelineError::Boundary(format!("{error:?}")))?;
    let mut schedule = SourceSchedule::new(runtime, allocator).map_err(PipelineError::Schedule)?;
    schedule.prefinalize(graph);
    Ok(schedule)
}

pub fn analyze<'a>(
    inputs: CorePhaseInputs<'a>,
    library: &'a Store,
    callbacks: &TargetCallbackState<'a>,
    layout: &NativeAtomLayout,
    scope: String,
    solvers: SolverSelection,
    mode: AnalysisMode,
    report_modules: Option<&BTreeSet<String>>,
) -> Result<CollectedAnalysis, PipelineError> {
    analyze_prepared(
        inputs, library, callbacks, layout, scope, solvers, mode, report_modules, None,
    )
}

/// What setup prepared ahead for `analyze_prepared`.
pub struct PreparedPipeline<'a> {
    pub schedule: Result<PipelineSchedule, PipelineError>,
    pub phases: PreparedPhaseInputs<'a>,
}

/// `analyze`, starting from parts setup built ahead when given.
#[allow(clippy::too_many_arguments)]
pub fn analyze_prepared<'a>(
    inputs: CorePhaseInputs<'a>,
    library: &'a Store,
    callbacks: &TargetCallbackState<'a>,
    layout: &NativeAtomLayout,
    scope: String,
    solvers: SolverSelection,
    mode: AnalysisMode,
    report_modules: Option<&BTreeSet<String>>,
    prepared: Option<PreparedPipeline<'a>>,
) -> Result<CollectedAnalysis, PipelineError> {
    let mut timings = crate::phase_timings::PhaseTimings::new("core_pipeline");
    let analyzer = CoreAnalyzer {
        nodes: inputs.nodes,
        program: inputs.program,
        graph: inputs.graph,
        bifs: inputs.bifs,
        next_core_label: inputs.nodes.labels().next_label,
    };
    let names = inputs
        .graph
        .source_graph()
        .ordered_vertices()
        .map_err(|error| PipelineError::Boundary(format!("{error:?}")))?
        .into_iter()
        .filter_map(|vertex| match vertex {
            GraphVertex::Named { module, .. } => Some(module),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let modules = names
        .into_iter()
        .map(|name| {
            inputs
                .graph
                .module_names
                .iter()
                .position(|module| module == &name)
                .map(ModuleId)
                .ok_or_else(|| PipelineError::Boundary("unpruned external module".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let candidates = inputs
        .definitions
        .contracts
        .callables()
        .map(|(name, contract)| (name.clone(), (*contract.summary).clone()))
        .collect();
    let graph = inputs.graph;
    let (mut schedule, mut phases) = match prepared {
        Some(prepared) => {
            let schedule = prepared.schedule?;
            let phases = CorePhases::with_prepared(inputs, library, solvers, prepared.phases)
                .map_err(PipelineError::Phase)?;
            (schedule, phases)
        }
        None => {
            let runtime = GenerationRuntimeCaptures::for_analysis(
                scope.clone(),
                analyzer.next_core_label,
                inputs.program,
            )
            .map_err(PipelineError::Boundary)?;
            let allocator = ScopedReferenceAllocator::new(scope)
                .map_err(|error| PipelineError::Boundary(format!("{error:?}")))?;
            let schedule =
                SourceSchedule::new(runtime, allocator).map_err(PipelineError::Schedule)?;
            let phases =
                CorePhases::new(inputs, library, solvers).map_err(PipelineError::Phase)?;
            (schedule, phases)
        }
    };
    timings.mark("setup");
    let state = analyzer
        .run(
            AnalysisState {
                signatures: FunctionTypes::new(),
                contracts: ContractBook::new(candidates),
            },
            &mut phases,
            &mut schedule,
        )
        .map_err(|error| PipelineError::Analysis(Box::new(error)))?;
    timings.mark("inference_and_refinement");
    if mode == AnalysisMode::PltBuild {
        return Ok(CollectedAnalysis {
            state,
            documentation: None,
            raw_warnings: Vec::new(),
        });
    }
    // Warnings are only produced for reported modules (dialyxir reports none
    // for PLT modules); inference above still covers every module.
    let modules = match report_modules {
        Some(report) => modules
            .into_iter()
            .filter(|m| report.contains(&graph.module_names[m.0]))
            .collect(),
        None => modules,
    };
    let mut diagnostic_timings = crate::phase_timings::PhaseTimings::new("diagnostic_components");
    let diagnostics = DiagnosticInputs {
        nodes: phases.nodes,
        program: phases.program,
        callgraph: phases.graph,
        library,
        signatures: &state.signatures,
        atom_layout: layout,
        target_callbacks: callbacks,
    }
    .prepare(&modules)
    .map_err(|e| PipelineError::Boundary(format!("diagnostics prepare: {e:?}")))?;
    diagnostic_timings.mark("prepare_inputs");
    let mut raw_warnings = diagnostics
        .contracts(phases.bifs)
        .map_err(|e| PipelineError::Boundary(format!("diagnostics contracts: {e:?}")))?;
    diagnostic_timings.mark("contract_warnings");
    let mut documentation = FunctionTypes::new();
    let bodies = phases.module_bodies();
    let workers = std::thread::available_parallelism().map(usize::from).unwrap_or(1);
    crate::ordered_parallel::ordered_prefetch(modules.len(), workers, |index| {
        let module = modules[index];
        let mut module_timings = crate::phase_timings::PhaseTimings::new("diagnostic_module_components");
        let name = &phases.program.modules[module.0].name;
        module_timings.label(|| name.clone());
        let missing = diagnostics.missing_contract_functions(module).map_err(|e| {
            PipelineError::Boundary(format!("module {name} missing_contract_functions: {e:?}"))
        })?;
        module_timings.mark("missing_contract_functions");
        let previous = analyzer.module_previous(module, &state).map_err(PipelineError::Boundary)?;
        module_timings.mark("previous_signatures");
        let body = bodies.module_diagnostics(module, &previous).map_err(PipelineError::Phase)?;
        module_timings.mark("body_diagnostics");
        let callbacks = diagnostics.callbacks(module).map_err(|e| {
            PipelineError::Boundary(format!("module {name} callbacks: {e:?}"))
        })?;
        module_timings.mark("callback_warnings");
        Ok((missing, body, callbacks))
    }, |_, (missing, body, callbacks)| {
        raw_warnings.extend(missing);
        raw_warnings.extend(body.warnings);
        raw_warnings.extend(callbacks);
        documentation.extend(body.signatures);
        Ok(())
    })?;
    diagnostic_timings.mark("all_modules");
    timings.mark("diagnostics");
    drop(bodies);
    crate::teardown::release(diagnostics);
    crate::teardown::release(phases);
    crate::teardown::release(schedule);
    Ok(CollectedAnalysis {
        state,
        documentation: Some(documentation),
        raw_warnings,
    })
}

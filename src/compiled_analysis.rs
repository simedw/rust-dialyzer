use crate::{
    bifs::registry::Registry,
    binder::{Binder, BinderState, OpaqueMatch},
    callgraph_cleanup::{self, CleanupContext, CleanupOutcome},
    closure_dependencies::{ClosureDependencies, DependencyPolicy},
    compiled_definitions::PreparedDefinitions,
    constraints::{analyze_scc::SolverSelection, closure_order::ClosureCodeProfile},
    core::CoreProgram,
    core_phases::CorePhaseInputs,
    core_pipeline::{self, CollectedAnalysis},
    dataflow_nodes::DataflowNodes,
    dependencies::{ImportPlan, PriorPlt, Store},
    diagnostic_inputs::{PreparedCallbackGroups, TargetCallbackState},
    folded_core::FoldedCore,
    labels::{CoreLabel, Labels},
    module_publication::{ModuleDependencies, PublicationInput},
    patterns::Patterns,
    program::{ModuleId, Program, TargetContracts},
    source_callgraph::SourceCallGraph,
    warning_policy::Policy,
    warning_publication::{self, PublishedWarnings, UnknownWarnings},
    warning_render::LocationOption,
    warnings::{Profile, Registry as WarningRegistry, Tag},
};

pub use crate::core_pipeline::AnalysisMode;

pub struct AnalysisInput<'a> {
    pub core: &'a CoreProgram,
    pub mode: AnalysisMode,
    pub prior: &'a str,
    pub import: &'a ImportPlan,
    pub bifs: &'a Registry,
    pub dependency_policy: &'a DependencyPolicy,
    pub warning_registry: &'a WarningRegistry,
    pub legal_warnings: &'a Profile,
    pub closure_profile: &'a ClosureCodeProfile,
    pub contracts: TargetContracts,
    pub solvers: SolverSelection,
    pub location: LocationOption,
    /// Modules whose warnings are reported; `None` reports every module.
    pub report_modules: Option<std::collections::BTreeSet<String>>,
}

pub struct AnalysisOutput {
    pub program: Program,
    pub graph: SourceCallGraph,
    pub library: Store,
    pub definitions: PreparedDefinitions,
    pub callback_groups: PreparedCallbackGroups,
    pub analysis: CollectedAnalysis,
    pub cleanup: CleanupOutcome,
    pub module_dependencies: ModuleDependencies,
    pub unknown: UnknownWarnings,
    pub warnings: PublishedWarnings,
}

pub fn analyze(input: AnalysisInput<'_>) -> Result<AnalysisOutput, String> {
    let mut phase = "labels_and_dataflow";
    analyze_phases(input, &mut phase).map_err(|error| format!("analysis phase {phase}: {error}"))
}

fn analyze_phases(
    input: AnalysisInput<'_>,
    phase: &mut &'static str,
) -> Result<AnalysisOutput, String> {
    let mut timings = crate::phase_timings::PhaseTimings::new("compiled_analysis");
    let layout = input
        .core
        .atom_layout
        .as_ref()
        .ok_or("compiler atom layout missing")?;
    let labels = Labels::build(input.core, CoreLabel(0))?;
    let nodes = DataflowNodes::build(input.core, &labels)?;
    timings.mark("labels_and_dataflow");
    *phase = "program_dependencies_callgraph";
    let program = Program::build_with_contracts(input.core, input.contracts)?;
    let dependencies = ClosureDependencies::analyze(input.core, &program, input.dependency_policy)?;
    let mut graph =
        SourceCallGraph::build(&nodes, &program, &dependencies, input.bifs)
            .map_err(|error| error.to_string())?;
    graph
        .attach_layout(layout.clone())
        .map_err(|error| error.to_string())?;
    timings.mark("program_dependencies_callgraph");
    *phase = "prior_parse";
    let prior = PriorPlt::parse(input.prior, input.core, input.import)?;
    timings.mark("prior_parse");
    *phase = "policy_and_cleanup";
    let policy = Policy::build(input.core, input.warning_registry, input.legal_warnings)
        .map_err(|error| format!("{error:?}"))?;
    *phase = "policy_and_cleanup/binder_states";
    let states = (0..input.core.modules.len())
        .map(|m| {
            BinderState::<OpaqueMatch>::for_compiler_module(input.core, ModuleId(m), true)
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut patterns = Patterns::new(&nodes);
    let binder = Binder {
        patterns: &mut patterns,
    };
    let context = CleanupContext {
        program: &program,
        binder: &binder,
        module_states: &states,
        policy: &policy,
        original_plt: prior.original_membership(),
    };
    *phase = "policy_and_cleanup/cleanup_callgraph";
    let cleanup = callgraph_cleanup::cleanup_callgraph(&mut graph, &context)
        .map_err(|error| error.to_string())?;
    let mut calls = if input.legal_warnings.contains(Tag::WarnUnknown) {
        context.unknown_functions(&cleanup.external_calls)
    } else {
        Vec::new()
    };
    if let Some(report) = &input.report_modules {
        calls.retain(|(module, _)| report.contains(module));
    }
    timings.mark("policy_and_cleanup");
    *phase = "prepare_definitions";
    let definitions = prior
        .prepare_definitions(input.core, &program)
        .map_err(|error| error.to_string())?;
    timings.mark("prepare_definitions");
    *phase = "prior_cleanup";
    let library = prior.after_cleanup(&graph)?;
    timings.mark("prior_cleanup");
    *phase = "callback_preparation";
    let targets = (0..input.core.modules.len())
        .map(ModuleId)
        .collect::<Vec<_>>();
    let callbacks = TargetCallbackState::from_definitions(&definitions, layout)
        .map_err(|error| format!("{error:?}"))?;
    let callback_groups = callbacks
        .persistence_groups()
        .map_err(|error| format!("{error:?}"))?;
    timings.mark("callback_preparation");
    *phase = "folded_core";
    let folded = FoldedCore::build(input.core);
    timings.mark("folded_core");
    *phase = "core_pipeline";
    let analysis = core_pipeline::analyze(
        CorePhaseInputs {
            nodes: &nodes,
            program: &program,
            dependencies: &dependencies,
            graph: &graph,
            definitions: &definitions,
            profile: input.closure_profile,
            folded: &folded,
            bifs: input.bifs,
        },
        &library,
        &callbacks,
        layout,
        format!("rust-analysis/{}", layout.scope),
        input.solvers,
        input.mode,
        input.report_modules.as_ref(),
    )
    .map_err(|error| format!("{error:?}"))?;
    timings.mark("core_pipeline");
    *phase = "publication";
    let module_dependencies = PublicationInput {
        core: input.core,
        program: &program,
        definitions: &definitions,
        targets: &targets,
        use_contracts: input.contracts == TargetContracts::Use,
    }
    .publish(&cleanup.module_dependencies)
    .map_err(|error| format!("{error:?}"))?;
    let unknown = warning_publication::collect_unknown(
        calls,
        &definitions.external_types,
        input.legal_warnings,
        input.report_modules.as_ref(),
    )
    .map_err(|error| format!("{error:?}"))?;
    let unknown_flat = unknown
        .by_module
        .iter()
        .map(|(_, warning)| warning.clone())
        .collect::<Vec<_>>();
    let warnings = warning_publication::publish(
        &analysis.raw_warnings,
        &cleanup.missing_calls,
        &unknown_flat,
        &definitions,
        &policy,
        input.location,
    )
    .map_err(|error| format!("{error:?}"))?;
    timings.mark("publication");
    Ok(AnalysisOutput {
        program,
        graph,
        library,
        definitions,
        callback_groups,
        analysis,
        cleanup,
        module_dependencies,
        unknown,
        warnings,
    })
}

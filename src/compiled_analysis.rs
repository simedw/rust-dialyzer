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
    ordered_parallel::{join, setup_map},
    module_publication::{ModuleDependencies, PublicationInput},
    patterns::Patterns,
    phase_timings::PhaseTimings,
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
    let mut timings = PhaseTimings::new("compiled_analysis");
    let layout = input
        .core
        .atom_layout
        .as_ref()
        .ok_or("compiler atom layout missing")?;
    // Setup steps run concurrently where their inputs allow; failures are
    // checked afterwards in the order the steps run sequentially.
    let core = input.core;
    let ((labels, folded), ((program, prior), (policy, states))) = join(
        || {
            join(
                || {
                    let mut timings = PhaseTimings::new("setup_branch");
                    timings.label(|| "labels".into());
                    let labels = Labels::build(core, CoreLabel(0));
                    timings.mark("labels");
                    labels
                },
                || {
                    let mut timings = PhaseTimings::new("setup_branch");
                    timings.label(|| "folded_core".into());
                    let folded = FoldedCore::build(core);
                    timings.mark("folded_core");
                    folded
                },
            )
        },
        || {
            join(
                || {
                    let mut timings = PhaseTimings::new("setup_branch");
                    timings.label(|| "program".into());
                    let program = Program::build_with_contracts(core, input.contracts);
                    timings.mark("program");
                    let prior = PriorPlt::parse(input.prior, core, input.import);
                    timings.mark("prior_parse");
                    (program, prior)
                },
                || {
                    let mut timings = PhaseTimings::new("setup_branch");
                    timings.label(|| "policy".into());
                    let policy = Policy::build(core, input.warning_registry, input.legal_warnings)
                        .map_err(|error| format!("{error:?}"));
                    timings.mark("policy");
                    let modules = (0..core.modules.len()).map(ModuleId).collect::<Vec<_>>();
                    let states = setup_map(&modules, |&module| {
                        BinderState::<OpaqueMatch>::for_compiler_module(core, module, true)
                            .map_err(|error| error.to_string())
                    })
                    .into_iter()
                    .collect::<Result<Vec<_>, _>>();
                    timings.mark("binder_states");
                    (policy, states)
                },
            )
        },
    );
    let labels = labels?;
    timings.mark("setup_independent");
    let (definitions, (nodes, dependencies, graph, cleanup)) = join(
        || {
            let (Ok(prior), Ok(program)) = (&prior, &program) else {
                return None;
            };
            Some(
                prior
                    .prepare_definitions(core, program)
                    .map_err(|error| error.to_string()),
            )
        },
        || {
            let mut timings = PhaseTimings::new("setup_branch");
            timings.label(|| "callgraph".into());
            let (nodes, dependencies) = join(
                || DataflowNodes::build(core, &labels),
                || {
                    program.as_ref().ok().map(|program| {
                        ClosureDependencies::analyze(core, program, input.dependency_policy)
                    })
                },
            );
            timings.mark("dataflow_and_dependencies");
            let mut graph = match (&nodes, &program, &dependencies) {
                (Ok(nodes), Ok(program), Some(Ok(dependencies))) => Some(
                    SourceCallGraph::build(nodes, program, dependencies, input.bifs)
                        .and_then(|mut graph| {
                            graph.attach_layout(layout.clone())?;
                            Ok(graph)
                        })
                        .map_err(|error| error.to_string()),
                ),
                _ => None,
            };
            timings.mark("callgraph");
            let cleanup = match (&nodes, &program, &mut graph, &prior, &policy, &states) {
                (Ok(nodes), Ok(program), Some(Ok(graph)), Ok(prior), Ok(policy), Ok(states)) => {
                    let mut patterns = Patterns::new(nodes);
                    let binder = Binder {
                        patterns: &mut patterns,
                    };
                    let context = CleanupContext {
                        program,
                        binder: &binder,
                        module_states: states,
                        policy,
                        original_plt: prior.original_membership(),
                    };
                    Some(
                        callgraph_cleanup::cleanup_callgraph(graph, &context)
                            .map(|cleanup| {
                                let mut calls = if input.legal_warnings.contains(Tag::WarnUnknown) {
                                    context.unknown_functions(&cleanup.external_calls)
                                } else {
                                    Vec::new()
                                };
                                if let Some(report) = &input.report_modules {
                                    calls.retain(|(module, _)| report.contains(module));
                                }
                                (cleanup, calls)
                            })
                            .map_err(|error| error.to_string()),
                    )
                }
                _ => None,
            };
            timings.mark("cleanup");
            (nodes, dependencies, graph, cleanup)
        },
    );
    timings.mark("setup_dependent");
    let nodes = nodes?;
    *phase = "program_dependencies_callgraph";
    let program = program?;
    let dependencies = dependencies.expect("program built")?;
    let graph = graph.expect("callgraph inputs built")?;
    *phase = "prior_parse";
    let prior = prior?;
    *phase = "policy_and_cleanup";
    let policy = policy?;
    *phase = "policy_and_cleanup/binder_states";
    states?;
    *phase = "policy_and_cleanup/cleanup_callgraph";
    let (cleanup, calls) = cleanup.expect("cleanup inputs built")?;
    *phase = "prepare_definitions";
    let definitions = definitions.expect("definition inputs built")?;
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
    // Module dependency publication does not read the analysis; it runs
    // alongside it.
    let (analysis, module_dependencies) = join(
        || {
            core_pipeline::analyze(
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
            .map_err(|error| format!("{error:?}"))
        },
        || {
            let mut timings = PhaseTimings::new("setup_branch");
            timings.label(|| "module_dependencies".into());
            let published = PublicationInput {
                core: input.core,
                program: &program,
                definitions: &definitions,
                targets: &targets,
                use_contracts: input.contracts == TargetContracts::Use,
            }
            .publish(&cleanup.module_dependencies)
            .map_err(|error| format!("{error:?}"));
            timings.mark("module_dependencies");
            published
        },
    );
    *phase = "core_pipeline";
    let analysis = analysis?;
    timings.mark("core_pipeline");
    *phase = "publication";
    let module_dependencies = module_dependencies?;
    let unknown = warning_publication::collect_unknown(
        calls,
        &definitions.external_types,
        input.legal_warnings,
        input.report_modules.as_ref(),
    )
    .map_err(|error| format!("{error:?}"))?;
    timings.mark("unknown_warnings");
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

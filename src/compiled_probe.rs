use crate::{
    bifs::registry::Registry,
    closure_dependencies::DependencyPolicy,
    compiled_analysis::{self, AnalysisInput},
    constraints::{analyze_scc::SolverSelection, closure_order::ClosureCodeProfile},
    core::CoreProgram,
    dependencies::ImportPlan,
    lattice::Type,
    program::TargetContracts,
    warning_payload::NativeWarning,
    warning_render::LocationOption,
    warnings::{Profile, Registry as WarningRegistry, Tag},
};
use serde_json::{Value, json};

pub fn analyze(
    core: &CoreProgram,
    row: &Value,
    library: &str,
    profile: &ClosureCodeProfile,
) -> Result<Value, String> {
    let mut timings = crate::phase_timings::PhaseTimings::new("probe_run");
    let tags: Vec<Tag> =
        serde_json::from_value(row["legal_warnings"].clone()).map_err(|error| error.to_string())?;
    let purged = row["purged_modules"]
        .as_array()
        .ok_or("purged_modules list missing")?
        .iter()
        .map(|module| {
            module
                .as_str()
                .map(str::to_owned)
                .ok_or("purged module must be atom name")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let bifs = Registry::load(include_str!("../corpus/bifs.json"))?;
    let policy = DependencyPolicy::parse(include_str!("../corpus/dependency_policy.json"))?;
    let warnings = WarningRegistry::load(include_str!("../corpus/warnings.json"))?;
    let solvers: Vec<String> = row
        .get("solvers")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    timings.mark("registries_and_configuration");
    let output = compiled_analysis::analyze(AnalysisInput {
        mode: if row["analysis_mode"] == "plt_build" {
            crate::compiled_analysis::AnalysisMode::PltBuild
        } else {
            crate::compiled_analysis::AnalysisMode::SuccessTypings
        },
        core,
        prior: library,
        import: &ImportPlan::before_cleanup(purged),
        bifs: &bifs,
        dependency_policy: &policy,
        warning_registry: &warnings,
        legal_warnings: &Profile::from_tags(tags),
        closure_profile: profile,
        contracts: if row["use_contracts"].as_bool() == Some(false) {
            TargetContracts::Ignore
        } else {
            TargetContracts::Use
        },
        solvers: SolverSelection::parse(&solvers).map_err(|error| format!("{error:?}"))?,
        location: if row["location"] == "line" {
            LocationOption::Line
        } else {
            LocationOption::Column
        },
        report_modules: row["report_modules"].as_array().map(|names| {
            names
                .iter()
                .filter_map(|n| n.as_str().map(str::to_owned))
                .collect()
        }),
    })?;
    timings.mark("analysis");
    let raw = NativeWarning::list(
        &output
            .warnings
            .raw
            .iter()
            .cloned()
            .map(|raw| NativeWarning { raw })
            .collect::<Vec<_>>(),
    );
    let signatures =
        output
            .analysis
            .state
            .signatures
            .iter()
            .filter_map(|(label, signature)| {
                output.graph.function_name(*label).map(|name| json!({
            "module":name.module,"function":name.function,"arity":name.arity,
            "arguments":signature.arguments.iter().map(Type::to_value).collect::<Vec<_>>(),
            "result":signature.result.to_value(),
        }))
            })
            .collect::<Vec<_>>();
    let result = json!({"id":row["id"],"warnings":crate::type_exports::arena_to_value(&raw.arena().terms,raw.id()),
        "public_warning_count":output.warnings.raw.len(), "signatures":signatures,
        "cleanup_warning_count":output.cleanup.missing_calls.len(),
        "unknown_warning_count":output.unknown.by_module.len(),
        "module_dependencies":output.module_dependencies.named_projection()?});
    timings.mark("signatures_and_output_projection");
    Ok(result)
}

use rust_dialyzer_poc::{
    bifs::registry::Registry,
    closure_dependencies::DependencyPolicy,
    compiled_analysis::{self, AnalysisInput},
    constraints::{analyze_scc::SolverSelection, closure_order::ClosureCodeProfile},
    core::{CoreProgram, ErlangTerms},
    dependencies::ImportPlan,
    lattice::Type,
    program::TargetContracts,
    term_order::compare_map_keys,
    warning_payload::NativeWarning,
    warning_render::LocationOption,
    warnings::{Profile, Registry as WarningRegistry, Tag},
};
use serde_json::{Value, json};
use std::io::{self, BufRead};

fn run(row: &Value, library: &str, profile: &ClosureCodeProfile) -> Result<Value, String> {
    let core = CoreProgram::parse(&row["core"].to_string())?;
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
    let bifs = Registry::load(include_str!("../../corpus/bifs.json"))?;
    let policy = DependencyPolicy::parse(include_str!("../../corpus/dependency_policy.json"))?;
    let warnings = WarningRegistry::load(include_str!("../../corpus/warnings.json"))?;
    let solvers: Vec<String> = row
        .get("solvers")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    let output = compiled_analysis::analyze(AnalysisInput {
        mode: if row["analysis_mode"] == "plt_build" {
            rust_dialyzer_poc::compiled_analysis::AnalysisMode::PltBuild
        } else {
            rust_dialyzer_poc::compiled_analysis::AnalysisMode::SuccessTypings
        },
        core: &core,
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
    })?;
    let raw = NativeWarning::list(
        &output
            .warnings
            .raw
            .iter()
            .cloned()
            .map(|raw| NativeWarning { raw })
            .collect::<Vec<_>>(),
    );
    let expected = ErlangTerms::parse(&row["warnings"])?;
    let matched = compare_map_keys(raw.arena(), raw.id(), &expected, expected.root)?.is_eq();
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
    Ok(json!({"id":row["id"],"public_warnings_match":matched,
        "warnings":rust_dialyzer_poc::type_exports::arena_to_value(&raw.arena().terms,raw.id()),
        "public_warning_count":output.warnings.raw.len(), "documentation_collected":output.analysis.documentation.is_some(), "signatures":signatures,
        "cleanup_warning_count":output.cleanup.missing_calls.len(),
        "unknown_warning_count":output.unknown.by_module.len(),
        "module_dependencies":rust_dialyzer_poc::type_exports::arena_to_value(&output.module_dependencies.to_raw().arena().terms,output.module_dependencies.to_raw().id())}))
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let library = std::fs::read_to_string(&args[0]).expect("library snapshot");
    let profile = ClosureCodeProfile::parse(
        &serde_json::from_str::<Value>(
            &std::fs::read_to_string(&args[1]).expect("closure profile"),
        )
        .expect("profile JSON"),
    )
    .expect("closure profile");
    for line in io::stdin().lock().lines() {
        let result = line
            .map_err(|error| error.to_string())
            .and_then(|line| {
                serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())
            })
            .and_then(|row| run(&row, &library, &profile));
        println!("{}", result.unwrap_or_else(|error| json!({"error":error})));
    }
}

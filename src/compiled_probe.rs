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

/// Analysis outcome with the signature rows already serialized as JSON.
/// Each `signatures` entry is one row; `result` holds everything else.
pub struct Projected {
    pub result: Value,
    pub signatures: Vec<Vec<u8>>,
}

pub fn analyze(
    core: &CoreProgram,
    row: &Value,
    library: &str,
    profile: &ClosureCodeProfile,
) -> Result<Value, String> {
    let Projected { mut result, signatures } = analyze_projected(core, row, library, profile)?;
    result["signatures"] = signatures
        .iter()
        .map(|row| serde_json::from_slice::<Value>(row).map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?
        .into();
    Ok(result)
}

// A signature's Value tree is about twenty times its JSON text and the
// analysis state is still live here, so rows are serialized one at a time.
pub fn analyze_projected(
    core: &CoreProgram,
    row: &Value,
    library: &str,
    profile: &ClosureCodeProfile,
) -> Result<Projected, String> {
    project(core, row, library, profile, SignatureProjection::Include)
}

pub fn analyze_warnings(
    core: &CoreProgram,
    row: &Value,
    library: &str,
    profile: &ClosureCodeProfile,
) -> Result<Value, String> {
    project(core, row, library, profile, SignatureProjection::Omit).map(|output| output.result)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SignatureProjection {
    Include,
    Omit,
}

fn project(
    core: &CoreProgram,
    row: &Value,
    library: &str,
    profile: &ClosureCodeProfile,
    signatures: SignatureProjection,
) -> Result<Projected, String> {
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
    let bifs = Registry::standard()?;
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
    // With report_modules, only those modules' signatures are emitted.
    let report_modules: Option<std::collections::BTreeSet<String>> =
        row["report_modules"].as_array().map(|names| {
            names.iter().filter_map(|n| n.as_str().map(str::to_owned)).collect()
        });
    let signature_rows = if signatures == SignatureProjection::Include {
        output.analysis.state.signatures.iter().filter_map(|(label, signature)| {
            output.graph.function_name(*label)
                .filter(|name| report_modules.as_ref().is_none_or(|r| r.contains(name.module.as_str())))
                .map(|name| (name, signature))
        }).collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let signatures = crate::ordered_parallel::setup_map(&signature_rows, |(name, signature)| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            serde_json::to_vec(&crate::jobj!({
                "module":json!(name.module),"function":json!(name.function),"arity":name.arity,
                "arguments":signature.arguments.iter().map(Type::to_value).collect::<Vec<_>>(),
                "result":signature.result.to_value(),
            }))
            .expect("serialize signature")
        }))
    })
    .into_iter()
    .map(|completed| match completed {
        Ok(row) => row,
        Err(payload) => std::panic::resume_unwind(payload),
    })
    .collect::<Vec<_>>();
    drop(signature_rows);
    let result = crate::jobj!({"id":row["id"].clone(),"warnings":crate::type_exports::arena_to_value(&raw.arena().terms,raw.id()),
        "public_warning_count":output.warnings.raw.len(),
        "cleanup_warning_count":output.cleanup.missing_calls.len(),
        "unknown_warning_count":output.unknown.by_module.len(),
        "module_dependencies":serde_json::to_value(output.module_dependencies.named_projection()?).map_err(|e| e.to_string())?});
    timings.mark("signatures_and_output_projection");
    crate::teardown::release(output);
    crate::teardown::release(raw);
    Ok(Projected { result, signatures })
}

use rust_dialyzer_poc::{
    bifs::registry::Registry,
    classic_persistence::CacheIdentity,
    closure_dependencies::DependencyPolicy,
    compiled_analysis::{self, AnalysisInput},
    constraints::{analyze_scc::SolverSelection, closure_order::ClosureCodeProfile},
    core::{CoreProgram, ErlangTerms},
    dependencies::ImportPlan,
    native_form_environment::RawFormTerm,
    program::TargetContracts,
    warning_render::LocationOption,
    warnings::Registry as WarningRegistry,
};
use std::{
    io::{self, BufRead},
    path::Path,
    sync::Arc,
};
fn main() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let profile = ClosureCodeProfile::parse(
        &serde_json::from_str(&std::fs::read_to_string(&args[0]).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?,
    )?;
    let bifs = Registry::load(include_str!("../../corpus/bifs.json"))?;
    let policy = DependencyPolicy::parse(include_str!("../../corpus/dependency_policy.json"))?;
    let warnings = WarningRegistry::load(include_str!("../../corpus/warnings.json"))?;
    let imported = if args.len() > 2 {
        Some(
            serde_json::from_str::<serde_json::Value>(
                &std::fs::read_to_string(&args[2]).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };
    for line in io::stdin().lock().lines() {
        let row: serde_json::Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let core = CoreProgram::parse(&row["core"].to_string())?;
        let prior = serde_json::json!({"version":4,"profile":row["core"]["profile"],"source":{"kind":"classic_plt","path":"cold-empty","sha256":"empty","files":[]},"modules":[],"summaries":[],"contracts":[],"raw_callback_groups":{"root":0,"terms":[{"kind":"nil"}]},"type_tables":{"root":0,"terms":[{"kind":"map","entries":[]}]},"exported_types":{"root":0,"terms":[{"kind":"nil"}]}});
        let prior = row.get("prior").or(imported.as_ref()).unwrap_or(&prior);
        let output = compiled_analysis::analyze(AnalysisInput {
            mode: compiled_analysis::AnalysisMode::PltBuild,
            core: &core,
            prior: &prior.to_string(),
            import: &if let Some(modules) = row
                .get("prior_plt")
                .and_then(|p| p.get("purged_modules"))
                .and_then(|p| p.as_array())
            {
                ImportPlan::before_cleanup(
                    modules
                        .iter()
                        .map(|p| p.as_str().unwrap().to_owned())
                        .collect::<Vec<_>>(),
                )
            } else if row.get("prior").is_some() {
                ImportPlan::before_cleanup(Vec::<String>::new())
            } else {
                ImportPlan::cold_targets(&core)
            },
            bifs: &bifs,
            dependency_policy: &policy,
            warning_registry: &warnings,
            legal_warnings: &warnings.default_profile(),
            closure_profile: &profile,
            contracts: TargetContracts::Use,
            solvers: SolverSelection::parse(&[]).map_err(|e| format!("{e:?}"))?,
            location: LocationOption::Line,
        })?;
        let empty = Arc::new(ErlangTerms::parse(
            &serde_json::json!({"root":0,"terms":[{"kind":"nil"}]}),
        )?);
        let empty = RawFormTerm::new(empty, rust_dialyzer_poc::core::TermId(0))?;
        let implementation_arena = ErlangTerms::parse(&row["implementation"])?;
        let implementation = RawFormTerm::new(
            Arc::new(implementation_arena),
            rust_dialyzer_poc::core::TermId(
                row["implementation"]["root"].as_u64().unwrap() as usize
            ),
        )?;
        let deps = Default::default();
        let legal_arena = ErlangTerms::parse(&row["legal"])?;
        let legal = RawFormTerm::new(
            Arc::new(legal_arena),
            rust_dialyzer_poc::core::TermId(row["legal"]["root"].as_u64().unwrap() as usize),
        )?;
        let header = rust_dialyzer_poc::incremental_persistence::IncrementalHeader {
            version: "v5.4",
            module_fingerprints: &empty,
            implementation_fingerprints: &implementation,
            repaired_dependencies: &deps,
            legal_warnings: &legal,
        };
        let count = output.warnings.raw.len();
        let completed = rust_dialyzer_poc::plt_tables::CompletedPlt::from_analysis(&core, output)
            .map_err(|e| format!("{e:?}"))?;
        rust_dialyzer_poc::incremental_persistence::write(
            completed,
            &rust_dialyzer_poc::incremental_warning_cache::CachedWarnings::merge(
                Some(&[]),
                &[],
                &rust_dialyzer_poc::incremental_warning_cache::CachedWarnings::Unavailable,
            )
            .map_err(|e| format!("{e:?}"))?,
            Path::new(row["out"].as_str().ok_or("output")?),
            header,
            rust_dialyzer_poc::external_term::Limits {
                bytes: 512 * 1024 * 1024,
                nodes: 20_000_000,
                expanded: 20_000_000,
            },
            &CacheIdentity::from_implementation_sha256(&args[1]).map_err(|e| format!("{e:?}"))?,
        )
        .map_err(|e| format!("{e:?}"))?;
        println!(
            "{}",
            serde_json::json!({"id":row["id"],"written":true,"warning_count":count})
        );
    }
    Ok(())
}

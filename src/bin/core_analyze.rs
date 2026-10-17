use rust_dialyzer_poc::provider::ExternalConstraints;
use rust_dialyzer_poc::{
    bifs::registry::Registry,
    core::CoreProgram,
    flow::{self},
    program::Program,
    schedule::Plan,
};
use serde_json::json;
fn detail(c: &flow::WarningCause) -> serde_json::Value {
    let ts = |xs: &[rust_dialyzer_poc::lattice::Type]| {
        xs.iter().map(|x| x.to_value()).collect::<Vec<_>>()
    };
    match c {
        flow::WarningCause::CallArguments {
            target,
            actual,
            expected,
            evidence,
        } => {
            json!({"category":"call","target":{"module":target.module,"name":target.name,"arity":target.arity},"actual":ts(actual),"expected":ts(expected),"evidence":evidence.as_ref().map(|e|json!({"success_arguments":ts(&e.success.arguments),"success_result":e.success.result.to_value(),"contract_arguments":e.contract.as_ref().map(|c|ts(&c.arguments)),"contract_result":e.contract.as_ref().map(|c|c.result.to_value()),"bif_arguments":e.bif.as_ref().map(|b|ts(&b.arguments)),"bif_result":e.bif.as_ref().map(|b|b.result.to_value()),"narrowed_arguments":ts(&e.narrowed_arguments),"failed_success":e.failed_success,"failed_contract":e.failed_contract,"failed_bif":e.failed_bif}))})
        }
        flow::WarningCause::ContractRange {
            target,
            actual,
            evidence,
        } => {
            json!({"category":"contract_range","target":target,"actual":ts(actual),"evidence":evidence})
        }
        flow::WarningCause::OpaqueCall {
            target,
            actual,
            evidence,
        } => {
            json!({"category":"opaque_call","target":target,"actual":ts(actual),"evidence":evidence})
        }
        flow::WarningCause::OpaqueApply {
            target,
            actual,
            evidence,
        } => {
            json!({"category":"opaque_apply","target":target.0,"actual":ts(actual),"evidence":evidence})
        }
        flow::WarningCause::RemoteTarget {
            field,
            module,
            name,
            arguments,
            actual,
            expected,
        } => {
            json!({"category":"app_call","field":field,"module_node":module.0,"name_node":name.0,"arguments":ts(arguments),"actual":actual.to_value(),"expected":expected.to_value()})
        }
        flow::WarningCause::ApplyCallArguments {
            target,
            actual,
            success,
        } => {
            json!({"category":"apply","target":target.0,"actual":ts(actual),"expected":ts(&success.arguments),"result":success.result.to_value()})
        }
        flow::WarningCause::ApplyArguments { actual, expected } => {
            json!({"category":"fun_app_args","actual":ts(actual),"expected":ts(expected)})
        }
        flow::WarningCause::ApplyNonFunction { actual, arity } => {
            json!({"category":"fun_app_no_fun","actual":actual.to_value(),"arity":arity})
        }
        flow::WarningCause::GuardImpossible {
            actual,
            operation,
            arguments,
        } => {
            json!({"category":"guard_fail","actual":actual.to_value(),"operation":operation.as_ref().map(|n|json!({"module":n.module,"name":n.name,"arity":n.arity})),"arguments":ts(arguments)})
        }
        flow::WarningCause::PatternImpossible { pattern, actual } => {
            json!({"category":"pattern_match","pattern":pattern.0,"actual":actual.to_value()})
        }
        flow::WarningCause::PatternCovered { pattern, original } => {
            json!({"category":"pattern_match_cov","pattern":pattern.0,"actual":original.to_value()})
        }
        flow::WarningCause::MapUpdate { map, key } => {
            json!({"category":"map_construction","map":map.to_value(),"key":key.to_value()})
        }
        flow::WarningCause::BinaryConstruction {
            segment,
            field,
            actual,
        } => {
            json!({"category":"bin_construction","segment":segment.0,"field":field,"actual":actual.to_value()})
        }
        flow::WarningCause::NoReturn { result } => {
            json!({"category":"no_return","result":result.to_value()})
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("need Core file")?;
    let core = CoreProgram::parse(&std::fs::read_to_string(path)?)?;
    let program = Program::build(&core)?;
    let closure_policy = rust_dialyzer_poc::closure_dependencies::DependencyPolicy::parse(
        include_str!("../../corpus/dependency_policy.json"),
    )?;
    let closures = rust_dialyzer_poc::closure_dependencies::ClosureDependencies::analyze(
        &core,
        &program,
        &closure_policy,
    )?;
    let plan = Plan::build(&program, closures)?;
    let bifs = Registry::standard()?;
    let dependencies = std::env::args()
        .nth(2)
        .map(|path| {
            std::fs::read_to_string(path)
                .map_err(|e| e.to_string())
                .and_then(|raw| {
                    rust_dialyzer_poc::dependencies::PriorPlt::parse(
                        &raw,
                        &core,
                        &rust_dialyzer_poc::dependencies::ImportPlan::cold_targets(&core),
                    )
                    .and_then(|prior| prior.into_cold_store())
                })
        })
        .transpose()?;
    let constraints = ExternalConstraints::new(dependencies.as_ref());
    let engine = flow::Engine {
        core: &core,
        program: &program,
        plan: &plan,
        bifs: &bifs,
        constraints: &constraints,
        depth: 3,
        iteration_limit: 100,
    };
    match engine.infer() {
        Ok(out) => {
            let warnings:Vec<_>=out.warnings.iter().map(|w|{let origin=program.origins[w.origin.module.0].node(w.origin.node);json!({"function":format!("{:?}",program.functions[w.function.0].identity),"named_function":match &program.functions[w.function.0].identity {rust_dialyzer_poc::program::FunctionIdentity::Named(n)=>Some(n),_=>None},"line":origin.line,"column":origin.column,"generated":origin.generated,"forced":w.forced,"provenance":{"module":w.provenance.module.0,"node":w.provenance.node},"cause":format!("{:?}",w.cause),"details":detail(&w.cause)})}).collect();
            let signatures:Vec<_>=program.functions.iter().zip(out.signatures).map(|(f,s)|json!({"identity":format!("{:?}",f.identity),"named_function":match &f.identity {rust_dialyzer_poc::program::FunctionIdentity::Named(n)=>Some(n),_=>None},"exported":f.exported,"arguments":s.arguments.iter().map(|t|t.to_value()).collect::<Vec<_>>(),"result":s.result.to_value()})).collect();
            println!(
                "{}",
                json!({"status":"inferred","warnings":warnings,"signatures":signatures,"evaluations":out.evaluations,"unresolved_functions":constraints.unresolved(),"contracts_verified":false})
            );
        }
        Err(e) => {
            println!(
                "{}",
                json!({"status":"unsupported","origin":format!("{:?}",e.origin),"error":e.message})
            );
        }
    }
    Ok(())
}

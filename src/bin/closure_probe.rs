use rust_dialyzer_poc::{
    closure_dependencies::{ClosureDependencies, DependencyPolicy},
    core::CoreProgram,
    program::Program,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = std::env::args().nth(1).ok_or("need Core file")?;
    let core = CoreProgram::parse(&std::fs::read_to_string(file)?)?;
    let program = Program::build(&core)?;
    let policy = DependencyPolicy::parse(include_str!("../../corpus/dependency_policy.json"))?;
    let dep = ClosureDependencies::analyze(&core, &program, &policy)?;
    let targets: Vec<_> = dep
        .call_targets
        .iter()
        .map(|(site, t)| serde_json::json!({"module":site.module.0,"node":site.node,"targets":t}))
        .collect();
    println!(
        "{}",
        serde_json::json!({"variables":program.binder_nodes.iter().chain(program.variable_uses.iter()).map(|(site,id)|serde_json::json!({"module":site.module.0,"node":site.node,"binding":id.0})).collect::<Vec<_>>(),"functions":program.functions.iter().map(|f|serde_json::json!({"id":f.id.0,"module":f.module.0,"node":f.node.0,"arity":f.arity,"identity":format!("{:?}",f.identity)})).collect::<Vec<_>>(),"calls":targets,"escaping":dep.escaping,"dependencies":dep.dependencies,"roots":dep.root_dependencies,"external_dependencies":dep.external_dependencies,"letrecs":dep.letrec_targets.iter().map(|(binding,target)|serde_json::json!({"binding":binding.0,"target":target.0})).collect::<Vec<_>>(),"return_arities":dep.letrec_return_arities.iter().map(|(binding,arity)|serde_json::json!({"binding":binding.0,"arity":arity})).collect::<Vec<_>>()})
    );
    Ok(())
}

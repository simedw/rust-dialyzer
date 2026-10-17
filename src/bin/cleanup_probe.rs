use rust_dialyzer_poc::{
    bifs::registry::Registry,
    binder::{Binder, BinderState},
    callgraph_cleanup::{CleanupContext, cleanup_callgraph},
    closure_dependencies::{ClosureDependencies, DependencyPolicy},
    core::{CoreProgram, ErlangTerms},
    dataflow_nodes::DataflowNodes,
    labels::{CoreLabel, Labels},
    native_form_environment::RawFormTerm,
    patterns::Patterns,
    program::{ModuleId, Program},
    term_order::compare_map_keys,
    warning_payload::NativeWarning,
    warning_policy::Policy,
    warnings::{Profile, Registry as WarningRegistry, Tag},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    io::{self, BufRead},
    sync::Arc,
};
fn matches(raw: &RawFormTerm, v: &Value) -> bool {
    let a = ErlangTerms::parse(v).unwrap();
    compare_map_keys(raw.arena(), raw.id(), &a, a.root)
        .unwrap()
        .is_eq()
}
fn rows(
    a: &ErlangTerms,
    mut id: rust_dialyzer_poc::core::TermId,
) -> Result<Vec<rust_dialyzer_poc::core::TermId>, String> {
    let mut out = vec![];
    loop {
        match &a.terms[id.0] {
            rust_dialyzer_poc::core::ErlangTerm::Nil => return Ok(out),
            rust_dialyzer_poc::core::ErlangTerm::List { items, tail } => {
                out.extend(items);
                id = *tail
            }
            _ => return Err("proper list required".into()),
        }
    }
}
#[derive(Debug)]
enum ProbeError {
    Boundary(String),
    Graph(rust_dialyzer_poc::source_callgraph::CallGraphError),
}
impl From<String> for ProbeError {
    fn from(v: String) -> Self {
        Self::Boundary(v)
    }
}
impl From<&str> for ProbeError {
    fn from(v: &str) -> Self {
        Self::Boundary(v.into())
    }
}
fn run(row: &Value) -> Result<Value, ProbeError> {
    let core = CoreProgram::parse(
        &std::fs::read_to_string(
            row["proof_core_json"]
                .as_str()
                .ok_or("proof core missing")?,
        )
        .map_err(|e| e.to_string())?,
    )?;
    let p = Program::build(&core)?;
    let labels = Labels::build(&core, CoreLabel(0))?;
    let nodes = DataflowNodes::build(&core, &labels)?;
    let deps = ClosureDependencies::analyze(
        &core,
        &p,
        &DependencyPolicy::parse(include_str!("../../corpus/dependency_policy.json"))?,
    )?;
    let mut graph = rust_dialyzer_poc::source_callgraph::SourceCallGraph::build(&nodes, &p,
        &deps,
        &Registry::standard()?,
    )
    .map_err(ProbeError::Graph)?;
    graph
        .attach_layout(core.atom_layout.clone().ok_or("layout missing")?)
        .map_err(ProbeError::Graph)?;
    let states = (0..core.modules.len())
        .map(|i| {
            BinderState::for_compiler_module(&core, ModuleId(i), true).map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut patterns = Patterns::new(&nodes);
    let binder = Binder {
        patterns: &mut patterns,
    };
    let tags: Vec<Tag> = serde_json::from_value(row["tags"].clone()).map_err(|e| e.to_string())?;
    let policy = Policy::build(
        &core,
        &WarningRegistry::load(include_str!("../../corpus/warnings.json"))?,
        &Profile::from_tags(tags),
    )
    .map_err(|e| format!("{e:?}"))?;
    let keys = |field: &str| row[field].as_array().cloned().unwrap_or_default();
    let mfa = |v: &Value| json!({"module":v[0],"function":v[1],"arity":v[2]});
    let any = json!({"kind":"atom","value":"any"});
    let nil = json!({"root":0,"terms":[{"kind":"nil"}]});
    let entry = |v: &Value| json!({"mfa":mfa(v),"contract":{"alternatives":[],"arguments":vec![any.clone();v[2].as_u64().unwrap() as usize],"forms":nil.clone(),"location":null,"flags":null}});
    let summaries=keys("plt_info").iter().map(|v|json!({"mfa":mfa(v),"arguments":vec![any.clone();v[2].as_u64().unwrap() as usize],"result":any.clone()})).collect::<Vec<_>>();
    let contracts = keys("plt_contracts").iter().map(entry).collect::<Vec<_>>();
    let callbacks = keys("plt_callbacks").iter().map(entry).collect::<Vec<_>>();
    let modules = keys("plt_info")
        .into_iter()
        .chain(keys("plt_contracts"))
        .chain(keys("plt_callbacks"))
        .map(|v| v[0].as_str().unwrap().to_owned())
        .collect::<BTreeSet<_>>();
    let artifact = json!({"version":2,"profile":{"otp":core.profile.otp,"erts":core.profile.erts,"dialyzer":core.profile.dialyzer},"source":{"kind":"classic_plt","path":"explicit-membership-fixture","sha256":"fixture","files":[]},"modules":modules,"summaries":summaries,"contracts":contracts,"callbacks":callbacks,"type_tables":nil,"exported_types":nil});
    let library = rust_dialyzer_poc::dependencies::PriorPlt::parse(
        &artifact.to_string(),
        &core,
        &rust_dialyzer_poc::dependencies::ImportPlan::before_cleanup([]),
    )?;
    let original = library.original_membership();
    let ctx = CleanupContext {
        program: &p,
        binder: &binder,
        module_states: &states,
        policy: &policy,
        original_plt: original,
    };
    let out = cleanup_callgraph(&mut graph, &ctx).map_err(ProbeError::Graph)?;
    let missing = matches(&NativeWarning::list(&out.missing_calls), &row["warnings"]);
    let unknown = ctx.unknown_functions(&out.external_calls);
    let expected = Arc::new(ErlangTerms::parse(&row["unknown"])?);
    let expected_rows = rows(&expected, expected.root)?;
    let unknown_match=expected_rows.len()==unknown.len() && expected_rows.iter().zip(&unknown).all(|(id,(m,w))|{
 let rust_dialyzer_poc::core::ErlangTerm::Tuple(xs)=&expected.terms[id.0] else{return false};
 xs.len()==2 && matches!(&expected.terms[xs[0].0],rust_dialyzer_poc::core::ErlangTerm::Atom(a) if a==m)
 && compare_map_keys(w.raw.arena(),w.raw.id(),&expected,xs[1]).is_ok_and(|o|o.is_eq())
 });
    let expected = Arc::new(ErlangTerms::parse(&row["external"])?);
    let expected_rows = rows(&expected, expected.root)?;
    let external_match=expected_rows.len()==out.external_calls.len() && expected_rows.iter().zip(&out.external_calls).all(|(id,c)|{
 let rust_dialyzer_poc::core::ErlangTerm::Tuple(xs)=&expected.terms[id.0] else{return false};if xs.len()!=2{return false}
 let rust_dialyzer_poc::core::ErlangTerm::Tuple(mfa)=&expected.terms[xs[0].0] else{return false};if mfa.len()!=3{return false}
 let target=matches!(&expected.terms[mfa[0].0],rust_dialyzer_poc::core::ErlangTerm::Atom(a) if a==&*c.target.module)
 && matches!(&expected.terms[mfa[1].0],rust_dialyzer_poc::core::ErlangTerm::Atom(a) if a==&*c.target.function)
 && matches!(&expected.terms[mfa[2].0],rust_dialyzer_poc::core::ErlangTerm::Integer(a) if a==&c.target.arity.to_string());
 let origin=RawFormTerm::new(expected.clone(),xs[1]).unwrap();
 let fake=NativeWarning::from_value("fixture",&c.origin,&rust_dialyzer_poc::lattice::NativeValue::Atom("fixture".into()));
 let rust_dialyzer_poc::core::ErlangTerm::Tuple(parts)=fake.raw.term() else{unreachable!()};
 compare_map_keys(fake.raw.arena(),parts[1],origin.arena(),origin.id()).is_ok_and(|o|o.is_eq()) && target
 });
    let dependencies =
        serde_json::to_value(out.module_dependencies).unwrap() == row["module_dependencies"];
    Ok(
        json!({"id":row["id"],"missing":missing,"unknown":unknown_match,"external":external_match,"dependencies":dependencies,"count_missing":out.missing_calls.len(),"count_external":out.external_calls.len()}),
    )
}
fn main() {
    for line in io::stdin().lock().lines() {
        let row: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let out = match run(&row) {
            Ok(v) => v,
            Err(ProbeError::Graph(
                rust_dialyzer_poc::source_callgraph::CallGraphError::Native(e),
            )) => json!({"id":row["id"],"error":e.report()}),
            Err(ProbeError::Boundary(e)) => json!({"id":row["id"],"boundary":e}),
            Err(e) => json!({"id":row["id"],"boundary":format!("{e:?}")}),
        };
        println!("{}", out)
    }
}

use crate::{
    bifs::boundary::NativeTerm,
    core::{CoreProgram, ErlangTerm, ErlangTerms, TermId},
    forms::{FormId, Forms},
    program::Program,
    resolver::{Resolver, Site},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
pub fn native(arena: &ErlangTerms, id: TermId) -> Result<NativeTerm, String> {
    rust_dialyzer_poc::type_exports::native_type_term(arena, id)
}
type EnvironmentArenas = (std::rc::Rc<ErlangTerms>, std::rc::Rc<ErlangTerms>);
thread_local! {
    static ENVIRONMENTS: std::cell::RefCell<BTreeMap<String,EnvironmentArenas>> = const { std::cell::RefCell::new(BTreeMap::new()) };
}
pub(crate) fn environment_arenas(path: &str) -> Result<EnvironmentArenas, String> {
    if let Some(value) = ENVIRONMENTS.with(|cache| cache.borrow().get(path).cloned()) {
        return Ok(value);
    }
    let metadata: Value =
        serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let value = (
        std::rc::Rc::new(ErlangTerms::parse(&metadata["type_tables"])?),
        std::rc::Rc::new(ErlangTerms::parse(&metadata["exported_types"])?),
    );
    ENVIRONMENTS.with(|cache| cache.borrow_mut().insert(path.into(), value.clone()));
    Ok(value)
}
pub fn compare(
    core: &str,
    reference: &Value,
    environment: Option<&str>,
    mode: &str,
) -> Result<Value, String> {
    let core = CoreProgram::parse(&std::fs::read_to_string(core).map_err(|e| e.to_string())?)?;
    let program = Program::build(&core)?;
    let mut forms = Forms::build(&core, &program)?;
    let mut names = core
        .modules
        .iter()
        .map(|m| m.name.clone())
        .collect::<Vec<_>>();
    if let Some(path) = environment {
        let (tables, exports) = environment_arenas(path)?;
        let targets = names.iter().cloned().collect();
        crate::environment::import(&mut forms, &mut names, &tables, &exports, &targets)?;
    }
    let mut resolver = Resolver::new(&forms, &names);
    let arena = ErlangTerms::parse(&reference["types"])?;
    let items = match &arena.terms[arena.root.0] {
        ErlangTerm::List { items, .. } => items.as_slice(),
        ErlangTerm::Nil => &[],
        _ => return Err("expected type results list".into()),
    };
    let mut matched = 0;
    let mut failures = Vec::new();
    let mut native_errors = 0;
    let mut missing = 0;
    for (i, root) in reference["roots"]
        .as_array()
        .ok_or("roots")?
        .iter()
        .enumerate()
    {
        resolver.missing.clear();
        let actual = match mode {
            "without_remote" => resolver.resolve_without_remote(FormId(i), Site::Spec),
            "check_remote_check" => resolver
                .check_remote_at(FormId(i), Site::Check)
                .map(|_| crate::lattice::Type::atom("ok")),
            "check_remote" => resolver
                .check_remote(FormId(i))
                .map(|_| crate::lattice::Type::atom("ok")),
            "check_fields" => resolver
                .check_record_fields(FormId(i), &BTreeMap::new())
                .map(|_| crate::lattice::Type::atom("ok")),
            "standard" => resolver.resolve(FormId(i), Site::Spec, BTreeMap::new()),
            _ => return Err("unsupported export mode".into()),
        }
        .map_err(|error| error.to_string());
        missing += resolver.missing.len();
        if root["status"] == "error" {
            native_errors += 1;
            if actual.is_err() {
                matched += 1;
            } else {
                failures.push(
                    json!({"id":i,"kind":root["kind"],"expected_error":root,"actual":actual}),
                );
            }
            continue;
        }
        let expected = native(&arena, items[i])?.type_value();
        if actual == expected {
            matched += 1;
        } else {
            failures.push(json!({"id":i,"kind":root["kind"],"expected":expected,"actual":actual}));
        }
    }
    Ok(
        json!({"roots":items.len(),"matched":matched,"native_errors":native_errors,"missing_events":missing,"failed":failures.len(),"failures":failures}),
    )
}

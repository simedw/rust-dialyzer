use crate::{
    compare::native,
    contract_runtime::ContractSummary,
    contracts,
    core::{CoreProgram, ErlangTerm, ErlangTerms, TermId},
    forms::Forms,
    program::Program,
    resolver::Resolver,
};
use serde_json::{Value, json};
fn list(arena: &ErlangTerms, id: TermId) -> Result<&[TermId], String> {
    match &arena.terms[id.0] {
        ErlangTerm::List { items, .. } => Ok(items),
        ErlangTerm::Nil => Ok(&[]),
        _ => Err("list".into()),
    }
}
fn expected(arena: &ErlangTerms, id: TermId) -> Result<ContractSummary, String> {
    let mut alternatives = Vec::new();
    for pair in list(arena, id)? {
        let ErlangTerm::Tuple(pair) = &arena.terms[pair.0] else {
            return Err("contract pair".into());
        };
        let signature = native(arena, pair[0])?.type_value()?;
        let mut constraints = Vec::new();
        for constraint in list(arena, pair[1])? {
            let ErlangTerm::Tuple(parts) = &arena.terms[constraint.0] else {
                return Err("subtype".into());
            };
            constraints.push((
                native(arena, parts[1])?.type_value()?,
                native(arena, parts[2])?.type_value()?,
            ));
        }
        alternatives.push((signature, constraints));
    }
    ContractSummary::from_parts(alternatives).map_err(|e| e.to_string())
}
fn types_equal(a: &ContractSummary, b: &ContractSummary) -> bool {
    a.arguments == b.arguments
        && a.alternatives.len() == b.alternatives.len()
        && a.alternatives
            .iter()
            .zip(&b.alternatives)
            .all(|(a, b)| a.signature == b.signature && a.constraints == b.constraints)
}
pub fn run(path: &str, reference: &Value, environment: Option<&str>) -> Result<Value, String> {
    let core = CoreProgram::parse(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)?;
    let program = Program::build(&core)?;
    let mut forms = Forms::build(&core, &program)?;
    let mut names = core
        .modules
        .iter()
        .map(|m| m.name.clone())
        .collect::<Vec<_>>();
    let mut prototypes = Vec::new();
    let targets = names
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    if let Some(path) = environment {
        let (tables, exports) = crate::compare::environment_arenas(path)?;
        prototypes =
            crate::environment::import(&mut forms, &mut names, &tables, &exports, &targets)?
                .record_prototypes;
    }
    let mut resolver = Resolver::new(&forms, &names);
    resolver.set_record_types(
        prototypes
            .into_iter()
            .map(|r| {
                (
                    (
                        crate::program::ModuleId(
                            names
                                .iter()
                                .position(|m| m == &r.module)
                                .expect("imported module"),
                        ),
                        r.name,
                    ),
                    r.fields,
                )
            })
            .collect(),
    )?;
    if environment.is_some() {
        let modules = names
            .iter()
            .enumerate()
            .filter(|(_, name)| targets.contains(*name))
            .map(|(i, _)| crate::program::ModuleId(i))
            .collect();
        resolver.finalize_records(&modules)?;
        resolver.check_declarations(&modules)?;
    }
    let arena = ErlangTerms::parse(&reference["types"])?;
    let values = list(&arena, arena.root)?;
    let mut passed = 0;
    let mut failures = Vec::new();
    let mut errors = 0;
    for (i, row) in reference["contracts"]
        .as_array()
        .ok_or("contracts")?
        .iter()
        .enumerate()
    {
        let mfa = &row["mfa"];
        let contract = forms
            .contracts
            .iter()
            .find(|c| {
                c.name.callable().is_some_and(|name| {
                    name.module == mfa[0].as_str().unwrap_or("")
                        && name.name == mfa[1].as_str().unwrap_or("")
                        && name.arity == mfa[2].as_u64().unwrap_or(u64::MAX) as usize
                }) && c.callback == row["callback"].as_bool().unwrap_or(false)
            })
            .ok_or("missing contract")?;
        let actual = contracts::compile(&mut resolver, contract);
        if row["result"]["status"] == "error" {
            errors += 1;
            if actual.is_err() {
                passed += 1
            } else {
                failures.push(json!({"mfa":mfa,"expected_error":row["result"],"actual":"success"}));
            }
            continue;
        }
        let expected = expected(&arena, values[i]);
        match (actual, expected) {
            (Ok(a), Ok(b)) if types_equal(&a, &b) => passed += 1,
            (a, b) => failures
                .push(json!({"mfa":mfa,"actual":format!("{a:?}"),"expected":format!("{b:?}")})),
        }
    }
    let mut record_fields = 0;
    if environment.is_some() && !reference["metadata"].is_null() {
        let metadata = ErlangTerms::parse(&reference["metadata"])?;
        let exports = ErlangTerms {
            root: TermId(0),
            terms: vec![ErlangTerm::Nil].into(),
        };
        let report = crate::environment::import(
            &mut Forms::default(),
            &mut Vec::new(),
            &metadata,
            &exports,
            &Default::default(),
        )?;
        for record in report.record_prototypes {
            let module = crate::program::ModuleId(
                names
                    .iter()
                    .position(|name| name == &record.module)
                    .ok_or("native record module")?,
            );
            record_fields += record.fields.len();
            if resolver.record_prototype(module, &record.name) != Some(record.fields.as_slice()) {
                failures.push(json!({"record":record.name,"module":record.module,"expected_fields":record.fields,"actual_fields":resolver.record_prototype(module,&record.name)}));
            }
        }
    }
    Ok(
        json!({"record_fields":record_fields,"contracts":values.len(),"passed":passed,"native_errors":errors,"failed":failures.len(),"failures":failures}),
    )
}

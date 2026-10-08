use crate::{
    compare::native,
    contracts,
    core::{CoreProgram, ErlangTerm, ErlangTerms, TermId},
    forms::Forms,
    program::Program,
    resolver::Resolver,
    validation::{self, RangeWarning, Validation},
};
use serde_json::{Value, json};
fn tuple(a: &ErlangTerms, id: TermId, len: usize) -> Result<&[TermId], String> {
    match &a.terms[id.0] {
        ErlangTerm::Tuple(ids) if ids.len() == len => Ok(ids),
        _ => Err("validation tuple".into()),
    }
}
fn atom(a: &ErlangTerms, id: TermId) -> Result<&str, String> {
    match &a.terms[id.0] {
        ErlangTerm::Atom(v) => Ok(v),
        _ => Err("validation atom".into()),
    }
}
fn list(a: &ErlangTerms, id: TermId) -> Result<&[TermId], String> {
    match &a.terms[id.0] {
        ErlangTerm::List { items, .. } => Ok(items),
        ErlangTerm::Nil => Ok(&[]),
        _ => Err("validation list".into()),
    }
}
fn validation(a: &ErlangTerms, id: TermId) -> Result<Validation, String> {
    if let ErlangTerm::Atom(v) = &a.terms[id.0]
        && v == "ok"
    {
        return Ok(Validation::Valid);
    }
    let pair = tuple(a, id, 2)?;
    match atom(a, pair[0])? {
        "error" => {
            if matches!(&a.terms[pair[1].0],ErlangTerm::Atom(v)if v=="invalid_contract") {
                return Ok(Validation::Invalid {
                    arguments: None,
                    return_invalid: None,
                });
            }
            let reason = tuple(a, pair[1], 2)?;
            Ok(match atom(a, reason[0])? {
                "overlapping_contract" => Validation::Overlapping,
                "opaque_mismatch" => Validation::OpaqueMismatch {
                    range: native(a, reason[1])?.type_value()?,
                },
                "invalid_contract" => {
                    let details = tuple(a, reason[1], 2)?;
                    let args = list(a, details[0])?
                        .iter()
                        .map(|id| match &a.terms[id.0] {
                            ErlangTerm::Integer(v) => {
                                v.parse().map_err(|_| "argument index".into())
                            }
                            _ => Err("argument index".into()),
                        })
                        .collect::<Result<Vec<usize>, String>>()?;
                    Validation::Invalid {
                        arguments: Some(args),
                        return_invalid: Some(atom(a, details[1])? == "true"),
                    }
                }
                other => return Err(format!("unknown native validation error {other}")),
            })
        }
        "range_warnings" => {
            let warnings = list(a, pair[1])?
                .iter()
                .map(|id| {
                    let error = tuple(a, *id, 2)?;
                    let reason = tuple(a, error[1], 3)?;
                    let x = native(a, reason[1])?.type_value()?;
                    let y = native(a, reason[2])?.type_value()?;
                    Ok(match atom(a, reason[0])? {
                        "extra_range" => RangeWarning::Extra {
                            extra: x,
                            success: y,
                        },
                        "missing_range" => RangeWarning::Missing {
                            missing: x,
                            contract: y,
                        },
                        other => return Err(format!("range warning {other}")),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(Validation::RangeWarnings { warnings })
        }
        other => Err(format!("validation result {other}")),
    }
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
    let mut compared = 0;
    let mut missing = 0;
    let mut failures = Vec::new();
    let mut native_errors = 0;
    for (i, row) in reference["rows"]
        .as_array()
        .ok_or("rows")?
        .iter()
        .enumerate()
    {
        if row["status"] == "missing_success" {
            missing += 1;
            continue;
        }
        compared += 1;
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
            .ok_or("contract")?;
        let parts = tuple(&arena, values[i], 2)?;
        let success = native(&arena, parts[0])?.type_value()?;
        let summary = contracts::compile(&mut resolver, contract)?;
        let actual = validation::validate(&summary, &success, mfa[0].as_str().unwrap_or(""))
            .map_err(|e| e.to_string());
        if row["result"]["status"] == "error" {
            native_errors += 1;
            failures.push(json!({"mfa":mfa,"native_error":row["result"],"actual":actual}));
            continue;
        }
        let expected = validation(&arena, parts[1])?;
        if actual.as_ref().ok() != Some(&expected) {
            failures.push(json!({"mfa":mfa,"actual":actual,"expected":expected}));
        }
    }
    Ok(
        json!({"compared":compared,"missing_success":missing,"native_errors":native_errors,"matched":compared-failures.len(),"failed":failures.len(),"failures":failures}),
    )
}

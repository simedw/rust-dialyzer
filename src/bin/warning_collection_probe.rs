use rust_dialyzer_poc::{
    core::{ErlangTerm, ErlangTerms, TermId},
    program::FunctionName,
    type_exports,
    warning_collection::{self, ContractOrigin, ContractOrigins},
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn rows(a: &ErlangTerms, mut id: TermId) -> Result<Vec<TermId>, String> {
    let mut out = Vec::new();
    loop {
        match &a.terms[id.0] {
            ErlangTerm::Nil => return Ok(out),
            ErlangTerm::List { items, tail } => {
                out.extend(items);
                id = *tail;
            }
            _ => return Err("proper list required".into()),
        }
    }
}
fn tuple(a: &ErlangTerms, id: TermId, n: usize) -> Result<&[TermId], String> {
    match &a.terms[id.0] {
        ErlangTerm::Tuple(xs) if xs.len() == n => Ok(xs),
        _ => Err("tuple arity".into()),
    }
}
fn atom(a: &ErlangTerms, id: TermId) -> Result<String, String> {
    match &a.terms[id.0] {
        ErlangTerm::Atom(s) => Ok(s.clone()),
        _ => Err("atom required".into()),
    }
}
fn run(v: &Value) -> Result<Value, String> {
    let mut a = ErlangTerms::parse(&v["arena"])?;
    let root = tuple(&a, a.root, 2)?;
    let warnings = rows(&a, root[0])?;
    let mut contracts = ContractOrigins::new();
    for entry in rows(&a, root[1])? {
        let e = tuple(&a, entry, 2)?;
        let m = tuple(&a, e[0], 3)?;
        let o = tuple(&a, e[1], 2)?;
        let ErlangTerm::Integer(arity) = &a.terms[m[2].0] else {
            return Err("arity required".into());
        };
        contracts.insert(
            FunctionName {
                module: atom(&a, m[0])?,
                name: atom(&a, m[1])?,
                arity: arity.parse().map_err(|_| "arity required")?,
            },
            ContractOrigin {
                file: o[0],
                location: o[1],
            },
        );
    }
    let output = warning_collection::postprocess(&mut a, &warnings, &contracts)
        .map_err(|e| format!("{e:?}"))?;
    let nil = TermId(a.terms.len());
    a.terms.push(ErlangTerm::Nil);
    let root = TermId(a.terms.len());
    a.terms.push(ErlangTerm::List {
        items: output,
        tail: nil,
    });
    Ok(json!({"id":v["id"],"arena":type_exports::arena_to_value(&a.terms,root)}))
}
fn main() {
    for line in io::stdin().lock().lines() {
        let v: Value = serde_json::from_str(&line.unwrap()).unwrap();
        println!(
            "{}",
            run(&v).unwrap_or_else(|error| json!({"boundary_error":error}))
        );
    }
}

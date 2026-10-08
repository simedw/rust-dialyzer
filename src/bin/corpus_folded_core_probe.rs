use rust_dialyzer_poc::folded_core::FoldedCore;
use rust_dialyzer_poc::{
    core::{CoreProgram, ErlangTerm, ErlangTerms, TermId},
    program::{ModuleId, NodeRef},
    term_order::TermOrderArena,
};
use serde_json::{Value, json};
use std::{
    cmp::Ordering,
    io::{self, BufRead},
};
fn list(arena: &ErlangTerms, id: TermId) -> Result<Vec<TermId>, String> {
    let mut out = Vec::new();
    let mut id = id;
    loop {
        match &arena.terms[id.0] {
            ErlangTerm::Nil => return Ok(out),
            ErlangTerm::List { items, tail } => {
                out.extend(items);
                id = *tail
            }
            _ => return Err("expected native outcomes list".into()),
        }
    }
}
fn run(v: &Value) -> Result<Value, String> {
    let source = CoreProgram::parse(
        &std::fs::read_to_string(v["core_json"].as_str().ok_or("core_json")?)
            .map_err(|e| e.to_string())?,
    )?;
    let folded = FoldedCore::build(&source);
    let mut count = 0;
    let mut errors = Vec::new();
    for (m, module) in v["modules"].as_array().ok_or("modules")?.iter().enumerate() {
        let actual = ErlangTerms {
            root: folded.program.modules[m].source_forms,
            terms: folded.program.modules[m].terms.clone(),
        };
        let expected = ErlangTerms::parse(&module["outcomes"])?;
        let actual_order = TermOrderArena::new(&actual)?;
        let expected_order = TermOrderArena::new(&expected)?;
        for id in list(&expected, expected.root)? {
            let ErlangTerm::Tuple(items) = &expected.terms[id.0] else {
                return Err("outcome tuple".into());
            };
            if items.len() != 3 {
                return Err("outcome arity".into());
            }
            let ErlangTerm::Integer(node) = &expected.terms[items[0].0] else {
                return Err("node integer".into());
            };
            let literal = matches!(&expected.terms[items[1].0],ErlangTerm::Atom(s)if s=="true");
            let node = node.parse::<usize>().map_err(|e| e.to_string())?;
            let value = folded.literal(NodeRef {
                module: ModuleId(m),
                node,
            })?;
            let equal = if let Some(value) = value {
                actual_order.compare_map_keys(value, &expected_order, items[2])? == Ordering::Equal
            } else {
                true
            };
            if value.is_some() != literal || !equal {
                errors.push(json!({"module":module["module"],"node":node,"literal":value.is_some(),"expected_literal":literal,"value_equal":equal}));
            }
            count += 1;
        }
    }
    Ok(json!({"id":v["id"],"modules":source.modules.len(),"cases":count,"errors":errors}))
}
fn main() {
    for line in io::stdin().lock().lines() {
        let v: Value = serde_json::from_str(&line.expect("line")).expect("request");
        println!("{}", run(&v).expect("fold proof"));
    }
}

use rust_dialyzer_poc::folded_core::FoldedCore;
use rust_dialyzer_poc::{
    core::{CoreProgram, ErlangTerms},
    core_print::CorePrinter,
    dataflow_nodes::DataflowNodes,
    labels::{CoreLabel, Labels},
    patterns::Patterns,
    program::{ModuleId, NodeRef},
    term_order::compare_map_keys,
};
use serde_json::{Value, json};
use std::{
    cmp::Ordering,
    io::{self, BufRead},
};
fn run(v: &Value) -> Result<Value, String> {
    let source = CoreProgram::parse(&v["core"].to_string())?;
    let folded = FoldedCore::build(&source);
    let labels = Labels::build(&folded.program, CoreLabel(0))?;
    let nodes = DataflowNodes::build(&folded.program, &labels)?;
    let patterns = Patterns::new(&nodes);
    let printer = CorePrinter {
        patterns: &patterns,
    };
    let mut count = 0;
    let mut errors = Vec::new();
    for (m, module) in v["modules"].as_array().ok_or("modules")?.iter().enumerate() {
        let arena = ErlangTerms {
            root: folded.program.modules[m].source_forms,
            terms: folded.program.modules[m].terms.clone(),
        };
        for r in module["results"].as_array().ok_or("results")? {
            let original = NodeRef {
                module: ModuleId(m),
                node: r["node"].as_u64().ok_or("node")? as usize,
            };
            let literal = folded.literal(original)?;
            let text = printer
                .format_cerl(folded.root(original)?)
                .map_err(|e| format!("{e:?}"))?;
            let equal = if let Some(id) = literal {
                let expected = ErlangTerms::parse(&r["value"])?;
                compare_map_keys(&arena, id, &expected, expected.root)? == Ordering::Equal
            } else {
                true
            };
            if literal.is_some() != r["literal"].as_bool().ok_or("literal")?
                || !equal
                || text != r["text"].as_str().ok_or("text")?
            {
                errors.push(json!({"module":module["module"],"node":r["node"],"literal":literal.is_some(),"value_equal":equal,"text":text,"expected_text":r["text"]}));
            }
            count += 1;
        }
    }
    Ok(json!({"id":v["id"],"cases":count,"errors":errors}))
}
fn main() {
    for line in io::stdin().lock().lines() {
        let v: Value = serde_json::from_str(&line.expect("line")).expect("request");
        println!("{}", run(&v).expect("fold proof"));
    }
}

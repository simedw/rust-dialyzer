use rust_dialyzer_poc::{
    core::{CoreExpr, CoreProgram, ErlangTerms},
    labels::{CoreLabel, Labels},
    literal_types::LiteralTypes,
    native_error::NativeTransferError,
    program::ModuleId,
    underapprox::{SafeUnderapprox, UnderapproxError},
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn run(v: &Value) -> Result<Value, String> {
    let serialized = if let Some(path) = v["core_json"].as_str() {
        std::fs::read_to_string(path).map_err(|e| e.to_string())?
    } else {
        v["program"].to_string()
    };
    let core = CoreProgram::parse(&serialized)?;
    let labels = Labels::build(&core, CoreLabel(0))?;
    let literals = core
        .modules
        .iter()
        .map(|m| {
            let roots = m
                .nodes
                .iter()
                .filter_map(|n| {
                    if let CoreExpr::Literal(id) = n.expr {
                        Some(id)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            LiteralTypes::build_roots(
                &ErlangTerms {
                    root: m.source_forms,
                    terms: m.terms.clone(),
                },
                &roots,
            )
            .map_err(|e| format!("{e:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !v["core_json"].is_null() {
        let under = SafeUnderapprox::new(&core, &labels, &literals);
        let mut results = Vec::new();
        for (module, m) in core.modules.iter().enumerate() {
            for (node, n) in m.nodes.iter().enumerate() {
                if let CoreExpr::Clause {
                    patterns, guard, ..
                } = &n.expr
                {
                    let result = match under.evaluate(ModuleId(module), patterns, *guard) {
                        Ok(t) => json!({"module":module,"node":node,"type":t.to_value()}),
                        Err(UnderapproxError::Native(e)) => {
                            json!({"module":module,"node":node,"error":NativeTransferError::from(e).report()})
                        }
                        Err(UnderapproxError::NativeCaseClauseAtom(a)) => {
                            json!({"module":module,"node":node,"error":{"kind":"error","class":"case_clause","payload":{"kind":"atom","value":a}}})
                        }
                        Err(e) => return Err(format!("{e:?}")),
                    };
                    results.push(result);
                }
            }
        }
        return Ok(json!({"id":v["id"],"clauses":results}));
    }
    let module = &core.modules[0];
    let CoreExpr::Module { definitions, .. } = &module.nodes[module.core.0].expr else {
        return Err("module expected".into());
    };
    let CoreExpr::Fun { body, .. } = &module.nodes[definitions[0].1.0].expr else {
        return Err("fun expected".into());
    };
    let CoreExpr::Case { clauses, .. } = &module.nodes[body.0].expr else {
        return Err("case expected".into());
    };
    let CoreExpr::Clause {
        patterns, guard, ..
    } = &module.nodes[clauses[0].0].expr
    else {
        return Err("clause expected".into());
    };
    let nodes = rust_dialyzer_poc::dataflow_nodes::DataflowNodes::build(&core, &labels)?;
    let mut refold = rust_dialyzer_poc::patterns::Patterns::new(&nodes);
    let under = SafeUnderapprox::new(&core, &labels, &literals);
    let result = if v["with_allocations"].as_bool() == Some(true) {
        under.evaluate_with_refold_events(ModuleId(0), patterns, *guard, |node| {
            refold
                .refold(rust_dialyzer_poc::patterns::PatternRef::Core(node))
                .map(|_| ())
                .map_err(|e| UnderapproxError::Boundary(format!("refold event: {e:?}")))
        })
    } else {
        under.evaluate(ModuleId(0), patterns, *guard)
    };
    let mut output = match result {
        Ok(t) => Ok(json!({"type":t.to_value()})),
        Err(UnderapproxError::Native(e)) => {
            Ok(json!({"error":NativeTransferError::from(e).report()}))
        }
        Err(UnderapproxError::NativeCaseClauseAtom(a)) => Ok(
            json!({"error":{"kind":"error","class":"case_clause","payload":{"kind":"atom","value":a}}}),
        ),
        Err(e) => Err(format!("{e:?}")),
    }?;
    if v["with_allocations"].as_bool() == Some(true) {
        output["refold_allocations"] = json!(refold.allocation_count());
    }
    Ok(output)
}
fn main() {
    for line in io::stdin().lock().lines() {
        let result = (|| {
            let v: Value = serde_json::from_str(&line.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            run(&v)
        })();
        println!(
            "{}",
            match result {
                Ok(v) => v,
                Err(e) => json!({"error":e}),
            }
        );
    }
}

use rust_dialyzer_poc::{
    behaviours::{CallbackEvidence, CallbackFinding, check_callback},
    call_transfer::Signature,
    contract_runtime::ContractSummary,
    lattice::{PresentationContext, Type},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{self, BufRead};
#[derive(Deserialize)]
struct Inferred {
    arguments: Vec<Type>,
    result: Type,
}
#[derive(Deserialize)]
struct Request {
    id: usize,
    callback: Type,
    inferred: Option<Inferred>,
    declared: Option<Type>,
    optional: bool,
    exported: bool,
}
fn display(findings: Vec<CallbackFinding>) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let context = PresentationContext::default();
    findings.into_iter().map(|finding|Ok(match finding {
        CallbackFinding::Missing=>json!({"kind":"missing"}),
        CallbackFinding::NotExported=>json!({"kind":"not_exported"}),
        CallbackFinding::Return{actual,expected}=>json!({"kind":"return","actual":context.type_to_string(&actual)?,"expected":context.type_to_string(&expected)?}),
        CallbackFinding::Argument{position,actual,expected}=>json!({"kind":"argument","position":position,"actual":context.type_to_string(&actual)?,"expected":context.type_to_string(&expected)?}),
        CallbackFinding::SpecReturn{extra,expected}=>json!({"kind":"spec_return","extra":context.type_to_string(&extra)?,"expected":context.type_to_string(&expected)?}),
        CallbackFinding::SpecArgument{position,actual,expected}=>json!({"kind":"spec_argument","position":position,"actual":context.type_to_string(&actual)?,"expected":context.type_to_string(&expected)?}),
    })).collect()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for line in io::stdin().lock().lines() {
        let row: Request = serde_json::from_str(&line?)?;
        let callback = ContractSummary::from_parts(vec![(row.callback, Vec::new())])?;
        let declared = row
            .declared
            .map(|signature| ContractSummary::from_parts(vec![(signature, Vec::new())]))
            .transpose()?;
        let inferred = row.inferred.map(|s| Signature {
            arguments: s.arguments,
            result: s.result,
        });
        let findings = check_callback(
            &callback,
            CallbackEvidence {
                optional: row.optional,
                exported: row.exported,
                inferred: inferred.as_ref(),
                declared: declared.as_ref(),
            },
        )?;
        println!("{}", json!({"id":row.id,"findings":display(findings)?}));
    }
    Ok(())
}

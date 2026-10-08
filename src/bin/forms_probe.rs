pub use rust_dialyzer_poc::{
    bifs, contract_runtime, contracts, core, environment, forms, lattice, program, resolver,
    validation,
};
#[path = "forms_probe/compare.rs"]
mod compare;
#[path = "forms_probe/compare_contracts.rs"]
mod compare_contracts;
#[path = "forms_probe/compare_validation.rs"]
mod compare_validation;
use compare::native;
use rust_dialyzer_poc::{
    core::CoreProgram,
    forms::{Forms, TypeForm},
    program::Program,
};
use serde_json::json;
fn main() -> Result<(), String> {
    if std::env::args().nth(1).as_deref() == Some("--validation") {
        use std::io::{self, BufRead};
        for line in io::stdin().lock().lines() {
            let r: serde_json::Value = serde_json::from_str(&line.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            println!(
                "{}",
                compare_validation::run(
                    r["core"].as_str().ok_or("core")?,
                    &r["reference"],
                    r["environment"].as_str()
                )?
            );
        }
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--import-environment") {
        use rust_dialyzer_poc::{core::ErlangTerms, environment};
        use std::collections::BTreeSet;
        let argv = std::env::args().collect::<Vec<_>>();
        let core =
            CoreProgram::parse(&std::fs::read_to_string(&argv[2]).map_err(|e| e.to_string())?)?;
        let program = Program::build(&core)?;
        let mut forms = Forms::build(&core, &program)?;
        let mut names = core
            .modules
            .iter()
            .map(|m| m.name.clone())
            .collect::<Vec<_>>();
        let targets = names.iter().cloned().collect::<BTreeSet<_>>();
        let metadata: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&argv[3]).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let report = environment::import(
            &mut forms,
            &mut names,
            &ErlangTerms::parse(&metadata["type_tables"])?,
            &ErlangTerms::parse(&metadata["exported_types"])?,
            &targets,
        )?;
        println!("{}", json!(report));
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--local-contracts") {
        use std::io::{self, BufRead};
        for line in io::stdin().lock().lines() {
            let r: serde_json::Value = serde_json::from_str(&line.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            println!(
                "{}",
                compare_contracts::run(
                    r["core"].as_str().ok_or("core")?,
                    &r["reference"],
                    r["environment"].as_str()
                )?
            );
        }
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--runtime-arena") {
        use rust_dialyzer_poc::{
            bifs::boundary::NativeTerm,
            contract_runtime::ContractSummary,
            core::{ErlangTerm, ErlangTerms},
        };
        let argv = std::env::args().collect::<Vec<_>>();
        let source: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&argv[2]).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let requests = std::fs::read_to_string(&argv[3])
            .map_err(|e| e.to_string())?
            .lines()
            .map(|l| serde_json::from_str::<serde_json::Value>(l).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let arena = ErlangTerms::parse(
            &serde_json::from_str(&std::fs::read_to_string(&argv[4]).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?,
        )?;
        let ErlangTerm::List { items, .. } = &arena.terms[arena.root.0] else {
            return Err("result arena list".into());
        };
        let decode = |v: &serde_json::Value| -> Result<rust_dialyzer_poc::lattice::Type, String> {
            serde_json::from_value::<NativeTerm>(v.clone())
                .map_err(|e| e.to_string())?
                .type_value()
        };
        let mut compiled = Vec::new();
        let mut domains = Vec::new();
        for item in source["contracts"].as_array().ok_or("contracts")? {
            let c = &item["contract"];
            let alternatives = c["alternatives"]
                .as_array()
                .ok_or("alternatives")?
                .iter()
                .map(|a| {
                    Ok((
                        decode(&a["signature"])?,
                        a["constraints"]
                            .as_array()
                            .ok_or("constraints")?
                            .iter()
                            .map(|p| Ok((decode(&p["left"])?, decode(&p["right"])?)))
                            .collect::<Result<Vec<_>, String>>()?,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            compiled.push(ContractSummary::from_parts(alternatives).map_err(|e| e.to_string())?);
            domains.push(
                c["arguments"]
                    .as_array()
                    .ok_or("arguments")?
                    .iter()
                    .map(decode)
                    .collect::<Result<Vec<_>, String>>()?,
            );
        }
        for (request, item) in requests.iter().zip(items) {
            let ErlangTerm::Tuple(pair) = &arena.terms[item.0] else {
                return Err("result pair".into());
            };
            let args = match native(&arena, pair[0])? {
                NativeTerm::List { items, .. } => items,
                NativeTerm::Nil => vec![],
                _ => return Err("actual args list".into()),
            };
            let arguments = args
                .iter()
                .map(NativeTerm::type_value)
                .collect::<Result<Vec<_>, _>>()?;
            let expected = native(&arena, pair[1])?.type_value()?;
            let i = request["contract_index"].as_u64().ok_or("index")? as usize;
            let result = compiled[i]
                .return_for(&arguments)
                .map_err(|e| e.to_string())?;
            println!(
                "{}",
                json!({"id":request["id"],"parity":result==expected,"domain_parity":compiled[i].arguments==domains[i],"result":result,"expected":expected})
            );
        }
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--contracts") {
        use rust_dialyzer_poc::{bifs::boundary::NativeTerm, contract_runtime::ContractSummary};
        use std::io::{self, BufRead};
        for line in io::stdin().lock().lines() {
            let r: serde_json::Value = serde_json::from_str(&line.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            let decode =
                |v: &serde_json::Value| -> Result<rust_dialyzer_poc::lattice::Type, String> {
                    serde_json::from_value::<NativeTerm>(v.clone())
                        .map_err(|e| e.to_string())?
                        .type_value()
                };
            let alternatives = r["contract"]["alternatives"]
                .as_array()
                .ok_or("alternatives")?
                .iter()
                .map(|a| {
                    Ok((
                        decode(&a["signature"])?,
                        a["constraints"]
                            .as_array()
                            .ok_or("constraints")?
                            .iter()
                            .map(|p| Ok((decode(&p["left"])?, decode(&p["right"])?)))
                            .collect::<Result<Vec<_>, String>>()?,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let summary = ContractSummary::from_parts(alternatives).map_err(|e| e.to_string())?;
            let arguments = r["actual"]
                .as_array()
                .ok_or("actual")?
                .iter()
                .map(decode)
                .collect::<Result<Vec<_>, String>>()?;
            let expected = decode(&r["expected"])?;
            let domains = r["contract"]["arguments"]
                .as_array()
                .ok_or("arguments")?
                .iter()
                .map(decode)
                .collect::<Result<Vec<_>, String>>()?;
            let result = summary.return_for(&arguments).map_err(|e| e.to_string())?;
            println!(
                "{}",
                json!({"id":r["id"],"parity":result==expected,"domain_parity":summary.arguments==domains,"result":result,"expected":expected})
            );
        }
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--compare") {
        use std::io::{self, BufRead};
        for line in io::stdin().lock().lines() {
            let request: serde_json::Value =
                serde_json::from_str(&line.map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            println!(
                "{}",
                compare::compare(
                    request["core"].as_str().ok_or("core")?,
                    &request["reference"],
                    request["environment"].as_str(),
                    request["mode"].as_str().unwrap_or("standard")
                )?
            );
        }
        return Ok(());
    }
    let path = std::env::args().nth(1).ok_or("core path")?;
    let core = CoreProgram::parse(&std::fs::read_to_string(&path).map_err(|e| e.to_string())?)?;
    let program = Program::build(&core)?;
    let forms = Forms::build(&core, &program)?;
    let nodes=forms.nodes.iter().enumerate().map(|(id,form)| json!({"id":id,"module":form.module.0,"raw":form.raw.0,"kind":match form.value{TypeForm::DeferredRaw=>"deferred_raw",TypeForm::UnrecognizedBuiltin{..}=>"unrecognized_builtin",TypeForm::Variable(_)=>"variable",TypeForm::Atom(_)=>"atom",TypeForm::Integer(_)=>"integer",TypeForm::Operator{..}=>"operator",TypeForm::Annotated{..}=>"annotated",TypeForm::Parenthesized(_)=>"parenthesized",TypeForm::Builtin{..}=>"builtin",TypeForm::Named{..}=>"named",TypeForm::BoundedFunction{..}=>"bounded",TypeForm::Constraint{..}=>"constraint"}})).collect::<Vec<_>>();
    println!(
        "{}",
        json!({"core":path,"nodes":nodes,"inspection":forms.inspect()})
    );
    Ok(())
}

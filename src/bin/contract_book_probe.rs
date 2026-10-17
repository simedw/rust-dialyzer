use rust_dialyzer_poc::{
    bifs::registry::{Mfa, Registry},
    contract_book::ContractBook,
    contract_runtime::ContractSummary,
    dependencies::NativeAlternative,
    lattice::Type,
};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    io::{self, BufRead},
};
#[derive(Deserialize)]
struct Contract {
    alternatives: Vec<NativeAlternative>,
}
#[derive(Deserialize)]
struct Entry {
    mfa: Mfa,
    contract: Contract,
}
#[derive(Deserialize)]
struct Source {
    contracts: Vec<Entry>,
}
#[derive(Deserialize)]
struct Request {
    contract_index: usize,
    successes: Vec<Type>,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("contract source expected")?;
    let source: Source = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let registry = Registry::standard()?;
    let compiled = source
        .contracts
        .iter()
        .map(|entry| {
            let parts = entry
                .contract
                .alternatives
                .iter()
                .map(|alternative| {
                    Ok((
                        alternative.signature.type_value()?,
                        alternative
                            .constraints
                            .iter()
                            .map(|constraint| {
                                Ok((
                                    constraint.left.type_value()?,
                                    constraint.right.type_value()?,
                                ))
                            })
                            .collect::<Result<Vec<_>, String>>()?,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            ContractSummary::from_parts(parts).map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, String>>()?;
    for line in io::stdin().lock().lines() {
        let request: Request = serde_json::from_str(&line?)?;
        let entry = source
            .contracts
            .get(request.contract_index)
            .ok_or("contract index outside source")?;
        let mut book = ContractBook::new(BTreeMap::from([(
            entry.mfa.clone(),
            compiled[request.contract_index].clone(),
        )]));
        let mut added = Vec::new();
        for success in request.successes {
            added.push(
                !book
                    .activate(&BTreeMap::from([(entry.mfa.clone(), success)]), &registry)?
                    .is_empty(),
            );
        }
        println!(
            "{}",
            serde_json::json!({"added":added,"active":book.active(&entry.mfa).is_some()})
        );
    }
    Ok(())
}

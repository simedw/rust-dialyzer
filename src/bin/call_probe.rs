use rust_dialyzer_poc::{
    bifs::{
        boundary::NativeTerm,
        registry::{Mfa, Registry, TransferError},
    },
    call_transfer::*,
    contract_runtime::{ContractError, ContractSummary},
    core::{ErlangTerm, ErlangTerms, TermId},
    lattice::Type,
    native_error::NativeTransferError,
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn tuple(a: &ErlangTerms, id: TermId, n: usize) -> Result<&[TermId], String> {
    match &a.terms[id.0] {
        ErlangTerm::Tuple(xs) if xs.len() == n => Ok(xs),
        _ => Err(format!("expected tuple{n}")),
    }
}
fn atom(a: &ErlangTerms, id: TermId) -> Result<&str, String> {
    match &a.terms[id.0] {
        ErlangTerm::Atom(v) => Ok(v),
        _ => Err("expected atom".into()),
    }
}
fn integer(a: &ErlangTerms, id: TermId) -> Result<usize, String> {
    match &a.terms[id.0] {
        ErlangTerm::Integer(v) => v.parse().map_err(|_| "expected index".into()),
        _ => Err("expected integer".into()),
    }
}
fn list(a: &ErlangTerms, mut id: TermId) -> Result<Vec<TermId>, String> {
    let mut values = Vec::new();
    loop {
        match &a.terms[id.0] {
            ErlangTerm::Nil => return Ok(values),
            ErlangTerm::List { items, tail } => {
                values.extend(items);
                id = *tail;
            }
            _ => return Err("expected proper list".into()),
        }
    }
}
fn ty(a: &ErlangTerms, id: TermId) -> Result<Type, String> {
    NativeTerm::from_arena(a, id)?.type_value()
}
fn types(a: &ErlangTerms, id: TermId) -> Result<Vec<Type>, String> {
    list(a, id)?.into_iter().map(|id| ty(a, id)).collect()
}
fn boolean(a: &ErlangTerms, id: TermId) -> Result<bool, String> {
    match atom(a, id)? {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err("boolean".into()),
    }
}
fn expected(row: &Value) -> Result<CallTransfer, String> {
    let a = ErlangTerms::parse(&row["outcome"]["value"])?;
    let v = tuple(&a, a.root, 20)?;
    let diagnostic = match &a.terms[v[17].0] {
        ErlangTerm::Atom(name) if name == "none" => Diagnostic::None,
        ErlangTerm::Tuple(xs) => match atom(&a, xs[0])? {
            "contract_range" => Diagnostic::ContractRange {
                without_contract: ty(&a, xs[1])?,
            },
            "failing_call" => Diagnostic::FailingCall {
                failed_success: boolean(&a, xs[1])?,
                failed_contract: boolean(&a, xs[2])?,
                failed_bif: boolean(&a, xs[3])?,
                reason: match atom(&a, xs[4])? {
                    "only_sig" => FailureReason::OnlySignature,
                    "only_contract" => FailureReason::OnlyContract,
                    "both" => FailureReason::Both,
                    _ => return Err("failure reason".into()),
                },
                argument_indices: list(&a, xs[5])?
                    .into_iter()
                    .map(|id| integer(&a, id))
                    .collect::<Result<_, _>>()?,
                expected_signature: ty(&a, xs[6])?,
            },
            _ => return Err("diagnostic".into()),
        },
        _ => return Err("diagnostic".into()),
    };
    let opacity = tuple(&a, v[18], 2)?;
    let reason = match atom(&a, opacity[0])? {
        "none" => None,
        "expected_opaque" => Some(OpacityReason::ExpectedOpaque),
        "expected_transparent" => Some(OpacityReason::ExpectedTransparent),
        _ => return Err("opacity".into()),
    };
    let conflicts = list(&a, opacity[1])?
        .into_iter()
        .map(|id| {
            let xs = tuple(&a, id, 3)?;
            Ok(OpaqueArgument {
                index: integer(&a, xs[0])?,
                conflicting_type: ty(&a, xs[1])?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(CallTransfer {
        arguments: ArgumentEvidence {
            success_domains: types(&a, v[0])?,
            contract_domains: types(&a, v[1])?,
            bif_domains: types(&a, v[2])?,
            success: types(&a, v[3])?,
            contract: types(&a, v[4])?,
            bif: types(&a, v[5])?,
            conjunction: types(&a, v[6])?,
        },
        ranges: RangeEvidence {
            success: ty(&a, v[7])?,
            contract: ty(&a, v[8])?,
            bif: ty(&a, v[9])?,
            without_contract: ty(&a, v[10])?,
            without_local: ty(&a, v[11])?,
            local: ty(&a, v[12])?,
            total: ty(&a, v[13])?,
        },
        failed_conjunction: boolean(&a, v[14])?,
        unconditional_success_failure: boolean(&a, v[15])?,
        unconditional_bif_failure: boolean(&a, v[16])?,
        diagnostic,
        opacity: OpacityEvidence {
            reason,
            arguments: conflicts,
            expected: types(&a, v[19])?,
        },
    })
}
fn input(row: &Value, registry: &Registry) -> Result<Result<CallTransfer, CallError>, String> {
    let a = ErlangTerms::parse(&row["input"])?;
    let v = tuple(&a, a.root, 7)?;
    let mode = atom(&a, v[0])?;
    let fun = tuple(&a, v[1], 3)?;
    let mfa = Mfa {
        module: atom(&a, fun[0])?.into(),
        function: atom(&a, fun[1])?.into(),
        arity: integer(&a, fun[2])?,
    };
    let success = if matches!(&a.terms[v[2].0],ErlangTerm::Atom(s)if s=="none") {
        None
    } else {
        let sig = tuple(&a, v[2], 2)?;
        let parts = tuple(&a, sig[1], 2)?;
        Some(Signature {
            arguments: types(&a, parts[1])?,
            result: ty(&a, parts[0])?,
        })
    };
    let contract = if matches!(&a.terms[v[3].0],ErlangTerm::Atom(s)if s=="none") {
        None
    } else {
        let value = tuple(&a, v[3], 2)?;
        let contract = tuple(&a, value[1], 4)?;
        let pairs = list(&a, contract[1])?
            .into_iter()
            .map(|id| {
                let pair = tuple(&a, id, 2)?;
                let constraints = list(&a, pair[1])?
                    .into_iter()
                    .map(|id| {
                        let values = tuple(&a, id, 3)?;
                        Ok((ty(&a, values[1])?, ty(&a, values[2])?))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Ok((ty(&a, pair[0])?, constraints))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let summary = ContractSummary::from_parts(pairs).map_err(|e| e.to_string())?;
        if summary.arguments != types(&a, contract[2])? {
            return Err("contract general domain mismatch".into());
        }
        Some(summary)
    };
    let local = ty(&a, v[4])?;
    let actual = types(&a, v[5])?;
    let module = atom(&a, v[6])?;
    let bif_domains = if mode == "remote" && registry.is_known(&mfa) {
        registry
            .admissible_domains(&mfa)
            .map_err(|e| e.to_string())?
    } else {
        None
    };
    let bif_return = |args: &[Type]| {
        registry
            .transfer(&mfa, args)
            .map(|o| o.result)
            .map_err(|e| match e {
                TransferError::NativeException(error) => CallError::NativeRange {
                    contribution: Contribution::Bif,
                    error,
                },
                other => CallError::Provider {
                    contribution: Contribution::Bif,
                    message: other.to_string(),
                },
            })
    };
    let contract_return = |args: &[Type]| {
        contract
            .as_ref()
            .ok_or_else(|| CallError::Provider {
                contribution: Contribution::Contract,
                message: "missing contract".into(),
            })?
            .return_for(args)
            .map_err(|e| match e {
                ContractError::Lattice(error) => CallError::Lattice(error),
                other => CallError::Provider {
                    contribution: Contribution::Contract,
                    message: other.to_string(),
                },
            })
    };
    Ok(evaluate(CallInput {
        actual: &actual,
        success: success.as_ref(),
        contract: contract.as_ref().map(|c| DependentSummary {
            arguments: &c.arguments,
            range: &contract_return,
        }),
        bif: bif_domains.as_ref().map(|domains| DependentSummary {
            arguments: domains,
            range: &bif_return,
        }),
        local_return: &local,
        module,
    }))
}
fn native_error(row: &Value) -> Result<Value, String> {
    let a = ErlangTerms::parse(&row["outcome"]["reason"])?;
    let error = match &a.terms[a.root.0] {
        ErlangTerm::Atom(name) if name == "function_clause" => NativeTransferError::FunctionClause,
        ErlangTerm::Atom(name) if name == "badarg" => NativeTransferError::Badarg,
        ErlangTerm::Tuple(xs) if xs.len() == 2 => match atom(&a, xs[0])? {
            "case_clause" => {
                if matches!(&a.terms[xs[1].0], ErlangTerm::Nil) {
                    NativeTransferError::CaseClauseNil
                } else {
                    NativeTransferError::CaseClauseType(Box::new(ty(&a, xs[1])?))
                }
            }
            "badmatch" => {
                if let Ok(value) = boolean(&a, xs[1]) {
                    NativeTransferError::BadMatchBoolean(value)
                } else {
                    NativeTransferError::BadMatchType(Box::new(ty(&a, xs[1])?))
                }
            }
            _ => return Err("unsupported native error tuple".into()),
        },
        _ => return Err("unsupported native error".into()),
    };
    Ok(error.report())
}
fn accumulate(row: &Value, registry: &Registry) -> Result<Value, String> {
    let actual_arena = ErlangTerms::parse(&row["actual"])?;
    let actual = types(&actual_arena, actual_arena.root)?;
    let mut accumulator = AlternativeAccumulator::<String>::new(actual.len());
    for child in row["candidates"].as_array().ok_or("candidates")? {
        if child["external"] == true {
            accumulator.external(&actual).map_err(|e| e.to_string())?;
        } else {
            let transfer = input(child, registry)?.map_err(|e| e.to_string())?;
            let warnings = child["warnings"]
                .as_array()
                .ok_or("warnings")?
                .iter()
                .map(|v| v.as_str().map(String::from).ok_or("warning token".into()))
                .collect::<Result<Vec<_>, String>>()?;
            accumulator
                .add(&transfer, warnings)
                .map_err(|e| e.to_string())?;
        }
    }
    let a = ErlangTerms::parse(&row["expected"])?;
    let e = tuple(&a, a.root, 5)?;
    let expected_args = types(&a, e[0])?;
    let expected_return = ty(&a, e[1])?;
    let external = boolean(&a, e[2])?;
    let count = match atom(&a, e[3])? {
        "none" => CandidateMultiplicity::None,
        "one" => CandidateMultiplicity::One,
        "many" => CandidateMultiplicity::Many,
        _ => return Err("multiplicity".into()),
    };
    let warnings = list(&a, e[4])?
        .into_iter()
        .map(|id| atom(&a, id).map(String::from))
        .collect::<Result<Vec<_>, _>>()?;
    let matched = accumulator.arguments == expected_args
        && accumulator.result == expected_return
        && accumulator.had_external == external
        && accumulator.multiplicity() == count
        && accumulator.warnings == warnings;
    Ok(
        json!({"id":row["id"],"matched":matched,"actual_arguments":accumulator.arguments,"expected_arguments":expected_args,"actual_return":accumulator.result,"expected_return":expected_return,"actual_external":accumulator.had_external,"expected_external":external,"actual_multiplicity":accumulator.multiplicity(),"expected_multiplicity":count,"actual_warnings":accumulator.warnings,"expected_warnings":warnings}),
    )
}
fn run(row: &Value, registry: &Registry) -> Result<Value, String> {
    if row["operation"] == "accumulate" {
        return accumulate(row, registry);
    }
    let actual = input(row, registry)?;
    if row["outcome"]["status"] == "ok" {
        let expected = expected(row)?;
        Ok(
            json!({"id":row["id"],"matched":actual.as_ref().ok()==Some(&expected),"actual":actual.as_ref().ok(),"expected":expected,"error":actual.err().map(|e|e.to_string())}),
        )
    } else {
        let expected = native_error(row)?;
        let actual = match actual {
            Err(CallError::Lattice(e)) => NativeTransferError::from(e).report(),
            Err(CallError::NativeRange { error, .. }) => error.report(),
            other => json!({"unexpected":format!("{other:?}")}),
        };
        Ok(
            json!({"id":row["id"],"matched":actual==expected,"actual_error":actual,"expected_error":expected}),
        )
    }
}
fn main() -> Result<(), String> {
    let registry = Registry::standard()?;
    for line in io::stdin().lock().lines() {
        let row: Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        println!("{}", run(&row, &registry)?);
    }
    Ok(())
}

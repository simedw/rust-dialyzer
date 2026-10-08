use crate::{
    call_transfer::Signature,
    contract_runtime::{ContractError, ContractSummary},
    lattice::{LatticeError, Type},
};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CallbackFinding {
    Missing,
    NotExported,
    Return {
        actual: Type,
        expected: Type,
    },
    Argument {
        position: usize,
        actual: Type,
        expected: Type,
    },
    SpecReturn {
        extra: Type,
        expected: Type,
    },
    SpecArgument {
        position: usize,
        actual: Type,
        expected: Type,
    },
}

pub struct CallbackEvidence<'a> {
    pub optional: bool,
    pub exported: bool,
    pub inferred: Option<&'a Signature>,
    pub declared: Option<&'a ContractSummary>,
}

fn arguments(
    actual: &[Type],
    expected: &[Type],
    declared: bool,
    findings: &mut Vec<CallbackFinding>,
) -> Result<(), LatticeError> {
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        if actual.meet(expected)?.is_none() {
            findings.push(if declared {
                CallbackFinding::SpecArgument {
                    position: index + 1,
                    actual: actual.clone(),
                    expected: expected.clone(),
                }
            } else {
                CallbackFinding::Argument {
                    position: index + 1,
                    actual: actual.clone(),
                    expected: expected.clone(),
                }
            });
        }
    }
    Ok(())
}

pub fn check_callback(
    callback: &ContractSummary,
    evidence: CallbackEvidence<'_>,
) -> Result<Vec<CallbackFinding>, ContractError> {
    let Some(inferred) = evidence.inferred else {
        return Ok(if evidence.optional {
            Vec::new()
        } else {
            vec![CallbackFinding::Missing]
        });
    };
    if !evidence.exported {
        return Ok(if evidence.optional {
            Vec::new()
        } else {
            vec![CallbackFinding::NotExported]
        });
    }
    if inferred.arguments.len() != callback.arguments.len() {
        return Err(ContractError::ArityMismatch {
            expected: callback.arguments.len(),
            actual: inferred.arguments.len(),
        });
    }
    let result = callback.return_for(&callback.arguments)?;
    let mut findings = Vec::new();
    if !inferred.result.is_none() && inferred.result.meet(&result)?.is_none() {
        findings.push(CallbackFinding::Return {
            actual: inferred.result.clone(),
            expected: result.clone(),
        });
    }
    arguments(
        &inferred.arguments,
        &callback.arguments,
        false,
        &mut findings,
    )?;
    if let Some(declared) = evidence.declared {
        if declared.arguments.len() != callback.arguments.len() {
            return Err(ContractError::ArityMismatch {
                expected: callback.arguments.len(),
                actual: declared.arguments.len(),
            });
        }
        let declared_result = declared
            .return_for(&declared.arguments)?
            .substitute(&BTreeMap::new())?;
        let declared_arguments = declared
            .arguments
            .iter()
            .map(|arg| arg.substitute(&BTreeMap::new()))
            .collect::<Result<Vec<_>, _>>()?;
        if !declared_result.is_none() && declared_result.meet(&result)?.is_none() {
            findings.push(CallbackFinding::SpecReturn {
                extra: declared_result.subtract(&result)?,
                expected: result,
            });
        }
        arguments(
            &declared_arguments,
            &callback.arguments,
            true,
            &mut findings,
        )?;
    }
    findings.reverse();
    Ok(findings)
}

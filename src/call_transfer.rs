use crate::{
    lattice::{LatticeError, OpacityConflict, Type},
    native_error::NativeTransferError,
};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Signature {
    pub arguments: Vec<Type>,
    pub result: Type,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Contribution {
    Success,
    Contract,
    Bif,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallError {
    Lattice(LatticeError),
    Arity {
        contribution: Contribution,
        expected: usize,
        actual: usize,
    },
    NativeRange {
        contribution: Contribution,
        error: NativeTransferError,
    },
    Provider {
        contribution: Contribution,
        message: String,
    },
}
impl From<LatticeError> for CallError {
    fn from(e: LatticeError) -> Self {
        Self::Lattice(e)
    }
}
impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for CallError {}
type Result<T> = std::result::Result<T, CallError>;
pub type RangeProvider<'a> = dyn Fn(&[Type]) -> Result<Type> + 'a;
pub struct DependentSummary<'a> {
    pub arguments: &'a [Type],
    pub range: &'a RangeProvider<'a>,
}
pub struct CallInput<'a> {
    pub actual: &'a [Type],
    pub success: Option<&'a Signature>,
    pub contract: Option<DependentSummary<'a>>,
    pub bif: Option<DependentSummary<'a>>,
    pub local_return: &'a Type,
    pub module: &'a str,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureReason {
    OnlySignature,
    OnlyContract,
    Both,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Diagnostic {
    None,
    ContractRange {
        without_contract: Type,
    },
    FailingCall {
        failed_success: bool,
        failed_contract: bool,
        failed_bif: bool,
        reason: FailureReason,
        argument_indices: Vec<usize>,
        expected_signature: Type,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpacityReason {
    ExpectedOpaque,
    ExpectedTransparent,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OpaqueArgument {
    pub index: usize,
    pub conflicting_type: Type,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OpacityEvidence {
    pub reason: Option<OpacityReason>,
    pub arguments: Vec<OpaqueArgument>,
    pub expected: Vec<Type>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ArgumentEvidence {
    pub success_domains: Vec<Type>,
    pub contract_domains: Vec<Type>,
    pub bif_domains: Vec<Type>,
    pub success: Vec<Type>,
    pub contract: Vec<Type>,
    pub bif: Vec<Type>,
    pub conjunction: Vec<Type>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RangeEvidence {
    pub success: Type,
    pub contract: Type,
    pub bif: Type,
    pub without_contract: Type,
    pub without_local: Type,
    pub local: Type,
    pub total: Type,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CallTransfer {
    pub arguments: ArgumentEvidence,
    pub ranges: RangeEvidence,
    pub failed_conjunction: bool,
    pub unconditional_success_failure: bool,
    pub unconditional_bif_failure: bool,
    pub diagnostic: Diagnostic,
    pub opacity: OpacityEvidence,
}
fn meet_lists(a: &[Type], b: &[Type], source: Contribution) -> Result<Vec<Type>> {
    if a.len() != b.len() {
        return Err(CallError::Arity {
            contribution: source,
            expected: a.len(),
            actual: b.len(),
        });
    }
    a.iter()
        .zip(b)
        .map(|(a, b)| a.meet(b).map_err(Into::into))
        .collect()
}
fn any_none(values: &[Type]) -> bool {
    values.iter().any(Type::is_none)
}
fn range(provider: Option<&DependentSummary<'_>>, args: &[Type]) -> Result<Type> {
    match provider {
        Some(p) => (p.range)(args),
        None => Ok(Type::any()),
    }
}
pub fn evaluate(input: CallInput<'_>) -> Result<CallTransfer> {
    let any_args = vec![Type::any(); input.actual.len()];
    let sig_args = input
        .success
        .map(|s| s.arguments.as_slice())
        .unwrap_or(&any_args);
    let sig_range = input
        .success
        .map(|s| s.result.clone())
        .unwrap_or_else(Type::any);
    let cargs = input
        .contract
        .as_ref()
        .map(|s| s.arguments)
        .unwrap_or(&any_args);
    let bargs = input.bif.as_ref().map(|s| s.arguments).unwrap_or(&any_args);
    let new_sig = meet_lists(sig_args, input.actual, Contribution::Success)?;
    let new_contract = meet_lists(cargs, input.actual, Contribution::Contract)?;
    let new_bif = meet_lists(bargs, input.actual, Contribution::Bif)?;
    let sig_contract = meet_lists(&new_sig, &new_contract, Contribution::Contract)?;
    let conjunction = meet_lists(&sig_contract, &new_bif, Contribution::Bif)?;
    let bif_ret = range(input.bif.as_ref(), &conjunction)?;
    let contract_ret = range(input.contract.as_ref(), &conjunction)?;
    let without_contract = sig_range.meet(&bif_ret)?;
    let without_local = contract_ret.meet(&without_contract)?;
    let failed_conjunction = without_local.is_none() || any_none(&conjunction);
    let bif_general_range = range(input.bif.as_ref(), bargs)?;
    let unconditional_bif_failure = bif_general_range.is_none();
    let unconditional_success_failure = sig_range.is_none();
    let expected_opacity = meet_lists(cargs, sig_args, Contribution::Contract)?;
    let mut opacity = OpacityEvidence {
        reason: None,
        arguments: Vec::new(),
        expected: expected_opacity.clone(),
    };
    for (index, (given, expected)) in input.actual.iter().zip(&expected_opacity).enumerate() {
        let conflict = crate::binder::opacity_conflict(given, expected, input.module)?;
        match conflict {
            OpacityConflict::None => {}
            OpacityConflict::ExpectedOpaque => {
                opacity.reason = Some(OpacityReason::ExpectedOpaque);
                opacity.arguments.push(OpaqueArgument {
                    index: index + 1,
                    conflicting_type: expected.clone(),
                });
            }
            OpacityConflict::ExpectedTransparent => {
                if opacity.reason.is_none() {
                    opacity.reason = Some(OpacityReason::ExpectedTransparent);
                }
                opacity.arguments.push(OpaqueArgument {
                    index: index + 1,
                    conflicting_type: given.clone(),
                });
            }
        }
    }
    let diagnostic = if failed_conjunction
        && !(unconditional_bif_failure || unconditional_success_failure)
    {
        if without_local.is_none() && !without_contract.is_none() && !any_none(&conjunction) {
            Diagnostic::ContractRange {
                without_contract: without_contract.clone(),
            }
        } else {
            let failed_success = any_none(&new_sig);
            let failed_contract =
                range(input.contract.as_ref(), &new_contract)?.is_none() || any_none(&new_contract);
            let failed_bif = range(input.bif.as_ref(), &new_bif)?.is_none() || any_none(&new_bif);
            let reason = if (failed_success || failed_bif) && !failed_contract {
                FailureReason::OnlySignature
            } else if failed_contract && !(failed_success || failed_bif) {
                FailureReason::OnlyContract
            } else {
                FailureReason::Both
            };
            let expected_signature = Type::function(Some(sig_args.to_vec()), sig_range.clone())
                .meet(&Type::function(Some(bargs.to_vec()), bif_general_range))?;
            Diagnostic::FailingCall {
                failed_success,
                failed_contract,
                failed_bif,
                reason,
                argument_indices: conjunction
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.is_none())
                    .map(|(i, _)| i + 1)
                    .collect(),
                expected_signature,
            }
        }
    } else {
        Diagnostic::None
    };
    let total = if input.local_return.is_none() && without_local.is_unit() {
        without_local.clone()
    } else {
        without_local.meet(input.local_return)?
    };
    Ok(CallTransfer {
        arguments: ArgumentEvidence {
            success_domains: sig_args.to_vec(),
            contract_domains: cargs.to_vec(),
            bif_domains: bargs.to_vec(),
            success: new_sig,
            contract: new_contract,
            bif: new_bif,
            conjunction,
        },
        ranges: RangeEvidence {
            success: sig_range,
            contract: contract_ret,
            bif: bif_ret,
            without_contract,
            without_local,
            local: input.local_return.clone(),
            total,
        },
        failed_conjunction,
        unconditional_success_failure,
        unconditional_bif_failure,
        diagnostic,
        opacity,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateMultiplicity {
    None,
    One,
    Many,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlternativeAccumulator<W> {
    pub arguments: Vec<Type>,
    pub result: Type,
    pub had_external: bool,
    pub warnings: Vec<W>,
    candidates: usize,
}
impl<W: Clone> AlternativeAccumulator<W> {
    pub fn new(arity: usize) -> Self {
        Self {
            arguments: vec![Type::none(); arity],
            result: Type::none(),
            had_external: false,
            warnings: Vec::new(),
            candidates: 0,
        }
    }
    pub fn multiplicity(&self) -> CandidateMultiplicity {
        match self.candidates {
            0 => CandidateMultiplicity::None,
            1 => CandidateMultiplicity::One,
            _ => CandidateMultiplicity::Many,
        }
    }
    fn warnings(&mut self, new: Vec<W>) {
        if self.candidates == 0 {
            self.warnings = new;
        } else if self.warnings.is_empty() || new.is_empty() {
            self.warnings.clear();
        } else {
            let mut merged = new;
            merged.append(&mut self.warnings);
            self.warnings = merged;
        }
        self.candidates += 1;
    }
    pub fn add(&mut self, transfer: &CallTransfer, warnings: Vec<W>) -> Result<()> {
        if self.arguments.len() != transfer.arguments.conjunction.len() {
            return Err(CallError::Arity {
                contribution: Contribution::Success,
                expected: self.arguments.len(),
                actual: transfer.arguments.conjunction.len(),
            });
        }
        let arguments = if !transfer.failed_conjunction {
            self.arguments
                .iter()
                .zip(&transfer.arguments.conjunction)
                .map(|(a, b)| a.join(b).map_err(Into::into))
                .collect::<Result<_>>()?
        } else {
            self.arguments.clone()
        };
        let result = self.result.join(&transfer.ranges.total)?;
        self.arguments = arguments;
        self.result = result;
        self.warnings(warnings);
        Ok(())
    }
    pub fn external(&mut self, actual: &[Type]) -> Result<()> {
        if self.arguments.len() != actual.len() {
            return Err(CallError::Arity {
                contribution: Contribution::Success,
                expected: self.arguments.len(),
                actual: actual.len(),
            });
        }
        self.arguments = actual.to_vec();
        self.result = Type::any();
        self.had_external = true;
        self.warnings(Vec::new());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lattice::BuiltinType;
    #[test]
    fn local_none_preserves_no_return_unit() {
        let success = Signature {
            arguments: vec![],
            result: Type::unit(),
        };
        let result = evaluate(CallInput {
            actual: &[],
            success: Some(&success),
            contract: None,
            bif: None,
            local_return: &Type::none(),
            module: "caller",
        })
        .unwrap();
        assert_eq!(result.ranges.total, Type::unit());
        assert!(!result.failed_conjunction);
        assert_eq!(result.diagnostic, Diagnostic::None);
    }
    #[test]
    fn contract_range_and_unconditional_failure_are_distinct() {
        let success = Signature {
            arguments: vec![],
            result: Type::number(),
        };
        let returns_atom = |_: &[Type]| Ok(Type::atom("ok"));
        let result = evaluate(CallInput {
            actual: &[],
            success: Some(&success),
            contract: Some(DependentSummary {
                arguments: &[],
                range: &returns_atom,
            }),
            bif: None,
            local_return: &Type::any(),
            module: "caller",
        })
        .unwrap();
        assert_eq!(
            result.diagnostic,
            Diagnostic::ContractRange {
                without_contract: Type::number()
            }
        );
        let never = Signature {
            arguments: vec![],
            result: Type::none(),
        };
        let result = evaluate(CallInput {
            actual: &[],
            success: Some(&never),
            contract: Some(DependentSummary {
                arguments: &[],
                range: &returns_atom,
            }),
            bif: None,
            local_return: &Type::any(),
            module: "caller",
        })
        .unwrap();
        assert_eq!(result.diagnostic, Diagnostic::None);
        assert!(result.unconditional_success_failure);
    }
    #[test]
    fn broader_failure_range_is_evaluated_only_for_failing_call() {
        use crate::bifs::registry::{Mfa, Registry, TransferError};
        let registry = Registry::standard().unwrap();
        let mfa = Mfa {
            module: "erlang".into(),
            function: "is_record".into(),
            arity: 2,
        };
        let tuple = Type::tuple(vec![
            Type::atom("tag"),
            Type::builtin(BuiltinType::Integer).unwrap(),
        ])
        .unwrap();
        let actual = vec![
            tuple.join(&Type::tuple(vec![]).unwrap()).unwrap(),
            Type::builtin(BuiltinType::Atom).unwrap(),
        ];
        let success = Signature {
            arguments: vec![tuple, Type::atom("tag")],
            result: Type::any(),
        };
        let domains = registry.admissible_domains(&mfa).unwrap().unwrap();
        let range = |args: &[Type]| {
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
        let good = evaluate(CallInput {
            actual: &actual,
            success: Some(&success),
            contract: None,
            bif: Some(DependentSummary {
                arguments: &domains,
                range: &range,
            }),
            local_return: &Type::any(),
            module: "caller",
        })
        .unwrap();
        assert_eq!(good.diagnostic, Diagnostic::None);
        let success = Signature {
            result: Type::atom("false"),
            ..success
        };
        assert_eq!(
            evaluate(CallInput {
                actual: &actual,
                success: Some(&success),
                contract: None,
                bif: Some(DependentSummary {
                    arguments: &domains,
                    range: &range
                }),
                local_return: &Type::any(),
                module: "caller"
            }),
            Err(CallError::NativeRange {
                contribution: Contribution::Bif,
                error: NativeTransferError::CaseClauseNil
            })
        );
    }
    #[test]
    fn failed_arguments_do_not_force_total_range_to_bottom() {
        let success = Signature {
            arguments: vec![Type::atom("wanted")],
            result: Type::atom("returned"),
        };
        let result = evaluate(CallInput {
            actual: &[Type::atom("given")],
            success: Some(&success),
            contract: None,
            bif: None,
            local_return: &Type::any(),
            module: "caller",
        })
        .unwrap();
        assert!(result.failed_conjunction);
        assert_eq!(result.ranges.total, Type::atom("returned"));
        let mut accumulator = AlternativeAccumulator::new(1);
        accumulator.add(&result, vec!["bad"]).unwrap();
        assert_eq!(accumulator.arguments, [Type::none()]);
        assert_eq!(accumulator.result, Type::atom("returned"));
    }
    #[test]
    fn warning_consensus_clears_after_one_successful_alternative() {
        let transfer = evaluate(CallInput {
            actual: &[],
            success: None,
            contract: None,
            bif: None,
            local_return: &Type::any(),
            module: "caller",
        })
        .unwrap();
        let mut accumulator = AlternativeAccumulator::new(0);
        accumulator.add(&transfer, vec![1]).unwrap();
        accumulator.add(&transfer, vec![]).unwrap();
        accumulator.add(&transfer, vec![2]).unwrap();
        assert!(accumulator.warnings.is_empty());
        assert_eq!(accumulator.multiplicity(), CandidateMultiplicity::Many);
    }
}

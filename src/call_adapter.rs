//! Explicit bridge between phase-owned call facts and the pure native call algebra.
use crate::call_transfer::{
    self, CallError, CallInput, CallTransfer, Contribution, DependentSummary, Signature,
};
use crate::flow::{BifProvider, ConstraintProvider};
use crate::lattice::Type;
use crate::program::FunctionName;

pub struct CallFacts<'a> {
    /// Raw input-PLT success signature, separately owned by the phase orchestrator.
    pub success: Option<&'a Signature>,
    /// Current local function-table return; remote calls supply Any.
    pub local_return: &'a Type,
    /// Contracts are consulted only outside the active SCC.
    pub contract_visible: bool,
    /// Native local-label calls use the generic BIF contribution.
    pub remote: bool,
}

pub fn evaluate_named_call(
    name: &FunctionName,
    actual: &[Type],
    caller_module: &str,
    facts: CallFacts<'_>,
    constraints: &dyn ConstraintProvider,
    bifs: &dyn BifProvider,
) -> Result<CallTransfer, CallError> {
    let contract_domains = if facts.contract_visible {
        constraints
            .contract_domains(name)
            .map_err(|message| CallError::Provider {
                contribution: Contribution::Contract,
                message,
            })?
    } else {
        None
    };
    let bif_domains = if facts.remote {
        bifs.admissible_domains(name)
            .map_err(|message| CallError::Provider {
                contribution: Contribution::Bif,
                message,
            })?
    } else {
        None
    };
    let contract_range = |arguments: &[Type]| {
        constraints
            .contract_constraint(name, arguments)
            .map_err(|message| CallError::Provider {
                contribution: Contribution::Contract,
                message,
            })?
            .map(|signature| signature.result)
            .ok_or_else(|| CallError::Provider {
                contribution: Contribution::Contract,
                message: "contract domains present but return provider missing".into(),
            })
    };
    let bif_range = |arguments: &[Type]| {
        bifs.transfer(name, arguments)
            .map_err(|message| CallError::Provider {
                contribution: Contribution::Bif,
                message,
            })?
            .map(|transfer| transfer.result)
            .ok_or_else(|| CallError::Provider {
                contribution: Contribution::Bif,
                message: "BIF domains present but transfer missing".into(),
            })
    };
    call_transfer::evaluate(CallInput {
        actual,
        success: facts.success,
        contract: contract_domains
            .as_deref()
            .map(|arguments| DependentSummary {
                arguments,
                range: &contract_range,
            }),
        bif: bif_domains.as_deref().map(|arguments| DependentSummary {
            arguments,
            range: &bif_range,
        }),
        local_return: facts.local_return,
        module: caller_module,
    })
}

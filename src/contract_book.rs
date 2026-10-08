use crate::{
    bifs::registry::{Mfa, Registry},
    contract_runtime::ContractSummary,
    lattice::{LatticeError, Type},
    validation::{Validation, validate},
};
use std::collections::BTreeMap;

pub struct ContractBook {
    candidates: BTreeMap<Mfa, ContractSummary>,
    active: BTreeMap<Mfa, ContractSummary>,
}
pub struct ValidatedContracts(Vec<(Mfa, ContractSummary)>);

impl ContractBook {
    pub fn active_names(&self) -> impl Iterator<Item = &Mfa> {
        self.active.keys()
    }

    pub fn new(candidates: BTreeMap<Mfa, ContractSummary>) -> Self {
        Self {
            candidates,
            active: BTreeMap::new(),
        }
    }
    pub fn candidate(&self, name: &Mfa) -> Option<&ContractSummary> {
        self.candidates.get(name)
    }
    pub fn active(&self, name: &Mfa) -> Option<&ContractSummary> {
        self.active.get(name)
    }
    pub fn validate(
        &self,
        signatures: &BTreeMap<Mfa, Type>,
        bifs: &Registry,
    ) -> Result<ValidatedContracts, LatticeError> {
        let mut accepted = Vec::new();
        for (name, success) in signatures {
            let Some(contract) = self.candidates.get(name) else {
                continue;
            };
            let validation = validate(contract, success, &name.module)?;
            let keep = match validation {
                Validation::Valid => !bifs.is_known(name),
                Validation::RangeWarnings { .. } => true,
                _ => false,
            };
            if keep {
                accepted.push((name.clone(), contract.clone()));
            }
        }
        Ok(ValidatedContracts(accepted))
    }
    pub fn commit(&mut self, validated: ValidatedContracts) -> Vec<Mfa> {
        let mut added = Vec::new();
        for (name, contract) in validated.0 {
            if let std::collections::btree_map::Entry::Vacant(slot) =
                self.active.entry(name.clone())
            {
                slot.insert(contract);
                added.push(name);
            }
        }
        added
    }
    pub fn activate(
        &mut self,
        signatures: &BTreeMap<Mfa, Type>,
        bifs: &Registry,
    ) -> Result<Vec<Mfa>, LatticeError> {
        let validated = self.validate(signatures, bifs)?;
        Ok(self.commit(validated))
    }
}

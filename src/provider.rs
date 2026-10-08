use crate::bifs::registry::{Mfa, Registry};
use crate::dependencies::Store;
use crate::flow::{BifProvider, ConstraintProvider, GuardTransfer, Signature, Transfer};
use crate::lattice::Type;
use crate::program::FunctionName;
use std::cell::RefCell;
use std::collections::BTreeSet;

impl From<&FunctionName> for Mfa {
    fn from(name: &FunctionName) -> Self {
        Self {
            module: name.module.clone(),
            function: name.name.clone(),
            arity: name.arity,
        }
    }
}
impl BifProvider for Registry {
    fn admissible_domains(&self, name: &FunctionName) -> Result<Option<Vec<Type>>, String> {
        Registry::admissible_domains(self, &Mfa::from(name)).map_err(|e| e.to_string())
    }
    fn transfer(
        &self,
        name: &FunctionName,
        arguments: &[Type],
    ) -> Result<Option<Transfer>, String> {
        let mfa = Mfa::from(name);
        if !self.is_known(&mfa) && !self.is_runtime_builtin(&mfa) {
            return Ok(None);
        }
        let outcome = Registry::transfer(self, &mfa, arguments).map_err(|e| e.to_string())?;
        let domains = self
            .admissible_domains(&mfa)
            .map_err(|e| e.to_string())?
            .unwrap_or_else(|| vec![Type::any(); name.arity]);
        Ok(Some(Transfer {
            accepted: domains,
            result: outcome.result,
            can_raise: false,
        }))
    }
    fn guard(
        &self,
        name: &FunctionName,
        arguments: &[Type],
        truth: bool,
    ) -> Result<Option<GuardTransfer>, String> {
        let mfa = Mfa::from(name);
        if !self.is_known(&mfa) && !self.is_runtime_builtin(&mfa) {
            return Ok(None);
        }
        let outcome = Registry::transfer(self, &mfa, arguments).map_err(|e| e.to_string())?;
        Ok(self
            .guard_refinements(&mfa, arguments, truth)
            .map_err(|e| e.to_string())?
            .map(|arguments| GuardTransfer {
                arguments,
                result: outcome.result,
            }))
    }
}
pub struct ExternalConstraints<'a> {
    pub dependencies: Option<&'a Store>,
    unresolved: RefCell<BTreeSet<FunctionName>>,
}
impl<'a> ExternalConstraints<'a> {
    pub fn new(dependencies: Option<&'a Store>) -> Self {
        Self {
            dependencies,
            unresolved: RefCell::default(),
        }
    }
    pub fn unresolved(&self) -> Vec<FunctionName> {
        self.unresolved.borrow().iter().cloned().collect()
    }
}
impl ConstraintProvider for ExternalConstraints<'_> {
    fn external_summary(&self, name: &FunctionName) -> Result<Signature, String> {
        if let Some(store) = self.dependencies
            && let Some(summary) = store.summary(&Mfa::from(name))?
        {
            return Ok(Signature {
                arguments: summary.arguments,
                result: summary.result,
            });
        }
        self.unresolved.borrow_mut().insert(name.clone());
        Ok(Signature {
            arguments: vec![Type::any(); name.arity],
            result: Type::any(),
        })
    }
    fn contract_domains(&self, name: &FunctionName) -> Result<Option<Vec<Type>>, String> {
        self.dependencies
            .and_then(|store| store.contracts.get(&Mfa::from(name)))
            .map(|contract| Ok(contract.summary()?.arguments.clone()))
            .transpose()
    }
    fn contract_constraint(
        &self,
        name: &FunctionName,
        actual: &[Type],
    ) -> Result<Option<Signature>, String> {
        self.dependencies
            .and_then(|store| store.contracts.get(&Mfa::from(name)))
            .map(|contract| {
                let summary = contract.summary()?;
                Ok(Signature {
                    arguments: summary.arguments.clone(),
                    result: summary.return_for(actual).map_err(|e| e.to_string())?,
                })
            })
            .transpose()
    }
}

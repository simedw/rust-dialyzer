use crate::lattice::{LatticeError, Type, TypeView, VariableId, VariableView};
use std::collections::BTreeMap;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContractError {
    Lattice(LatticeError),
    InvalidConstraintLeft(Box<Type>),
    UnknownArity,
    ArityMismatch { expected: usize, actual: usize },
}
impl From<LatticeError> for ContractError {
    fn from(e: LatticeError) -> Self {
        Self::Lattice(e)
    }
}
impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ContractError {}
#[derive(Clone, Debug)]
pub struct ContractAlternative {
    pub signature: Type,
    pub constraints: BTreeMap<VariableId, Type>,
}
#[derive(Clone, Debug)]
pub struct ContractSummary {
    pub alternatives: Vec<ContractAlternative>,
    pub arguments: Vec<Type>,
}
fn parts(value: &Type) -> Result<(Vec<Type>, Type), ContractError> {
    let value = value.shallow_structural()?;
    if let TypeView::Families(slots) = value.view()
        && let Some(fun) = slots.functions
    {
        return Ok((
            fun.domain.ok_or(ContractError::UnknownArity)?.to_vec(),
            fun.range.clone(),
        ));
    }
    Err(LatticeError::FunctionClause.into())
}
impl ContractSummary {
    pub fn from_parts(alternatives: Vec<(Type, Vec<(Type, Type)>)>) -> Result<Self, ContractError> {
        let alternatives = alternatives
            .into_iter()
            .map(|(signature, pairs)| {
                let mut constraints: BTreeMap<VariableId, Type> = BTreeMap::new();
                for (left, right) in pairs {
                    let id = match left.view() {
                        TypeView::Variable(VariableView::Atom(name)) => {
                            VariableId::Atom(name.into())
                        }
                        TypeView::Variable(VariableView::Integer(id)) => {
                            VariableId::Integer(id.clone())
                        }
                        _ => return Err(ContractError::InvalidConstraintLeft(Box::new(left))),
                    };
                    let value = if let Some(previous) = constraints.get(&id) {
                        previous.meet(&right)?
                    } else {
                        right
                    };
                    constraints.insert(id, value);
                }
                Ok(ContractAlternative {
                    signature,
                    constraints,
                })
            })
            .collect::<Result<Vec<_>, ContractError>>()?;
        let general = alternatives
            .iter()
            .try_fold(Type::none(), |acc, a| {
                acc.join(&a.signature.substitute(&a.constraints)?)
            })?
            .substitute(&BTreeMap::new())?;
        let arguments = parts(&general)?.0;
        Ok(Self {
            alternatives,
            arguments,
        })
    }
    pub fn return_for(&self, actual: &[Type]) -> Result<Type, ContractError> {
        let mut result = Type::none();
        for alternative in &self.alternatives {
            let (domain, _) = parts(&alternative.signature)?;
            let domain =
                Type::function(Some(domain), Type::any()).substitute(&alternative.constraints)?;
            let (domain, _) = parts(&domain)?;
            if domain.len() != actual.len() {
                return Err(ContractError::ArityMismatch {
                    expected: domain.len(),
                    actual: actual.len(),
                });
            }
            let mut feasible = true;
            for (expected, actual) in domain.iter().zip(actual) {
                if expected.meet(actual)?.is_impossible() {
                    feasible = false;
                    break;
                }
            }
            if feasible {
                let (_, range) = parts(&alternative.signature)?;
                result = result.join(&range.substitute(&alternative.constraints)?)?;
            }
        }
        Ok(result)
    }
}

pub use crate::constraints::bindings::{BindingKey, BindingMap};
use crate::constraints::derived::{BifResultProvider, DerivedArena, DerivedId, EvalError};
use crate::lattice::{LatticeError, Type, TypePredicate, UnificationError, UnificationValue};
use std::collections::BTreeSet;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Equal,
    Subtype,
}
#[derive(Clone, Debug)]
pub enum ConstraintValue {
    Type(Type),
    Derived(DerivedId),
}
#[derive(Clone, Debug)]
pub struct AtomicConstraint {
    pub left: Type,
    pub relation: Relation,
    pub right: ConstraintValue,
}
#[derive(Clone, Debug)]
pub enum SolveError {
    Native(LatticeError),
    NonTerminatingUnification,
    Evaluation(EvalError),
    InvalidDerivedIdentity(DerivedId),
}
#[derive(Clone, Debug)]
pub enum AtomicOutcome {
    Failed,
    Solved {
        bindings: BindingMap,
        updated: BTreeSet<BindingKey>,
    },
}
impl From<LatticeError> for SolveError {
    fn from(e: LatticeError) -> Self {
        Self::Native(e)
    }
}
impl AtomicConstraint {
    pub fn solve(
        &self,
        bindings: BindingMap,
        arena: &DerivedArena,
        provider: &dyn BifResultProvider,
    ) -> Result<AtomicOutcome, SolveError> {
        let lookup = |id: &crate::lattice::VariableId| bindings.get(&BindingKey::Label(id.clone()));
        let left = self.left.substitute_with(&lookup)?;
        let right = match &self.right {
            ConstraintValue::Type(ty) => ty.substitute_with(&lookup)?,
            ConstraintValue::Derived(id) => arena
                .get(*id)
                .ok_or(SolveError::InvalidDerivedIdentity(*id))?
                .evaluate(&bindings, provider)
                .map_err(SolveError::Evaluation)?,
        };
        let intersection = left.meet(&right)?;
        if intersection.predicate(TypePredicate::None)? {
            return Ok(AtomicOutcome::Failed);
        }
        let first = refine(&self.left, &intersection, bindings, BTreeSet::new())?;
        match (self.relation, first) {
            (_, AtomicOutcome::Failed) => Ok(AtomicOutcome::Failed),
            (Relation::Subtype, solved) => Ok(solved),
            (Relation::Equal, AtomicOutcome::Solved { bindings, updated }) => match &self.right {
                ConstraintValue::Type(ty) => refine(ty, &intersection, bindings, updated),
                ConstraintValue::Derived(_) => Ok(AtomicOutcome::Failed),
            },
        }
    }
}
pub(crate) fn refine(
    original: &Type,
    intersection: &Type,
    bindings: BindingMap,
    updated: BTreeSet<BindingKey>,
) -> Result<AtomicOutcome, SolveError> {
    if let Some(variable) = original.variable_ref() {
        if intersection.variable_ref().is_none() {
            return refine_values(
                std::iter::once((
                    BindingKey::Label(variable.clone()),
                    UnificationValue::Type(intersection.clone()),
                )),
                bindings,
                updated,
            );
        }
    }
    let unified = match original.unify_bindings(intersection) {
        Ok(v) => v,
        Err(UnificationError::Mismatch { .. }) => return Ok(AtomicOutcome::Failed),
        Err(UnificationError::NativeError(e)) => return Err(SolveError::Native(e)),
        Err(UnificationError::NativeNonTerminating) => {
            return Err(SolveError::NonTerminatingUnification);
        }
    };
    refine_values(
        unified
            .into_iter()
            .map(|(variable, value)| (BindingKey::Label(variable), value)),
        bindings,
        updated,
    )
}
pub(crate) fn refine_values(
    values: impl IntoIterator<Item = (BindingKey, UnificationValue)>,
    mut bindings: BindingMap,
    mut updated: BTreeSet<BindingKey>,
) -> Result<AtomicOutcome, SolveError> {
    for (variable, value) in values {
        if matches!(&value,UnificationValue::Type(t) if *t==Type::any()) {
            if bindings.contains_key(&variable) {
                bindings.remove(&variable);
            }
            continue;
        }
        let limited = value.limit(5)?;
        if let Some(old) = bindings.get(&variable) {
            let narrowed = old.meet(&limited)?;
            if narrowed.predicate(TypePredicate::None)? {
                return Ok(AtomicOutcome::Failed);
            }
            if *old != narrowed {
                updated.insert(variable.clone());
                bindings.insert(variable, narrowed);
            }
        } else {
            updated.insert(variable.clone());
            bindings.insert(variable, limited);
        }
    }
    Ok(AtomicOutcome::Solved { bindings, updated })
}

#[cfg(test)]
mod substitution_lookup_tests {
    use super::*;
    use crate::{
        constraints::derived::NoBifProvider,
        lattice::{MapTypeEntry, Presence, VariableId},
    };
    fn variable(n: i64) -> Type {
        Type::variable_integer(n.into())
    }
    fn label(n: i64) -> BindingKey {
        BindingKey::Label(VariableId::Integer(n.into()))
    }
    #[test]
    fn lookup_substitutes_map_entry_values_absent_from_collected_names() {
        let map = |value, default| {
            Type::map_types(
                vec![MapTypeEntry {
                    key: Type::atom("entry"),
                    presence: Presence::Mandatory,
                    value,
                }],
                Type::all_atoms(),
                default,
            )
            .unwrap()
        };
        let original = map(variable(1), variable(2));
        assert!(
            !original
                .collect_var_names()
                .contains(&VariableId::Integer(1.into()))
        );
        let mut bindings = BindingMap::new();
        bindings.insert(label(1), Type::atom("value"));
        bindings.insert(label(2), Type::atom("default"));
        let constraint = AtomicConstraint {
            left: original,
            relation: Relation::Subtype,
            right: ConstraintValue::Type(map(Type::atom("value"), Type::atom("default"))),
        };
        let AtomicOutcome::Solved {
            bindings: actual,
            updated,
        } = constraint
            .solve(bindings.clone(), &DerivedArena::default(), &NoBifProvider)
            .unwrap()
        else {
            panic!("map values lost during substitution")
        };
        assert_eq!(actual, bindings);
        assert!(updated.is_empty());
    }
    #[test]
    fn lookup_ignores_raw_variable_records() {
        let raw = BindingKey::RawVariableRecord(VariableId::Integer(1.into()));
        let mut bindings = BindingMap::new();
        bindings.insert(raw.clone(), Type::atom("raw"));
        let constraint = AtomicConstraint {
            left: variable(1),
            relation: Relation::Subtype,
            right: ConstraintValue::Type(Type::atom("label")),
        };
        let AtomicOutcome::Solved { bindings, updated } = constraint
            .solve(bindings, &DerivedArena::default(), &NoBifProvider)
            .unwrap()
        else {
            panic!("raw record used as label")
        };
        assert_eq!(bindings[&label(1)], Type::atom("label"));
        assert_eq!(bindings[&raw], Type::atom("raw"));
        assert_eq!(updated, BTreeSet::from([label(1)]));
    }
    #[test]
    fn public_and_borrowed_substitution_keep_one_step_replacement() {
        let bindings = std::collections::BTreeMap::from([
            (VariableId::Integer(1.into()), variable(2)),
            (VariableId::Integer(2.into()), Type::atom("transitive")),
        ]);
        assert_eq!(variable(1).substitute(&bindings).unwrap(), variable(2));
        assert_eq!(
            variable(1).substitute_with(&|id| bindings.get(id)).unwrap(),
            variable(2)
        );
    }
}

#[cfg(test)]
mod variable_refinement_tests {
    use super::*;
    use crate::lattice::VariableId;

    #[test]
    fn variable_any_removal_preserves_update_set_and_raw_record_binding() {
        let id = VariableId::Integer(17.into());
        let label = BindingKey::Label(id.clone());
        let raw = BindingKey::RawVariableRecord(id.clone());
        let bindings = [
            (label.clone(), Type::atom("old")),
            (raw.clone(), Type::atom("raw")),
        ]
        .into_iter()
        .collect();
        let updated = [label.clone()].into_iter().collect();
        let AtomicOutcome::Solved { bindings, updated } =
            refine(&Type::variable_id(id), &Type::any(), bindings, updated).unwrap()
        else {
            panic!("expected solution")
        };
        assert!(!bindings.contains_key(&label));
        assert_eq!(bindings.get(&raw), Some(&Type::atom("raw")));
        assert!(updated.contains(&label));
    }

    #[test]
    fn both_variable_refinement_preserves_left_alias_and_conflicting_binding_failure() {
        let x = VariableId::Integer(17.into());
        let y = VariableId::Atom("captured".into());
        let left = Type::variable_id(x.clone());
        let right = Type::variable_id(y.clone());
        let AtomicOutcome::Solved { bindings, updated } =
            refine(&left, &right, BindingMap::new(), BTreeSet::new()).unwrap()
        else {
            panic!("expected solution")
        };
        assert_eq!(bindings.get(&BindingKey::Label(x.clone())), Some(&left));
        assert_eq!(bindings.get(&BindingKey::Label(y.clone())), Some(&left));
        assert_eq!(updated.len(), 2);
        let bindings = [(BindingKey::Label(x), Type::atom("old"))]
            .into_iter()
            .collect();
        assert!(matches!(
            refine(&left, &Type::atom("new"), bindings, BTreeSet::new()).unwrap(),
            AtomicOutcome::Failed
        ));
    }
}

#[cfg(test)]
mod absent_removal_proof {
    use super::*;
    use crate::lattice::VariableId;
    #[test]
    fn absent_any_removal_retains_root_and_raw_keys() {
        let mut input = BindingMap::new();
        for n in 0..300 {
            input.insert(
                BindingKey::Label(VariableId::Integer(n.into())),
                Type::atom("payload"),
            );
        }
        input.insert(
            BindingKey::RawVariableRecord(VariableId::Integer(999.into())),
            Type::atom("raw"),
        );
        let updated: BTreeSet<BindingKey> =
            [BindingKey::Label(VariableId::Integer(3.into()))].into();
        let result = refine_values(
            [(
                BindingKey::Label(VariableId::Integer(999.into())),
                UnificationValue::Type(Type::any()),
            )],
            input.clone(),
            updated.clone(),
        )
        .unwrap();
        let AtomicOutcome::Solved {
            bindings,
            updated: actual,
        } = result
        else {
            panic!("must solve")
        };
        assert!(bindings.ptr_eq(&input));
        assert_eq!(bindings, input);
        assert_eq!(actual, updated);
    }
    #[test]
    fn present_any_removal_preserves_atomic_updated_convention() {
        let key = BindingKey::Label(VariableId::Integer(1.into()));
        let input = [(key.clone(), Type::any())].into_iter().collect();
        let AtomicOutcome::Solved { bindings, updated } = refine_values(
            [(key, UnificationValue::Type(Type::any()))],
            input,
            BTreeSet::new(),
        )
        .unwrap() else {
            panic!("must solve")
        };
        assert!(bindings.is_empty());
        assert!(updated.is_empty());
    }
}

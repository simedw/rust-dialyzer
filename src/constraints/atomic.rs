use crate::constraints::bindings::BindingAccess;
pub use crate::constraints::bindings::{BindingKey, BindingMap, KeySet};
use crate::constraints::derived::{BifResultProvider, DerivedArena, DerivedId, EvalError};
use crate::lattice::{LatticeError, Type, TypePredicate, UnificationError, UnificationValue};

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
        updated: KeySet,
    },
}
pub(super) enum AtomicStatus {
    Failed,
    Solved(KeySet),
}
impl AtomicStatus {
    fn into_outcome(self, bindings: BindingMap) -> AtomicOutcome {
        match self {
            Self::Failed => AtomicOutcome::Failed,
            Self::Solved(updated) => AtomicOutcome::Solved { bindings, updated },
        }
    }
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
        self.solve_logged(bindings, arena, provider, None)
    }
    /// Solves, recording each written key and its previous value in write order.
    pub(crate) fn solve_logged(
        &self,
        mut bindings: BindingMap,
        arena: &DerivedArena,
        provider: &dyn BifResultProvider,
        log: Option<&mut WriteLog>,
    ) -> Result<AtomicOutcome, SolveError> {
        let status = self.solve_access(&mut bindings, arena, provider, log)?;
        Ok(status.into_outcome(bindings))
    }
    pub(super) fn solve_access<A: BindingAccess>(
        &self,
        bindings: &mut A,
        arena: &DerivedArena,
        provider: &dyn BifResultProvider,
        mut log: Option<&mut WriteLog>,
    ) -> Result<AtomicStatus, SolveError> {
        let lookup = |id: &crate::lattice::VariableId| bindings.get(&BindingKey::Label(id.clone()));
        let left = self.left.substitute_with(&lookup)?;
        let right = match &self.right {
            ConstraintValue::Type(ty) => ty.substitute_with(&lookup)?,
            ConstraintValue::Derived(id) => arena
                .get(*id)
                .ok_or(SolveError::InvalidDerivedIdentity(*id))?
                .evaluate_access(bindings, provider)
                .map_err(SolveError::Evaluation)?,
        };
        let intersection = left.meet(&right)?;
        if intersection.predicate(TypePredicate::None)? {
            return Ok(AtomicStatus::Failed);
        }
        let first = refine_access(
            &self.left,
            &intersection,
            bindings,
            KeySet::new(),
            log.as_deref_mut(),
        )?;
        match (self.relation, first) {
            (_, AtomicStatus::Failed) => Ok(AtomicStatus::Failed),
            (Relation::Subtype, solved) => Ok(solved),
            (Relation::Equal, AtomicStatus::Solved(updated)) => match &self.right {
                ConstraintValue::Type(ty) => {
                    refine_access(ty, &intersection, bindings, updated, log)
                }
                ConstraintValue::Derived(_) => Ok(AtomicStatus::Failed),
            },
        }
    }
}
/// Each written binding with its value just before the write, in write order.
pub(crate) type WriteLog = Vec<(BindingKey, Option<Type>)>;
pub(crate) fn refine(
    original: &Type,
    intersection: &Type,
    mut bindings: BindingMap,
    updated: KeySet,
) -> Result<AtomicOutcome, SolveError> {
    let status = refine_access(original, intersection, &mut bindings, updated, None)?;
    Ok(status.into_outcome(bindings))
}
pub(super) fn refine_access<A: BindingAccess>(
    original: &Type,
    intersection: &Type,
    bindings: &mut A,
    updated: KeySet,
    log: Option<&mut WriteLog>,
) -> Result<AtomicStatus, SolveError> {
    if let Some(variable) = original.variable_ref() {
        if intersection.variable_ref().is_none() {
            return refine_values_access(
                std::iter::once((
                    BindingKey::Label(variable.clone()),
                    UnificationValue::Type(intersection.clone()),
                )),
                bindings,
                updated,
                log,
            );
        }
    }
    let unified = match original.unify_bindings(intersection) {
        Ok(v) => v,
        Err(UnificationError::Mismatch { .. }) => return Ok(AtomicStatus::Failed),
        Err(UnificationError::NativeError(e)) => return Err(SolveError::Native(e)),
        Err(UnificationError::NativeNonTerminating) => {
            return Err(SolveError::NonTerminatingUnification);
        }
    };
    refine_values_access(
        unified
            .into_iter()
            .map(|(variable, value)| (BindingKey::Label(variable), value)),
        bindings,
        updated,
        log,
    )
}
pub(crate) fn refine_values(
    values: impl IntoIterator<Item = (BindingKey, UnificationValue)>,
    mut bindings: BindingMap,
    updated: KeySet,
) -> Result<AtomicOutcome, SolveError> {
    let status = refine_values_access(values, &mut bindings, updated, None)?;
    Ok(status.into_outcome(bindings))
}
pub(super) fn refine_values_access<A: BindingAccess>(
    values: impl IntoIterator<Item = (BindingKey, UnificationValue)>,
    bindings: &mut A,
    mut updated: KeySet,
    mut log: Option<&mut WriteLog>,
) -> Result<AtomicStatus, SolveError> {
    for (variable, value) in values {
        if matches!(&value,UnificationValue::Type(t) if *t==Type::any()) {
            if bindings.contains_key(&variable) {
                let old = bindings.remove(&variable);
                if let Some(log) = log.as_deref_mut() {
                    log.push((variable, old));
                }
            }
            continue;
        }
        let limited = value.limit(5)?;
        if let Some(old) = bindings.get(&variable) {
            let narrowed = old.meet(&limited)?;
            if narrowed.predicate(TypePredicate::None)? {
                return Ok(AtomicStatus::Failed);
            }
            if *old != narrowed {
                updated.insert(variable.clone());
                match log.as_deref_mut() {
                    Some(log) => {
                        let old = bindings.insert(variable.clone(), narrowed);
                        log.push((variable, old));
                    }
                    None => {
                        bindings.insert(variable, narrowed);
                    }
                }
            }
        } else {
            updated.insert(variable.clone());
            if let Some(log) = log.as_deref_mut() {
                log.push((variable.clone(), None));
            }
            bindings.insert(variable, limited);
        }
    }
    Ok(AtomicStatus::Solved(updated))
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
        let mut names = std::collections::BTreeSet::new();
        original.append_var_names(&mut names);
        assert!(
            !names.contains(&VariableId::Integer(1.into()))
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
        assert_eq!(updated, KeySet::from([label(1)]));
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
            refine(&left, &right, BindingMap::new(), KeySet::new()).unwrap()
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
            refine(&left, &Type::atom("new"), bindings, KeySet::new()).unwrap(),
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
        let updated: KeySet =
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
            KeySet::new(),
        )
        .unwrap() else {
            panic!("must solve")
        };
        assert!(bindings.is_empty());
        assert!(updated.is_empty());
    }
}

#[cfg(test)]
mod native_access_order_tests {
    use super::*;
    use crate::bifs::registry::Mfa;
    use crate::constraints::derived::DerivedOp;
    use crate::lattice::VariableId;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn key(n: i64) -> BindingKey {
        BindingKey::Label(VariableId::Integer(n.into()))
    }
    struct PanicWrite(BindingMap);
    impl BindingAccess for PanicWrite {
        fn get(&self, key: &BindingKey) -> Option<&Type> {
            self.0.get(key)
        }
        fn insert(&mut self, _: BindingKey, _: Type) -> Option<Type> {
            panic!("write boundary");
        }
        fn remove(&mut self, _: &BindingKey) -> Option<Type> {
            panic!("write boundary");
        }
        fn native_map(&self) -> &BindingMap {
            &self.0
        }
    }
    #[test]
    fn absent_log_precedes_write_but_changed_and_removed_logs_follow_write() {
        for (present, value, expected_log) in [
            (false, Type::atom("new"), vec![(key(1), None)]),
            (true, Type::atom("new"), vec![]),
            (true, Type::any(), vec![]),
        ] {
            let initial = if present {
                BindingMap::from(vec![(key(1), Type::any())])
            } else {
                BindingMap::new()
            };
            let mut access = PanicWrite(initial);
            let mut log = vec![];
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                refine_values_access(
                    [(key(1), UnificationValue::Type(value))],
                    &mut access,
                    KeySet::new(),
                    Some(&mut log),
                )
            }));
            assert!(result.is_err());
            assert_eq!(log, expected_log);
        }
    }
    #[test]
    fn failed_refinement_keeps_prior_write_log_and_stops_before_later_values() {
        let retained = BindingMap::from(vec![(key(2), Type::atom("old"))]);
        let mut current = retained.clone();
        let mut log = vec![];
        let status = refine_values_access(
            [
                (key(1), UnificationValue::Type(Type::atom("written"))),
                (key(2), UnificationValue::Type(Type::atom("conflict"))),
                (key(3), UnificationValue::Type(Type::atom("unreached"))),
            ],
            &mut current,
            KeySet::new(),
            Some(&mut log),
        )
        .unwrap();
        assert!(matches!(status, AtomicStatus::Failed));
        assert_eq!(log, vec![(key(1), None)]);
        assert_eq!(current.get(&key(1)), Some(&Type::atom("written")));
        assert_eq!(current.get(&key(2)), Some(&Type::atom("old")));
        assert_eq!(current.get(&key(3)), None);
        assert_eq!(retained.get(&key(1)), None);
    }
    struct EffectProvider(Rc<RefCell<Vec<Vec<Type>>>>);
    impl BifResultProvider for EffectProvider {
        fn result(&self, _: &Mfa, arguments: &[Type]) -> Result<Type, EvalError> {
            self.0.borrow_mut().push(arguments.to_vec());
            Ok(Type::atom("written"))
        }
    }
    #[test]
    fn equal_derived_writes_left_then_fails_with_one_opaque_provider_call() {
        let effects = Rc::new(RefCell::new(vec![]));
        let provider = EffectProvider(effects.clone());
        let mut arena = DerivedArena::default();
        let derived = arena.push(DerivedOp::BifReturn {
            mfa: Mfa {
                module: "proof".into(),
                function: "effect".into(),
                arity: 1,
            },
            arguments: vec![Type::variable_integer(9.into())],
        });
        let atomic = AtomicConstraint {
            left: Type::variable_integer(1.into()),
            relation: Relation::Equal,
            right: ConstraintValue::Derived(derived),
        };
        let retained = BindingMap::from(vec![(key(9), Type::atom("argument"))]);
        let mut current = retained.clone();
        let mut log = vec![];
        let status = atomic
            .solve_access(&mut current, &arena, &provider, Some(&mut log))
            .unwrap();
        assert!(matches!(status, AtomicStatus::Failed));
        assert_eq!(current.get(&key(1)), Some(&Type::atom("written")));
        assert_eq!(log, vec![(key(1), None)]);
        assert_eq!(*effects.borrow(), vec![vec![Type::atom("argument")]]);
        assert_eq!(retained.get(&key(1)), None);
        assert!(matches!(
            atomic.solve(retained.clone(), &arena, &provider).unwrap(),
            AtomicOutcome::Failed
        ));
        assert_eq!(effects.borrow().len(), 2);
        assert_eq!(retained.get(&key(1)), None);
    }
}

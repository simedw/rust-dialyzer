use crate::constraints::atomic::*;
use crate::constraints::derived::{DerivedArena, NoBifProvider};
use crate::constraints::tree::*;
use crate::lattice::{Type, VariableId};
fn variable(id: i64) -> Type {
    Type::variable_integer(id.into())
}
fn add(tree: &mut ConstraintTree, left: Type, right: Type) -> ConstraintId {
    tree.push(Constraint::Atomic(AtomicConstraint {
        left,
        right: ConstraintValue::Type(right),
        relation: Relation::Equal,
    }))
    .unwrap()
}
fn group(tree: &mut ConstraintTree, nodes: Vec<ConstraintId>) -> ConstraintId {
    tree.push(Constraint::Conjunction(nodes)).unwrap()
}
#[test]
fn equality_refines_both_sides_and_reports_each_changed_variable() {
    let x = variable(1);
    let y = variable(2);
    let c = AtomicConstraint {
        left: Type::tuple(vec![x, Type::atom("a")]).unwrap(),
        right: ConstraintValue::Type(Type::tuple(vec![Type::integer(42.into()), y]).unwrap()),
        relation: Relation::Equal,
    };
    let AtomicOutcome::Solved { bindings, updated } = c
        .solve(BindingMap::new(), &DerivedArena::default(), &NoBifProvider)
        .unwrap()
    else {
        panic!("unexpected failed equality")
    };
    assert_eq!(
        bindings[&BindingKey::Label(VariableId::Integer(1.into()))],
        Type::integer(42.into())
    );
    assert_eq!(
        bindings[&BindingKey::Label(VariableId::Integer(2.into()))],
        Type::atom("a")
    );
    assert_eq!(updated.len(), 2);
}
#[test]
fn disjunction_does_not_constrain_variables_missing_from_any_live_branch() {
    let mut tree = ConstraintTree::default();
    let x = add(&mut tree, variable(1), Type::atom("a"));
    let y = add(&mut tree, variable(2), Type::integer(1.into()));
    let bx = group(&mut tree, vec![x]);
    let by = group(&mut tree, vec![y]);
    let root = tree.push(Constraint::Disjunction(vec![bx, by])).unwrap();
    let out = tree
        .finish(root)
        .unwrap()
        .solve(BindingMap::new(), &NoBifProvider)
        .unwrap();
    let AtomicOutcome::Solved { bindings, updated } = out.outcome else {
        panic!("disjunction failed")
    };
    assert!(bindings.is_empty());
    assert!(updated.is_empty());
}
#[test]
fn outer_constraint_invalidates_cached_disjunction_and_prunes_failed_branch() {
    let mut tree = ConstraintTree::default();
    let x1 = add(&mut tree, variable(1), Type::atom("a"));
    let y1 = add(&mut tree, variable(2), Type::atom("first"));
    let b1 = group(&mut tree, vec![x1, y1]);
    let x2 = add(&mut tree, variable(1), Type::atom("z"));
    let y2 = add(&mut tree, variable(2), Type::atom("second"));
    let b2 = group(&mut tree, vec![x2, y2]);
    let disj = tree.push(Constraint::Disjunction(vec![b1, b2])).unwrap();
    let xz = add(&mut tree, variable(1), Type::atom("z"));
    let flat = group(&mut tree, vec![xz]);
    let root = group(&mut tree, vec![disj, flat]);
    let out = tree
        .finish(root)
        .unwrap()
        .solve(BindingMap::new(), &NoBifProvider)
        .unwrap();
    let AtomicOutcome::Solved { bindings, .. } = out.outcome else {
        panic!("outer conjunction failed")
    };
    assert_eq!(
        bindings[&BindingKey::Label(VariableId::Integer(2.into()))],
        Type::atom("second")
    );
}
#[test]
fn prepared_boundary_rejects_scope_aliasing_and_mixed_native_enumeration() {
    let mut tree = ConstraintTree::default();
    let x = add(&mut tree, variable(1), Type::atom("a"));
    let scope = group(&mut tree, vec![x]);
    assert!(tree.push(Constraint::Conjunction(vec![x, scope])).is_err());
    let root = group(&mut tree, vec![scope, scope]);
    assert!(tree.finish(root).is_err());
}
#[test]
fn deeply_nested_scopes_execute_and_drop_iteratively() {
    let mut tree = ConstraintTree::default();
    let x = add(&mut tree, variable(1), Type::atom("a"));
    let mut root = group(&mut tree, vec![x]);
    for _ in 0..20000 {
        root = group(&mut tree, vec![root]);
    }
    let prepared = tree.finish(root).unwrap();
    let out = prepared.solve(BindingMap::new(), &NoBifProvider).unwrap();
    assert!(matches!(out.outcome, AtomicOutcome::Solved { .. }));
    drop(prepared);
}
#[test]
fn reference_failure_uses_propagated_domain_and_preserves_raw_recursive_key() {
    let mut tree = ConstraintTree::default();
    let impossible = add(&mut tree, Type::atom("a"), Type::integer(1.into()));
    let body = group(&mut tree, vec![impossible]);
    let function = FunctionVariable::new(VariableId::Integer(100.into())).unwrap();
    tree.define_function(
        function.clone(),
        FunctionDefinition {
            body,
            arity: 1,
            recursive_variable: Some(VariableId::Integer(77.into())),
            propagated_domain: Some(vec![Type::atom("argument")]),
            self_recursive: false,
        },
    )
    .unwrap();
    let root = tree
        .push(Constraint::Reference {
            function,
            dependencies: Default::default(),
        })
        .unwrap();
    let AtomicOutcome::Solved { bindings, updated } = tree
        .finish(root)
        .unwrap()
        .solve(BindingMap::new(), &NoBifProvider)
        .unwrap()
        .outcome
    else {
        panic!("reference must succeed")
    };
    let expected = Type::function(Some(vec![Type::atom("argument")]), Type::none());
    let raw = BindingKey::RawVariableRecord(VariableId::Integer(77.into()));
    assert_eq!(bindings[&raw], expected);
    assert!(updated.contains(&raw));
    assert!(!bindings.contains_key(&BindingKey::Label(VariableId::Integer(77.into()))));
}
#[test]
fn self_recursion_retries_unit_and_keeps_integer_and_raw_variable_keys() {
    let mut tree = ConstraintTree::default();
    let prototype = Type::function(Some(vec![variable(1)]), variable(2));
    let inferred = add(&mut tree, variable(100), prototype.clone());
    let recurrence = add(&mut tree, variable(77), prototype);
    let body = group(&mut tree, vec![inferred, recurrence]);
    let function = FunctionVariable::new(VariableId::Integer(100.into())).unwrap();
    tree.define_function(
        function.clone(),
        FunctionDefinition {
            body,
            arity: 1,
            recursive_variable: Some(VariableId::Integer(77.into())),
            propagated_domain: None,
            self_recursive: true,
        },
    )
    .unwrap();
    let root = tree
        .push(Constraint::Reference {
            function,
            dependencies: [1, 2, 77, 100]
                .into_iter()
                .map(|v| VariableId::Integer(v.into()))
                .collect(),
        })
        .unwrap();
    let AtomicOutcome::Solved { bindings, updated } = tree
        .finish(root)
        .unwrap()
        .solve(
            [(
                BindingKey::Label(VariableId::Integer(77.into())),
                Type::any(),
            )]
            .into_iter()
            .collect(),
            &NoBifProvider,
        )
        .unwrap()
        .outcome
    else {
        panic!("unit retry must succeed")
    };
    let expected = Type::function(Some(vec![Type::any()]), Type::unit());
    let ordinary = BindingKey::Label(VariableId::Integer(77.into()));
    let raw = BindingKey::RawVariableRecord(VariableId::Integer(77.into()));
    assert_eq!(bindings[&ordinary], Type::any());
    assert_eq!(bindings[&raw], expected);
    assert!(!updated.contains(&ordinary));
    assert!(updated.contains(&raw));
    assert_ne!(ordinary, raw);
}
#[test]
fn mutual_recursion_uses_the_native_unit_second_pass() {
    let mut tree = ConstraintTree::default();
    let mut refs = Vec::new();
    let mut functions = Vec::new();
    for (fid, x, ret, rec, other) in [(100, 1, 2, 77, 88), (200, 3, 4, 88, 77)] {
        let prototype = Type::function(Some(vec![variable(x)]), variable(ret));
        let declaration = add(&mut tree, variable(fid), prototype.clone());
        let call = add(&mut tree, variable(other), prototype);
        let body = group(&mut tree, vec![declaration, call]);
        let function = FunctionVariable::new(VariableId::Integer(fid.into())).unwrap();
        tree.define_function(
            function.clone(),
            FunctionDefinition {
                body,
                arity: 1,
                recursive_variable: Some(VariableId::Integer(rec.into())),
                propagated_domain: None,
                self_recursive: false,
            },
        )
        .unwrap();
        refs.push(
            tree.push(Constraint::Reference {
                function: function.clone(),
                dependencies: [fid, x, ret, other]
                    .into_iter()
                    .map(|v| VariableId::Integer(v.into()))
                    .collect(),
            })
            .unwrap(),
        );
        functions.push(function);
    }
    let root = group(&mut tree, refs);
    let result = tree
        .finish(root)
        .unwrap()
        .prepare_scc(functions)
        .unwrap()
        .solve(BindingMap::new(), &NoBifProvider)
        .unwrap();
    assert!(result.tried_unit);
    assert!(result.iterations >= 2);
    let expected = Type::function(Some(vec![Type::any()]), Type::unit());
    for n in [77, 88, 100, 200] {
        assert_eq!(
            result.bindings[&BindingKey::Label(VariableId::Integer(n.into()))],
            expected
        );
    }
    assert!(result.bindings.keys().all(|k| k.label().is_some()));
}
#[test]
fn conflicting_raw_recursive_binding_preserves_native_reference_assertion_error() {
    let mut tree = ConstraintTree::default();
    let prototype = add(
        &mut tree,
        variable(100),
        Type::function(Some(vec![variable(1)]), variable(2)),
    );
    let body = group(&mut tree, vec![prototype]);
    let function = FunctionVariable::new(VariableId::Integer(100.into())).unwrap();
    tree.define_function(
        function.clone(),
        FunctionDefinition {
            body,
            arity: 1,
            recursive_variable: Some(VariableId::Integer(77.into())),
            propagated_domain: None,
            self_recursive: false,
        },
    )
    .unwrap();
    let root = tree
        .push(Constraint::Reference {
            function,
            dependencies: [1, 2, 100]
                .into_iter()
                .map(|n| VariableId::Integer(n.into()))
                .collect(),
        })
        .unwrap();
    let map = [(
        BindingKey::RawVariableRecord(VariableId::Integer(77.into())),
        Type::atom("not_callable"),
    )]
    .into_iter()
    .collect();
    let error = tree
        .finish(root)
        .unwrap()
        .solve(map, &NoBifProvider)
        .unwrap_err();
    assert!(
        matches!(error,SolveError::Native(crate::lattice::LatticeError::BadMatchNative(payload)) if *payload==crate::lattice::NativeValue::Atom("error".into()))
    );
}

#[test]
fn dynamic_rhs_equality_fails_as_native_raw_fun_var_unification() {
    let mut arena = DerivedArena::default();
    let id = arena.push(crate::constraints::derived::DerivedOp::PredicateArgument {
        destination: Type::any(),
        test: Type::all_integers(),
    });
    let c = AtomicConstraint {
        left: variable(1),
        right: ConstraintValue::Derived(id),
        relation: Relation::Equal,
    };
    assert!(matches!(
        c.solve(BindingMap::new(), &arena, &NoBifProvider).unwrap(),
        AtomicOutcome::Failed
    ));
}
#[test]
fn exact_equality_success_constraint_retains_boolean_for_same_singleton() {
    let op = crate::constraints::derived::DerivedOp::ExactEqualityReturn {
        left: Type::atom("same"),
        right: Type::atom("same"),
    };
    let ty = op.evaluate(&BindingMap::new(), &NoBifProvider).unwrap();
    assert_eq!(ty, Type::atoms(vec!["true", "false"]));
}

#[test]
fn binary_segment_mode_keeps_native_construct_and_match_period_distinct() {
    use crate::constraints::derived::{BinaryMode, DerivedOp, SegmentUnit};
    let unit = SegmentUnit::new(8).unwrap();
    let run = |mode| {
        DerivedOp::BinaryLength {
            size: Type::integer(4.into()),
            unit,
            mode,
        }
        .evaluate(&BindingMap::new(), &NoBifProvider)
        .unwrap()
    };
    assert_eq!(
        run(BinaryMode::Construct),
        Type::bitstring(0.into(), 32.into()).unwrap()
    );
    assert_eq!(
        run(BinaryMode::Match),
        Type::bitstring(1.into(), 32.into()).unwrap()
    );
    assert!(SegmentUnit::new(0).is_err());
    assert!(SegmentUnit::new(257).is_err());
}

#[test]
fn potentially_shadowing_map_key_does_not_constrain_earlier_value() {
    use crate::constraints::derived::DerivedOp;
    let map = Type::map_any()
        .map_put(Type::atom("key"), Type::integer(1.into()))
        .unwrap();
    let constrained = DerivedOp::MapValue {
        key: Type::atom("key"),
        map: map.clone(),
        shadow_keys: vec![],
    }
    .evaluate(&BindingMap::new(), &NoBifProvider)
    .unwrap();
    assert_eq!(constrained, Type::integer(1.into()));
    let shadowed = DerivedOp::MapValue {
        key: Type::atom("key"),
        map,
        shadow_keys: vec![Type::any()],
    }
    .evaluate(&BindingMap::new(), &NoBifProvider)
    .unwrap();
    assert_eq!(shadowed, Type::any());
}

#[test]
fn native_closure_order_uses_new_index_and_ignores_old_index_and_local_arity() {
    use crate::constraints::closure_order::ClosureOrderKey;
    use serde_json::json;
    let raw = json!({"kind":"local","module":"dialyzer_typesig","name":"site","new_index":1,"index":999,"uniq":23,"arity":1,"env_count":0,"env_terms":{"root":0,"terms":[{"kind":"nil"}]}});
    let a = ClosureOrderKey::parse(&raw).unwrap();
    let mut other = raw.clone();
    other["index"] = json!(0);
    other["arity"] = json!(2);
    assert_eq!(
        a.compare(&ClosureOrderKey::parse(&other).unwrap()).unwrap(),
        std::cmp::Ordering::Equal
    );
    other["new_index"] = json!(2);
    assert_eq!(
        a.compare(&ClosureOrderKey::parse(&other).unwrap()).unwrap(),
        std::cmp::Ordering::Less
    );
}

#[test]
fn native_closure_profile_checks_capture_shape_and_site_identity_once() {
    use crate::constraints::closure_order::ClosureCodeProfile;
    use serde_json::json;
    let raw = json!({"module":"dialyzer_typesig","original_sha256":"0000000000000000000000000000000000000000000000000000000000000000","fun_table":[{"name":"site","new_index":0,"uniq":23,"arity":2,"env_count":1}]});
    let profile = ClosureCodeProfile::parse(&raw).unwrap();
    let nil = crate::core::ErlangTerms::parse(&json!({"root":0,"terms":[{"kind":"nil"}]})).unwrap();
    let nil = crate::term_order::CaptureEnvironment::literals(&nil).unwrap();
    assert!(profile.instantiate("site", nil.clone()).is_err());
    assert!(profile.instantiate("missing", nil).is_err());
    let mut bad = raw;
    bad["fun_table"][0]["new_index"] = json!(1);
    assert!(ClosureCodeProfile::parse(&bad).is_err());
}

#[test]
fn function_arity_constraint_validates_only_reached_true_branch() {
    use crate::constraints::derived::DerivedOp;
    let false_branch = DerivedOp::FunctionArityArgument {
        destination: Type::atom("false"),
        arity: Type::integer((-1).into()),
    };
    assert_eq!(
        false_branch
            .evaluate(&BindingMap::new(), &NoBifProvider)
            .unwrap(),
        Type::any()
    );
    let true_branch = DerivedOp::FunctionArityArgument {
        destination: Type::atom("true"),
        arity: Type::integer(256.into()),
    };
    assert!(matches!(
        true_branch.evaluate(&BindingMap::new(), &NoBifProvider),
        Err(crate::constraints::derived::EvalError::Native(
            crate::lattice::LatticeError::FunctionClause
        ))
    ));
}
#[test]
fn clause_residual_preserves_native_occurrence_dependencies() {
    use crate::constraints::derived::DerivedOp;
    let mut arena = DerivedArena::default();
    let id = arena.push(DerivedOp::ClauseResidual {
        argument: variable(1),
        subtract: vec![variable(42)],
    });
    assert_eq!(
        arena.dependencies(id).unwrap(),
        &std::collections::BTreeSet::from([VariableId::Integer(1.into())])
    );
    let residual = DerivedOp::ClauseResidual {
        argument: Type::atoms(["a", "b"]),
        subtract: vec![Type::atom("a")],
    };
    assert_eq!(
        residual
            .evaluate(&BindingMap::new(), &NoBifProvider)
            .unwrap(),
        Type::atom("b")
    );
}

#[test]
fn shared_contract_snapshot_refines_actual_arguments_and_keeps_plt_return_separate() {
    use crate::constraints::derived::DerivedOp;
    use crate::contract_runtime::ContractSummary;
    let summary = std::sync::Arc::new(
        ContractSummary::from_parts(vec![
            (
                Type::function(Some(vec![Type::atom("a")]), Type::atom("yes")),
                vec![],
            ),
            (
                Type::function(Some(vec![Type::atom("b")]), Type::atom("no")),
                vec![],
            ),
        ])
        .unwrap(),
    );
    let call = DerivedOp::ContractReturn {
        summary: summary.clone(),
        arguments: vec![Type::atom("a")],
        signature_return: Some(Type::atom("no")),
    };
    assert_eq!(
        call.evaluate(&BindingMap::new(), &NoBifProvider).unwrap(),
        Type::none()
    );
    let without_signature = DerivedOp::ContractReturn {
        summary,
        arguments: vec![Type::atom("a")],
        signature_return: None,
    };
    assert_eq!(
        without_signature
            .evaluate(&BindingMap::new(), &NoBifProvider)
            .unwrap(),
        Type::atom("yes")
    );
}

#[test]
fn disjunction_joins_evaluated_maps_in_native_reverse_order() {
    let nominal = Type::from_value(&serde_json::json!({
        "kind":"nominal", "name":{"module":"opaque_digraph","name":"vertex","arity":0,"opacity":"opaque"},
        "inner":{"kind":"any"}
    })).unwrap();
    for reverse in [false, true] {
        let mut tree = ConstraintTree::default();
        let mut branches = Vec::new();
        for value in [Type::any(), nominal.clone()] {
            let node = add(
                &mut tree,
                Type::tuple(vec![variable(1), variable(2)]).unwrap(),
                Type::tuple(vec![
                    value,
                    Type::from_value(&serde_json::json!({"kind":"integer"})).unwrap(),
                ])
                .unwrap(),
            );
            branches.push(group(&mut tree, vec![node]));
        }
        if reverse {
            branches.reverse();
        }
        let root = tree.push(Constraint::Disjunction(branches)).unwrap();
        let out = tree
            .finish(root)
            .unwrap()
            .solve(BindingMap::new(), &NoBifProvider)
            .unwrap();
        let AtomicOutcome::Solved { bindings, .. } = out.outcome else {
            panic!("disjunction failed")
        };
        let key = BindingKey::Label(VariableId::Integer(1.into()));
        if reverse {
            assert!(!bindings.contains_key(&key));
        } else {
            assert_eq!(bindings[&key], nominal);
        }
    }
}

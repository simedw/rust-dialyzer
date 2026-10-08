use rust_dialyzer_poc::{
    bifs::registry::{Registry, TransferError},
    constraints::derived::*,
};
use rust_dialyzer_poc::{
    constraints::atomic::*,
    constraints::tree::{
        Constraint, ConstraintId, ConstraintTree, FunctionDefinition, FunctionVariable,
    },
    lattice::{LatticeError, Type, VariableId},
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn variable(v: &Value) -> Result<VariableId, String> {
    if let Some(a) = v["atom"].as_str() {
        Ok(VariableId::Atom(a.into()))
    } else {
        let n = v["integer"]
            .as_str()
            .ok_or("integer variable identifier expected")?
            .parse()
            .map_err(|e| format!("{e}"))?;
        Ok(VariableId::Integer(n))
    }
}
fn id(v: &BindingKey) -> Value {
    if let BindingKey::RawVariableRecord(v) = v {
        return json!({"raw_variable_record":id(&BindingKey::Label(v.clone()))});
    }
    let BindingKey::Label(v) = v else {
        unreachable!()
    };
    match v {
        VariableId::Atom(a) => json!({"atom":a}),
        VariableId::Integer(n) => json!({"integer":n.to_string()}),
    }
}
fn native(e: LatticeError) -> Value {
    let class = match &e {
        LatticeError::FunctionClause | LatticeError::OpaqueSentinelSubtraction => "function_clause",
        LatticeError::BadArg => "badarg",
        LatticeError::CaseClause(_) => "case_clause",
        LatticeError::BadMatch(_)
        | LatticeError::BadMatchBoolean(_)
        | LatticeError::BadMatchNative(_) => "badmatch",
        LatticeError::BadGenerators(_) => "bad_generators",
        LatticeError::BadGenerator(_) => "bad_generator",
        LatticeError::NativeNonTerminating => "native_nonterminating",
    };
    let payload = match &e {
        LatticeError::CaseClause(t) | LatticeError::BadMatch(t) | LatticeError::BadGenerator(t) => {
            t.to_value()
        }
        LatticeError::BadMatchBoolean(v) => json!(v),
        LatticeError::BadMatchNative(v) => v.to_value(),
        LatticeError::BadGenerators(t) => json!([
            t.0.iter().map(Type::to_value).collect::<Vec<_>>(),
            t.1.iter().map(Type::to_value).collect::<Vec<_>>()
        ]),
        _ => Value::Null,
    };
    json!({"status":"native_error","class":class,"payload":payload,"detail":e.to_string()})
}
fn parse_atomic(v: &Value) -> Result<AtomicConstraint, String> {
    let c = AtomicConstraint {
        left: Type::from_value(&v["left"])?,
        right: if let Some(id) = v["right"].get("derived") {
            ConstraintValue::Derived(DerivedId(
                id.as_u64().ok_or("derived index expected")? as usize
            ))
        } else {
            ConstraintValue::Type(Type::from_value(&v["right"])?)
        },
        relation: match v["relation"].as_str() {
            Some("eq") => Relation::Equal,
            Some("sub") => Relation::Subtype,
            _ => return Err("relation must be eq or sub".into()),
        },
    };
    Ok(c)
}
fn parse_derived(d: &Value) -> Result<DerivedOp, String> {
    let ty = |field: &str| Type::from_value(&d[field]);
    let op = match d["operation"].as_str() {
        Some("contract_return") => {
            let alternatives = d["contract"]["alternatives"]
                .as_array()
                .ok_or("contract alternatives required")?
                .iter()
                .map(|a| {
                    let signature = Type::from_value(&a["signature"])?;
                    let pairs = a["constraints"]
                        .as_array()
                        .ok_or("constraint pairs required")?
                        .iter()
                        .map(|p| {
                            let pair = p.as_array().ok_or("pair required")?;
                            if pair.len() != 2 {
                                return Err("constraint pair length must be two".into());
                            }
                            Ok((Type::from_value(&pair[0])?, Type::from_value(&pair[1])?))
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    Ok((signature, pairs))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let summary =
                rust_dialyzer_poc::contract_runtime::ContractSummary::from_parts(alternatives)
                    .map_err(|e| e.to_string())?;
            DerivedOp::ContractReturn {
                summary: std::sync::Arc::new(summary),
                arguments: d["arguments"]
                    .as_array()
                    .ok_or("actual argument variables required")?
                    .iter()
                    .map(Type::from_value)
                    .collect::<Result<_, _>>()?,
                signature_return: if d["signature_return"].is_null() {
                    None
                } else {
                    Some(ty("signature_return")?)
                },
            }
        }
        Some("function_arity_argument") => DerivedOp::FunctionArityArgument {
            destination: ty("destination")?,
            arity: ty("arity")?,
        },
        Some("clause_residual") => DerivedOp::ClauseResidual {
            argument: ty("argument")?,
            subtract: d["subtract"]
                .as_array()
                .ok_or("subtract type list expected")?
                .iter()
                .map(Type::from_value)
                .collect::<Result<_, _>>()?,
        },
        Some("binary_segments") => DerivedOp::BinarySegments {
            segments: d["segments"]
                .as_array()
                .ok_or("segments array expected")?
                .iter()
                .map(Type::from_value)
                .collect::<Result<_, _>>()?,
        },
        Some("append_head") => DerivedOp::AppendHead {
            destination: ty("destination")?,
        },
        Some("append_tail") => DerivedOp::AppendTail {
            destination: ty("destination")?,
        },
        Some("map_value") => DerivedOp::MapValue {
            key: ty("key")?,
            map: ty("map")?,
            shadow_keys: d["shadow_keys"]
                .as_array()
                .ok_or("shadow keys expected")?
                .iter()
                .map(Type::from_value)
                .collect::<Result<_, _>>()?,
        },
        Some("map_construct") | Some("map_argument") => {
            let entries = d["entries"]
                .as_array()
                .ok_or("map entries expected")?
                .iter()
                .map(|p| {
                    Ok(rust_dialyzer_poc::constraints::derived::MapAssociation {
                        key: Type::from_value(&p["key"])?,
                        value: Type::from_value(&p["value"])?,
                        operation: match p["operation"].as_str() {
                            Some("associate") => {
                                rust_dialyzer_poc::constraints::derived::MapOperation::Associate
                            }
                            Some("exact") => {
                                rust_dialyzer_poc::constraints::derived::MapOperation::Exact
                            }
                            _ => return Err("map operation expected".to_string()),
                        },
                    })
                })
                .collect::<Result<_, String>>()?;
            if d["operation"] == "map_construct" {
                DerivedOp::MapConstruct {
                    argument: ty("argument")?,
                    entries,
                }
            } else {
                DerivedOp::MapArgument { entries }
            }
        }
        Some("binary_length") | Some("binary_value") => {
            let unit = rust_dialyzer_poc::constraints::derived::SegmentUnit::new(
                u16::try_from(d["unit"].as_u64().ok_or("unit expected")?)
                    .map_err(|_| "unit too large")?,
            )?;
            if d["operation"] == "binary_value" {
                DerivedOp::BinaryValue {
                    size: ty("size")?,
                    unit,
                    signed: d["signed"].as_bool().ok_or("signed expected")?,
                }
            } else {
                DerivedOp::BinaryLength {
                    size: ty("size")?,
                    unit,
                    mode: match d["mode"].as_str() {
                        Some("construct") => {
                            rust_dialyzer_poc::constraints::derived::BinaryMode::Construct
                        }
                        Some("match") => rust_dialyzer_poc::constraints::derived::BinaryMode::Match,
                        _ => return Err("binary mode expected".into()),
                    },
                }
            }
        }
        Some("cons") => DerivedOp::Cons {
            head: ty("head")?,
            tail: ty("tail")?,
        },
        Some("cons_head") => DerivedOp::ConsHead {
            value: ty("value")?,
        },
        Some("cons_tail") => DerivedOp::ConsTail {
            value: ty("value")?,
        },
        Some("binary_concat") => DerivedOp::BinaryConcat {
            left: ty("left")?,
            right: ty("right")?,
        },
        Some("binary_match") => DerivedOp::BinaryMatch {
            left: ty("left")?,
            right: ty("right")?,
        },
        Some("boolean_argument") => DerivedOp::BooleanArgument {
            operation: match d["boolean"].as_str() {
                Some("and") => BooleanOperation::And,
                Some("or") => BooleanOperation::Or,
                _ => return Err("boolean operation expected".into()),
            },
            destination: ty("destination")?,
            other: ty("other")?,
        },
        Some("boolean_return") => DerivedOp::BooleanReturn {
            operation: match d["boolean"].as_str() {
                Some("and") => BooleanOperation::And,
                Some("or") => BooleanOperation::Or,
                _ => return Err("boolean operation expected".into()),
            },
            left: ty("left")?,
            right: ty("right")?,
        },
        Some("not") => DerivedOp::BooleanNot {
            value: ty("value")?,
        },
        Some("equality_argument") => DerivedOp::EqualityArgument {
            kind: match d["equality"].as_str() {
                Some("=:=") => EqualityKind::Exact,
                Some("==") => EqualityKind::Coercing,
                _ => return Err("equality operation expected".into()),
            },
            destination: ty("destination")?,
            argument: ty("argument")?,
            other: ty("other")?,
        },
        Some("exact_equality_return") => DerivedOp::ExactEqualityReturn {
            left: ty("left")?,
            right: ty("right")?,
        },
        Some("bif") => DerivedOp::BifReturn {
            mfa: serde_json::from_value(d["mfa"].clone()).map_err(|e| e.to_string())?,
            arguments: d["arguments"]
                .as_array()
                .ok_or("derived arguments expected")?
                .iter()
                .map(Type::from_value)
                .collect::<Result<_, _>>()?,
        },
        Some("inverse") => DerivedOp::InverseArithmetic {
            operation: match d["arithmetic"].as_str() {
                Some("+") => Arithmetic::Add,
                Some("-") => Arithmetic::Subtract,
                Some("*") => Arithmetic::Multiply,
                _ => return Err("inverse arithmetic operation expected".into()),
            },
            known_position: match d["known_position"].as_u64() {
                Some(1) => KnownArgumentPosition::First,
                Some(2) => KnownArgumentPosition::Second,
                _ => return Err("known position must be 1 or 2".into()),
            },
            destination: ty("destination")?,
            other: ty("other")?,
        },
        Some("ordering") => DerivedOp::OrderingArgument {
            operation: match d["ordering"].as_str() {
                Some("<") => OrderingOperation::Less,
                Some("=<") => OrderingOperation::LessEqual,
                Some(">") => OrderingOperation::Greater,
                Some(">=") => OrderingOperation::GreaterEqual,
                _ => return Err("ordering operation expected".into()),
            },
            destination: ty("destination")?,
            argument: ty("argument")?,
            other: ty("other")?,
        },
        Some("predicate_argument") => DerivedOp::PredicateArgument {
            destination: ty("destination")?,
            test: ty("test")?,
        },
        Some("predicate_return") => DerivedOp::PredicateReturn {
            argument: ty("argument")?,
            test: ty("test")?,
        },
        _ => return Err("unsupported derived operation".into()),
    };

    Ok(op)
}
fn registry() -> Result<&'static Registry, String> {
    static REGISTRY: std::sync::OnceLock<Result<Registry, String>> = std::sync::OnceLock::new();
    REGISTRY
        .get_or_init(|| Registry::load(include_str!("../../corpus/bifs.json")))
        .as_ref()
        .map_err(Clone::clone)
}
fn run(v: &Value) -> Result<Value, String> {
    if v["capture_compare"].is_object() {
        use rust_dialyzer_poc::term_order::parse_captures;
        let c = &v["capture_compare"];
        let (a, ai) = parse_captures(&c["a"])?;
        let (b, bi) = parse_captures(&c["b"])?;
        let order = if c["exact"].as_bool().unwrap_or(false) {
            a.compare_capture_keys(ai, &b, bi)?
        } else {
            a.compare_captures(ai, &b, bi)?
        };
        return Ok(
            json!({"order":match order {std::cmp::Ordering::Less=>-1,std::cmp::Ordering::Equal=>0,std::cmp::Ordering::Greater=>1}}),
        );
    }

    if v["normalize"].is_object() {
        use rust_dialyzer_poc::constraints::normalize::*;
        let result = (|| -> Result<Value, NormalizationError> {
            let mut arena = NativeConstraintArena::default();
            let mut ids = Vec::new();
            let desc = &v["normalize"];
            let nodes = desc["nodes"]
                .as_array()
                .ok_or_else(|| NormalizationError::Boundary("nodes expected".into()))?;
            for node in nodes {
                let id = match node["kind"].as_str() {
                    Some("atomic") => {
                        let parsed = parse_atomic(node).map_err(NormalizationError::Boundary)?;
                        let ConstraintValue::Type(right) = parsed.right else {
                            return Err(NormalizationError::Boundary(
                                "ordered derived operand support pending in probe".into(),
                            ));
                        };
                        arena.atomic(
                            parsed.left,
                            parsed.relation,
                            NativeOperand::Type(right),
                            &NoBifProvider,
                        )?
                    }
                    Some("ref") => {
                        let function = FunctionVariable::new(
                            variable(&node["function"]).map_err(NormalizationError::Boundary)?,
                        )
                        .map_err(NormalizationError::Boundary)?;
                        let deps = node["deps"]
                            .as_array()
                            .ok_or_else(|| NormalizationError::Boundary("deps expected".into()))?
                            .iter()
                            .map(variable)
                            .collect::<Result<_, _>>()
                            .map_err(NormalizationError::Boundary)?;
                        arena.reference(function, deps)
                    }
                    Some("conj") | Some("disj") => {
                        let children = node["children"]
                            .as_array()
                            .ok_or_else(|| {
                                NormalizationError::Boundary("children expected".into())
                            })?
                            .iter()
                            .map(|x| {
                                let idx = x.as_u64().ok_or_else(|| {
                                    NormalizationError::Boundary("child index expected".into())
                                })? as usize;
                                ids.get(idx).copied().ok_or_else(|| {
                                    NormalizationError::Boundary("child must precede parent".into())
                                })
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        arena.group(
                            if node["kind"] == "conj" {
                                GroupKind::Conjunction
                            } else {
                                GroupKind::Disjunction
                            },
                            children,
                        )?
                    }
                    _ => {
                        return Err(NormalizationError::Boundary(
                            "unknown native constructor".into(),
                        ));
                    }
                };
                ids.push(id);
            }
            let index = desc["root"]
                .as_u64()
                .ok_or_else(|| NormalizationError::Boundary("root expected".into()))?
                as usize;
            let root = ids
                .get(index)
                .copied()
                .ok_or_else(|| NormalizationError::Boundary("foreign root".into()))?;
            let root = if desc["dnf"] == true {
                arena.disjunctive_normal_form(root)?
            } else {
                root
            };
            if let Some(functions) = desc["functions"].as_array() {
                use rust_dialyzer_poc::constraints::preparation::{
                    FunctionBody, PreparedConstruction,
                };
                let functions = functions
                    .iter()
                    .map(|f| {
                        Ok(FunctionBody {
                            function: FunctionVariable::new(
                                variable(&f["function"]).map_err(NormalizationError::Boundary)?,
                            )
                            .map_err(NormalizationError::Boundary)?,
                            body: *ids
                                .get(f["body"].as_u64().ok_or_else(|| {
                                    NormalizationError::Boundary(
                                        "function body index expected".into(),
                                    )
                                })? as usize)
                                .ok_or_else(|| {
                                    NormalizationError::Boundary("foreign function body".into())
                                })?,
                        })
                    })
                    .collect::<Result<Vec<_>, NormalizationError>>()?;
                let prepared = PreparedConstruction::build(
                    &mut arena,
                    &functions,
                    if desc["solvers"].as_array().is_some_and(|solvers| {
                        solvers.iter().any(|solver| solver.as_str() == Some("v1"))
                    }) {
                        rust_dialyzer_poc::constraints::preparation::DependencyRetention::Retain
                    } else {
                        rust_dialyzer_poc::constraints::preparation::DependencyRetention::Clear
                    },
                )
                .map_err(NormalizationError::Boundary)?;
                if desc["solve"] == true {
                    use rust_dialyzer_poc::constraints::preparation::FunctionParameters;
                    let params = desc["functions"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|f| {
                            Ok((
                                FunctionVariable::new(
                                    variable(&f["function"])
                                        .map_err(NormalizationError::Boundary)?,
                                )
                                .map_err(NormalizationError::Boundary)?,
                                FunctionParameters {
                                    arity: f["arity"].as_u64().ok_or_else(|| {
                                        NormalizationError::Boundary("arity required".into())
                                    })? as usize,
                                    recursive_variable: None,
                                    propagated_domain: None,
                                    self_recursive: false,
                                },
                            ))
                        })
                        .collect::<Result<std::collections::BTreeMap<_, _>, NormalizationError>>(
                        )?;
                    let entry = functions[0].function.clone();
                    let tree = prepared
                        .into_tree(arena, params, vec![entry])
                        .map_err(NormalizationError::Boundary)?;
                    let result = tree.solve(BindingMap::new(), &NoBifProvider)?.outcome;
                    return Ok(match result {
                        AtomicOutcome::Failed => json!({"status":"failed"}),
                        AtomicOutcome::Solved { bindings, updated } => {
                            json!({"status":"solved","bindings":bindings.iter().map(|(v,t)|json!({"id":id(v),"type":t.to_value()})).collect::<Vec<_>>(),"updated":updated.iter().map(id).collect::<Vec<_>>()})
                        }
                    });
                }
                return Ok(
                    json!({"status":"prepared","graph":prepared.to_value(&arena).map_err(NormalizationError::Boundary)?}),
                );
            }
            Ok(json!({"status":"normalized","graph":arena.to_value(root)?}))
        })();
        return match result {
            Ok(v) => Ok(v),
            Err(NormalizationError::Unsatisfiable) => Ok(json!({"status":"throw_error"})),
            Err(NormalizationError::Lattice(e))
            | Err(NormalizationError::Solve(SolveError::Native(e))) => Ok(native(e)),
            Err(e) => Err(format!("{e:?}")),
        };
    }
    if v["closure_profile"].is_object() {
        let profile = rust_dialyzer_poc::constraints::closure_order::ClosureCodeProfile::parse(
            &v["closure_profile"],
        )?;
        let env = rust_dialyzer_poc::core::ErlangTerms::parse(&v["environment"])?;
        let key = profile.instantiate(
            v["site"].as_str().ok_or("closure site expected")?,
            rust_dialyzer_poc::term_order::CaptureEnvironment::literals(&env)?,
        )?;
        let expected =
            rust_dialyzer_poc::constraints::closure_order::ClosureOrderKey::parse(&v["expected"])?;
        return Ok(
            json!({"order":match key.compare(&expected)?{std::cmp::Ordering::Less=>-1,std::cmp::Ordering::Equal=>0,std::cmp::Ordering::Greater=>1}}),
        );
    }
    if v["closure_compare"].is_object() {
        let a = rust_dialyzer_poc::constraints::closure_order::ClosureOrderKey::parse(
            &v["closure_compare"]["a"],
        )?;
        let b = rust_dialyzer_poc::constraints::closure_order::ClosureOrderKey::parse(
            &v["closure_compare"]["b"],
        )?;
        return Ok(
            json!({"order":match a.compare(&b)?{std::cmp::Ordering::Less=>-1,std::cmp::Ordering::Equal=>0,std::cmp::Ordering::Greater=>1}}),
        );
    }

    let bindings: BindingMap = v["bindings"]
        .as_array()
        .ok_or("bindings array expected")?
        .iter()
        .map(|e| {
            Ok((
                if e["id"]["raw_variable_record"].is_null() {
                    BindingKey::Label(variable(&e["id"])?)
                } else {
                    BindingKey::RawVariableRecord(variable(&e["id"]["raw_variable_record"])?)
                },
                Type::from_value(&e["type"])?,
            ))
        })
        .collect::<Result<_, String>>()?;
    if let Some(d) = v.get("derived") {
        let op = parse_derived(d)?;
        let registry = registry()?;
        return Ok(match op.evaluate(&bindings, registry) {
            Ok(ty) => json!({"status":"type","type":ty.to_value()}),
            Err(EvalError::Native(e)) => native(e),
            Err(EvalError::Transfer(TransferError::NativeException(e))) => {
                json!({"status":"native_transfer_error","error":e.report()})
            }
            Err(e) => json!({"status":"evaluation_error","detail":format!("{e:?}")}),
        });
    }
    let provider: &dyn BifResultProvider = if v.get("derived_nodes").is_some() {
        registry()?
    } else {
        &NoBifProvider
    };
    let solved = if let Some(nodes) = v["tree"].as_array() {
        let mut tree = ConstraintTree::default();
        if let Some(ops) = v.get("derived_nodes") {
            for op in ops.as_array().ok_or("derived nodes array expected")? {
                tree.push_derived(parse_derived(op)?);
            }
        }
        for node in nodes {
            let constraint = match node["kind"].as_str() {
                Some("atomic") => Constraint::Atomic(parse_atomic(node)?),
                Some("reference") => Constraint::Reference {
                    function: FunctionVariable::new(variable(&node["function"])?)?,
                    dependencies: node["dependencies"]
                        .as_array()
                        .ok_or("reference dependencies expected")?
                        .iter()
                        .map(variable)
                        .collect::<Result<_, _>>()?,
                },
                Some(kind @ ("conj" | "disj")) => {
                    let children = node["children"]
                        .as_array()
                        .ok_or("children array expected")?
                        .iter()
                        .map(|i| {
                            i.as_u64()
                                .map(|i| ConstraintId(i as usize))
                                .ok_or("child integer expected")
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    if kind == "conj" {
                        Constraint::Conjunction(children)
                    } else {
                        Constraint::Disjunction(children)
                    }
                }
                _ => return Err("unsupported constraint node".into()),
            };
            tree.push(constraint)?;
        }
        if let Some(functions) = v["functions"].as_array() {
            for function in functions {
                tree.define_function(
                    FunctionVariable::new(variable(&function["id"])?)?,
                    FunctionDefinition {
                        body: ConstraintId(
                            function["body"]
                                .as_u64()
                                .ok_or("function body index expected")?
                                as usize,
                        ),
                        arity: function["arity"]
                            .as_u64()
                            .ok_or("function arity expected")?
                            as usize,
                        recursive_variable: if function["recursive_variable"].is_null() {
                            None
                        } else {
                            Some(variable(&function["recursive_variable"])?)
                        },
                        propagated_domain: if function["propagated_domain"].is_null() {
                            None
                        } else {
                            Some(
                                function["propagated_domain"]
                                    .as_array()
                                    .ok_or("propagated domain expected")?
                                    .iter()
                                    .map(Type::from_value)
                                    .collect::<Result<_, _>>()?,
                            )
                        },
                        self_recursive: function["self_recursive"].as_bool().unwrap_or(false),
                    },
                )?;
            }
        }
        let root = v["root"].as_u64().ok_or("root index expected")? as usize;
        let prepared = tree.finish(ConstraintId(root))?;
        if let Some(scc) = v["scc"].as_array() {
            let functions = scc
                .iter()
                .map(|v| FunctionVariable::new(variable(v)?))
                .collect::<Result<_, String>>()?;
            prepared
                .prepare_scc(functions)?
                .solve(bindings, provider)
                .map(|v| AtomicOutcome::Solved {
                    bindings: v.bindings,
                    updated: Default::default(),
                })
        } else {
            prepared.solve(bindings, provider).map(|v| v.outcome)
        }
    } else {
        parse_atomic(v)?.solve(bindings, &DerivedArena::default(), provider)
    };
    Ok(match solved {
        Ok(AtomicOutcome::Failed) => json!({"status":"failed"}),
        Ok(AtomicOutcome::Solved { bindings, updated }) => {
            json!({"status":"solved","bindings":bindings.iter().map(|(v,t)|json!({"id":id(v),"type":t.to_value()})).collect::<Vec<_>>(),"updated":updated.iter().map(id).collect::<Vec<_>>()})
        }
        Err(SolveError::Native(e)) => native(e),
        Err(SolveError::Evaluation(EvalError::Native(e))) => native(e),
        Err(SolveError::Evaluation(EvalError::Transfer(TransferError::NativeException(e)))) => {
            json!({"status":"native_transfer_error","error":e.report()})
        }
        Err(e) => json!({"status":"solver_error","detail":format!("{e:?}")}),
    })
}
fn main() {
    for line in io::stdin().lock().lines() {
        let out = match line {
            Ok(s) => serde_json::from_str::<Value>(&s)
                .map_err(|e| e.to_string())
                .and_then(|v| run(&v)),
            Err(e) => Err(e.to_string()),
        };
        println!(
            "{}",
            out.unwrap_or_else(|e| json!({"status":"boundary_error","error":e}))
        );
    }
}

use super::*;
use crate::constraints::derived::{
    Arithmetic, BooleanOperation, EqualityKind, KnownArgumentPosition, OrderingOperation,
};
use crate::lattice::{BuiltinType, TypePredicate};
impl Generator<'_> {
    fn make_atom(&mut self, left: Type, right: NativeOperand) -> Result<NativeConstraintId> {
        Ok(self
            .arena
            .atomic(left, Relation::Subtype, right, self.provider)?)
    }
    fn find_element(&self, args: &[Type]) -> Result<Option<Type>> {
        use crate::lattice::{Known, TypeGetter, TypeValue};
        use num_traits::ToPrimitive;
        let [position, tuple]: &[Type; 2] = args
            .try_into()
            .map_err(|_| GenerationError::Boundary("element/2 arguments differ".into()))?;
        if !position.predicate(TypePredicate::Number)? {
            return Ok(None);
        }
        let TypeValue::Integers(Known::Known(values)) = position.get(TypeGetter::NumberValues)?
        else {
            return Ok(None);
        };
        if values.len() != 1 {
            return Ok(None);
        }
        let mut todo = self.state.context.clone();
        while let Some(id) = todo.pop() {
            match self.arena.node(id).ok_or_else(|| {
                GenerationError::Boundary("missing source context constraint".into())
            })? {
                crate::constraints::normalize::NativeConstraint::Atomic {
                    left,
                    relation: Relation::Equal,
                    right: NativeOperand::Type(right),
                    ..
                } if right == tuple => {
                    if !left.predicate(TypePredicate::Tuple)? {
                        return Ok(None);
                    }
                    let TypeValue::Types(Known::Known(elements)) =
                        left.get(TypeGetter::TupleArgs)?
                    else {
                        return Err(LatticeError::FunctionClause.into());
                    };
                    let index = values[0]
                        .to_usize()
                        .and_then(|n| n.checked_sub(1))
                        .ok_or(LatticeError::FunctionClause)?;
                    let element = elements.get(index).ok_or(LatticeError::FunctionClause)?;
                    return Ok(if element.predicate(TypePredicate::Variable)? {
                        Some(element.clone())
                    } else {
                        None
                    });
                }
                crate::constraints::normalize::NativeConstraint::Group { children, .. } => {
                    todo.extend(children.iter().rev())
                }
                _ => {}
            }
        }
        Ok(None)
    }
    pub(super) fn bif_constraints(
        &mut self,
        mfa: &Mfa,
        dst: &Type,
        args: &[Type],
    ) -> Result<Option<NativeConstraintId>> {
        let native_list = || NativeValue::List(args.iter().map(Type::to_native_value).collect());
        let argument = |n: usize| args[n].to_native_value();
        let destination = || dst.to_native_value();
        let atom = |s: &str| NativeValue::Atom(s.into());
        let mut constraints = Vec::new();
        if mfa.module == "erlang"
            && mfa.arity == 2
            && ["+", "-", "*"].contains(&mfa.function.as_str())
        {
            let operation = match mfa.function.as_str() {
                "+" => Arithmetic::Add,
                "-" => Arithmetic::Subtract,
                _ => Arithmetic::Multiply,
            };
            let ret = self.derived(
                DerivedOp::BifReturn {
                    mfa: mfa.clone(),
                    arguments: args.to_vec(),
                },
                "-get_bif_constr/4-fun-0-",
                vec![native_list(), atom(&mfa.function)],
            )?;
            let first = self.derived(
                DerivedOp::InverseArithmetic {
                    operation,
                    known_position: KnownArgumentPosition::Second,
                    destination: dst.clone(),
                    other: args[1].clone(),
                },
                "-get_bif_constr/4-fun-1-",
                vec![
                    destination(),
                    NativeValue::Integer(2.into()),
                    argument(1),
                    atom(&mfa.function),
                ],
            )?;
            let second = self.derived(
                DerivedOp::InverseArithmetic {
                    operation,
                    known_position: KnownArgumentPosition::First,
                    destination: dst.clone(),
                    other: args[0].clone(),
                },
                "-get_bif_constr/4-fun-1-",
                vec![
                    destination(),
                    NativeValue::Integer(1.into()),
                    argument(0),
                    atom(&mfa.function),
                ],
            )?;
            constraints.push(self.make_atom(dst.clone(), ret)?);
            constraints.push(self.make_atom(args[0].clone(), first)?);
            constraints.push(self.make_atom(args[1].clone(), second)?);
        } else if mfa.module == "erlang" && mfa.arity == 1 && type_test(&mfa.function).is_some() {
            let test = Type::builtin(type_test(&mfa.function).unwrap())?;
            let av = self.derived(
                DerivedOp::PredicateArgument {
                    destination: dst.clone(),
                    test: test.clone(),
                },
                "-get_bif_test_constr/4-fun-0-",
                vec![destination(), test.to_native_value()],
            )?;
            let dv = self.derived(
                DerivedOp::PredicateReturn {
                    argument: args[0].clone(),
                    test: test.clone(),
                },
                "-get_bif_test_constr/4-fun-1-",
                vec![argument(0), test.to_native_value()],
            )?;
            constraints.push(self.make_atom(dst.clone(), dv)?);
            constraints.push(self.make_atom(args[0].clone(), av)?);
        } else if mfa.module == "erlang"
            && mfa.arity == 2
            && ["<", "=<", ">", ">="].contains(&mfa.function.as_str())
        {
            let name = mfa.function.as_str();
            let other = match name {
                "<" | "=<" => ">=",
                ">" => "<",
                _ => "=<",
            };
            let ret = self.derived(
                DerivedOp::BifReturn {
                    mfa: mfa.clone(),
                    arguments: args.to_vec(),
                },
                "-get_bif_constr/4-fun-5-",
                vec![native_list(), atom(name)],
            )?;
            constraints.push(self.make_atom(dst.clone(), ret)?);
            for (i, op) in [(0, name), (1, other)] {
                let operation = match op {
                    "<" => OrderingOperation::Less,
                    "=<" => OrderingOperation::LessEqual,
                    ">" => OrderingOperation::Greater,
                    _ => OrderingOperation::GreaterEqual,
                };
                let arg = self.derived(
                    DerivedOp::OrderingArgument {
                        operation,
                        destination: dst.clone(),
                        argument: args[i].clone(),
                        other: args[1 - i].clone(),
                    },
                    "-get_bif_constr/4-fun-3-",
                    vec![destination(), atom(op), argument(1 - i), argument(i)],
                )?;
                constraints.push(self.make_atom(args[i].clone(), arg)?);
            }
        } else if mfa.module == "erlang" && mfa.arity == 2 && mfa.function == "is_function" {
            let av = self.derived(
                DerivedOp::FunctionArityArgument {
                    destination: dst.clone(),
                    arity: args[1].clone(),
                },
                "-get_bif_constr/4-fun-10-",
                vec![destination(), argument(1)],
            )?;
            constraints.push(self.make_atom(
                dst.clone(),
                NativeOperand::Type(Type::builtin(BuiltinType::Boolean)?),
            )?);
            constraints.push(self.make_atom(
                args[1].clone(),
                NativeOperand::Type(Type::builtin(BuiltinType::Integer)?),
            )?);
            constraints.push(self.make_atom(args[0].clone(), av)?);
        } else if mfa.module == "erlang"
            && mfa.arity == 2
            && ["and", "or"].contains(&mfa.function.as_str())
        {
            let and = mfa.function == "and";
            let operation = if and {
                BooleanOperation::And
            } else {
                BooleanOperation::Or
            };
            let (true_t, false_t) = (Type::atom("true"), Type::atom("false"));
            let (t1, t2) = if and {
                (false_t.clone(), true_t.clone())
            } else {
                (true_t.clone(), false_t.clone())
            };
            let ret = self.derived(
                DerivedOp::BooleanReturn {
                    operation: operation.clone(),
                    left: args[0].clone(),
                    right: args[1].clone(),
                },
                if and {
                    "-get_bif_constr/4-fun-17-"
                } else {
                    "-get_bif_constr/4-fun-20-"
                },
                vec![
                    argument(0),
                    argument(1),
                    t1.to_native_value(),
                    t2.to_native_value(),
                ],
            )?;
            constraints.push(self.make_atom(dst.clone(), ret)?);
            for i in 0..2 {
                let av = self.derived(
                    DerivedOp::BooleanArgument {
                        operation: operation.clone(),
                        destination: dst.clone(),
                        other: args[1 - i].clone(),
                    },
                    if and {
                        "-get_bif_constr/4-fun-15-"
                    } else {
                        "-get_bif_constr/4-fun-18-"
                    },
                    vec![
                        destination(),
                        argument(1 - i),
                        t1.to_native_value(),
                        t2.to_native_value(),
                    ],
                )?;
                constraints.push(self.make_atom(args[i].clone(), av)?);
            }
            if !and {
                let mut alternatives =
                    vec![self.make_atom(dst.clone(), NativeOperand::Type(false_t))?];
                for a in args {
                    match self.make_atom(a.clone(), NativeOperand::Type(true_t.clone())) {
                        Ok(c) => alternatives.push(c),
                        Err(GenerationError::ConstructionFailure) => {}
                        Err(e) => return Err(e),
                    }
                }
                constraints.push(self.arena.group(GroupKind::Disjunction, alternatives)?);
            }
        } else if mfa.module == "erlang" && mfa.arity == 1 && mfa.function == "not" {
            for (left, operand) in [
                (args[0].clone(), dst.clone()),
                (dst.clone(), args[0].clone()),
            ] {
                let v = self.derived(
                    DerivedOp::BooleanNot {
                        value: operand.clone(),
                    },
                    "-get_bif_constr/4-fun-22-",
                    vec![
                        operand.to_native_value(),
                        Type::atom("true").to_native_value(),
                        Type::atom("false").to_native_value(),
                    ],
                )?;
                constraints.push(self.make_atom(left, v)?);
            }
        } else if mfa.module == "erlang"
            && mfa.arity == 2
            && ["=:=", "=="].contains(&mfa.function.as_str())
        {
            let exact = mfa.function == "=:=";
            let op = if exact {
                DerivedOp::ExactEqualityReturn {
                    left: args[0].clone(),
                    right: args[1].clone(),
                }
            } else {
                DerivedOp::BifReturn {
                    mfa: mfa.clone(),
                    arguments: args.to_vec(),
                }
            };
            let ret = self.derived(
                op,
                if exact {
                    "-get_bif_constr/4-fun-26-"
                } else {
                    "-get_bif_constr/4-fun-27-"
                },
                if exact {
                    vec![argument(0), argument(1)]
                } else {
                    vec![native_list()]
                },
            )?;
            constraints.push(self.make_atom(dst.clone(), ret)?);
            for i in 0..2 {
                let av = self.derived(
                    DerivedOp::EqualityArgument {
                        kind: if exact {
                            EqualityKind::Exact
                        } else {
                            EqualityKind::Coercing
                        },
                        destination: dst.clone(),
                        argument: args[i].clone(),
                        other: args[1 - i].clone(),
                    },
                    if exact {
                        "-get_bif_constr/4-fun-24-"
                    } else {
                        "-get_bif_constr/4-fun-28-"
                    },
                    if exact {
                        vec![destination(), argument(1 - i), argument(i)]
                    } else {
                        vec![destination(), argument(i), argument(1 - i)]
                    },
                )?;
                constraints.push(self.make_atom(args[i].clone(), av)?);
            }
        } else if mfa.module == "erlang" && mfa.arity == 3 && mfa.function == "is_record" {
            let source_state = self.capture_state()?;
            let module = self
                .state
                .module
                .ok_or_else(|| GenerationError::Boundary("record test module absent".into()))?;
            let records = Arc::new(
                self.records
                    .iter()
                    .filter(|((owner, _, _), _)| *owner == module)
                    .map(|((_, tag, arity), typ)| ((tag.clone(), *arity), typ.clone()))
                    .collect(),
            );
            let mut terms = Vec::new();
            let destination = crate::constraints::constraint_capture::typ(&mut terms, dst);
            let state = crate::constraints::constraint_capture::environment_values(
                &mut terms,
                &source_state,
            )
            .map_err(GenerationError::Boundary)?;
            let [state]: [CaptureId; 1] = state.try_into().map_err(|_| {
                GenerationError::Boundary("record capture state is not singleton".into())
            })?;
            let tag = crate::constraints::constraint_capture::typ(&mut terms, &args[1]);
            let arity = crate::constraints::constraint_capture::typ(&mut terms, &args[2]);
            let root = crate::constraints::constraint_capture::proper_list(
                &mut terms,
                vec![destination, state, tag, arity],
            );
            let environment = CaptureEnvironment::new(
                Arc::new(TermOrderArena::from_captures(&terms).map_err(GenerationError::Boundary)?),
                root,
            )
            .map_err(GenerationError::Boundary)?;
            let key = self
                .profile
                .instantiate("-get_bif_constr/4-fun-13-", environment)
                .map_err(GenerationError::Boundary)?;
            let argument = NativeOperand::Derived(self.arena.derived(
                DerivedOp::RecordTestArgument {
                    destination: dst.clone(),
                    tag: args[1].clone(),
                    arity: args[2].clone(),
                    records,
                },
                key,
                ClosureOrigin::Undefined,
            ));
            let returns = self.derived(
                DerivedOp::BifReturn {
                    mfa: mfa.clone(),
                    arguments: args.to_vec(),
                },
                "-get_bif_constr/4-fun-14-",
                vec![native_list()],
            )?;
            constraints.push(self.make_atom(dst.clone(), returns)?);
            constraints.push(self.make_atom(
                args[2].clone(),
                NativeOperand::Type(Type::builtin(BuiltinType::Integer)?),
            )?);
            constraints.push(self.make_atom(
                args[1].clone(),
                NativeOperand::Type(Type::builtin(BuiltinType::Atom)?),
            )?);
            constraints.push(self.make_atom(args[0].clone(), argument)?);
        } else if mfa.module == "erlang" && mfa.arity == 2 && mfa.function == "is_record" {
            let av = self.derived(
                DerivedOp::PredicateArgument {
                    destination: dst.clone(),
                    test: Type::builtin(BuiltinType::Tuple)?,
                },
                "-get_bif_constr/4-fun-11-",
                vec![destination()],
            )?;
            let dv = self.derived(
                DerivedOp::BifReturn {
                    mfa: mfa.clone(),
                    arguments: args.to_vec(),
                },
                "-get_bif_constr/4-fun-12-",
                vec![native_list()],
            )?;
            constraints.push(self.make_atom(dst.clone(), dv)?);
            constraints.push(self.make_atom(
                args[1].clone(),
                NativeOperand::Type(Type::builtin(BuiltinType::Atom)?),
            )?);
            constraints.push(self.make_atom(args[0].clone(), av)?);
        } else if mfa.module == "erlang" && mfa.arity == 2 && mfa.function == "++" {
            let hd = self.derived(
                DerivedOp::AppendHead {
                    destination: dst.clone(),
                },
                "-get_bif_constr/4-fun-6-",
                vec![destination()],
            )?;
            let tl = self.derived(
                DerivedOp::AppendTail {
                    destination: dst.clone(),
                },
                "-get_bif_constr/4-fun-7-",
                vec![destination()],
            )?;
            let ret = self.derived(
                DerivedOp::BifReturn {
                    mfa: mfa.clone(),
                    arguments: args.to_vec(),
                },
                "-get_bif_constr/4-fun-8-",
                vec![native_list()],
            )?;
            constraints.push(self.make_atom(dst.clone(), ret)?);
            constraints.push(self.make_atom(args[0].clone(), hd)?);
            constraints.push(self.make_atom(args[1].clone(), tl)?);
            for (a, t) in args.iter().cloned().zip(
                self.provider
                    .construction(mfa)
                    .map_err(GenerationError::Evaluation)?
                    .arguments,
            ) {
                constraints.push(self.make_atom(a, NativeOperand::Type(t))?);
            }
        } else if mfa.module == "erlang" && mfa.arity == 2 && mfa.function == "element" {
            let info = self
                .provider
                .construction(mfa)
                .map_err(GenerationError::Evaluation)?;
            if info.generic_return.predicate(TypePredicate::None)? {
                return Err(GenerationError::ConstructionFailure);
            }
            let ret = self.derived(
                DerivedOp::BifReturn {
                    mfa: mfa.clone(),
                    arguments: args.to_vec(),
                },
                "-get_bif_constr/4-fun-30-",
                vec![native_list()],
            )?;
            constraints.push(self.make_atom(dst.clone(), ret)?);
            if let Some(element) = self.find_element(args)? {
                constraints.push(self.arena.atomic(
                    dst.clone(),
                    Relation::Equal,
                    NativeOperand::Type(element),
                    self.provider,
                )?);
            }
            for (a, t) in args.iter().cloned().zip(info.arguments) {
                constraints.push(self.make_atom(a, NativeOperand::Type(t))?);
            }
        } else {
            let info = self
                .provider
                .construction(mfa)
                .map_err(GenerationError::Evaluation)?;
            if info.generic_return.predicate(TypePredicate::None)? {
                return Err(GenerationError::ConstructionFailure);
            }
            if !info.known && info.generic_return.predicate(TypePredicate::Any)? {
                return Ok(None);
            }
            let ret = self.derived(
                DerivedOp::BifReturn {
                    mfa: mfa.clone(),
                    arguments: args.to_vec(),
                },
                "-get_bif_constr/4-fun-31-",
                vec![
                    native_list(),
                    NativeValue::Integer(mfa.arity.into()),
                    atom(&mfa.function),
                    atom(&mfa.module),
                ],
            )?;
            let return_constraint = self.make_atom(dst.clone(), ret)?;
            if !info.known {
                return Ok(Some(return_constraint));
            }
            constraints.push(return_constraint);
            if info.known {
                for (a, domain) in args.iter().cloned().zip(info.arguments) {
                    constraints.push(self.make_atom(a, NativeOperand::Type(domain))?);
                }
            }
        }
        Ok(Some(self.arena.group(GroupKind::Conjunction, constraints)?))
    }
}
fn type_test(name: &str) -> Option<BuiltinType> {
    Some(match name {
        "is_atom" => BuiltinType::Atom,
        "is_binary" => BuiltinType::Binary,
        "is_bitstring" => BuiltinType::Bitstring,
        "is_boolean" => BuiltinType::Boolean,
        "is_float" => BuiltinType::Float,
        "is_function" => BuiltinType::Function,
        "is_integer" => BuiltinType::Integer,
        "is_list" => BuiltinType::MaybeImproperList,
        "is_map" => BuiltinType::Map,
        "is_number" => BuiltinType::Number,
        "is_pid" => BuiltinType::Pid,
        "is_port" => BuiltinType::Port,
        "is_reference" => BuiltinType::Reference,
        "is_tuple" => BuiltinType::Tuple,
        _ => return None,
    })
}

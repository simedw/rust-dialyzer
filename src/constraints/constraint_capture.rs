use crate::{
    constraints::atomic::Relation,
    constraints::closure_order::ClosureOrderKey,
    constraints::normalize::{
        ClosureOrigin, GroupKind, NativeConstraint, NativeConstraintArena, NativeConstraintId,
        NativeOperand,
    },
    lattice::{NativeValue, Type, VariableId},
    term_order::{CaptureEnvironment, CaptureId, CaptureTerm, CapturedFunction},
};

fn push(terms: &mut Vec<CaptureTerm>, term: CaptureTerm) -> CaptureId {
    let id = CaptureId(terms.len());
    terms.push(term);
    id
}
fn atom(terms: &mut Vec<CaptureTerm>, text: &str) -> CaptureId {
    push(terms, CaptureTerm::Atom(text.to_owned()))
}
fn tuple(terms: &mut Vec<CaptureTerm>, tag: &str, fields: Vec<CaptureId>) -> CaptureId {
    let mut items = vec![atom(terms, tag)];
    items.extend(fields);
    push(terms, CaptureTerm::Tuple(items))
}
pub fn proper_list(terms: &mut Vec<CaptureTerm>, items: Vec<CaptureId>) -> CaptureId {
    let tail = push(terms, CaptureTerm::Nil);
    push(terms, CaptureTerm::List { items, tail })
}
pub fn environment_values(
    terms: &mut Vec<CaptureTerm>,
    environment: &CaptureEnvironment,
) -> Result<Vec<CaptureId>, String> {
    let mut root = environment.append_to(terms);
    let mut values = Vec::with_capacity(environment.len());
    loop {
        match terms.get(root.0) {
            Some(CaptureTerm::Nil) => break,
            Some(CaptureTerm::List { items, tail }) => {
                values.extend(items);
                root = *tail;
            }
            _ => return Err("captured environment must be a proper list".into()),
        }
    }
    Ok(values)
}
pub fn native_value(terms: &mut Vec<CaptureTerm>, value: &NativeValue) -> CaptureId {
    enum Frame<'a> {
        Visit(&'a NativeValue),
        Tuple(usize),
        List(usize),
    }
    let mut todo = vec![Frame::Visit(value)];
    let mut built = Vec::new();
    while let Some(frame) = todo.pop() {
        let term = match frame {
            Frame::Visit(NativeValue::Tuple(items)) => {
                todo.push(Frame::Tuple(items.len()));
                todo.extend(items.iter().rev().map(Frame::Visit));
                continue;
            }
            Frame::Visit(NativeValue::List(items)) => {
                todo.push(Frame::List(items.len()));
                todo.extend(items.iter().rev().map(Frame::Visit));
                continue;
            }
            Frame::Visit(NativeValue::Integer(n)) => CaptureTerm::Integer(n.to_string()),
            Frame::Visit(NativeValue::Atom(a)) => CaptureTerm::Atom(a.clone()),
            Frame::Visit(NativeValue::Nil) => CaptureTerm::Nil,
            Frame::Tuple(count) => CaptureTerm::Tuple(built.split_off(built.len() - count)),
            Frame::List(count) => {
                let tail = push(terms, CaptureTerm::Nil);
                CaptureTerm::List {
                    items: built.split_off(built.len() - count),
                    tail,
                }
            }
        };
        built.push(push(terms, term));
    }
    built[0]
}
pub fn typ(terms: &mut Vec<CaptureTerm>, value: &Type) -> CaptureId {
    native_value(terms, &value.to_native_value())
}
pub fn variable(terms: &mut Vec<CaptureTerm>, value: &VariableId) -> CaptureId {
    match value {
        VariableId::Integer(n) => push(terms, CaptureTerm::Integer(n.to_string())),
        VariableId::Atom(a) => atom(terms, a),
    }
}
pub fn type_variable(terms: &mut Vec<CaptureTerm>, value: &VariableId) -> CaptureId {
    typ(
        terms,
        &match value {
            VariableId::Integer(n) => Type::variable_id(VariableId::Integer(n.clone())),
            VariableId::Atom(a) => Type::variable_atom(a.clone()),
        },
    )
}
fn dependencies(terms: &mut Vec<CaptureTerm>, values: &[VariableId]) -> CaptureId {
    let items = values.iter().map(|v| variable(terms, v)).collect();
    proper_list(terms, items)
}
fn closure(terms: &mut Vec<CaptureTerm>, key: &ClosureOrderKey) -> Result<CaptureId, String> {
    let function = match key {
        ClosureOrderKey::Local(value) => CapturedFunction::Local {
            module: value.module.clone(),
            new_index: value.new_index,
            old_uniq: value.old_uniq,
            arity: value.arity,
            name: value.name.clone(),
            environment: environment_values(terms, &value.environment)?,
        },
        ClosureOrderKey::External {
            module,
            function,
            arity,
        } => CapturedFunction::External {
            module: module.clone(),
            function: function.clone(),
            arity: *arity,
        },
    };
    Ok(push(terms, CaptureTerm::Function(function)))
}
/// The immutable raw, unprepared native constraint records. IDs stay source-arena identities.
pub struct ConstraintCaptures {
    nodes: Vec<CaptureId>,
}
impl ConstraintCaptures {
    pub fn append(
        arena: &NativeConstraintArena,
        terms: &mut Vec<CaptureTerm>,
    ) -> Result<Self, String> {
        let mut nodes = Vec::<CaptureId>::with_capacity(arena.len());
        for index in 0..arena.len() {
            let record = match arena
                .node(NativeConstraintId(index))
                .ok_or("constraint arena gap")?
            {
                NativeConstraint::Atomic {
                    left,
                    relation,
                    right,
                    deps,
                } => {
                    let left = typ(terms, left);
                    let relation = atom(
                        terms,
                        match relation {
                            Relation::Equal => "eq",
                            Relation::Subtype => "sub",
                        },
                    );
                    let right = match right {
                        NativeOperand::Type(value) => typ(terms, value),
                        NativeOperand::Derived(id) => {
                            let (key, origin, deps) = arena
                                .derived_capture(*id)
                                .ok_or("foreign derived capture")?;
                            let function = closure(terms, key)?;
                            let deps = dependencies(terms, deps);
                            let origin = match origin {
                                ClosureOrigin::Undefined => atom(terms, "undefined"),
                                ClosureOrigin::SourceLine(line) => {
                                    push(terms, CaptureTerm::Integer(line.to_string()))
                                }
                            };
                            tuple(terms, "fun_var", vec![function, deps, origin])
                        }
                    };
                    let deps = dependencies(terms, deps);
                    tuple(terms, "constraint", vec![left, relation, right, deps])
                }
                NativeConstraint::Group {
                    kind,
                    children,
                    deps,
                } => {
                    let kind = atom(
                        terms,
                        match kind {
                            GroupKind::Conjunction => "conj",
                            GroupKind::Disjunction => "disj",
                        },
                    );
                    let children = children
                        .iter()
                        .map(|id| {
                            nodes
                                .get(id.0)
                                .copied()
                                .ok_or("non-postorder constraint child".to_owned())
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let children = proper_list(terms, children);
                    let deps = dependencies(terms, deps);
                    let undefined = atom(terms, "undefined");
                    tuple(
                        terms,
                        "constraint_list",
                        vec![kind, children, deps, undefined, undefined],
                    )
                }
                NativeConstraint::Reference { function, deps } => {
                    let function = type_variable(terms, function.variable());
                    let deps = dependencies(terms, deps);
                    tuple(terms, "constraint_ref", vec![function, deps])
                }
            };
            nodes.push(record);
        }
        Ok(Self { nodes })
    }
    pub fn node(&self, id: NativeConstraintId) -> Result<CaptureId, String> {
        self.nodes
            .get(id.0)
            .copied()
            .ok_or("foreign constraint capture".into())
    }
}

use crate::{
    binder::BinderState,
    core::{CoreExpr, ErlangTerm},
    dataflow_key::DataflowKey,
    dataflow_map::DataflowMap,
    dataflow_nodes::DataflowNodes,
    lattice::{BuiltinType, LatticeError, Type},
    native_error::NativeTransferError,
    program::NodeRef,
};
use crate::{
    guard_literals::GuardLiterals,
    guard_ops::{GuardEval, eval_intersection},
    guard_producer::{GuardProducer, ProducerError},
};
use std::{cell::Cell, collections::BTreeMap, io::Write, sync::OnceLock, time::Instant};

thread_local! { static GUARD_VISITS: Cell<u64> = const { Cell::new(0) }; }
/// Deepest guard expression analyzed before giving up. Generated code (cowlib's
/// character-class macros, OTP's `json`) nests `orelse` chains in the hundreds.
/// Guard visiting recurses, so callers must provide a stack sized for this.
pub(crate) const MAX_GUARD_DEPTH: usize = 16_384;

fn guard_trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("RUST_DIALYZER_GUARD_TRACE").as_deref() == Some(std::ffi::OsStr::new("1"))
    })
}
fn guard_trace_record(value: serde_json::Value) {
    let _ = writeln!(std::io::stderr().lock(), "RUST_DIALYZER_GUARD {value}");
}
struct GuardTrace {
    node: NodeRef,
    started: Instant,
    visits: u64,
}
impl Drop for GuardTrace {
    fn drop(&mut self) {
        guard_trace_record(
            serde_json::json!({"event":"finish","module_id":self.node.module.0,"node":self.node.node,"elapsed_ms":self.started.elapsed().as_secs_f64()*1000.0,"visits":GUARD_VISITS.with(Cell::get).wrapping_sub(self.visits)}),
        );
    }
}

#[derive(Clone, Debug)]
pub enum GuardFailureMessage {
    Arguments {
        function: String,
        args: String,
    },
    Infix {
        left: String,
        operator: String,
        right: String,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardFailureKind {
    GuardFail,
    NegGuardFail,
}
#[derive(Clone, Debug)]
pub enum GuardReason {
    GuardFailure {
        guard: NodeRef,
        kind: GuardFailureKind,
        message: GuardFailureMessage,
    },
    RecordMatching {
        guard: NodeRef,
        actual: String,
        tag: String,
    },
}
#[derive(Clone, Debug)]
pub enum GuardError {
    Failure {
        fatal: bool,
        reason: Option<GuardReason>,
    },
    Native(NativeTransferError),
    Producer(ProducerError),
    Form(crate::form_error::FormError),
    Transfer(crate::bifs::registry::TransferError),
    Binder(crate::binder::BindError),
    Boundary(String),
    ResourceLimit,
    UnderapproxCaseClauseAtom(String),
    Unported {
        node: NodeRef,
        kind: &'static str,
    },
}
impl From<LatticeError> for GuardError {
    fn from(e: LatticeError) -> Self {
        Self::Native(e.into())
    }
}
impl From<String> for GuardError {
    fn from(e: String) -> Self {
        Self::Boundary(e)
    }
}
#[derive(Clone, Debug)]
pub struct GuardBound<W> {
    pub map: DataflowMap,
    pub value: Type,
    pub state: BinderState<W>,
}
pub type GuardEnvironment = BTreeMap<DataflowKey, NodeRef>;
pub struct GuardControl<'a, 'b, 'c> {
    pub nodes: &'a DataflowNodes<'b>,
    pub producer: &'a GuardProducer<'a, 'b, 'c>,
    pub literals: &'a GuardLiterals<'a>,
    pub registry: &'a crate::bifs::registry::Registry,
}
impl GuardControl<'_, '_, '_> {
    pub fn evaluate<
        W: Clone
            + PartialEq
            + From<crate::binder::OpaqueMatch>
            + From<crate::guard_warning::GuardWarning>,
    >(
        &self,
        node: NodeRef,
        map: &DataflowMap,
        env: &GuardEnvironment,
        eval: GuardEval,
        state: &BinderState<W>,
    ) -> Result<GuardBound<W>, GuardError> {
        let _trace = guard_trace_enabled().then(|| {
            guard_trace_record(serde_json::json!({"event":"start","module":state.module,"function":format!("{:?}",state.current_fun),"module_id":node.module.0,"node":node.node,"mode":format!("{eval:?}"),"bindings":map.types().len(),"environment":env.len(),"warning_mode":state.warnings.warning_mode}));
            GuardTrace { node, started:Instant::now(), visits:GUARD_VISITS.with(Cell::get) }
        });
        self.visit(node, map, env, eval, state, 0)
    }
    pub(crate) fn visit<
        W: Clone
            + PartialEq
            + From<crate::binder::OpaqueMatch>
            + From<crate::guard_warning::GuardWarning>,
    >(
        &self,
        node: NodeRef,
        map: &DataflowMap,
        env: &GuardEnvironment,
        eval: GuardEval,
        state: &BinderState<W>,
        depth: usize,
    ) -> Result<GuardBound<W>, GuardError> {
        if guard_trace_enabled() {
            GUARD_VISITS.with(|visits| {
                let count=visits.get().wrapping_add(1);
                visits.set(count);
                if count & 0x3fff == 0 {
                    guard_trace_record(serde_json::json!({"event":"progress","module_id":node.module.0,"node":node.node,"mode":format!("{eval:?}"),"depth":depth,"visits":count}));
                }
            });
        }
        if depth > MAX_GUARD_DEPTH {
            guard_trace_record(serde_json::json!({"event":"resource_limit","module_id":node.module.0,"node":node.node,"depth":depth}));
            return Err(GuardError::ResourceLimit);
        }
        let child = |id| DataflowNodes::node(node.module, id);
        let plain = |value| GuardBound {
            map: map.clone(),
            value,
            state: state.clone(),
        };
        Ok(match self.nodes.expression(node)? {
            CoreExpr::Literal(id) => plain(self.nodes.literal_type(node.module, *id)?),
            CoreExpr::Binary(_) => plain(Type::builtin(BuiltinType::Binary)?),
            CoreExpr::Cons { head, tail } => {
                let first =
                    self.visit(child(*head), map, env, GuardEval::Unknown, state, depth + 1)?;
                let second = self.visit(
                    child(*tail),
                    &first.map,
                    env,
                    GuardEval::Unknown,
                    &first.state,
                    depth + 1,
                )?;
                GuardBound {
                    map: second.map,
                    value: Type::cons(first.value, second.value)?,
                    state: second.state,
                }
            }
            CoreExpr::Tuple(elements) => {
                let (mut current_map, mut current_state) = (map.clone(), state.clone());
                let mut types = Vec::with_capacity(elements.len());
                for id in elements {
                    let bound = self.visit(
                        child(*id),
                        &current_map,
                        env,
                        GuardEval::Unknown,
                        &current_state,
                        depth + 1,
                    )?;
                    current_map = bound.map;
                    current_state = bound.state;
                    types.push(bound.value);
                }
                GuardBound {
                    map: current_map,
                    value: Type::tuple(types)?,
                    state: current_state,
                }
            }
            CoreExpr::Values(elements) => {
                let mut current_state = state.clone();
                let mut types = Vec::with_capacity(elements.len());
                for id in elements {
                    let bound = self.visit(
                        child(*id),
                        map,
                        env,
                        GuardEval::Unknown,
                        &current_state,
                        depth + 1,
                    )?;
                    current_state = bound.state;
                    types.push(bound.value);
                }
                GuardBound {
                    map: map.clone(),
                    value: Type::product(types),
                    state: current_state,
                }
            }
            CoreExpr::Case { argument, clauses } => {
                let clauses = clauses.iter().map(|id| child(*id)).collect::<Vec<_>>();
                self.case_guard(
                    node,
                    child(*argument),
                    &clauses,
                    map,
                    env,
                    eval,
                    state,
                    depth + 1,
                )?
            }
            CoreExpr::Let {
                vars,
                argument,
                body,
            } => {
                let [var] = vars.as_slice() else {
                    return Err(GuardError::Unported {
                        node,
                        kind: "malformed let variable list",
                    });
                };
                let mut env = env.clone();
                env.insert(self.nodes.label(child(*var))?.into(), child(*argument));
                self.visit(child(*body), map, &env, eval, state, depth + 1)?
            }
            CoreExpr::Var(_) => {
                let key = DataflowKey::from(self.nodes.label(node)?);
                let mut bound = if let Some(expression) = env.get(&key) {
                    self.visit(*expression, map, env, eval, state, depth + 1)?
                } else {
                    plain(eval_intersection(eval, &self.nodes.lookup(node, map)?)?)
                };
                self.nodes
                    .enter_type(node, bound.value.clone(), &mut bound.map)?;
                bound
            }
            CoreExpr::Map { .. } if eval != GuardEval::Unknown => plain(Type::none()),
            CoreExpr::Map { base, pairs, .. } => {
                let initial =
                    self.visit(child(*base), map, env, GuardEval::Unknown, state, depth + 1)?;
                let mut value = Type::builtin(BuiltinType::Map)?.meet(&initial.value)?;
                if value.is_impossible() {
                    GuardBound {
                        map: initial.map,
                        value: Type::none(),
                        state: initial.state,
                    }
                } else {
                    let (mut current_map, mut current_state) = (initial.map, initial.state);
                    let mut typed = Vec::with_capacity(pairs.len());
                    for pair in pairs {
                        let CoreExpr::MapPair {
                            operation,
                            key,
                            value: pair_value,
                        } = self.nodes.expression(child(*pair))?
                        else {
                            return Err(GuardError::Unported {
                                node: child(*pair),
                                kind: "malformed map pair",
                            });
                        };
                        let key = self.visit(
                            child(*key),
                            &current_map,
                            env,
                            GuardEval::Unknown,
                            &current_state,
                            depth + 1,
                        )?;
                        let val = self.visit(
                            child(*pair_value),
                            &key.map,
                            env,
                            GuardEval::Unknown,
                            &key.state,
                            depth + 1,
                        )?;
                        current_map = val.map;
                        current_state = val.state;
                        let CoreExpr::Literal(op) = self.nodes.expression(child(*operation))?
                        else {
                            return Err(GuardError::Native(NativeTransferError::FunctionClause));
                        };
                        let op = match &self.nodes.module(node.module)?.terms[op.0] {
                            ErlangTerm::Atom(s) if s == "assoc" => Some(false),
                            ErlangTerm::Atom(s) if s == "exact" => Some(true),
                            _ => None,
                        };
                        typed.push((key.value, val.value, op));
                    }
                    for (key, val, exact) in typed {
                        value = match exact {
                            Some(false) => value.map_put(key, val)?,
                            Some(true) => value.map_update(key, val)?,
                            None => {
                                return Err(GuardError::Native(
                                    NativeTransferError::FunctionClause,
                                ));
                            }
                        };
                    }
                    GuardBound {
                        map: current_map,
                        value,
                        state: current_state,
                    }
                }
            }
            CoreExpr::Try {
                argument,
                vars,
                body,
                exception_vars,
                handler,
            } => {
                let [var] = vars.as_slice() else {
                    return Err(GuardError::Unported {
                        node,
                        kind: "malformed try variable list",
                    });
                };
                let join = map.begin_join();
                let base = join.branch();
                let mut handler_map = base.clone();
                self.nodes.mark_fresh(
                    &exception_vars
                        .iter()
                        .copied()
                        .map(child)
                        .collect::<Vec<_>>(),
                    &mut handler_map,
                )?;
                let (handler, handler_error) =
                    match self.visit(child(*handler), &handler_map, env, eval, state, depth + 1) {
                        Ok(bound) => (bound, None),
                        Err(error @ GuardError::Failure { .. }) => (
                            GuardBound {
                                map: handler_map,
                                value: Type::none(),
                                state: state.clone(),
                            },
                            Some(error),
                        ),
                        Err(error) => return Err(error),
                    };
                let mut body_env = env.clone();
                body_env.insert(self.nodes.label(child(*var))?.into(), child(*argument));
                if eval_intersection(eval, &handler.value)?.is_none() {
                    self.visit(
                        child(*body),
                        map,
                        &body_env,
                        eval,
                        &handler.state,
                        depth + 1,
                    )?
                } else {
                    let (body, body_error) = match self.visit(
                        child(*body),
                        &base,
                        &body_env,
                        eval,
                        &handler.state,
                        depth + 1,
                    ) {
                        Ok(bound) => (bound, None),
                        Err(error @ GuardError::Failure { .. }) => (
                            GuardBound {
                                map: base,
                                value: Type::none(),
                                state: handler.state.clone(),
                            },
                            Some(error),
                        ),
                        Err(error) => return Err(error),
                    };
                    let joined = join.finish(&[body.map, handler.map])?;
                    let value = body.value.join(&handler.value)?;
                    if value.is_none() {
                        let fatal = [&body_error, &handler_error]
                            .iter()
                            .any(|e| matches!(e, Some(GuardError::Failure { fatal: true, .. })));
                        let reason = match (body_error, handler_error) {
                            (
                                Some(GuardError::Failure {
                                    reason: Some(reason),
                                    ..
                                }),
                                _,
                            ) => Some(reason),
                            (_, Some(GuardError::Failure { reason, .. })) => reason,
                            _ => None,
                        };
                        return Err(GuardError::Failure { fatal, reason });
                    }
                    GuardBound {
                        map: joined,
                        value,
                        state: body.state,
                    }
                }
            }
            CoreExpr::Call { module, name, args } => {
                let module = self.atom(child(*module))?;
                if module != "erlang" {
                    return Err(GuardError::Unported {
                        node,
                        kind: "non-erlang guard call assertion",
                    });
                }
                let function = self.atom(child(*name))?;
                let args = args.iter().map(|id| child(*id)).collect::<Vec<_>>();
                self.call(node, &function, &args, map, env, eval, state, depth)?
            }
            expression => {
                return Err(GuardError::Unported {
                    node,
                    kind: expression.kind(),
                });
            }
        })
    }
}

impl GuardReason {
    pub fn native_message(&self) -> crate::native_form_environment::RawFormTerm {
        use crate::{form_error::ReasonBuilder, lattice::NativeValue, warning_payload::string};
        let (kind, args) = match self {
            Self::GuardFailure { kind, message, .. } => {
                let args = match message {
                    GuardFailureMessage::Arguments { function, args } => {
                        vec![NativeValue::Atom(function.clone()), string(args)]
                    }
                    GuardFailureMessage::Infix {
                        left,
                        operator,
                        right,
                    } => vec![string(left), string(operator), string(right)],
                };
                (
                    match kind {
                        GuardFailureKind::GuardFail => "guard_fail",
                        GuardFailureKind::NegGuardFail => "neg_guard_fail",
                    },
                    args,
                )
            }
            Self::RecordMatching { actual, tag, .. } => (
                "record_matching",
                vec![string(actual), NativeValue::Atom(tag.clone())],
            ),
        };
        let mut builder = ReasonBuilder::new();
        let root = builder.native(&NativeValue::Tuple(vec![
            NativeValue::Atom(kind.into()),
            NativeValue::List(args),
        ]));
        builder.finish(root)
    }
}

use crate::{
    bifs::registry::{Mfa, Registry, TransferError},
    binder::{
        BindError, Binder, BinderState, Direction, FailureKind, OpaqueMatch, WarningFunction,
        WarningInfo,
    },
    core::{CoreExpr, ErlangTerm},
    core_print::{CorePrinter, PrintError},
    dataflow_map::DataflowMap,
    function_state::{FunctionState, StateError},
    labels::CoreLabel,
    lattice::{
        BuiltinType, Endpoint, Known, LatticeError, OpacityConflict, Type, TypeGetter,
        TypePredicate, TypeValue,
    },
    patterns::{PatternRef, Patterns},
    program::NodeRef,
    worklist::FunctionKey,
};
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BodyMessage {
    Function {
        warning: crate::emission::FunctionWarning,
        name: Option<(String, usize)>,
    },
    ImproperList {
        tail: String,
    },
    UnmatchedReturn {
        actual: String,
    },
    RecordConstruction {
        actual: String,
        fields: String,
    },
    RecordBinding {
        tag: String,
        pattern: String,
        actual: String,
    },
    RecordMatching {
        pattern: String,
        actual: String,
    },
    MapUpdate {
        map: String,
        key: String,
    },
    BinaryConstruction {
        part: &'static str,
        argument: String,
        segment: String,
        actual: String,
    },
    OpaqueSize {
        actual: String,
        size: String,
    },
}
#[derive(Clone, Debug)]
pub struct BodyWarning {
    pub source: NodeRef,
    pub info: WarningInfo,
    pub message: BodyMessage,
}
impl PartialEq for BodyWarning {
    fn eq(&self, other: &Self) -> bool {
        self.info == other.info && self.message == other.message
    }
}
impl Eq for BodyWarning {}
#[derive(Debug)]
pub enum BodyError {
    Native(LatticeError),
    Guard(Box<crate::guard_control::GuardError>),
    Call(crate::body_calls::CallControllerError),
    Contract(crate::contract_runtime::ContractError),
    NativeWarningTag(&'static str),
    Underapprox(crate::underapprox::UnderapproxError),
    Bind(BindError),
    State(StateError),
    Bif(TransferError),
    Print(PrintError),
    Boundary(String),
    Unported { node: NodeRef, kind: &'static str },
    ResourceLimit,
    BitstringDomain(crate::lattice::BitstringDomainError),
}
impl From<crate::lattice::BitstringDomainError> for BodyError {
    fn from(e: crate::lattice::BitstringDomainError) -> Self {
        Self::BitstringDomain(e)
    }
}
impl From<LatticeError> for BodyError {
    fn from(e: LatticeError) -> Self {
        Self::Native(e)
    }
}
impl From<BindError> for BodyError {
    fn from(e: BindError) -> Self {
        Self::Bind(e)
    }
}
impl From<StateError> for BodyError {
    fn from(e: StateError) -> Self {
        Self::State(e)
    }
}
impl From<TransferError> for BodyError {
    fn from(e: TransferError) -> Self {
        Self::Bif(e)
    }
}
impl From<PrintError> for BodyError {
    fn from(e: PrintError) -> Self {
        Self::Print(e)
    }
}
impl From<String> for BodyError {
    fn from(e: String) -> Self {
        Self::Boundary(e)
    }
}
type Result<T> = std::result::Result<T, BodyError>;
pub struct BodyBound<W> {
    pub map: DataflowMap,
    pub value: Type,
    pub state: BinderState<W>,
}
struct Value {
    map: DataflowMap,
    typ: Type,
}
enum Collection {
    Product,
    Binary,
    Call {
        tree: NodeRef,
        module: NodeRef,
        name: NodeRef,
        args: Vec<NodeRef>,
    },
    Apply {
        tree: NodeRef,
        operator: NodeRef,
        args: Vec<NodeRef>,
    },
    Tuple {
        node: NodeRef,
        elements: Vec<NodeRef>,
    },
    Cons {
        node: NodeRef,
    },
}
struct MapInsertion {
    pair: NodeRef,
    operation: String,
    key: Type,
    value: Type,
}
struct MapCursor {
    base: NodeRef,
    base_type: Type,
    remaining: VecDeque<NodeRef>,
    insertions: Vec<MapInsertion>,
    shadow: Type,
    exact_keys: Vec<Type>,
}
enum Frame {
    Visit(NodeRef, DataflowMap),
    Collection {
        remaining: VecDeque<NodeRef>,
        types: Vec<Type>,
        finish: Collection,
    },
    Let {
        vars: Vec<NodeRef>,
        body: NodeRef,
    },
    Seq {
        argument: NodeRef,
        body: NodeRef,
    },
    Catch(DataflowMap),
    Letrec {
        variable: NodeRef,
        remaining: VecDeque<(NodeRef, NodeRef)>,
        body: NodeRef,
    },
    Timeout,
    BitstrSize {
        node: NodeRef,
        value: NodeRef,
    },
    BitstrValue {
        node: NodeRef,
        size: Type,
    },
    MapBase {
        base: NodeRef,
        pairs: Vec<NodeRef>,
    },
    MapKey {
        cursor: MapCursor,
        pair: NodeRef,
        value: NodeRef,
    },
    MapValue {
        cursor: MapCursor,
        pair: NodeRef,
        key: Type,
    },
    TryArgument {
        argument: NodeRef,
        vars: Vec<NodeRef>,
        body: NodeRef,
        exceptions: Vec<NodeRef>,
        handler: NodeRef,
        outer: DataflowMap,
    },
    TryBody {
        exceptions: Vec<NodeRef>,
        handler: NodeRef,
        outer: DataflowMap,
        argument_map: DataflowMap,
    },
    CaseArgument {
        node: NodeRef,
        argument: NodeRef,
        clauses: Vec<NodeRef>,
    },
    CaseBody(CaseCursor),
    TryHandler {
        success: Value,
        argument_map: DataflowMap,
    },
}
pub struct BodyFlow<'a, 'b, 'c> {
    pub patterns: &'a mut Patterns<'b, 'c>,
    pub functions: &'a mut FunctionState,
    pub bifs: &'a Registry,
    pub calls: Option<(
        &'a crate::guard_producer::GuardProducer<'a, 'c, 'c>,
        &'a crate::body_calls::CallWorld<'a>,
    )>,
    pub guards: Option<&'a crate::guard_control::GuardControl<'a, 'c, 'c>>,
    pub letrec: &'a crate::letrec_types::LetrecLinks,
}
impl BodyFlow<'_, '_, '_> {
    pub fn analyze_module<
        W: Clone
            + PartialEq
            + From<OpaqueMatch>
            + From<BodyWarning>
            + From<crate::guard_warning::GuardWarning>
            + From<crate::call_warning::CallWarning>
            + From<CaseWarning>,
    >(
        &mut self,
        state: &BinderState<W>,
        get_warnings: bool,
    ) -> Result<BinderState<W>> {
        let mut state = state.clone();
        state.warnings.warning_mode = false;
        state = self.analyze_loop(&state)?;
        if get_warnings {
            self.functions.set_warning_mode();
            state.warnings.warning_mode = true;
            state = self.analyze_loop(&state)?;
        }
        Ok(state)
    }
    pub fn analyze_loop<
        W: Clone
            + PartialEq
            + From<OpaqueMatch>
            + From<BodyWarning>
            + From<crate::guard_warning::GuardWarning>
            + From<crate::call_warning::CallWarning>
            + From<CaseWarning>,
    >(
        &mut self,
        state: &BinderState<W>,
    ) -> Result<BinderState<W>> {
        let mut state = state.clone();
        let mut steps = 0usize;
        while let Some(key) = self.functions.pop_work() {
            steps += 1;
            if steps > 1_000_000 {
                return Err(BodyError::ResourceLimit);
            }
            let definition = self
                .functions
                .definitions()
                .get(&key)
                .ok_or(StateError::Missing(key))?;
            state.current_fun = match key {
                FunctionKey::Top => WarningFunction::Top,
                FunctionKey::Label(label) => self
                    .functions
                    .homes()
                    .get(&label)
                    .cloned()
                    .map(WarningFunction::Named)
                    .ok_or_else(|| BodyError::Boundary("module function home absent".into()))?,
            };
            let entry = self
                .functions
                .entries()
                .get(&key)
                .ok_or(StateError::Missing(key))?;
            if !entry.called() {
                continue;
            }
            let arguments = entry.signature().arguments.clone();
            let Some(mut map) = self.functions.environment(key).cloned() else {
                continue;
            };
            let node = definition
                .node
                .ok_or_else(|| BodyError::Boundary("source function node absent".into()))?;
            let CoreExpr::Fun { params, body } = self.patterns.nodes.expression(node)? else {
                return Err(LatticeError::FunctionClause.into());
            };
            let params = Self::refs(node, params);
            let body = NodeRef {
                module: node.module,
                node: body.0,
            };
            self.enter_list(&params, &arguments, &mut map)?;
            let bound = self.evaluate(body, &map, &state)?;
            state = bound.state;
            self.functions
                .update_return(key, &arguments, &bound.value)?;
        }
        state.current_fun = WarningFunction::Undefined;
        Ok(state)
    }
    fn refs(node: NodeRef, ids: &[crate::core::NodeId]) -> Vec<NodeRef> {
        ids.iter()
            .map(|id| NodeRef {
                module: node.module,
                node: id.0,
            })
            .collect()
    }
    fn function_type(&self, label: CoreLabel) -> Result<Type> {
        Ok(self.functions.function_type(FunctionKey::Label(label))?)
    }
    fn refresh_letrec<W>(&self, state: &mut BinderState<W>) -> Result<()> {
        let generation = self.functions.signature_generation();
        if state.letrec.is_current(self.letrec, generation) { return Ok(()); }
        let letrec = state.letrec.make_mut();
        for (&variable, &function) in self.letrec {
            letrec.insert(variable.into(), self.function_type(function)?);
        }
        state.letrec.mark_current(self.letrec, generation);
        Ok(())
    }
    fn atom(&self, node: NodeRef) -> Result<&str> {
        let CoreExpr::Literal(id) = self.patterns.nodes.expression(node)? else {
            return Err(LatticeError::FunctionClause.into());
        };
        let ErlangTerm::Atom(atom) = &self.patterns.nodes.module(node.module)?.terms[id.0] else {
            return Err(LatticeError::FunctionClause.into());
        };
        Ok(atom)
    }
    fn integer(&self, node: NodeRef) -> Result<num_bigint::BigInt> {
        let CoreExpr::Literal(id) = self.patterns.nodes.expression(node)? else {
            return Err(LatticeError::FunctionClause.into());
        };
        let ErlangTerm::Integer(number) = &self.patterns.nodes.module(node.module)?.terms[id.0]
        else {
            return Err(LatticeError::FunctionClause.into());
        };
        number.parse().map_err(|_| LatticeError::BadArg.into())
    }
    fn binary_warning<
        W: Clone
            + PartialEq
            + From<OpaqueMatch>
            + From<BodyWarning>
            + From<crate::guard_warning::GuardWarning>
            + From<crate::call_warning::CallWarning>
            + From<CaseWarning>,
    >(
        &mut self,
        node: NodeRef,
        argument: NodeRef,
        part: &'static str,
        actual: &Type,
        state: &mut BinderState<W>,
    ) -> Result<()> {
        let printer = CorePrinter {
            patterns: self.patterns,
        };
        let argument_string = printer.format_cerl(argument)?;
        let segment = printer.format_cerl(node)?;
        let actual = state.presentation_context().type_to_string(actual)?;
        self.emit(
            argument,
            BodyMessage::BinaryConstruction {
                part,
                argument: argument_string,
                segment,
                actual,
            },
            state,
        )
    }
    fn finish_bitstr<
        W: Clone
            + PartialEq
            + From<OpaqueMatch>
            + From<BodyWarning>
            + From<crate::guard_warning::GuardWarning>
            + From<crate::call_warning::CallWarning>
            + From<CaseWarning>,
    >(
        &mut self,
        node: NodeRef,
        size_type: Type,
        value: Value,
        state: &mut BinderState<W>,
    ) -> Result<Value> {
        let CoreExpr::Bitstr {
            value: val,
            size,
            unit,
            typ,
            ..
        } = self.patterns.nodes.expression(node)?
        else {
            return Err(LatticeError::FunctionClause.into());
        };
        let reference = |id: crate::core::NodeId| NodeRef {
            module: node.module,
            node: id.0,
        };
        let val = reference(*val);
        let size = reference(*size);
        let unit = reference(*unit);
        let kind = self.atom(reference(*typ))?.to_owned();
        let special = match self.atom(size).ok() {
            Some("all") if kind == "binary" => Some(BuiltinType::Bitstring),
            Some("undefined") if matches!(kind.as_str(), "utf8" | "utf16" | "utf32") => {
                Some(BuiltinType::Integer)
            }
            _ => None,
        };
        let mut map = value.map;
        if let Some(domain) = special {
            let refined = value.typ.meet(&Type::builtin(domain)?)?;
            self.patterns
                .nodes
                .enter_type(val, refined.clone(), &mut map)?;
            let typ = if refined.is_none() {
                self.binary_warning(node, val, "value", &value.typ, state)?;
                Type::none()
            } else {
                Type::builtin(BuiltinType::Bitstring)?
            };
            return Ok(Value { map, typ });
        }
        let refined_size = size_type.meet(&Type::builtin(BuiltinType::NonNegativeInteger)?)?;
        let domain = match kind.as_str() {
            "binary" => BuiltinType::Bitstring,
            "float" => BuiltinType::Number,
            "integer" => BuiltinType::Integer,
            _ => {
                return Err(BodyError::Unported {
                    node,
                    kind: "malformed binary segment type",
                });
            }
        };
        let refined_value = value.typ.meet(&Type::builtin(domain)?)?;
        if refined_size.is_none() || refined_value.is_none() {
            if refined_size.is_none() {
                self.binary_warning(node, size, "size", &size_type, state)?;
            } else {
                self.binary_warning(node, val, "value", &value.typ, state)?;
            }
            return Ok(Value {
                map,
                typ: Type::none(),
            });
        }
        let unit = self.integer(unit)?;
        let TypeValue::Integers(numbers) = refined_size.get(TypeGetter::NumberValues)? else {
            return Err(LatticeError::FunctionClause.into());
        };
        if refined_size.opacity_conflict(&refined_value, &state.module)? != OpacityConflict::None {
            let actual = state.presentation_context().type_to_string(&refined_size)?;
            let size_string = (CorePrinter {
                patterns: self.patterns,
            })
            .format_cerl(size)?;
            self.emit(
                size,
                BodyMessage::OpaqueSize {
                    actual,
                    size: size_string,
                },
                state,
            )?;
        }
        let typ = match numbers {
            Known::Known(numbers) if numbers.len() == 1 => {
                Type::bitstring(0.into(), &numbers[0] * &unit)?
            }
            Known::Unknown => Type::builtin(BuiltinType::Bitstring)?,
            Known::Known(_) => {
                let TypeValue::Endpoint(Endpoint::Finite(minimum)) =
                    refined_size.get(TypeGetter::NumberMin)?
                else {
                    return Err(LatticeError::BadArg.into());
                };
                Type::bitstring(unit.clone(), &unit * minimum)?
            }
        };
        for (node, typ) in [
            (val, refined_value),
            (size, refined_size),
            (node, typ.clone()),
        ] {
            self.patterns.nodes.enter_type(node, typ, &mut map)?;
        }
        Ok(Value { map, typ })
    }
    fn enter_list(&self, vars: &[NodeRef], types: &[Type], map: &mut DataflowMap) -> Result<()> {
        for (index, var) in vars.iter().enumerate() {
            self.patterns.nodes.enter_type(
                *var,
                types
                    .get(index)
                    .ok_or(LatticeError::FunctionClause)?
                    .clone(),
                map,
            )?;
        }
        if vars.len() != types.len() {
            return Err(LatticeError::FunctionClause.into());
        }
        Ok(())
    }
    fn type_list(typ: &Type) -> Result<Vec<Type>> {
        let TypeValue::Types(Known::Known(types)) = typ.get(TypeGetter::ToTypeList)? else {
            return Err(LatticeError::FunctionClause.into());
        };
        Ok(types)
    }
    fn emit<
        W: Clone
            + PartialEq
            + From<OpaqueMatch>
            + From<BodyWarning>
            + From<crate::guard_warning::GuardWarning>
            + From<crate::call_warning::CallWarning>
            + From<CaseWarning>,
    >(
        &mut self,
        node: NodeRef,
        message: BodyMessage,
        state: &mut BinderState<W>,
    ) -> Result<()> {
        let binder = Binder {
            patterns: self.patterns,
        };
        let pattern = PatternRef::Core(node);
        let annotation = binder.annotation(pattern)?;
        if state.warnings.warning_mode && !annotation.is_generated() {
            let info = binder.warning_info(pattern, state)?;
            state.warnings.add(
                annotation,
                BodyWarning {
                    source: node,
                    info,
                    message,
                }
                .into(),
                false,
            );
        }
        Ok(())
    }
    fn simple(typ: &Type) -> Result<bool> {
        for predicate in [
            TypePredicate::Atom,
            TypePredicate::Number,
            TypePredicate::Port,
            TypePredicate::Pid,
            TypePredicate::Reference,
            TypePredicate::Nil,
        ] {
            if typ.predicate(predicate)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn unmatched<
        W: Clone
            + PartialEq
            + From<OpaqueMatch>
            + From<BodyWarning>
            + From<crate::guard_warning::GuardWarning>
            + From<crate::call_warning::CallWarning>
            + From<CaseWarning>,
    >(
        &mut self,
        node: NodeRef,
        typ: &Type,
        state: &mut BinderState<W>,
    ) -> Result<()> {
        if !state.warnings.warning_mode || typ.predicate(TypePredicate::Any)? || Self::simple(typ)?
        {
            return Ok(());
        }
        if let CoreExpr::Call { module, name, args } = self.patterns.nodes.expression(node)?
            && args.len() == 2
            && self
                .atom(NodeRef {
                    module: node.module,
                    node: module.0,
                })
                .ok()
                == Some("erlang")
            && matches!(
                self.atom(NodeRef {
                    module: node.module,
                    node: name.0
                })
                .ok(),
                Some("!" | "send")
            )
        {
            return Ok(());
        }
        let m = self.patterns.nodes.module(node.module)?;
        let annotations = m.nodes[node.node].annotations;
        let comprehension = matches!(&m.terms[annotations.0],ErlangTerm::List {items,..} if items.iter().any(|id|matches!(&m.terms[id.0],ErlangTerm::Atom(a) if a=="list_comprehension")));
        if comprehension && typ.predicate(TypePredicate::List)? {
            let TypeValue::Type(elements) = typ.get(TypeGetter::ListElements)? else {
                return Err(LatticeError::FunctionClause.into());
            };
            if Self::simple(&elements)? {
                return Ok(());
            }
        }
        let actual = state.presentation_context().type_to_string(typ)?;
        self.emit(node, BodyMessage::UnmatchedReturn { actual }, state)
    }
    fn collect(
        todo: &mut Vec<Frame>,
        nodes: Vec<NodeRef>,
        map: DataflowMap,
        finish: Collection,
        values: &mut Vec<Value>,
        flow: &mut Self,
        state: &mut BinderState<
            impl Clone
            + PartialEq
            + From<OpaqueMatch>
            + From<BodyWarning>
            + From<crate::guard_warning::GuardWarning>
            + From<crate::call_warning::CallWarning>
            + From<CaseWarning>,
        >,
    ) -> Result<()> {
        let mut remaining: VecDeque<_> = nodes.into();
        if let Some(first) = remaining.pop_front() {
            todo.push(Frame::Collection {
                remaining,
                types: Vec::new(),
                finish,
            });
            todo.push(Frame::Visit(first, map));
        } else {
            values.push(flow.finish_collection(finish, Vec::new(), map, state)?);
        }
        Ok(())
    }
    fn advance_map<
        W: Clone
            + PartialEq
            + From<OpaqueMatch>
            + From<BodyWarning>
            + From<crate::guard_warning::GuardWarning>
            + From<crate::call_warning::CallWarning>
            + From<CaseWarning>,
    >(
        &mut self,
        mut cursor: MapCursor,
        map: DataflowMap,
        todo: &mut Vec<Frame>,
        values: &mut Vec<Value>,
        state: &mut BinderState<W>,
    ) -> Result<()> {
        if let Some(pair) = cursor.remaining.pop_front() {
            let CoreExpr::MapPair { key, value, .. } = self.patterns.nodes.expression(pair)? else {
                return Err(LatticeError::FunctionClause.into());
            };
            let key = NodeRef {
                module: pair.module,
                node: key.0,
            };
            let value = NodeRef {
                module: pair.module,
                node: value.0,
            };
            todo.push(Frame::MapKey {
                cursor,
                pair,
                value,
            });
            todo.push(Frame::Visit(key, map));
            return Ok(());
        }
        let mut result = cursor.base_type;
        for insertion in cursor.insertions {
            match insertion.operation.as_str() {
                "assoc" => result = result.map_put(insertion.key, insertion.value)?,
                "exact" => {
                    let updated = result.map_update(insertion.key.clone(), insertion.value)?;
                    if updated.is_none() {
                        let context = state.presentation_context();
                        let map_string = context.type_to_string(&result)?;
                        let key = context.type_to_string(&insertion.key)?;
                        self.emit(
                            insertion.pair,
                            BodyMessage::MapUpdate {
                                map: map_string,
                                key,
                            },
                            state,
                        )?;
                        values.push(Value {
                            map,
                            typ: Type::none(),
                        });
                        return Ok(());
                    }
                    result = updated;
                }
                _ => return Err(LatticeError::FunctionClause.into()),
            }
        }
        let mut bound_type = Type::map_any();
        for key in cursor.exact_keys {
            bound_type = bound_type.map_put(key, Type::any())?;
        }
        self.refresh_letrec(state)?;
        let map = match (Binder {
            patterns: self.patterns,
        })
        .bind(
            &[PatternRef::Core(cursor.base)],
            &[bound_type],
            &map,
            state,
            Direction::Reverse,
        ) {
            Ok(bound) => {
                *state = bound.state;
                bound.map
            }
            Err(BindError::Failure(_)) => map,
            Err(error) => return Err(error.into()),
        };
        values.push(Value { map, typ: result });
        Ok(())
    }
    fn finish_collection<
        W: Clone
            + PartialEq
            + From<OpaqueMatch>
            + From<BodyWarning>
            + From<crate::guard_warning::GuardWarning>
            + From<crate::call_warning::CallWarning>
            + From<CaseWarning>,
    >(
        &mut self,
        finish: Collection,
        types: Vec<Type>,
        mut map: DataflowMap,
        state: &mut BinderState<W>,
    ) -> Result<Value> {
        let typ = match finish {
            Collection::Call {
                tree,
                module,
                name,
                args,
            } => {
                let (producer, world) = self.calls.ok_or(BodyError::Unported {
                    node: tree,
                    kind: "call phase absent",
                })?;
                let mut types = types.into_iter();
                let module_type = types.next().ok_or(LatticeError::FunctionClause)?;
                let name_type = types.next().ok_or(LatticeError::FunctionClause)?;
                let actual = types.collect::<Vec<_>>();
                let bound = crate::body_calls::BodyCalls {
                    patterns: self.patterns,
                    producer,
                    functions: self.functions,
                    bifs: self.bifs,
                    world,
                }
                .finish_call(
                    crate::body_calls::CallOperands {
                        tree,
                        module,
                        name,
                        args: &args,
                        module_type,
                        name_type,
                        actual: &actual,
                    },
                    map,
                    state.clone(),
                )
                .map_err(BodyError::Call)?;
                *state = bound.state;
                map = bound.map;
                bound.value
            }
            Collection::Apply {
                tree,
                operator,
                args,
            } => {
                let (producer, world) = self.calls.ok_or(BodyError::Unported {
                    node: tree,
                    kind: "apply phase absent",
                })?;
                let mut actual = types;
                let operator_type = actual.pop().ok_or(LatticeError::FunctionClause)?;
                let bound = crate::body_calls::BodyCalls {
                    patterns: self.patterns,
                    producer,
                    functions: self.functions,
                    bifs: self.bifs,
                    world,
                }
                .finish_apply(
                    crate::body_calls::ApplyOperands {
                        tree,
                        operator,
                        args: &args,
                        operator_type,
                        actual: &actual,
                    },
                    map,
                    state.clone(),
                )
                .map_err(BodyError::Call)?;
                *state = bound.state;
                map = bound.map;
                bound.value
            }
            Collection::Product => Type::product(types),
            Collection::Binary => Type::bitstr_concat_types(&types)?,
            Collection::Cons { node } => {
                let [head, tail]: [Type; 2] =
                    types.try_into().map_err(|_| LatticeError::FunctionClause)?;
                if tail.meet(&Type::builtin(BuiltinType::List)?)?.is_none() {
                    let tail = state.presentation_context().type_to_string(&tail)?;
                    self.emit(node, BodyMessage::ImproperList { tail }, state)?;
                }
                Type::cons(head, tail)?
            }
            Collection::Tuple { node, elements } => {
                let tuple = Type::tuple(types)?;
                if tuple.is_none() {
                    return Ok(Value { map, typ: tuple });
                }
                let Some(first) = elements.first() else {
                    return Ok(Value { map, typ: tuple });
                };
                let Ok(tag) = self.atom(*first).map(str::to_owned) else {
                    return Ok(Value { map, typ: tuple });
                };
                if !self
                    .patterns
                    .has_record_annotation(PatternRef::Core(node))?
                {
                    return Ok(Value { map, typ: tuple });
                }
                let Some(record) = state
                    .records
                    .get(&(tag.clone(), elements.len() - 1))
                    .cloned()
                else {
                    return Ok(Value { map, typ: tuple });
                };
                if record.prototype.meet(&tuple)?.is_none() {
                    let context = state.presentation_context();
                    let actual = context.type_to_string(&tuple)?;
                    let (positions, fields) = tuple.record_field_diffs(&context)?;
                    let offending = *elements
                        .get(*positions.first().ok_or(LatticeError::FunctionClause)?)
                        .ok_or(LatticeError::FunctionClause)?;
                    self.emit(
                        offending,
                        BodyMessage::RecordConstruction { actual, fields },
                        state,
                    )?;
                    Type::none()
                } else {
                    let TypeValue::Types(Known::Known(expected)) =
                        record.prototype.get(TypeGetter::TupleArgs)?
                    else {
                        return Err(LatticeError::FunctionClause.into());
                    };
                    self.refresh_letrec(state)?;
                    let patterns: Vec<_> = elements.iter().copied().map(PatternRef::Core).collect();
                    match (Binder {
                        patterns: self.patterns,
                    })
                    .bind(
                        &patterns,
                        &expected,
                        &map,
                        state,
                        Direction::Forward,
                    ) {
                        Ok(bound) => {
                            map = bound.map;
                            *state = bound.state;
                            Type::tuple(bound.types)?
                        }
                        Err(BindError::Failure(failure)) => {
                            let pattern = CorePrinter {
                                patterns: self.patterns,
                            }
                            .format_patterns(&failure.patterns)?;
                            let actual = state
                                .presentation_context()
                                .type_to_string(&failure.actual)?;
                            match failure.kind {
                                FailureKind::Bind => {
                                    let PatternRef::Core(offending) = *failure
                                        .patterns
                                        .first()
                                        .ok_or(LatticeError::FunctionClause)?
                                    else {
                                        return Err(BodyError::Boundary(
                                            "record construction failure lost source node".into(),
                                        ));
                                    };
                                    self.emit(
                                        offending,
                                        BodyMessage::RecordBinding {
                                            tag,
                                            pattern,
                                            actual,
                                        },
                                        state,
                                    )?;
                                }
                                FailureKind::Record => self.emit(
                                    node,
                                    BodyMessage::RecordMatching { pattern, actual },
                                    state,
                                )?,
                            }
                            Type::none()
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
            }
        };
        Ok(Value { map, typ })
    }
    pub fn evaluate<
        W: Clone
            + PartialEq
            + From<OpaqueMatch>
            + From<BodyWarning>
            + From<crate::guard_warning::GuardWarning>
            + From<crate::call_warning::CallWarning>
            + From<CaseWarning>,
    >(
        &mut self,
        node: NodeRef,
        map: &DataflowMap,
        state: &BinderState<W>,
    ) -> Result<BodyBound<W>> {
        let mut state = state.clone();
        let mut todo = vec![Frame::Visit(node, map.clone())];
        let mut values: Vec<Value> = Vec::new();
        let mut work = 0usize;
        while let Some(frame) = todo.pop() {
            work += 1;
            if work > 1_000_000 {
                return Err(BodyError::ResourceLimit);
            }
            match frame {
                Frame::Visit(node, mut map) => {
                    let expr = self.patterns.nodes.expression(node)?.clone();
                    let reference = |id: crate::core::NodeId| NodeRef {
                        module: node.module,
                        node: id.0,
                    };
                    match expr {
                        CoreExpr::Case { argument, clauses } => {
                            let argument = reference(argument);
                            todo.push(Frame::CaseArgument {
                                node,
                                argument,
                                clauses: clauses.into_iter().map(reference).collect(),
                            });
                            todo.push(Frame::Visit(argument, map));
                        }
                        CoreExpr::Literal(id) => values.push(Value {
                            map,
                            typ: self.patterns.nodes.literal_type(node.module, id)?,
                        }),
                        CoreExpr::Var(_) => {
                            let label = self.patterns.nodes.label(node)?;
                            let typ = if let Some(function) = self.letrec.get(&label) {
                                self.function_type(*function)?
                            } else {
                                self.patterns.nodes.lookup(node, &map)?
                            };
                            values.push(Value { map, typ });
                        }
                        CoreExpr::Alias { pattern, .. } => {
                            todo.push(Frame::Visit(reference(pattern), map))
                        }
                        CoreExpr::Fun { .. } => {
                            let label = self.patterns.nodes.label(node)?;
                            let typ = self.functions.capture(label, map.clone())?;
                            values.push(Value { map, typ });
                        }
                        CoreExpr::Values(ids) => Self::collect(
                            &mut todo,
                            Self::refs(node, &ids),
                            map,
                            Collection::Product,
                            &mut values,
                            self,
                            &mut state,
                        )?,
                        CoreExpr::Binary(ids) => Self::collect(
                            &mut todo,
                            Self::refs(node, &ids),
                            map,
                            Collection::Binary,
                            &mut values,
                            self,
                            &mut state,
                        )?,
                        CoreExpr::Bitstr { value, size, .. } => {
                            todo.push(Frame::BitstrSize {
                                node,
                                value: reference(value),
                            });
                            todo.push(Frame::Visit(reference(size), map));
                        }
                        CoreExpr::Map { base, pairs, .. } => {
                            todo.push(Frame::MapBase {
                                base: reference(base),
                                pairs: Self::refs(node, &pairs),
                            });
                            todo.push(Frame::Visit(reference(base), map));
                        }
                        CoreExpr::Tuple(ids) => {
                            let elements = Self::refs(node, &ids);
                            Self::collect(
                                &mut todo,
                                elements.clone(),
                                map,
                                Collection::Tuple { node, elements },
                                &mut values,
                                self,
                                &mut state,
                            )?;
                        }
                        CoreExpr::Cons { head, tail } => Self::collect(
                            &mut todo,
                            vec![reference(head), reference(tail)],
                            map,
                            Collection::Cons { node },
                            &mut values,
                            self,
                            &mut state,
                        )?,
                        CoreExpr::Catch { body } => {
                            todo.push(Frame::Catch(map.clone()));
                            todo.push(Frame::Visit(reference(body), map));
                        }
                        CoreExpr::Let {
                            vars,
                            argument,
                            body,
                        } => {
                            if matches!(
                                self.patterns.nodes.expression(reference(argument))?,
                                CoreExpr::Var(_)
                            ) {
                                let [var]: [NodeRef; 1] = Self::refs(node, &vars)
                                    .try_into()
                                    .map_err(|_| LatticeError::FunctionClause)?;
                                self.patterns.nodes.enter_substitution(
                                    var,
                                    reference(argument),
                                    &mut map,
                                )?;
                            }
                            todo.push(Frame::Let {
                                vars: Self::refs(node, &vars),
                                body: reference(body),
                            });
                            todo.push(Frame::Visit(reference(argument), map));
                        }
                        CoreExpr::Seq { argument, body } => {
                            todo.push(Frame::Seq {
                                argument: reference(argument),
                                body: reference(body),
                            });
                            todo.push(Frame::Visit(reference(argument), map));
                        }
                        CoreExpr::Letrec { definitions, body } => {
                            let mut remaining: VecDeque<_> = definitions
                                .into_iter()
                                .map(|(v, f)| (reference(v), reference(f)))
                                .collect();
                            if let Some((variable, function)) = remaining.pop_front() {
                                todo.push(Frame::Letrec {
                                    variable,
                                    remaining,
                                    body: reference(body),
                                });
                                todo.push(Frame::Visit(function, map));
                            } else {
                                todo.push(Frame::Visit(reference(body), map));
                            }
                        }
                        CoreExpr::Call { module, name, args } => {
                            let module = reference(module);
                            let name = reference(name);
                            let args = Self::refs(node, &args);
                            let mut children = vec![module, name];
                            children.extend(args.iter().copied());
                            Self::collect(
                                &mut todo,
                                children,
                                map,
                                Collection::Call {
                                    tree: node,
                                    module,
                                    name,
                                    args,
                                },
                                &mut values,
                                self,
                                &mut state,
                            )?;
                        }
                        CoreExpr::Apply { operator, args } => {
                            let operator = reference(operator);
                            let args = Self::refs(node, &args);
                            let mut children = args.clone();
                            children.push(operator);
                            Self::collect(
                                &mut todo,
                                children,
                                map,
                                Collection::Apply {
                                    tree: node,
                                    operator,
                                    args,
                                },
                                &mut values,
                                self,
                                &mut state,
                            )?;
                        }
                        CoreExpr::Primop { name, args } => {
                            let name = self.atom(reference(name))?;
                            let typ = match name {
                                "match_fail" | "raise" => Type::none(),
                                "bs_init_writable" => Type::bitstring(0u8.into(), 0u8.into())?,
                                "build_stacktrace" => {
                                    self.bifs
                                        .transfer(
                                            &Mfa {
                                                module: "erlang".into(),
                                                function: name.into(),
                                                arity: 0,
                                            },
                                            &[],
                                        )?
                                        .result
                                }
                                "recv_peek_message" => Type::product(vec![
                                    Type::builtin(BuiltinType::Boolean)?,
                                    Type::any(),
                                ]),
                                "recv_wait_timeout" => {
                                    let [arg]: [crate::core::NodeId; 1] = args
                                        .try_into()
                                        .map_err(|_| LatticeError::FunctionClause)?;
                                    todo.push(Frame::Timeout);
                                    todo.push(Frame::Visit(reference(arg), map));
                                    continue;
                                }
                                "dialyzer_unknown" | "remove_message" | "nif_start"
                                | "debug_line" | "executable_line" => Type::any(),
                                _ => {
                                    return Err(BodyError::Unported {
                                        node,
                                        kind: "primop",
                                    });
                                }
                            };
                            values.push(Value { map, typ });
                        }
                        CoreExpr::Try {
                            argument,
                            vars,
                            body,
                            exception_vars,
                            handler,
                        } => {
                            todo.push(Frame::TryArgument {
                                argument: reference(argument),
                                vars: Self::refs(node, &vars),
                                body: reference(body),
                                exceptions: Self::refs(node, &exception_vars),
                                handler: reference(handler),
                                outer: map.clone(),
                            });
                            todo.push(Frame::Visit(reference(argument), map));
                        }
                        other => {
                            return Err(BodyError::Unported {
                                node,
                                kind: match other {
                                    CoreExpr::Binary(_) => "binary",
                                    CoreExpr::Bitstr { .. } => "bitstr",
                                    CoreExpr::Call { .. } => "call",
                                    CoreExpr::Apply { .. } => "apply",
                                    CoreExpr::Case { .. } => "case",
                                    CoreExpr::Map { .. } => "map",
                                    _ => "unsupported source node",
                                },
                            });
                        }
                    }
                }
                Frame::CaseArgument {
                    node,
                    argument,
                    clauses,
                } => {
                    let value = values
                        .pop()
                        .ok_or_else(|| BodyError::Boundary("case argument absent".into()))?;
                    if value.typ.is_impossible() {
                        values.push(value);
                    } else {
                        self.next_body_clause(
                            CaseCursor::new(node, argument, value, clauses),
                            &mut todo,
                            &mut values,
                            &mut state,
                        )?;
                    }
                }
                Frame::CaseBody(mut cursor) => {
                    cursor.body(
                        values
                            .pop()
                            .ok_or_else(|| BodyError::Boundary("case body absent".into()))?,
                    );
                    self.next_body_clause(cursor, &mut todo, &mut values, &mut state)?;
                }
                Frame::Collection {
                    mut remaining,
                    mut types,
                    finish,
                } => {
                    let Value { map, typ } = values
                        .pop()
                        .ok_or_else(|| BodyError::Boundary("collection result absent".into()))?;
                    types.push(typ);
                    if let Some(next) = remaining.pop_front() {
                        todo.push(Frame::Collection {
                            remaining,
                            types,
                            finish,
                        });
                        todo.push(Frame::Visit(next, map));
                    } else {
                        values.push(self.finish_collection(finish, types, map, &mut state)?);
                    }
                }
                Frame::Catch(map) => {
                    values
                        .pop()
                        .ok_or_else(|| BodyError::Boundary("catch result absent".into()))?;
                    values.push(Value {
                        map,
                        typ: Type::any(),
                    });
                }
                Frame::Let { vars, body } => {
                    let mut value = values
                        .pop()
                        .ok_or_else(|| BodyError::Boundary("let result absent".into()))?;
                    if value.typ.predicate(TypePredicate::Impossible)? {
                        values.push(value);
                    } else {
                        self.enter_list(&vars, &Self::type_list(&value.typ)?, &mut value.map)?;
                        todo.push(Frame::Visit(body, value.map));
                    }
                }
                Frame::Seq { argument, body } => {
                    let value = values
                        .pop()
                        .ok_or_else(|| BodyError::Boundary("seq result absent".into()))?;
                    if value.typ.predicate(TypePredicate::Impossible)? {
                        values.push(value);
                    } else {
                        self.unmatched(argument, &value.typ, &mut state)?;
                        todo.push(Frame::Visit(body, value.map));
                    }
                }
                Frame::Letrec {
                    variable,
                    mut remaining,
                    body,
                } => {
                    let mut value = values
                        .pop()
                        .ok_or_else(|| BodyError::Boundary("letrec result absent".into()))?;
                    self.patterns
                        .nodes
                        .enter_type(variable, value.typ, &mut value.map)?;
                    if let Some((variable, function)) = remaining.pop_front() {
                        todo.push(Frame::Letrec {
                            variable,
                            remaining,
                            body,
                        });
                        todo.push(Frame::Visit(function, value.map));
                    } else {
                        todo.push(Frame::Visit(body, value.map));
                    }
                }
                Frame::Timeout => {
                    values
                        .last_mut()
                        .ok_or_else(|| BodyError::Boundary("timeout result absent".into()))?
                        .typ = Type::builtin(BuiltinType::Boolean)?;
                }
                Frame::BitstrSize { node, value } => {
                    let size = values
                        .pop()
                        .ok_or_else(|| BodyError::Boundary("binary size absent".into()))?;
                    todo.push(Frame::BitstrValue {
                        node,
                        size: size.typ,
                    });
                    todo.push(Frame::Visit(value, size.map));
                }
                Frame::BitstrValue { node, size } => {
                    let value = values
                        .pop()
                        .ok_or_else(|| BodyError::Boundary("binary value absent".into()))?;
                    values.push(self.finish_bitstr(node, size, value, &mut state)?);
                }
                Frame::MapBase { base, pairs } => {
                    let value = values
                        .pop()
                        .ok_or_else(|| BodyError::Boundary("map base absent".into()))?;
                    let typ = value.typ.meet(&Type::map_any())?;
                    if typ.predicate(TypePredicate::Impossible)? {
                        values.push(Value {
                            map: value.map,
                            typ,
                        });
                    } else {
                        self.advance_map(
                            MapCursor {
                                base,
                                base_type: typ,
                                remaining: pairs.into(),
                                insertions: Vec::new(),
                                shadow: Type::none(),
                                exact_keys: Vec::new(),
                            },
                            value.map,
                            &mut todo,
                            &mut values,
                            &mut state,
                        )?;
                    }
                }
                Frame::MapKey {
                    cursor,
                    pair,
                    value,
                } => {
                    let key = values
                        .pop()
                        .ok_or_else(|| BodyError::Boundary("map key absent".into()))?;
                    todo.push(Frame::MapValue {
                        cursor,
                        pair,
                        key: key.typ,
                    });
                    todo.push(Frame::Visit(value, key.map));
                }
                Frame::MapValue {
                    mut cursor,
                    pair,
                    key,
                } => {
                    let value = values
                        .pop()
                        .ok_or_else(|| BodyError::Boundary("map value absent".into()))?;
                    let CoreExpr::MapPair { operation, .. } =
                        self.patterns.nodes.expression(pair)?
                    else {
                        return Err(LatticeError::FunctionClause.into());
                    };
                    let operation = self
                        .atom(NodeRef {
                            module: pair.module,
                            node: operation.0,
                        })?
                        .to_owned();
                    if operation == "exact"
                        && key.predicate(TypePredicate::Singleton)?
                        && cursor.shadow.meet(&key)?.is_none()
                    {
                        cursor.exact_keys.insert(0, key.clone());
                    }
                    cursor.shadow = key.join(&cursor.shadow)?;
                    cursor.insertions.push(MapInsertion {
                        pair,
                        operation,
                        key,
                        value: value.typ,
                    });
                    self.advance_map(cursor, value.map, &mut todo, &mut values, &mut state)?;
                }
                Frame::TryArgument {
                    argument,
                    vars,
                    body,
                    exceptions,
                    handler,
                    outer,
                } => {
                    let value = values
                        .pop()
                        .ok_or_else(|| BodyError::Boundary("try argument absent".into()))?;
                    let types = Self::type_list(&value.typ)?;
                    if vars.len() != types.len() {
                        values.push(value);
                        continue;
                    }
                    let mut map = value.map.clone();
                    self.patterns.nodes.mark_fresh(&vars, &mut map)?;
                    self.refresh_letrec(&mut state)?;
                    let patterns: Vec<_> = vars.iter().copied().map(PatternRef::Core).collect();
                    let success = match (Binder {
                        patterns: self.patterns,
                    })
                    .bind(&patterns, &types, &map, &state, Direction::Forward)
                    {
                        Ok(bound) => {
                            map = bound.map;
                            state = bound.state;
                            match (Binder {
                                patterns: self.patterns,
                            })
                            .bind(
                                &[PatternRef::Core(argument)],
                                &[Type::product(bound.types)],
                                &map,
                                &state,
                                Direction::Reverse,
                            ) {
                                Ok(bound) => {
                                    map = bound.map;
                                    state = bound.state;
                                }
                                Err(BindError::Failure(_)) => {}
                                Err(error) => return Err(error.into()),
                            }
                            true
                        }
                        Err(BindError::Failure(_)) => false,
                        Err(error) => return Err(error.into()),
                    };
                    todo.push(Frame::TryBody {
                        exceptions,
                        handler,
                        outer,
                        argument_map: value.map,
                    });
                    if success {
                        todo.push(Frame::Visit(body, map));
                    } else {
                        values.push(Value {
                            map: DataflowMap::default(),
                            typ: Type::none(),
                        });
                    }
                }
                Frame::TryBody {
                    exceptions,
                    handler,
                    mut outer,
                    argument_map,
                } => {
                    let success = values
                        .pop()
                        .ok_or_else(|| BodyError::Boundary("try body absent".into()))?;
                    self.patterns.nodes.mark_fresh(&exceptions, &mut outer)?;
                    todo.push(Frame::TryHandler {
                        success,
                        argument_map,
                    });
                    todo.push(Frame::Visit(handler, outer));
                }
                Frame::TryHandler {
                    success,
                    argument_map,
                } => {
                    let handler = values
                        .pop()
                        .ok_or_else(|| BodyError::Boundary("try handler absent".into()))?;
                    values.push(Value {
                        typ: success.typ.join(&handler.typ)?,
                        map: argument_map.join_all(&[handler.map, success.map])?,
                    });
                }
            }
        }
        let value = values
            .pop()
            .ok_or_else(|| BodyError::Boundary("body result absent".into()))?;
        if !values.is_empty() {
            return Err(BodyError::Boundary(
                "body continuation leaked results".into(),
            ));
        }
        Ok(BodyBound {
            map: value.map,
            value: value.typ,
            state,
        })
    }
}
impl BodyWarning {
    pub fn native_warning(&self) -> crate::warning_payload::NativeWarning {
        use crate::{
            lattice::NativeValue as N,
            warning_payload::{NativeWarning, string},
        };
        let (category, name, args) = match &self.message {
            BodyMessage::Function { warning, name } => {
                use crate::{emission::FunctionWarning, returns::ReturnKind};
                let mut args = Vec::new();
                let category = match warning {
                    FunctionWarning::Unused => "warn_not_called",
                    FunctionWarning::OnlyExit => "warn_return_only_exit",
                    FunctionWarning::NoReturn { .. } => "warn_return_no_exit",
                };
                if !matches!(warning, FunctionWarning::Unused) {
                    let kind = match warning {
                        FunctionWarning::OnlyExit => "only_explicit",
                        FunctionWarning::NoReturn {
                            kind: ReturnKind::NoMatch,
                        } => "no_match",
                        FunctionWarning::NoReturn {
                            kind: ReturnKind::OnlyNormal,
                        } => "only_normal",
                        FunctionWarning::NoReturn {
                            kind: ReturnKind::Both,
                        } => "both",
                        FunctionWarning::NoReturn {
                            kind: ReturnKind::OnlyExplicit,
                        } => "only_explicit",
                        FunctionWarning::Unused => unreachable!(),
                    };
                    args.push(N::Atom(kind.into()));
                }
                if let Some((function, arity)) = name {
                    args.extend([N::Atom(function.clone()), N::Integer((*arity).into())]);
                }
                (
                    category,
                    if matches!(warning, FunctionWarning::Unused) {
                        "unused_fun"
                    } else {
                        "no_return"
                    },
                    args,
                )
            }
            BodyMessage::ImproperList { tail } => (
                "warn_non_proper_list",
                "improper_list_constr",
                vec![string(tail)],
            ),
            BodyMessage::UnmatchedReturn { actual } => (
                "warn_umatched_return",
                "unmatched_return",
                vec![string(actual)],
            ),
            BodyMessage::RecordConstruction { actual, fields } => (
                "warn_matching",
                "record_constr",
                vec![string(actual), string(fields)],
            ),
            BodyMessage::RecordBinding {
                tag,
                pattern,
                actual,
            } => (
                "warn_matching",
                "record_constr",
                vec![N::Atom(tag.clone()), string(pattern), string(actual)],
            ),
            BodyMessage::RecordMatching { pattern, actual } => (
                "warn_matching",
                "record_match",
                vec![string(pattern), string(actual)],
            ),
            BodyMessage::MapUpdate { map, key } => (
                "warn_map_construction",
                "map_update",
                vec![string(map), string(key)],
            ),
            BodyMessage::BinaryConstruction {
                part,
                argument,
                segment,
                actual,
            } => (
                "warn_bin_construction",
                "bin_construction",
                vec![
                    string(part),
                    string(argument),
                    string(segment),
                    string(actual),
                ],
            ),
            BodyMessage::OpaqueSize { actual, size } => (
                "warn_opaque",
                "opaque_size",
                vec![string(actual), string(size)],
            ),
        };
        NativeWarning::from_value(
            category,
            &self.info,
            &N::Tuple(vec![N::Atom(name.into()), N::List(args)]),
        )
    }
}

#[path = "body_case.rs"]
mod case;
use case::CaseCursor;
pub use case::CaseWarning;

#[path = "body_function_warnings.rs"]
mod function_warnings;

#[cfg(test)]
mod snapshot_refresh_tests {
    use super::*;
    use crate::{core::CoreProgram, dataflow_nodes::DataflowNodes, labels::Labels, function_state::FunctionDefinition, emission::WarningState};
    #[test]
    fn sorted_refresh_matches_owned_map_and_preserves_partial_failure_snapshots() {
        let core = CoreProgram::parse(r#"{"version":5,"profile":{"term_map_order":"native_to_list_with_iterator","otp":"28","erts":"16","elixir":"1.20.4","dialyzer":"5","core_options":[]},"modules":[]}"#).unwrap();
        let labels = Labels::build(&core, CoreLabel(0)).unwrap();
        let nodes = DataflowNodes::build(&core, &labels).unwrap();
        let bifs = Registry::load(include_str!("../corpus/bifs.json")).unwrap();
        for missing_at in [None, Some(0), Some(1)] {
            let mut functions = FunctionState::new(vec![FunctionDefinition { key: FunctionKey::Label(CoreLabel(1)), node: None, arity: 1, escaping: false, name: None, raw_signature: None, callers: vec![] }]);
            let mut patterns = Patterns::new(&nodes);
            let links: crate::letrec_types::LetrecLinks = (0..2).map(|i| (CoreLabel(10 + i), CoreLabel(if missing_at == Some(i) { 2 } else { 1 }))).collect::<BTreeMap<_,_>>().into();
            let flow = BodyFlow { patterns: &mut patterns, functions: &mut functions, bifs: &bifs, calls: None, guards: None, letrec: &links };
            let mut state = BinderState::<u32> { module: "fixture".into(), current_fun: WarningFunction::Undefined, translated_files: BTreeMap::new().into(), records: BTreeMap::new().into(), letrec: BTreeMap::from([(CoreLabel(10).into(), Type::atom("old")), (CoreLabel(99).into(), Type::atom("extra"))]).into(), warnings: WarningState { warning_mode: true, warnings: vec![7] } };
            let prior = state.clone(); let mut expected = state.letrec.as_ref().clone();
            let legacy = (|| -> Result<()> { for (&variable, &function) in &links { expected.insert(variable.into(), flow.function_type(function)?); } Ok(()) })();
            let actual = flow.refresh_letrec(&mut state);
            assert_eq!(format!("{actual:?}"), format!("{legacy:?}"));
            assert_eq!(state.letrec.as_ref(), &expected);
            assert_eq!(prior.letrec[&CoreLabel(10).into()], Type::atom("old"));
            assert_eq!(state.letrec[&CoreLabel(99).into()], Type::atom("extra"));
            assert_eq!(state.warnings, prior.warnings);
        }
    }
}

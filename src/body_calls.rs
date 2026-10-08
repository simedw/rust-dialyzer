use crate::{
    bifs::registry::{Mfa, Registry, TransferError},
    binder::{BindError, Binder, BinderState, OpaqueMatch},
    call_transfer::{
        self, AlternativeAccumulator, CallError, CallInput, DependentSummary, Diagnostic,
        FailureReason, OpacityReason, Signature,
    },
    call_warning::CallWarning,
    contract_runtime::ContractSummary,
    core_print::{CorePrinter, PrintError},
    dataflow_map::DataflowMap,
    emission::{ComparisonWarning, EqualityOperator, comparison_warnings},
    function_state::{FunctionState, StateError},
    guard_producer::{ArgumentRef, GuardProducer, ProducerError},
    labels::CoreLabel,
    lattice::{
        BuiltinType, Known, LatticeError, NativeValue, Type, TypeGetter, TypePredicate, TypeValue,
    },
    native_form_environment::RawFormTerm,
    patterns::{PatternRef, Patterns},
    program::NodeRef,
    warning_payload::string,
    worklist::FunctionKey,
};
use std::{collections::BTreeMap, sync::Arc};
#[derive(Clone)]
pub struct ActiveContract {
    pub summary: Arc<ContractSummary>,
    pub capture: RawFormTerm,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyTarget {
    Local(FunctionKey),
    External,
}
/// Phase-owned declared/library facts and raw current-phase signature lookup.
/// Candidates are already ordered by the source callgraph adapter.
#[derive(Default)]
pub struct CallWorld<'library> {
    pub(crate) library: Option<&'library crate::dependencies::Store>,
    pub named_signatures: BTreeMap<Mfa, Signature>,
    pub anonymous_signatures: BTreeMap<CoreLabel, Signature>,
    pub active_contracts: BTreeMap<Mfa, ActiveContract>,
    pub call_sites: BTreeMap<NodeRef, Vec<ApplyTarget>>,
}
impl CallWorld<'_> {
    /// dialyzer_dep finalizes call targets with ordsets:from_list: labels ascend,
    /// then the atom external. Preserve missing sites for operator fallback.
    pub fn add_closure_sites(
        &mut self,
        program: &crate::program::Program,
        labels: &crate::labels::Labels,
        dependencies: &crate::closure_dependencies::ClosureDependencies,
    ) -> std::result::Result<(), String> {
        for (site, targets) in &dependencies.call_targets {
            let mut local = Vec::new();
            for target in &targets.local {
                let function = program
                    .functions
                    .get(target.0)
                    .ok_or("missing closure target")?;
                local.push(
                    labels
                        .label(NodeRef {
                            module: function.module,
                            node: function.node.0,
                        })
                        .ok_or("closure target has no label")?,
                );
            }
            local.sort();
            local.dedup();
            let mut ordered = local
                .into_iter()
                .map(|l| ApplyTarget::Local(FunctionKey::Label(l)))
                .collect::<Vec<_>>();
            if targets.external {
                ordered.push(ApplyTarget::External);
            }
            self.call_sites.insert(*site, ordered);
        }
        Ok(())
    }
}
#[derive(Debug)]
pub enum CallControllerError {
    Lattice(LatticeError),
    Transfer(CallError),
    Bif(TransferError),
    State(StateError),
    Bind(BindError),
    Print(PrintError),
    Producer(ProducerError),
    Form(crate::form_error::FormError),
    Boundary(String),
}
macro_rules! conversion {
    ($v:ident,$t:ty) => {
        impl From<$t> for CallControllerError {
            fn from(e: $t) -> Self {
                Self::$v(e)
            }
        }
    };
}
conversion!(Lattice, LatticeError);
conversion!(Transfer, CallError);
conversion!(Bif, TransferError);
conversion!(State, StateError);
conversion!(Bind, BindError);
conversion!(Print, PrintError);
conversion!(Producer, ProducerError);
conversion!(Form, crate::form_error::FormError);
conversion!(Boundary, String);
type Result<T> = std::result::Result<T, CallControllerError>;
pub struct CallBound<W> {
    pub map: DataflowMap,
    pub value: Type,
    pub state: BinderState<W>,
}
pub struct CallOperands<'a> {
    pub tree: NodeRef,
    pub module: NodeRef,
    pub name: NodeRef,
    pub args: &'a [NodeRef],
    pub module_type: Type,
    pub name_type: Type,
    pub actual: &'a [Type],
}
pub struct ApplyOperands<'a> {
    pub tree: NodeRef,
    pub operator: NodeRef,
    pub args: &'a [NodeRef],
    pub operator_type: Type,
    pub actual: &'a [Type],
}
pub struct BodyCalls<'a, 'b, 'c> {
    pub patterns: &'a mut Patterns<'b, 'c>,
    pub producer: &'a GuardProducer<'a, 'c, 'c>,
    pub functions: &'a mut FunctionState,
    pub bifs: &'a Registry,
    pub world: &'a CallWorld<'a>,
}
impl BodyCalls<'_, '_, '_> {
    fn format_args<W>(
        &self,
        args: &[NodeRef],
        types: &[Type],
        state: &BinderState<W>,
    ) -> Result<String> {
        Ok(self.producer.format_args(
            &args
                .iter()
                .copied()
                .map(ArgumentRef::Original)
                .collect::<Vec<_>>(),
            &types
                .iter()
                .cloned()
                .map(crate::native_type_operand::NativeTypeOperand::Canonical)
                .collect::<Vec<_>>(),
            state,
        )?)
    }
    fn format_type<W>(&self, t: &Type, state: &BinderState<W>) -> Result<String> {
        Ok(state.presentation_context().type_to_string(t)?)
    }
    fn print(&self, node: NodeRef) -> Result<String> {
        Ok(CorePrinter {
            patterns: self.patterns,
        }
        .format_cerl(node)?)
    }
    fn emit<W: Clone + PartialEq + From<OpaqueMatch> + From<CallWarning>>(
        &mut self,
        node: NodeRef,
        tag: &str,
        message: NativeValue,
        forced: bool,
        state: &mut BinderState<W>,
    ) -> Result<()> {
        let b = Binder {
            patterns: self.patterns,
        };
        let p = PatternRef::Core(node);
        let ann = b.annotation(p)?;
        if state.warnings.warning_mode && (forced || !ann.is_generated()) {
            let info = b.warning_info(p, state)?;
            state.warnings.add(
                ann,
                CallWarning::new(node, tag, &info, &message).into(),
                forced,
            );
        }
        Ok(())
    }
    fn message(tag: &str, parts: Vec<NativeValue>) -> NativeValue {
        NativeValue::Tuple(vec![
            NativeValue::Atom(tag.into()),
            NativeValue::List(parts),
        ])
    }
    fn select(tree: NodeRef, args: &[NodeRef], indices: &[usize]) -> Result<NodeRef> {
        match indices.first() {
            None => Ok(tree),
            Some(i) => args
                .get(i.saturating_sub(1))
                .copied()
                .ok_or_else(|| LatticeError::FunctionClause.into()),
        }
    }
    fn enter_args(&self, args: &[NodeRef], types: &[Type], map: &mut DataflowMap) -> Result<()> {
        if args.len() != types.len() {
            return Err(LatticeError::FunctionClause.into());
        }
        for (a, t) in args.iter().zip(types) {
            self.patterns.nodes.enter_type(*a, t.clone(), map)?;
        }
        Ok(())
    }
    fn atoms(t: &Type) -> Result<Option<Vec<String>>> {
        match t.get(TypeGetter::AtomValues)? {
            TypeValue::Atoms(Known::Known(xs)) => Ok(Some(xs)),
            TypeValue::Atoms(Known::Unknown) => Ok(None),
            _ => Err(LatticeError::FunctionClause.into()),
        }
    }
    pub fn finish_call<W: Clone + PartialEq + From<OpaqueMatch> + From<CallWarning>>(
        &mut self,
        input: CallOperands<'_>,
        mut map: DataflowMap,
        mut state: BinderState<W>,
    ) -> Result<CallBound<W>> {
        let CallOperands {
            tree,
            module,
            name,
            args,
            module_type,
            name_type,
            actual,
        } = input;
        let mt = Type::builtin(BuiltinType::Module)?.meet(&module_type)?;
        let ft = Type::all_atoms().meet(&name_type)?;
        self.enter_args(&[module, name], &[mt.clone(), ft.clone()], &mut map)?;
        if mt.is_none() || ft.is_none() || actual.iter().any(Type::is_none) {
            let bad = if mt.is_none() && !module_type.is_none() {
                Some((module, &module_type, Type::builtin(BuiltinType::Module)?))
            } else if ft.is_none() && !name_type.is_none() {
                Some((name, &name_type, Type::all_atoms()))
            } else {
                None
            };
            if let Some((source, original, expected)) = bad {
                let ms = self.print(module)?;
                let fs = self.print(name)?;
                let argstr = self.format_args(args, actual, &state)?;
                let culprit = self.print(source)?;
                let message = Self::message(
                    "app_call",
                    vec![
                        string(&ms),
                        string(&fs),
                        string(&argstr),
                        string(&culprit),
                        string(&self.format_type(&expected, &state)?),
                        string(&self.format_type(original, &state)?),
                    ],
                );
                self.emit(source, "warn_failing_call", message, false, &mut state)?;
            }
            return Ok(CallBound {
                map,
                value: Type::none(),
                state,
            });
        }
        if mt.predicate(TypePredicate::Atom)?
            && let (Some(m), Some(f)) = (Self::atoms(&mt)?, Self::atoms(&ft)?)
            && m.len() == 1
            && f.len() == 1
        {
            let mfa = Mfa {
                module: m[0].clone(),
                function: f[0].clone(),
                arity: args.len(),
            };
            return self
                .candidates(tree, args, actual, map, state, &[Candidate::Remote(mfa)])
                .map(|x| match x {
                    CandidateResult::Normal(x) => x,
                    CandidateResult::External(_) => {
                        unreachable!("remote candidates do not use external fallback")
                    }
                });
        }
        Ok(CallBound {
            map,
            value: Type::any(),
            state,
        })
    }
    pub fn finish_apply<W: Clone + PartialEq + From<OpaqueMatch> + From<CallWarning>>(
        &mut self,
        input: ApplyOperands<'_>,
        map: DataflowMap,
        state: BinderState<W>,
    ) -> Result<CallBound<W>> {
        let ApplyOperands {
            tree,
            operator,
            args,
            operator_type,
            actual,
        } = input;
        if actual.iter().any(Type::is_none) {
            return Ok(CallBound {
                map,
                value: Type::none(),
                state,
            });
        }
        let candidates = self
            .world
            .call_sites
            .get(&tree)
            .cloned()
            .unwrap_or_else(|| vec![ApplyTarget::External])
            .into_iter()
            .map(|x| match x {
                ApplyTarget::Local(k) => Candidate::Local(k),
                ApplyTarget::External => Candidate::External,
            })
            .collect::<Vec<_>>();
        match self.candidates(tree, args, actual, map.clone(), state, &candidates)? {
            CandidateResult::Normal(x) => Ok(x),
            CandidateResult::External(mut state) => {
                let refined = operator_type.meet(&Type::function(
                    Some(vec![Type::any(); args.len()]),
                    Type::any(),
                ))?;
                if refined.is_none() {
                    let message = Self::message(
                        "fun_app_no_fun",
                        vec![
                            string(&self.print(operator)?),
                            string(&self.format_type(&operator_type, &state)?),
                            NativeValue::Integer(args.len().into()),
                        ],
                    );
                    self.emit(tree, "warn_failing_call", message, false, &mut state)?;
                    return Ok(CallBound {
                        map,
                        value: Type::none(),
                        state,
                    });
                }
                let TypeValue::Types(Known::Known(domains)) =
                    refined.get(TypeGetter::FunctionArgs)?
                else {
                    return Err(LatticeError::FunctionClause.into());
                };
                let new = actual
                    .iter()
                    .zip(&domains)
                    .map(|(a, b)| a.meet(b))
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                let bad = new
                    .iter()
                    .enumerate()
                    .filter_map(|(i, t)| t.is_none().then_some(i + 1))
                    .collect::<Vec<_>>();
                let mut map = map;
                let value = if !bad.is_empty() {
                    let msg = Self::message(
                        "fun_app_args",
                        vec![
                            NativeValue::List(
                                bad.iter()
                                    .map(|i| NativeValue::Integer((*i).into()))
                                    .collect(),
                            ),
                            string(&self.format_args(args, actual, &state)?),
                            string(&self.format_type(&operator_type, &state)?),
                        ],
                    );
                    self.emit(
                        Self::select(tree, args, &bad)?,
                        "warn_failing_call",
                        msg,
                        false,
                        &mut state,
                    )?;
                    Type::none()
                } else {
                    self.enter_args(args, &new, &mut map)?;
                    let TypeValue::Type(range) = refined.get(TypeGetter::FunctionRange)? else {
                        return Err(LatticeError::FunctionClause.into());
                    };
                    if range.is_unit() { Type::none() } else { range }
                };
                self.patterns
                    .nodes
                    .enter_type(operator, refined, &mut map)?;
                Ok(CallBound { map, value, state })
            }
        }
    }
    fn candidates<W: Clone + PartialEq + From<OpaqueMatch> + From<CallWarning>>(
        &mut self,
        tree: NodeRef,
        args: &[NodeRef],
        actual: &[Type],
        mut map: DataflowMap,
        mut state: BinderState<W>,
        candidates: &[Candidate],
    ) -> Result<CandidateResult<W>> {
        let mut acc = AlternativeAccumulator::new(args.len());
        for candidate in candidates {
            if matches!(candidate, Candidate::External) {
                acc.external(actual)?;
                continue;
            }
            let (name, sig, local_return, local_key, escaping) = match candidate {
                Candidate::Remote(mfa) => (
                    Some(mfa.clone()),
                    self.world.named_signature(mfa)?,
                    Type::any(),
                    None,
                    true,
                ),
                Candidate::Local(key) => {
                    let def = self
                        .functions
                        .definitions()
                        .get(key)
                        .ok_or(StateError::Missing(*key))?;
                    let name = def.name.as_ref().map(|n| Mfa {
                        module: n.module.clone(),
                        function: n.name.clone(),
                        arity: n.arity,
                    });
                    let raw = match &def.raw_signature {
                        Some(signature) => Some(signature.clone()),
                        None => match &name {
                            Some(n) => self.world.named_signature(n)?,
                            None => match key {
                                FunctionKey::Label(l) => {
                                    self.world.anonymous_signatures.get(l).cloned()
                                }
                                FunctionKey::Top => None,
                            },
                        },
                    };
                    let ret = self
                        .functions
                        .entries()
                        .get(key)
                        .ok_or(StateError::Missing(*key))?
                        .signature()
                        .result
                        .clone();
                    (name, raw, ret, Some(*key), def.escaping)
                }
                Candidate::External => unreachable!(),
            };
            let contract = name
                .as_ref()
                .map(|n| self.world.active_contract(n))
                .transpose()?
                .flatten();
            let crange = |types: &[Type]| {
                contract
                    .as_ref()
                    .unwrap()
                    .summary
                    .return_for(types)
                    .map_err(|e| match e {
                        crate::contract_runtime::ContractError::Lattice(e) => CallError::Lattice(e),
                        other => CallError::Provider {
                            contribution: call_transfer::Contribution::Contract,
                            message: other.to_string(),
                        },
                    })
            };
            let bargs = if matches!(candidate, Candidate::Remote(_))
                && name.as_ref().is_some_and(|n| self.bifs.is_known(n))
            {
                Some(self.bifs.argument_domains(name.as_ref().unwrap())?)
            } else {
                None
            };
            let brange = |types: &[Type]| {
                self.bifs
                    .transfer(name.as_ref().unwrap(), types)
                    .map(|x| x.result)
                    .map_err(|e| match e {
                        TransferError::NativeException(e) => CallError::NativeRange {
                            contribution: call_transfer::Contribution::Bif,
                            error: e,
                        },
                        other => CallError::Provider {
                            contribution: call_transfer::Contribution::Bif,
                            message: other.to_string(),
                        },
                    })
            };
            let transfer = call_transfer::evaluate(CallInput {
                actual,
                success: sig.as_ref(),
                contract: contract.as_ref().map(|c| DependentSummary {
                    arguments: &c.summary.arguments,
                    range: &crange,
                }),
                bif: bargs.as_ref().map(|a| DependentSummary {
                    arguments: a,
                    range: &brange,
                }),
                local_return: &local_return,
                module: &state.module,
            })?;
            // Opacity warnings are part of State, not State3. Consensus must retain them.
            if let (Some(name), Some(reason), Some(first)) = (
                &name,
                transfer.opacity.reason,
                transfer.opacity.arguments.first(),
            ) {
                let conflicts = transfer
                    .opacity
                    .arguments
                    .iter()
                    .map(|e| {
                        Ok(NativeValue::Tuple(vec![
                            NativeValue::Integer(e.index.into()),
                            e.conflicting_type.to_native_value(),
                            string(&self.format_type(&e.conflicting_type, &state)?),
                        ]))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let description = match reason {
                    OpacityReason::ExpectedOpaque => "call_without_opaque",
                    OpacityReason::ExpectedTransparent => "call_with_opaque",
                };
                let message = Self::message(
                    description,
                    vec![
                        NativeValue::Atom(name.module.clone()),
                        NativeValue::Atom(name.function.clone()),
                        string(&self.format_args(args, actual, &state)?),
                        NativeValue::List(conflicts),
                        NativeValue::List(
                            transfer
                                .opacity
                                .expected
                                .iter()
                                .map(Type::to_native_value)
                                .collect(),
                        ),
                    ],
                );
                self.emit(
                    Self::select(tree, args, &[first.index])?,
                    "warn_opaque",
                    message,
                    false,
                    &mut state,
                )?;
            }
            let baseline = state.warnings.warnings.clone();
            match &transfer.diagnostic {
                Diagnostic::None => {}
                Diagnostic::ContractRange { without_contract } => {
                    let contract = contract.as_ref().ok_or_else(|| {
                        CallControllerError::Boundary(
                            "contract range without active contract".into(),
                        )
                    })?;
                    let cs = crate::contract_presentation::contract_to_string(&contract.capture)?;
                    let mfa = name.as_ref().ok_or(LatticeError::FunctionClause)?;
                    let args = self.format_args(args, actual, &state)?;
                    let ret = crate::lattice::PresentationContext::default()
                        .type_to_string(without_contract)?;
                    self.emit(
                        tree,
                        "warn_contract_range",
                        Self::message(
                            "contract_range",
                            vec![
                                string(&cs),
                                NativeValue::Atom(mfa.module.clone()),
                                NativeValue::Atom(mfa.function.clone()),
                                NativeValue::Integer(mfa.arity.into()),
                                string(&args),
                                string(&ret),
                            ],
                        ),
                        false,
                        &mut state,
                    )?;
                }
                Diagnostic::FailingCall {
                    reason,
                    argument_indices,
                    expected_signature,
                    ..
                } => {
                    let argstr = self.format_args(args, actual, &state)?;
                    let ci = if let Some(c) = &contract {
                        NativeValue::Tuple(vec![
                            NativeValue::Atom(
                                crate::contract_presentation::is_overloaded(&c.capture)?
                                    .to_string(),
                            ),
                            string(&crate::contract_presentation::contract_to_string(
                                &c.capture,
                            )?),
                        ])
                    } else {
                        NativeValue::Tuple(vec![
                            NativeValue::Atom("false".into()),
                            NativeValue::Atom("none".into()),
                        ])
                    };
                    let TypeValue::Types(Known::Known(domains)) =
                        expected_signature.get(TypeGetter::FunctionArgs)?
                    else {
                        return Err(LatticeError::FunctionClause.into());
                    };
                    let sigargs = format!(
                        "({})",
                        domains
                            .iter()
                            .map(|t| self.format_type(t, &state))
                            .collect::<Result<Vec<_>>>()?
                            .join(",")
                    );
                    let TypeValue::Type(ret) = expected_signature.get(TypeGetter::FunctionRange)?
                    else {
                        return Err(LatticeError::FunctionClause.into());
                    };
                    let mut parts = vec![
                        string(&argstr),
                        NativeValue::List(
                            argument_indices
                                .iter()
                                .map(|i| NativeValue::Integer((*i).into()))
                                .collect(),
                        ),
                        NativeValue::Atom(
                            match reason {
                                FailureReason::OnlySignature => "only_sig",
                                FailureReason::OnlyContract => "only_contract",
                                FailureReason::Both => "both",
                            }
                            .into(),
                        ),
                        string(&sigargs),
                        string(&self.format_type(&ret, &state)?),
                        ci,
                    ];
                    let tag = if let Some(n) = &name {
                        parts.insert(0, NativeValue::Atom(n.function.clone()));
                        parts.insert(0, NativeValue::Atom(n.module.clone()));
                        "call"
                    } else {
                        "apply"
                    };
                    let forced = name.as_ref().is_some_and(|n| {
                        n.module == "erlang" && n.function == "is_record" && n.arity == 3
                    });
                    self.emit(
                        Self::select(tree, args, argument_indices)?,
                        "warn_failing_call",
                        Self::message(tag, parts),
                        forced,
                        &mut state,
                    )?;
                }
            }
            if let Some(key) = local_key {
                if !escaping {
                    let forwarded = actual
                        .iter()
                        .map(|t| t.limit(3))
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    self.functions.forward_arguments(key, &forwarded)?;
                }
            } else if let Some(name) = &name {
                self.bif_warnings(tree, name, &transfer.arguments.conjunction, &mut state)?;
            }
            let added = state.warnings.remove_added(&baseline);
            acc.add(&transfer, added)?;
        }
        state.warnings.add_all(acc.warnings);
        if acc.had_external {
            return Ok(CandidateResult::External(state));
        }
        self.enter_args(args, &acc.arguments, &mut map)?;
        Ok(CandidateResult::Normal(CallBound {
            map,
            value: acc.result,
            state,
        }))
    }
    fn bif_warnings<W: Clone + PartialEq + From<OpaqueMatch> + From<CallWarning>>(
        &mut self,
        tree: NodeRef,
        name: &Mfa,
        args: &[Type],
        state: &mut BinderState<W>,
    ) -> Result<()> {
        if name.module != "erlang" || name.arity != 2 {
            return Ok(());
        }
        let operator = match name.function.as_str() {
            "=:=" => EqualityOperator::ExactEqual,
            "=/=" => EqualityOperator::ExactNotEqual,
            "==" => EqualityOperator::Equal,
            "/=" => EqualityOperator::NotEqual,
            _ => return Ok(()),
        };
        let [left, right] = args else {
            return Err(LatticeError::FunctionClause.into());
        };
        // Shared helper returns prepend order; source adds opacity before exact comparison.
        for warning in comparison_warnings(left, operator, right, &state.module)?
            .into_iter()
            .rev()
        {
            let (category, tag, left, right) = match warning {
                ComparisonWarning::Opaque { left, right, .. } => {
                    ("warn_opaque", "opaque_compare", left, right)
                }
                ComparisonWarning::Exact { left, right, .. } => {
                    ("warn_matching", "exact_compare", left, right)
                }
            };
            let message = Self::message(
                tag,
                vec![
                    string(&self.format_type(&left, state)?),
                    NativeValue::Atom(name.function.clone()),
                    string(&self.format_type(&right, state)?),
                ],
            );
            self.emit(tree, category, message, false, state)?;
        }
        Ok(())
    }
}
enum Candidate {
    Local(FunctionKey),
    Remote(Mfa),
    External,
}
enum CandidateResult<W> {
    Normal(CallBound<W>),
    External(BinderState<W>),
}

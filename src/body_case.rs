use super::*;
use crate::{
    dataflow_map::JoinScope,
    guard_control::{GuardEnvironment, GuardError, GuardFailureKind, GuardReason},
    guard_ops::GuardEval,
    guard_warning::GuardWarning,
    lattice::NativeValue,
    warning_payload::{NativeWarning, string},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseWarning {
    pub warning: NativeWarning,
}
impl CaseWarning {
    pub fn native_warning(&self) -> NativeWarning {
        self.warning.clone()
    }
}
enum CaseMessage {
    Text {
        tag: &'static str,
        args: Vec<NativeValue>,
    },
    Guard(GuardReason),
}
impl CaseMessage {
    fn tag(&self) -> &'static str {
        match self {
            Self::Text { tag, .. } => tag,
            Self::Guard(GuardReason::GuardFailure {
                kind: GuardFailureKind::GuardFail,
                ..
            }) => "guard_fail",
            Self::Guard(GuardReason::GuardFailure {
                kind: GuardFailureKind::NegGuardFail,
                ..
            }) => "neg_guard_fail",
            Self::Guard(GuardReason::RecordMatching { .. }) => "record_matching",
        }
    }
    fn warning(&self, category: &str, info: &WarningInfo) -> NativeWarning {
        match self {
            Self::Text { tag, args } => NativeWarning::from_value(
                category,
                info,
                &NativeValue::Tuple(vec![
                    NativeValue::Atom((*tag).into()),
                    NativeValue::List(args.clone()),
                ]),
            ),
            Self::Guard(reason) => {
                NativeWarning::from_parts(category, info, &reason.native_message())
            }
        }
    }
}
struct Pending {
    node: NodeRef,
    message: CaseMessage,
    forced: bool,
}
pub(super) struct CaseCursor {
    pub(super) node: NodeRef,
    pub(super) argument: NodeRef,
    pub(super) original: Type,
    pub(super) residual: Type,
    pub(super) scope: JoinScope,
    pub(super) remaining: VecDeque<NodeRef>,
    pub(super) branches: Vec<DataflowMap>,
    pub(super) successes: Vec<(NodeRef, Type)>,
    pending: Vec<Pending>,
    clause: Option<NodeRef>,
}
impl CaseCursor {
    pub(super) fn new(
        node: NodeRef,
        argument: NodeRef,
        value: Value,
        clauses: Vec<NodeRef>,
    ) -> Self {
        Self {
            node,
            argument,
            original: value.typ.clone(),
            residual: value.typ,
            scope: value.map.begin_join(),
            remaining: clauses.into(),
            branches: vec![],
            successes: vec![],
            pending: vec![],
            clause: None,
        }
    }
    pub(super) fn body(&mut self, value: Value) {
        if !value.typ.is_none() {
            self.branches.push(value.map);
            self.successes.insert(
                0,
                (self.clause.take().expect("active case clause"), value.typ),
            );
        }
    }
}
impl BodyFlow<'_, '_, '_> {
    fn clause_substitutions(
        &self,
        argument: NodeRef,
        patterns: &[NodeRef],
        map: &mut DataflowMap,
    ) -> Result<()> {
        match self.patterns.nodes.expression(argument)? {
            CoreExpr::Values(args) => {
                let mut todo: Vec<_> = Self::refs(argument, args)
                    .into_iter()
                    .zip(patterns.iter().copied())
                    .rev()
                    .collect();
                if args.len() != patterns.len() {
                    return Err(LatticeError::FunctionClause.into());
                }
                while let Some((a, p)) = todo.pop() {
                    match (
                        self.patterns.nodes.expression(a)?,
                        self.patterns.nodes.expression(p)?,
                    ) {
                        (CoreExpr::Var(_), CoreExpr::Var(_)) => {
                            self.patterns.nodes.enter_substitution(a, p, map)?
                        }
                        (CoreExpr::Var(_), CoreExpr::Alias { pattern, .. }) => {
                            self.patterns.nodes.enter_substitution(
                                a,
                                NodeRef {
                                    module: p.module,
                                    node: pattern.0,
                                },
                                map,
                            )?
                        }
                        (CoreExpr::Literal(_), CoreExpr::Literal(_)) => {}
                        (a_expr, p_expr)
                            if std::mem::discriminant(a_expr) == std::mem::discriminant(p_expr) =>
                        {
                            let aa = self.patterns.nodes.native_subtrees(a)?;
                            let pp = self.patterns.nodes.native_subtrees(p)?;
                            if aa.len() != pp.len() {
                                return Err(LatticeError::FunctionClause.into());
                            }
                            todo.extend(aa.into_iter().zip(pp).rev());
                        }
                        _ => {}
                    }
                }
            }
            CoreExpr::Var(_) => {
                let [pattern] = patterns else {
                    return Err(LatticeError::FunctionClause.into());
                };
                self.patterns
                    .nodes
                    .enter_substitution(argument, *pattern, map)?;
            }
            _ => {}
        }
        Ok(())
    }
    fn clause_args(typ: &Type, count: usize) -> Result<Vec<Type>> {
        if typ.is_any() {
            Ok(vec![typ.clone(); count])
        } else {
            Self::type_list(typ)
        }
    }
    fn clause_strings<W>(
        &self,
        patterns: &[NodeRef],
        typ: &Type,
        state: &BinderState<W>,
    ) -> Result<Vec<NativeValue>> {
        let pats = patterns
            .iter()
            .copied()
            .map(PatternRef::Core)
            .collect::<Vec<_>>();
        let pattern = (CorePrinter {
            patterns: self.patterns,
        })
        .format_patterns(&pats)?;
        let actual = state.presentation_context().type_to_string(typ)?;
        Ok(vec![string(&pattern), string(&actual)])
    }
    fn clause_failure<
        W: Clone
            + PartialEq
            + From<OpaqueMatch>
            + From<BodyWarning>
            + From<GuardWarning>
            + From<crate::call_warning::CallWarning>
            + From<CaseWarning>,
    >(
        &mut self,
        clause: NodeRef,
        pats: &[NodeRef],
        actual: &Type,
        failure: &crate::binder::BindFailure,
        state: &BinderState<W>,
        forced: bool,
    ) -> Result<Pending> {
        let (tag, args) = match failure.kind {
            FailureKind::Bind => ("pattern_match", self.clause_strings(pats, actual, state)?),
            FailureKind::Record => {
                let pattern = (CorePrinter {
                    patterns: self.patterns,
                })
                .format_patterns(&failure.patterns)?;
                let typ = state
                    .presentation_context()
                    .type_to_string(&failure.actual)?;
                ("record_match", vec![string(&pattern), string(&typ)])
            }
        };
        Ok(Pending {
            node: clause,
            message: CaseMessage::Text { tag, args },
            forced,
        })
    }
    fn lc_default(&mut self, clause: NodeRef, pats: &[NodeRef], guard: NodeRef) -> Result<bool> {
        if !(Binder {
            patterns: self.patterns,
        })
        .annotation(PatternRef::Core(clause))?
        .is_generated()
            || self.atom(guard).ok() != Some("true")
        {
            return Ok(false);
        }
        let cons_vars = |pat: NodeRef| -> Result<bool> {
            let CoreExpr::Cons { head, tail } = self.patterns.nodes.expression(pat)? else {
                return Ok(false);
            };
            Ok(matches!(
                self.patterns.nodes.expression(NodeRef {
                    module: pat.module,
                    node: head.0
                })?,
                CoreExpr::Var(_)
            ) && matches!(
                self.patterns.nodes.expression(NodeRef {
                    module: pat.module,
                    node: tail.0
                })?,
                CoreExpr::Var(_)
            ))
        };
        Ok(match pats {
            [p] => cons_vars(*p)?,
            [p, q] => {
                cons_vars(*p)? && matches!(self.patterns.nodes.expression(*q)?, CoreExpr::Var(_))
            }
            _ => false,
        })
    }
    pub(super) fn next_body_clause<
        W: Clone
            + PartialEq
            + From<OpaqueMatch>
            + From<BodyWarning>
            + From<GuardWarning>
            + From<crate::call_warning::CallWarning>
            + From<CaseWarning>,
    >(
        &mut self,
        mut cursor: CaseCursor,
        todo: &mut Vec<Frame>,
        values: &mut Vec<Value>,
        state: &mut BinderState<W>,
    ) -> Result<()> {
        while let Some(clause) = cursor.remaining.pop_front() {
            let CoreExpr::Clause {
                patterns,
                guard,
                body,
            } = self.patterns.nodes.expression(clause)?.clone()
            else {
                return Err(LatticeError::FunctionClause.into());
            };
            let pats = Self::refs(clause, &patterns);
            let guard = NodeRef {
                module: clause.module,
                node: guard.0,
            };
            let mut map = cursor.scope.branch();
            self.patterns.nodes.mark_fresh(&pats, &mut map)?;
            self.clause_substitutions(cursor.argument, &pats, &mut map)?;
            self.refresh_letrec(state)?;
            let references = pats
                .iter()
                .copied()
                .map(PatternRef::Core)
                .collect::<Vec<_>>();
            if cursor.residual.is_none() {
                if state.warnings.warning_mode {
                    let original = Self::clause_args(&cursor.original, pats.len())?;
                    let pending = match (Binder {
                        patterns: self.patterns,
                    })
                    .bind(
                        &references,
                        &original,
                        &map,
                        state,
                        Direction::Forward,
                    ) {
                        Ok(bound) => Pending {
                            node: clause,
                            message: CaseMessage::Text {
                                tag: "pattern_match_cov",
                                args: self.clause_strings(&pats, &cursor.original, &bound.state)?,
                            },
                            forced: false,
                        },
                        Err(BindError::Failure(mut f)) => {
                            f.patterns = references.clone();
                            f.actual = cursor.original.clone();
                            self.clause_failure(clause, &pats, &cursor.original, &f, state, false)?
                        }
                        Err(e) => return Err(e.into()),
                    };
                    cursor.pending.insert(0, pending);
                }
                continue;
            }
            let actual = Self::clause_args(&cursor.residual, pats.len())?;
            let bound = match (Binder {
                patterns: self.patterns,
            })
            .bind(&references, &actual, &map, state, Direction::Forward)
            {
                Ok(bound) => bound,
                Err(BindError::Failure(f)) => {
                    if state.warnings.warning_mode {
                        let forced = !self.lc_default(clause, &pats, guard)?;
                        let warning = self.clause_failure(
                            clause,
                            &pats,
                            &cursor.residual,
                            &f,
                            state,
                            forced,
                        )?;
                        cursor.pending.insert(0, warning);
                    }
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            *state = bound.state;
            map = bound.map;
            let matched = Type::product(bound.types);
            match (Binder {
                patterns: self.patterns,
            })
            .bind(
                &[PatternRef::Core(cursor.argument)],
                &[matched],
                &map,
                state,
                Direction::Reverse,
            ) {
                Ok(bound) => map = bound.map,
                Err(BindError::Failure(_)) => {}
                Err(e) => return Err(e.into()),
            }
            let nodes = self.patterns.nodes;
            let approximation = nodes
                .clause_underapprox(
                    clause.module,
                    &patterns,
                    crate::core::NodeId(guard.node),
                    |node| {
                        self.patterns
                            .refold(PatternRef::Core(node))
                            .map(|_| ())
                            .map_err(|e| {
                                crate::underapprox::UnderapproxError::Boundary(format!(
                                    "case refold: {e:?}"
                                ))
                            })
                    },
                )
                .map_err(BodyError::Underapprox)?;
            let guard_argument = cursor.residual.clone();
            cursor.residual =
                Type::product(Self::type_list(&cursor.residual)?).subtract(&approximation)?;
            let controller = self.guards.ok_or(BodyError::Unported {
                node: guard,
                kind: "case guard controller absent",
            })?;
            match controller.evaluate(
                guard,
                &map,
                &GuardEnvironment::new(),
                GuardEval::Positive,
                state,
            ) {
                Ok(bound) => {
                    map = bound.map;
                    *state = bound.state;
                }
                Err(GuardError::Failure { reason, .. }) => {
                    let pending =
                        self.guard_clause_warning(clause, &pats, &guard_argument, reason, state)?;
                    cursor.pending.insert(0, pending);
                    continue;
                }
                Err(e) => return Err(BodyError::Guard(Box::new(e))),
            }
            cursor.clause = Some(clause);
            todo.push(Frame::CaseBody(cursor));
            todo.push(Frame::Visit(
                NodeRef {
                    module: clause.module,
                    node: body.0,
                },
                map,
            ));
            return Ok(());
        }
        let mut typ = Type::none();
        for (_, value) in &cursor.successes {
            typ = typ.join(value)?;
        }
        let force = !(Binder {
            patterns: self.patterns,
        })
        .annotation(PatternRef::Core(cursor.node))?
        .is_generated()
            || typ.is_none();
        let mut pending = self.opaque_case_warnings(&cursor.successes, state)?;
        pending.extend(cursor.pending);
        for pending in pending {
            let binder = Binder {
                patterns: self.patterns,
            };
            let pattern = PatternRef::Core(pending.node);
            let annotation = binder.annotation(pattern)?;
            let forced = pending.forced && force;
            if state.warnings.warning_mode && (forced || !annotation.is_generated()) {
                let category = match pending.message.tag() {
                    "opaque_union" => "warn_opaque_union",
                    "opaque_guard" | "opaque_match" => "warn_opaque",
                    _ => "warn_matching",
                };
                let info = binder.warning_info(pattern, state)?;
                let warning = pending.message.warning(category, &info);
                state
                    .warnings
                    .add(annotation, CaseWarning { warning }.into(), forced);
            }
        }
        values.push(Value {
            map: cursor.scope.finish(&cursor.branches)?,
            typ,
        });
        Ok(())
    }
    fn guard_clause_warning<W>(
        &mut self,
        clause: NodeRef,
        pats: &[NodeRef],
        actual: &Type,
        reason: Option<GuardReason>,
        state: &BinderState<W>,
    ) -> Result<Pending> {
        let (node, message) = match reason {
            None => (
                clause,
                CaseMessage::Text {
                    tag: if pats.is_empty() {
                        "guard_fail"
                    } else {
                        "guard_fail_pat"
                    },
                    args: if pats.is_empty() {
                        vec![]
                    } else {
                        self.clause_strings(pats, actual, state)?
                    },
                },
            ),
            Some(reason @ GuardReason::GuardFailure { guard, .. }) => {
                (guard, CaseMessage::Guard(reason))
            }
            Some(reason @ GuardReason::RecordMatching { guard, .. }) => {
                if !(Binder {
                    patterns: self.patterns,
                })
                .annotation(PatternRef::Core(guard))?
                .is_generated()
                {
                    return Err(BodyError::NativeWarningTag("record_matching"));
                }
                (guard, CaseMessage::Guard(reason))
            }
        };
        let node = if (Binder {
            patterns: self.patterns,
        })
        .annotation(PatternRef::Core(node))?
        .is_generated()
        {
            clause
        } else {
            node
        };
        Ok(Pending {
            node,
            message,
            forced: false,
        })
    }
    fn opaque_case_warnings<W>(
        &self,
        successes: &[(NodeRef, Type)],
        state: &BinderState<W>,
    ) -> Result<Vec<Pending>> {
        // Without any nominal (or the opaque sentinel) among the clause
        // results, no element can be opaque and `elements` cannot fail, so
        // the answer is empty; skip building native values.
        if successes.iter().all(|(_, typ)| !typ.contains_nominal()) {
            if crate::lattice::verify_fast_paths() {
                assert!(
                    self.opaque_case_warnings_full(successes, state)?.is_empty(),
                    "opaque_case_warnings fast path"
                );
            }
            return Ok(vec![]);
        }
        self.opaque_case_warnings_full(successes, state)
    }
    fn opaque_case_warnings_full<W>(
        &self,
        successes: &[(NodeRef, Type)],
        state: &BinderState<W>,
    ) -> Result<Vec<Pending>> {
        if !has_distinct_native_returns(successes.iter().map(|(_, typ)| typ)) {
            return Ok(vec![]);
        }
        let mut flat = vec![];
        for (_, ty) in successes {
            flat.extend(ty.elements()?);
        }
        let Some(opaque) = flat.iter().find(|ty| ty.is_opaque(&state.module)) else {
            return Ok(vec![]);
        };
        let TypeValue::Module(module) = opaque.get(TypeGetter::NominalModule)? else {
            return Err(LatticeError::FunctionClause.into());
        };
        if flat.iter().all(|ty|ty.is_any() || ty.is_impossible() || (ty.is_opaque(&state.module) && matches!(ty.get(TypeGetter::NominalModule),Ok(TypeValue::Module(m)) if m==module))) {return Ok(vec![]);}
        successes
            .iter()
            .filter(|(_, ty)| !ty.is_any() && !ty.is_impossible())
            .map(|(node, ty)| {
                Ok(Pending {
                    node: *node,
                    message: CaseMessage::Text {
                        tag: "opaque_union",
                        args: vec![
                            NativeValue::Atom(
                                if ty.is_opaque(&state.module) {
                                    "true"
                                } else {
                                    "false"
                                }
                                .into(),
                            ),
                            string(&state.presentation_context().type_to_string(ty)?),
                        ],
                    },
                    forced: false,
                })
            })
            .collect()
    }
}

fn has_distinct_native_returns<'a>(mut types: impl Iterator<Item = &'a Type>) -> bool {
    let Some(first) = types.next() else {
        return false;
    };
    let mut native_first = None;
    types.any(|typ| {
        if typ == first {
            return false;
        }
        let native_first = native_first.get_or_insert_with(|| first.to_native_value());
        typ.to_native_value() != *native_first
    })
}

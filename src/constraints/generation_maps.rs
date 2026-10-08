use super::*;
use crate::{
    constraints::derived::MapOperation,
    patterns::{LiteralRef, PairOperation, PatternRef, Shape},
};
impl Generator<'_> {
    pub(super) fn next_map_pair(
        &mut self,
        mut context: MapContext,
        todo: &mut Vec<Frame>,
        values: &mut Vec<Type>,
    ) -> Result<()> {
        values.truncate(context.base);
        if context.next < context.entries.len() {
            let entry = context.entries[context.next];
            context.next += 1;
            let Shape::Pair {
                operation,
                key,
                value,
            } = self
                .patterns
                .shape(entry)
                .map_err(GenerationError::Boundary)?
            else {
                return Err(GenerationError::Boundary("map entry is not a pair".into()));
            };
            let exact = match operation {
                PairOperation::Exact => true,
                PairOperation::Node(node) => match self
                    .patterns
                    .shape(node)
                    .map_err(GenerationError::Boundary)?
                {
                    Shape::Literal(LiteralRef::Term { module, term }) => {
                        matches!(self.patterns.term(module, term).map_err(GenerationError::Boundary)?, ErlangTerm::Atom(a) if a == "exact")
                    }
                    _ => false,
                },
            };
            self.state.in_match = false;
            let origin = context.origin;
            let defined = context.defined.clone();
            todo.push(Frame::MapKey {
                context,
                value,
                operation: if exact {
                    MapOperation::Exact
                } else {
                    MapOperation::Associate
                },
            });
            todo.push(Frame::PatternVisit {
                pattern: key,
                origin,
                defined,
            });
        } else {
            self.state.in_match = context.in_match;
            if context.in_match {
                values.push(self.finish_map(context, Type::map_any())?);
            } else {
                let PatternRef::Core(node) = context.pattern else {
                    return Err(GenerationError::Boundary(
                        "synthetic map entered expression mode".into(),
                    ));
                };
                let CoreExpr::Map { base, .. } =
                    self.core.modules[node.module.0].nodes[node.node].expr
                else {
                    return Err(GenerationError::Boundary(
                        "map context has nonmap source".into(),
                    ));
                };
                let defined = context.defined.clone();
                todo.push(Frame::MapArgument { context });
                todo.push(Frame::Visit(
                    Site {
                        module: node.module,
                        node: base,
                    },
                    defined,
                ));
            }
        }
        Ok(())
    }
    pub(super) fn finish_map(&mut self, context: MapContext, argument: Type) -> Result<Type> {
        let map = self.pattern_variable(context.pattern)?;
        let pairs = NativeValue::List(
            context
                .pairs
                .iter()
                .map(|p| {
                    NativeValue::Tuple(vec![p.key.to_native_value(), p.value.to_native_value()])
                })
                .collect(),
        );
        let construct = self.derived(
            DerivedOp::MapConstruct {
                argument: argument.clone(),
                entries: context.pairs.clone(),
            },
            "-traverse/3-fun-10-",
            vec![pairs.clone(), argument.to_native_value()],
        )?;
        let mut shadow_keys = Vec::<Type>::new();
        for pair in context.pairs.iter().rev() {
            let rhs = self.derived(
                DerivedOp::MapValue {
                    key: pair.key.clone(),
                    map: map.clone(),
                    shadow_keys: shadow_keys.clone(),
                },
                "-traverse/3-fun-12-",
                vec![
                    pair.key.to_native_value(),
                    NativeValue::List(shadow_keys.iter().map(Type::to_native_value).collect()),
                    map.to_native_value(),
                ],
            )?;
            self.atomic(pair.value.clone(), Relation::Subtype, rhs)?;
            shadow_keys.insert(0, pair.key.clone());
        }
        if !context.in_match {
            let environment = self.map_argument_capture(&context, pairs)?;
            let key = self
                .profile
                .instantiate("-traverse/3-fun-15-", environment)
                .map_err(GenerationError::Boundary)?;
            let rhs = NativeOperand::Derived(self.arena.derived(
                DerivedOp::MapArgument {
                    entries: context.pairs.clone(),
                },
                key,
                ClosureOrigin::Undefined,
            ));
            self.atomic(argument, Relation::Subtype, rhs)?;
        }
        self.atomic(map.clone(), Relation::Subtype, construct)?;
        Ok(map)
    }
    fn map_argument_capture(
        &mut self,
        context: &MapContext,
        pairs: NativeValue,
    ) -> Result<CaptureEnvironment> {
        let mut terms = Vec::new();
        let list = capture_values(&[pairs])
            .map_err(GenerationError::Boundary)?
            .append_to(&mut terms);
        let CaptureTerm::List { items, .. } = &terms[list.0] else {
            return Err(GenerationError::Boundary(
                "pair capture root is not list".into(),
            ));
        };
        let pairs = *items
            .first()
            .ok_or_else(|| GenerationError::Boundary("pair capture root empty".into()))?;
        let module = context.origin.module;
        match self.capture_inputs.and_then(|inputs| inputs.core_captures) {
            Some(store) => store.check_module(module),
            None => crate::core_capture::CoreCaptures::check_module(self.core, self.labels, module),
        }
        .map_err(GenerationError::Boundary)?;
        let entries = context
            .entries
            .iter()
            .map(|entry| match entry {
                PatternRef::Core(node) if node.module == module => Ok(NodeId(node.node)),
                _ => Err(GenerationError::Boundary(
                    "expression map entry is synthetic or foreign".into(),
                )),
            })
            .collect::<Result<Vec<_>>>()?;
        let full = crate::lattice::verify_fast_paths().then(|| terms.clone());
        let environment = crate::core_capture::CoreCaptures::append_selected_from_source(
            self.core,
            self.labels,
            module,
            &entries,
            &mut terms,
        )
        .and_then(|entries| map_argument_environment(terms, pairs, entries));
        if let Some(mut full) = full {
            let full = crate::core_capture::CoreCaptures::build(self.core, self.labels, module)
                .and_then(|captures| captures.append_selected(&entries, &mut full))
                .and_then(|entries| map_argument_environment(full, pairs, entries));
            match (&environment, &full) {
                (Ok(fast), Ok(full)) => assert_eq!(
                    fast.compare(full),
                    Ok(std::cmp::Ordering::Equal),
                    "selected map capture environment diverged"
                ),
                (fast, full) => assert_eq!(
                    fast.as_ref().err(),
                    full.as_ref().err(),
                    "selected map capture error diverged"
                ),
            }
        }
        environment.map_err(GenerationError::Boundary)
    }
}
fn map_argument_environment(
    mut terms: Vec<CaptureTerm>,
    pairs: CaptureId,
    entries: Vec<CaptureId>,
) -> std::result::Result<CaptureEnvironment, String> {
    let nil = CaptureId(terms.len());
    terms.push(CaptureTerm::Nil);
    let entries_root = CaptureId(terms.len());
    terms.push(CaptureTerm::List {
        items: entries,
        tail: nil,
    });
    let root = CaptureId(terms.len());
    terms.push(CaptureTerm::List {
        items: vec![pairs, entries_root],
        tail: nil,
    });
    // Only about a fifth of these environments are ever compared, so conversion
    // is deferred; its error is still raised here.
    if let Some(error) = TermOrderArena::captures_error(&terms) {
        return Err(error);
    }
    Ok(CaptureEnvironment::lazy(2, move || {
        CaptureEnvironment::new(Arc::new(TermOrderArena::from_captures(&terms)?), root)
    }))
}

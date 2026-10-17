use crate::emission::{SourceAnnotation, WarningState};
use crate::{
    core::ErlangTerm,
    dataflow_key::DataflowKey,
    dataflow_map::DataflowMap,
    lattice::{
        BuiltinType, Endpoint, Known, LatticeError, OpacityConflict, Type, TypeGetter,
        TypePredicate, TypeValue,
    },
    patterns::{LiteralRef, PairOperation, PatternRef, Patterns, Shape},
};
use num_bigint::BigInt;
use num_traits::{One, Signed, ToPrimitive, Zero};
use std::{collections::BTreeMap, sync::Arc};
#[cfg(otp_domain = "29")]
#[path = "binder/native29.rs"]
mod native29;
/// `given.opacity_conflict(required, module)`, memoized per thread.
///
/// The result is a function of the two (interned) types and the module name.
/// Refinement calls it outside any lattice operation, so no nominal pair is
/// active. Only successful results are stored; types serve only as hash keys.
pub(crate) fn opacity_conflict(
    given: &Type,
    required: &Type,
    module: &str,
) -> Result<OpacityConflict, LatticeError> {
    if !given.contains_nominal() && !required.contains_nominal() {
        return given.opacity_conflict(required, module);
    }
    type Memo = crate::fast_hash::FastHashMap<(Type, Type), (Box<str>, OpacityConflict)>;
    thread_local! {
        static MEMO: std::cell::RefCell<Memo> = std::cell::RefCell::new(Memo::default());
    }
    let key = (*given, *required);
    let hit = MEMO.with(|memo| {
        memo.borrow()
            .get(&key)
            .and_then(|(m, conflict)| (**m == *module).then_some(*conflict))
    });
    if let Some(conflict) = hit {
        if crate::lattice::verify_fast_paths() {
            assert_eq!(
                Ok(conflict),
                given.opacity_conflict(required, module),
                "memoized opacity conflict: {given:?} vs {required:?}"
            );
        }
        return Ok(conflict);
    }
    let conflict = given.opacity_conflict(required, module)?;
    MEMO.with(|memo| {
        let mut memo = memo.borrow_mut();
        if memo.len() >= 1 << 20 {
            memo.clear();
        }
        memo.insert(key, (module.into(), conflict));
    });
    Ok(conflict)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Reverse,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    Bind,
    Record,
}
#[derive(Clone, Debug)]
pub struct BindFailure {
    pub kind: FailureKind,
    pub patterns: Vec<PatternRef>,
    pub actual: Type,
}
#[derive(Clone, Debug)]
pub enum BindError {
    Failure(BindFailure),
    Native(LatticeError),
    Boundary(String),
    Print(crate::core_print::PrintError),
    BitstringDomain(crate::lattice::BitstringDomainError),
    ResourceLimit,
}
impl From<crate::patterns::RefoldError> for BindError {
    fn from(e: crate::patterns::RefoldError) -> Self {
        match e {
            crate::patterns::RefoldError::Boundary(s) => Self::Boundary(s),
            crate::patterns::RefoldError::ResourceLimit { .. } => Self::ResourceLimit,
        }
    }
}
impl From<LatticeError> for BindError {
    fn from(e: LatticeError) -> Self {
        Self::Native(e)
    }
}
impl From<crate::lattice::BitstringDomainError> for BindError {
    fn from(e: crate::lattice::BitstringDomainError) -> Self {
        Self::BitstringDomain(e)
    }
}
impl From<String> for BindError {
    fn from(e: String) -> Self {
        Self::Boundary(e)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WarningLocation {
    Line(i64),
    Position { line: i64, column: i64 },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WarningFunction {
    Named(crate::program::FunctionName),
    Label(crate::labels::CoreLabel),
    Top,
    Undefined,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WarningInfo {
    pub filename: String,
    pub location: WarningLocation,
    pub current_fun: WarningFunction,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpaqueMatch {
    pub pattern: PatternRef,
    pub actual: Type,
    pub expected: Type,
    pub intersection: Type,
    pub message: [String; 3],
    pub info: WarningInfo,
}
#[derive(Clone, Debug)]
pub struct BinderRecord {
    pub prototype: Type,
    pub fields: Vec<crate::lattice::PresentationRecordField>,
}
#[derive(Clone, Debug)]
pub struct BinderState<W = OpaqueMatch> {
    pub module: std::sync::Arc<str>,
    pub current_fun: WarningFunction,
    pub translated_files: Arc<BTreeMap<(crate::program::ModuleId, crate::core::TermId), String>>,
    pub letrec: crate::letrec_types::LetrecTypes,
    pub records: Arc<BTreeMap<(String, usize), BinderRecord>>,
    #[cfg(otp_domain = "29")]
    pub native_records: Arc<BTreeMap<(String, String), BinderRecord>>,
    #[cfg(otp_domain = "29")]
    pub native_record_imports: Arc<BTreeMap<String, String>>,
    pub warnings: WarningState<W>,
}
impl<W> BinderState<W> {
    #[cfg(otp_domain = "29")]
    pub fn native_record_module<'a>(&'a self, name: &str) -> &'a str {
        self.native_record_imports.get(name).map(String::as_str).unwrap_or(&self.module)
    }
    #[cfg(otp_domain = "29")]
    pub fn resolve_native_record_identity(&self, identity: &crate::patterns::NativeRecordIdentity) -> crate::lattice::NativeRecordName {
        use crate::{lattice::NativeRecordName, patterns::NativeRecordIdentity};
        match identity {
            NativeRecordIdentity::Any => NativeRecordName::Any,
            NativeRecordIdentity::Local(name) => NativeRecordName::Named { module: self.native_record_module(name).into(), name: name.as_str().into() },
            NativeRecordIdentity::Qualified { module, name } => NativeRecordName::Named { module: module.as_str().into(), name: name.as_str().into() },
        }
    }
    #[cfg(otp_domain = "29")]
    pub fn resolve_native_record_id(&self, terms: &crate::shared_vec::SharedVec<ErlangTerm>, id: crate::core::TermId) -> Result<crate::lattice::NativeRecordName, String> {
        Ok(self.resolve_native_record_identity(&crate::patterns::NativeRecordIdentity::parse(terms, id)?))
    }
    pub fn presentation_context(&self) -> crate::lattice::PresentationContext {
        let mut context = crate::lattice::PresentationContext::default();
        for ((tag, _), record) in self.records.iter() {
            context.insert_record(tag.clone(), record.fields.clone());
        }
        context
    }
}
#[derive(Clone, Debug)]
pub struct Bound<W = OpaqueMatch> {
    pub map: DataflowMap,
    pub types: Vec<Type>,
    pub state: BinderState<W>,
}
pub struct Binder<'a, 'b, 'c> {
    pub patterns: &'a mut Patterns<'b, 'c>,
}
impl Binder<'_, '_, '_> {
    pub fn bind<W: Clone + PartialEq + From<OpaqueMatch>>(
        &mut self,
        pats: &[PatternRef],
        types: &[Type],
        map: &DataflowMap,
        state: &BinderState<W>,
        direction: Direction,
    ) -> Result<Bound<W>, BindError> {
        self.bind_owned(pats, types, map.clone(), state, direction)
    }
    /// `bind`, consuming `map` (dropped on failure).
    pub fn bind_owned<W: Clone + PartialEq + From<OpaqueMatch>>(
        &mut self,
        pats: &[PatternRef],
        types: &[Type],
        mut map: DataflowMap,
        state: &BinderState<W>,
        direction: Direction,
    ) -> Result<Bound<W>, BindError> {
        let mut state = state.clone();
        let mut types_out = Vec::with_capacity(pats.len());
        self.sequence(pats, types, &mut map, &mut state, direction, 0, &mut types_out)?;
        Ok(Bound {
            map,
            types: types_out,
            state,
        })
    }
    /// Binds `pats` to `types` in order, updating `map` and `state` in place
    /// and appending one type per pattern to `output`. On error, `map` and
    /// `state` hold partial updates; callers discard them, as they discarded
    /// the by-value results before.
    #[allow(clippy::too_many_arguments)]
    fn sequence<W: Clone + PartialEq + From<OpaqueMatch>>(
        &mut self,
        pats: &[PatternRef],
        types: &[Type],
        map: &mut DataflowMap,
        state: &mut BinderState<W>,
        dir: Direction,
        depth: usize,
        output: &mut Vec<Type>,
    ) -> Result<(), BindError> {
        if depth > 128 {
            return Err(BindError::ResourceLimit);
        }
        for (index, &pat) in pats.iter().enumerate() {
            let actual = types.get(index).ok_or(LatticeError::FunctionClause)?;
            output.push(self.one(pat, actual, map, state, dir, depth + 1)?);
        }
        if types.len() > pats.len() {
            return Err(LatticeError::FunctionClause.into());
        }
        Ok(())
    }
    /// `sequence` of a single pattern, returning its type.
    fn single<W: Clone + PartialEq + From<OpaqueMatch>>(
        &mut self,
        pat: PatternRef,
        actual: Type,
        map: &mut DataflowMap,
        state: &mut BinderState<W>,
        dir: Direction,
        depth: usize,
    ) -> Result<Type, BindError> {
        if depth > 128 {
            return Err(BindError::ResourceLimit);
        }
        self.one(pat, &actual, map, state, dir, depth + 1)
    }
    fn failure(&self, pat: PatternRef, actual: &Type, kind: FailureKind) -> BindError {
        match self.patterns.has_record_annotation(pat) {
            Ok(record) => BindError::Failure(BindFailure {
                kind: if kind == FailureKind::Bind && record {
                    FailureKind::Record
                } else {
                    kind
                },
                patterns: vec![pat],
                actual: actual.clone(),
            }),
            Err(e) => BindError::Boundary(e),
        }
    }
    pub fn annotation(&self, pat: PatternRef) -> Result<SourceAnnotation, BindError> {
        let mut out = SourceAnnotation {
            line: None,
            compiler_generated: false,
        };
        let Some(node) = self.patterns.annotation(pat) else {
            #[cfg(otp_domain = "29")]
            if matches!(self.patterns.shape(pat)?, Shape::Record { .. }) { out.compiler_generated = true; }
            return Ok(out);
        };
        let ann = self.patterns.nodes.module(node.module)?.nodes[node.node].annotations;
        let ErlangTerm::List { items, .. } = self.patterns.term(node.module, ann)? else {
            return Ok(out);
        };
        for id in items {
            match self.patterns.term(node.module, *id)? {
                ErlangTerm::Atom(a) if a == "compiler_generated" => out.compiler_generated = true,
                ErlangTerm::Integer(n) if out.line.is_none() => {
                    out.line =
                        Some(n.parse().map_err(|_| {
                            BindError::Boundary("annotation line outside i64".into())
                        })?)
                }
                ErlangTerm::Tuple(xs) if xs.len() == 2 && out.line.is_none() => {
                    if let ErlangTerm::Integer(n) = self.patterns.term(node.module, xs[0])? {
                        out.line = Some(n.parse().map_err(|_| {
                            BindError::Boundary("annotation line outside i64".into())
                        })?)
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }
    pub fn warning_location(
        &self,
        pat: PatternRef,
        default: WarningLocation,
    ) -> Result<WarningLocation, BindError> {
        let mut pending = vec![pat];
        let mut best: Option<WarningLocation> = None;
        while let Some(pat) = pending.pop() {
            if !self.annotation(pat)?.is_generated() {
                let Some(node) = self.patterns.annotation(pat) else {
                    return Err(BindError::Boundary(
                        "non-generated synthetic annotation missing".into(),
                    ));
                };
                let ann = self.patterns.nodes.module(node.module)?.nodes[node.node].annotations;
                let mut location = None;
                if let ErlangTerm::List { items, .. } = self.patterns.term(node.module, ann)? {
                    for &id in items {
                        location = match self.patterns.term(node.module, id)? {
                            ErlangTerm::Integer(n) => {
                                Some(WarningLocation::Line(n.parse().map_err(|_| {
                                    BindError::Boundary("annotation line outside i64".into())
                                })?))
                            }
                            ErlangTerm::Tuple(xs) if xs.len() == 2 => match (
                                self.patterns.term(node.module, xs[0])?,
                                self.patterns.term(node.module, xs[1])?,
                            ) {
                                (ErlangTerm::Integer(l), ErlangTerm::Integer(c)) => {
                                    Some(WarningLocation::Position {
                                        line: l.parse().map_err(|_| {
                                            BindError::Boundary(
                                                "annotation line outside i64".into(),
                                            )
                                        })?,
                                        column: c.parse().map_err(|_| {
                                            BindError::Boundary(
                                                "annotation column outside i64".into(),
                                            )
                                        })?,
                                    })
                                }
                                _ => None,
                            },
                            _ => None,
                        };
                        if location.is_some() {
                            break;
                        }
                    }
                }
                let location = location.ok_or(LatticeError::FunctionClause)?;
                let key = |l: &WarningLocation| match l {
                    WarningLocation::Line(n) => (*n, 1),
                    WarningLocation::Position { line, column } => (*line, *column),
                };
                if best.as_ref().is_none_or(|old| key(&location) < key(old)) {
                    best = Some(location)
                }
            }
            pending.extend(self.patterns.native_subtrees(pat)?.into_iter().rev());
        }
        Ok(best.unwrap_or(default))
    }
    pub fn warning_info<W: Clone + PartialEq + From<OpaqueMatch>>(
        &self,
        pat: PatternRef,
        state: &BinderState<W>,
    ) -> Result<WarningInfo, BindError> {
        let mut filename = String::new();
        if let Some(node) = self.patterns.annotation(pat) {
            let ann = self.patterns.nodes.module(node.module)?.nodes[node.node].annotations;
            if let ErlangTerm::List { items, .. } = self.patterns.term(node.module, ann)? {
                for &id in items {
                    if let ErlangTerm::Tuple(xs) = self.patterns.term(node.module, id)?
                        && xs.len() == 2
                        && matches!(self.patterns.term(node.module,xs[0])?,ErlangTerm::Atom(a)if a=="file")
                    {
                        filename = state
                            .translated_files
                            .get(&(node.module, xs[1]))
                            .cloned()
                            .ok_or_else(|| {
                                BindError::Boundary("unresolved native filename translation".into())
                            })?;
                        break;
                    }
                }
            }
        }
        Ok(WarningInfo {
            filename,
            location: self.warning_location(pat, WarningLocation::Line(1))?,
            current_fun: state.current_fun.clone(),
        })
    }
    fn checked<W: Clone + PartialEq + From<OpaqueMatch>>(
        &self,
        pat: PatternRef,
        expected: &Type,
        actual: &Type,
        state: &mut BinderState<W>,
    ) -> Result<Type, BindError> {
        let inf = expected.meet(actual)?;
        if opacity_conflict(actual, expected, &state.module)? != OpacityConflict::None {
            let context = state.presentation_context();
            let text = crate::core_print::CorePrinter {
                patterns: self.patterns,
            }
            .format_patterns(&[pat])
            .map_err(BindError::Print)?;
            let message = [
                text,
                context.type_to_string(expected)?,
                context.type_to_string(&inf)?,
            ];
            let annotation = self.annotation(pat)?;
            if state.warnings.warning_mode && !annotation.is_generated() {
                let info = self.warning_info(pat, state)?;
                state.warnings.add(
                    annotation,
                    OpaqueMatch {
                        pattern: pat,
                        actual: actual.clone(),
                        expected: expected.clone(),
                        intersection: inf.clone(),
                        message,
                        info,
                    }
                    .into(),
                    false,
                )
            }
        }
        if inf.is_impossible() {
            return Err(self.failure(pat, actual, FailureKind::Bind));
        }
        Ok(inf)
    }
    fn lookup<W: Clone + PartialEq + From<OpaqueMatch>>(
        &self,
        pat: PatternRef,
        map: &DataflowMap,
        state: &BinderState<W>,
    ) -> Result<Type, BindError> {
        match self.patterns.shape(pat)? {
            Shape::Literal(lit) => Ok(self.patterns.literal_type(lit)?),
            Shape::Variable { key, .. } => Ok(state
                .letrec
                .get(&key)
                .cloned()
                .unwrap_or_else(|| map.lookup(key))),
            _ => Err(LatticeError::FunctionClause.into()),
        }
    }
    fn atom(&self, pat: PatternRef) -> Result<String, BindError> {
        let LiteralRef::Term { module, term } = self.patterns.literal(pat)? else {
            return Err(LatticeError::FunctionClause.into());
        };
        if let ErlangTerm::Atom(s) = self.patterns.term(module, term)? {
            Ok(s.clone())
        } else {
            Err(LatticeError::FunctionClause.into())
        }
    }
    fn integer(&self, pat: PatternRef) -> Result<BigInt, BindError> {
        let LiteralRef::Term { module, term } = self.patterns.literal(pat)? else {
            return Err(LatticeError::FunctionClause.into());
        };
        if let ErlangTerm::Integer(s) = self.patterns.term(module, term)? {
            s.parse()
                .map_err(|_| BindError::Boundary("invalid integer".into()))
        } else {
            Err(LatticeError::BadArg.into())
        }
    }
    fn one<W: Clone + PartialEq + From<OpaqueMatch>>(
        &mut self,
        pat: PatternRef,
        actual: &Type,
        map: &mut DataflowMap,
        state: &mut BinderState<W>,
        dir: Direction,
        depth: usize,
    ) -> Result<Type, BindError> {
        let out = match self.patterns.shape(pat)? {
            Shape::Alias { variable, pattern } => {
                let pattern = self.patterns.refold(pattern)?;
                let Shape::Variable { key, .. } = self.patterns.shape(variable)? else {
                    return Err(LatticeError::FunctionClause.into());
                };
                match self.patterns.shape(pattern)? {
                    Shape::Literal(lit) if !self.patterns.contains_map(lit)? => {
                        map.enter_literal_substitution(key, self.patterns.literal_type(lit)?)
                    }
                    Shape::Variable { key: target, .. } => {
                        map.enter_variable_substitution(key, target)
                    }
                    _ => {}
                }
                let typ = self.single(pattern, actual.clone(), map, state, dir, depth)?;
                map.enter_type(key, typ.clone());
                typ
            }
            Shape::Literal(lit) => {
                let expanded = self.patterns.refold(pat)?;
                if expanded != pat {
                    return self.single(expanded, actual.clone(), map, state, dir, depth);
                }
                self.checked(pat, &self.patterns.literal_type(lit)?, actual, state)?
            }
            Shape::Variable { key, .. } => {
                let expected = state
                    .letrec
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| map.lookup(key));
                let typ = self.checked(pat, &expected, actual, state)?;
                map.enter_type(key, typ.clone());
                typ
            }
            Shape::Cons { head, tail } => {
                let typ = self.checked(pat, &Type::builtin(BuiltinType::Cons)?, actual, state)?;
                let head_type = get_type(&typ, TypeGetter::ConsHead)?;
                let tail_type = get_type(&typ, TypeGetter::ConsTail)?;
                if depth > 128 {
                    return Err(BindError::ResourceLimit);
                }
                let head = self.one(head, &head_type, map, state, dir, depth + 1)?;
                let tail = self.one(tail, &tail_type, map, state, dir, depth + 1)?;
                Type::cons(head, tail)?
            }
            Shape::Values(xs) => {
                let mut types = Vec::with_capacity(xs.len());
                self.sequence(
                    &xs,
                    &get_types(actual, TypeGetter::ToTypeList)?,
                    map,
                    state,
                    dir,
                    depth,
                    &mut types,
                )?;
                Type::product(types)
            }
            Shape::Map(pairs) => {
                let typ = self.checked(pat, &Type::map_any(), actual, state)?;
                if dir == Direction::Reverse {
                    typ
                } else {
                    let mut constraints = Vec::new();
                    for pair in pairs {
                        let Shape::Pair {
                            operation,
                            key,
                            value,
                        } = self.patterns.shape(pair)?
                        else {
                            return Err(LatticeError::FunctionClause.into());
                        };
                        if let PairOperation::Node(op) = operation
                            && self.atom(op)? != "exact"
                        {
                            return Err(BindError::Boundary("map pattern operation is not exact (native badmatch payload pending)".into()));
                        }
                        let key_type = self.lookup(key, map, state)?;
                        let domain = typ.map_get(&key_type)?;
                        let val = self.single(value, domain, map, state, dir, depth)?;
                        if key_type.predicate(TypePredicate::Singleton)? {
                            constraints.insert(0, (key_type, val))
                        }
                    }
                    typ.meet(&Type::map_pairs(&constraints)?)?
                }
            }
            #[cfg(otp_domain = "29")]
            Shape::Record { identity, pairs } => return self.native_record(pat, identity, pairs, actual, map, state, dir, depth),
            Shape::Tuple(xs) => {
                let mut typed_record = false;
                let prototype = if xs.is_empty() {
                    Type::tuple(vec![])?
                } else if self.patterns.has_record_annotation(pat)? && self.atom(xs[0]).is_ok() {
                    let tag = self.atom(xs[0])?;
                    let mut fields = vec![Type::any(); xs.len()];
                    fields[0] = Type::atom(&tag);
                    let untyped = Type::tuple(fields)?;
                    if let Some(record) = state.records.get(&(tag, xs.len() - 1)) {
                        typed_record = record.prototype != untyped;
                        record.prototype.clone()
                    } else {
                        untyped
                    }
                } else {
                    Type::tuple_arity(xs.len())
                };
                let typ = self.checked(pat, &prototype, actual, state)?;
                let alternatives = get_types(&typ, TypeGetter::TupleSubtypes)?;
                let scope = map.begin_join();
                let mut maps = Vec::new();
                let mut tuple_types = Vec::new();
                for alternative in alternatives {
                    let mut branch = scope.branch();
                    let mut branch_state = state.clone();
                    let mut types = Vec::with_capacity(xs.len());
                    match self.sequence(
                        &xs,
                        &get_types(&alternative, TypeGetter::TupleArgs)?,
                        &mut branch,
                        &mut branch_state,
                        dir,
                        depth,
                        &mut types,
                    ) {
                        Ok(()) => {
                            maps.push(branch);
                            *state = branch_state;
                            tuple_types.push(Type::tuple(types)?)
                        }
                        Err(BindError::Failure(_)) => {}
                        Err(e) => return Err(e),
                    }
                }
                if maps.is_empty() {
                    return Err(self.failure(
                        pat,
                        &typ,
                        if typed_record {
                            FailureKind::Record
                        } else {
                            FailureKind::Bind
                        },
                    ));
                }
                *map = scope.finish(&maps)?;
                Type::supremum(&tuple_types)?
            }
            Shape::Binary(xs) => {
                if dir == Direction::Reverse {
                    Type::builtin(BuiltinType::Bitstring)?
                } else {
                    let bin = self.checked(
                        pat,
                        &Type::builtin(BuiltinType::Bitstring)?,
                        actual,
                        state,
                    )?;
                    let segments = self.binary(&xs, bin, map, state, depth)?;
                    Type::bitstr_concat_types(&segments)?
                }
            }
            _ => {
                if dir == Direction::Forward {
                    return Err(LatticeError::BadMatchBoolean(false).into());
                }
                return Err(self.failure(pat, actual, FailureKind::Bind));
            }
        };
        Ok(out)
    }
    fn binary<W: Clone + PartialEq + From<OpaqueMatch>>(
        &mut self,
        segments: &[PatternRef],
        mut bin: Type,
        map: &mut DataflowMap,
        state: &mut BinderState<W>,
        depth: usize,
    ) -> Result<Vec<Type>, BindError> {
        let mut output = Vec::new();
        for (index, &seg) in segments.iter().enumerate() {
            let Shape::Segment {
                value,
                size,
                unit,
                typ,
                flags,
            } = self.patterns.shape(seg)?
            else {
                return Err(LatticeError::FunctionClause.into());
            };
            let kind = self.atom(typ)?;
            let unit_node = unit;
            let size_lit = match self.patterns.shape(size)? {
                Shape::Literal(lit) => Some(lit),
                _ => None,
            };
            let atom_size = self.atom(size).ok();
            if atom_size.as_deref() == Some("all") {
                if kind != "binary" || index + 1 != segments.len() {
                    return Err(BindError::Boundary(
                        "binary all assertion payload pending".into(),
                    ));
                }
                let domain = bitstring(self.integer(unit_node)?, BigInt::zero())?.meet(&bin)?;
                let typ = self.single(value, domain, map, state, Direction::Forward, depth)?;
                output.push(typ.structural()?);
                bin = bitstring(BigInt::zero(), BigInt::zero())?;
                continue;
            }
            if matches!(kind.as_str(), "utf8" | "utf16" | "utf32") {
                if atom_size.as_deref() != Some("undefined") {
                    return Err(BindError::Boundary(
                        "UTF size assertion payload pending".into(),
                    ));
                }
                self.single(
                    value,
                    Type::all_integers(),
                    map,
                    state,
                    Direction::Forward,
                    depth,
                )?;
                let t = Type::binary();
                bin = t.bitstr_match(&bin)?;
                output.push(t);
                continue;
            }
            if size_lit.is_some() && self.integer(size).is_err()
                || self.integer(size).is_ok_and(|n| n.is_negative())
            {
                return Err(self.failure(seg, &bin, FailureKind::Bind));
            }
            let size_type = &self.single(
                size,
                Type::builtin(BuiltinType::NonNegativeInteger)?,
                map,
                state,
                Direction::Forward,
                depth,
            )?;
            let unit = self.integer(unit_node)?;
            let TypeValue::Integers(values) = size_type.get(TypeGetter::NumberValues)? else {
                return Err(LatticeError::FunctionClause.into());
            };
            let seg_type = match &values {
                Known::Known(xs) if xs.len() == 1 => bitstring(BigInt::zero(), &unit * &xs[0])?,
                _ => {
                    let TypeValue::Endpoint(Endpoint::Finite(min)) =
                        size_type.get(TypeGetter::NumberMin)?
                    else {
                        return Err(LatticeError::BadArg.into());
                    };
                    bitstring(unit.clone(), &unit * min)?
                }
            };
            let domain = match kind.as_str() {
                "binary" => seg_type.clone(),
                "float" => Type::float(),
                "integer" => match values {
                    Known::Unknown => Type::all_integers(),
                    Known::Known(xs) => {
                        let n = xs.iter().max().ok_or(LatticeError::FunctionClause)? * &unit;
                        let signed = self.signed(flags)?;
                        if n >= BigInt::from(128) {
                            if signed {
                                Type::all_integers()
                            } else {
                                Type::range(
                                    Endpoint::Finite(BigInt::zero()),
                                    Endpoint::PositiveInfinity,
                                )?
                            }
                        } else {
                            let n = n.to_usize().ok_or(LatticeError::BadArg)?;
                            let limit = if signed {
                                if n == 0 {
                                    BigInt::zero()
                                } else {
                                    BigInt::one() << (n - 1)
                                }
                            } else {
                                BigInt::one() << n
                            };
                            Type::range(
                                Endpoint::Finite(if signed { -&limit } else { BigInt::zero() }),
                                Endpoint::Finite(limit - 1),
                            )?
                        }
                    }
                },
                _ => {
                    return Err(BindError::Boundary(
                        "unsupported binary segment atom (native case_clause payload pending)"
                            .into(),
                    ));
                }
            };
            self.single(value, domain, map, state, Direction::Forward, depth)?;
            let remaining = seg_type.bitstr_match(&bin)?;
            if remaining.is_none() {
                return Err(self.failure(seg, &bin, FailureKind::Bind));
            }
            bin = remaining;
            output.push(seg_type)
        }
        Ok(output)
    }
    fn signed(&self, flags: PatternRef) -> Result<bool, BindError> {
        let LiteralRef::Term { module, term } = self.patterns.literal(flags)? else {
            return Err(LatticeError::FunctionClause.into());
        };
        let mut pending = vec![term];
        while let Some(id) = pending.pop() {
            match self.patterns.term(module, id)? {
                ErlangTerm::Nil => {}
                ErlangTerm::List { items, tail } => {
                    for &item in items {
                        if matches!(self.patterns.term(module,item)?,ErlangTerm::Atom(a)if a=="signed")
                        {
                            return Ok(true);
                        }
                    }
                    pending.push(*tail)
                }
                _ => return Err(LatticeError::FunctionClause.into()),
            }
        }
        Ok(false)
    }
}
fn get_type(t: &Type, getter: TypeGetter) -> Result<Type, BindError> {
    match t.get(getter)? {
        TypeValue::Type(t) => Ok(t),
        _ => Err(LatticeError::FunctionClause.into()),
    }
}
fn get_types(t: &Type, getter: TypeGetter) -> Result<Vec<Type>, BindError> {
    match t.get(getter)? {
        TypeValue::Types(Known::Known(t)) => Ok(t),
        _ => Err(LatticeError::FunctionClause.into()),
    }
}
fn bitstring(unit: BigInt, base: BigInt) -> Result<Type, BindError> {
    Ok(Type::bitstring(unit, base)?)
}

#[cfg(test)]
mod snapshot_ownership_tests {
    use super::*;
    use crate::{core::TermId, emission::WarningState, labels::CoreLabel, program::ModuleId};
    fn state() -> BinderState<u32> {
        BinderState {
            module: "fixture".into(), current_fun: WarningFunction::Undefined,
            translated_files: BTreeMap::from([((ModuleId(0), TermId(3)), "λ/record.ex".into())]).into(),
            letrec: BTreeMap::from([(CoreLabel(1).into(), Type::function(Some(vec![Type::atom("ok")]), Type::any()))]).into(),
            records: BTreeMap::from([(("record".into(), 1), BinderRecord {
                prototype: Type::tuple(vec![Type::atom("record"), Type::any()]).unwrap(),
                fields: vec![crate::lattice::PresentationRecordField { name: "field".into(), declared_type: Type::any() }],
            })]).into(),
            #[cfg(otp_domain = "29")]
            native_records: BTreeMap::new().into(),
            #[cfg(otp_domain = "29")]
            native_record_imports: BTreeMap::new().into(),
            warnings: WarningState { warning_mode: true, warnings: vec![1] },
        }
    }
    #[test]
    fn snapshots_share_tables_and_keep_warning_values_independent() {
        let original = state(); let mut branch = original.clone();
        assert!(Arc::ptr_eq(&original.translated_files, &branch.translated_files));
        assert!(Arc::ptr_eq(&original.records, &branch.records));
        assert!(original.letrec.shares_storage(&branch.letrec));
        branch.warnings.warnings.push(2); branch.warnings.warning_mode = false;
        assert_eq!(original.warnings.warnings, vec![1]);
        assert!(original.warnings.warning_mode);
    }
    #[test]
    fn copy_on_write_preserves_old_lookup_and_record_field_snapshots() {
        let original = state(); let mut branch = original.clone();
        branch.letrec.make_mut().insert(CoreLabel(1).into(), Type::none());
        Arc::make_mut(&mut branch.records).get_mut(&("record".into(), 1)).unwrap().fields[0].name = "changed".into();
        Arc::make_mut(&mut branch.translated_files).clear();
        assert!(!original.letrec[&CoreLabel(1).into()].is_none());
        assert!(branch.letrec[&CoreLabel(1).into()].is_none());
        assert_eq!(original.records[&("record".into(), 1)].fields[0].name, "field");
        assert_eq!(original.translated_files[&(ModuleId(0), TermId(3))], "λ/record.ex");
    }
    #[test]
    fn uniquely_owned_function_table_updates_keep_its_allocation() {
        let mut state = state(); let before = state.letrec.storage_address();
        state.letrec.make_mut().insert(CoreLabel(2).into(), Type::atom("new"));
        assert_eq!(before, state.letrec.storage_address());
    }
}

//! Checked exported-native operands preserve unchecked records at a flat boundary.
use crate::{
    core::{ErlangTerm, ErlangTerms, TermId},
    form_error::{FormError, ReasonBuilder},
    lattice::Type,
    native_error::NativeTransferError,
    native_form_environment::RawFormTerm,
    term_order::TermOrderArena,
};

#[derive(Clone, Debug)]
pub enum NativeTypeOperand {
    Canonical(Type),
    Raw(RawFormTerm),
}
impl NativeTypeOperand {
    pub fn from_raw(raw: RawFormTerm) -> Self {
        if bounded(&raw)
            && let Ok(value) = crate::type_exports::type_from_arena(raw.arena(), raw.id())
        {
            let wire = type_wire(&value);
            if exact(&raw, &wire).unwrap_or(false) {
                return Self::Canonical(value);
            }
        }
        Self::Raw(raw)
    }
    pub fn to_raw(&self) -> RawFormTerm {
        match self {
            Self::Canonical(value) => type_wire(value),
            Self::Raw(raw) => raw.clone(),
        }
    }
    pub fn is_impossible(&self) -> bool {
        match self {
            Self::Canonical(value) => value.is_impossible(),
            Self::Raw(raw) => matches!(raw.term(),ErlangTerm::Atom(a)if a=="none"||a=="unit"),
        }
    }
    pub fn is_opaque(&self, module: &str) -> Result<bool, FormError> {
        if let Self::Canonical(value) = self {
            return Ok(value.is_opaque(module));
        }
        let Self::Raw(raw) = self else { unreachable!() };
        enum Work {
            Type(TermId),
            List(TermId, bool),
            Finish(TermId),
        }
        let terms = &raw.arena().terms;
        let mut work = vec![Work::Type(raw.id())];
        // Terms already found not to be opaque. A set, not a vector over the
        // whole arena: the walk visits a handful of terms of a large arena.
        let mut false_results = crate::fast_hash::FastHashSet::default();
        while let Some(next) = work.pop() {
            match next {
                Work::Finish(id) => {
                    false_results.insert(id.0);
                }
                Work::List(id, initial) => match &terms[id.0] {
                    ErlangTerm::Nil => {}
                    ErlangTerm::List { items, tail } => {
                        work.push(Work::List(*tail, false));
                        work.extend(items.iter().rev().copied().map(Work::Type));
                    }
                    _ => {
                        if initial {
                            let mut b = ReasonBuilder::new();
                            let tag = b.atom("case_clause");
                            let payload = b.copy(&RawFormTerm::new(raw.arena().clone(), id)?);
                            let reason = b.tuple(vec![tag, payload]);
                            return Err(FormError::ErrorReason(b.finish(reason)));
                        }
                        return Err(FormError::Native(NativeTransferError::FunctionClause));
                    }
                },
                Work::Type(id) => {
                    if false_results.contains(&id.0) {
                        continue;
                    }
                    work.push(Work::Finish(id));
                    let ErlangTerm::Tuple(fields) = &terms[id.0] else {
                        continue;
                    };
                    if fields.len() != 4
                        || !matches!(&terms[fields[0].0], ErlangTerm::Atom(s) if s == "c")
                    {
                        continue;
                    }
                    let ErlangTerm::Tuple(elements) = &terms[fields[2].0] else {
                        continue;
                    };
                    if elements.len() != 2 {
                        continue;
                    }
                    if matches!(&terms[fields[1].0], ErlangTerm::Atom(s) if s == "nominal") {
                        let ErlangTerm::Tuple(name) = &terms[elements[0].0] else {
                            continue;
                        };
                        if name.len() == 4
                            && matches!(&terms[name[3].0], ErlangTerm::Atom(s) if s == "opaque")
                            && !matches!(&terms[name[0].0], ErlangTerm::Atom(s) if s == module)
                        {
                            return Ok(true);
                        }
                    } else if matches!(&terms[fields[1].0], ErlangTerm::Atom(s) if s == "nominal_set")
                        && matches!(&terms[elements[1].0], ErlangTerm::Atom(s) if s == "none")
                    {
                        work.push(Work::List(elements[0], true));
                    }
                }
            }
        }
        Ok(false)
    }
    pub fn to_string_with(
        &self,
        context: &crate::lattice::PresentationContext,
    ) -> Result<String, FormError> {
        match self {
            Self::Canonical(value) => Ok(context.type_to_string(value)?),
            Self::Raw(raw) => {
                if matches!(raw.term(), ErlangTerm::Tuple(fields) if fields.first().is_some_and(|id|matches!(&raw.arena().terms[id.0], ErlangTerm::Atom(s) if s == "c_literal")))
                {
                    return Err(FormError::Native(NativeTransferError::FunctionClause));
                }
                Err(FormError::Transport(
                    "native raw t_to_string reached unimplemented runtime operand clause".into(),
                ))
            }
        }
    }
    fn early_inf(&self, rhs: &Self) -> Result<Option<Self>, FormError> {
        let left = self.to_raw();
        let right = rhs.to_raw();
        let l = Shape::new(&left);
        let r = Shape::new(&right);
        if l.tag == Some("var") && r.tag == Some("var") {
            return Ok(Some(Self::Canonical(Type::any())));
        }
        if l.tag == Some("var") {
            return Ok(Some(rhs.clone()));
        }
        if r.tag == Some("var") {
            return Ok(Some(self.clone()));
        }
        if l.atom == Some("any") {
            return Ok(Some(rhs.clone()));
        }
        if r.atom == Some("any") {
            return Ok(Some(self.clone()));
        }
        if l.atom == Some("none") || r.atom == Some("none") {
            return Ok(Some(Self::Canonical(Type::none())));
        }
        if l.atom == Some("unit") || r.atom == Some("unit") {
            return Ok(Some(Self::Canonical(Type::unit())));
        }
        if exact(&left, &right)? {
            return Ok(Some(self.clone()));
        }
        if let (Self::Canonical(a), Self::Canonical(b)) = (self, rhs) {
            return Ok(Some(Self::Canonical(a.meet(b)?)));
        }
        Ok(None)
    }
    pub fn inf(&self, rhs: &Self) -> Result<Self, FormError> {
        eval_inf(self.clone(), rhs.clone())
    }
    fn family_inf(&self, rhs: &Self) -> Result<Self, FormError> {
        let left = self.to_raw();
        let right = rhs.to_raw();
        let l = Shape::new(&left);
        let r = Shape::new(&right);
        let unsupported = |family: &str| {
            FormError::Transport(format!(
                "native raw t_inf reached unimplemented {family} clause"
            ))
        };
        if l.tag == Some("atom") && r.tag == Some("atom") {
            return raw_set_inf(&left, &right, "atom");
        }
        for (tag, size) in [("binary", 2), ("function", 2), ("map", 3)] {
            if l.matches(tag, size) && r.matches(tag, size) {
                return Err(unsupported(tag));
            }
        }
        if l.tag == Some("identifier") && r.tag == Some("identifier") {
            return raw_set_inf(&left, &right, "identifier");
        }
        if l.matches("nominal", 2)
            || r.matches("nominal", 2)
            || l.matches("nominal_set", 2)
            || r.matches("nominal_set", 2)
        {
            return Err(unsupported("nominal"));
        }
        if l.tag == Some("nil") && r.tag == Some("nil") {
            return Ok(Self::Canonical(Type::nil()));
        }
        if (l.tag == Some("nil") && r.matches("list", 2) && r.qual_atom == Some("nonempty"))
            || (r.tag == Some("nil") && l.matches("list", 2) && l.qual_atom == Some("nonempty"))
        {
            return Ok(Self::Canonical(Type::none()));
        }
        if (l.tag == Some("nil") && r.matches("list", 2))
            || (r.tag == Some("nil") && l.matches("list", 2))
            || (l.matches("list", 2) && r.matches("list", 2))
        {
            return Err(unsupported("list"));
        }
        if l.tag == Some("number") && r.tag == Some("number") {
            return raw_number_inf(self, rhs, &left, &right);
        }
        if l.tag == Some("product") && r.tag == Some("product") {
            return Err(unsupported("product pairs"));
        }
        if l.tag == Some("product") || r.tag == Some("product") {
            return Ok(Self::Canonical(Type::none()));
        }
        if l.generic_tuple() && (r.tuple() || r.tag == Some("tuple_set")) {
            return Ok(rhs.clone());
        }
        if r.generic_tuple() && (l.tuple() || l.tag == Some("tuple_set")) {
            return Ok(self.clone());
        }
        if l.tuple() && r.tuple() {
            let a = l.qual_tuple.expect("tuple qualifier")[0];
            let b = r.qual_tuple.expect("tuple qualifier")[0];
            let ra = RawFormTerm::new(left.arena().clone(), a)?;
            let rb = RawFormTerm::new(right.arena().clone(), b)?;
            if exact(&ra, &rb)? {
                return Err(unsupported("equal-arity tuples"));
            }
        }
        if (l.tag == Some("tuple_set") && (r.tag == Some("tuple_set") || r.tuple()))
            || (r.tag == Some("tuple_set") && l.tuple())
        {
            return Err(unsupported("tuple sets"));
        }
        if l.atom == Some("opaque") || r.atom == Some("opaque") {
            return Ok(Self::Canonical(Type::none()));
        }
        if l.tag == Some("union") || r.tag == Some("union") {
            return Err(unsupported("union"));
        }
        if l.record && r.record {
            return Ok(Self::Canonical(Type::none()));
        }
        Err(FormError::Native(NativeTransferError::FunctionClause))
    }
}
struct Shape<'a> {
    arena: &'a ErlangTerms,
    record: bool,
    atom: Option<&'a str>,
    tag: Option<&'a str>,
    elements: Option<&'a ErlangTerm>,
    qual_atom: Option<&'a str>,
    qual_tuple: Option<&'a [TermId]>,
}
impl<'a> Shape<'a> {
    fn new(raw: &'a RawFormTerm) -> Self {
        let atom = match raw.term() {
            ErlangTerm::Atom(a) => Some(a.as_str()),
            _ => None,
        };
        let record = match raw.term() {
            ErlangTerm::Tuple(v)
                if v.len() == 4
                    && matches!(&raw.arena().terms[v[0].0],ErlangTerm::Atom(a)if a=="c") =>
            {
                Some(v)
            }
            _ => None,
        };
        let tag = record.and_then(|v| match &raw.arena().terms[v[1].0] {
            ErlangTerm::Atom(a) => Some(a.as_str()),
            _ => None,
        });
        let elements = record.map(|v| &raw.arena().terms[v[2].0]);
        let qualifier = record.map(|v| &raw.arena().terms[v[3].0]);
        let qual_atom = qualifier.and_then(|q| match q {
            ErlangTerm::Atom(a) => Some(a.as_str()),
            _ => None,
        });
        let qual_tuple = qualifier.and_then(|q| match q {
            ErlangTerm::Tuple(v) if v.len() == 2 => Some(v.as_slice()),
            _ => None,
        });
        Self {
            arena: raw.arena(),
            record: record.is_some(),
            atom,
            tag,
            elements,
            qual_atom,
            qual_tuple,
        }
    }
    fn matches(&self, tag: &str, size: usize) -> bool {
        self.tag == Some(tag) && matches!(self.elements,Some(ErlangTerm::Tuple(v))if v.len()==size)
    }
    fn tuple(&self) -> bool {
        self.tag == Some("tuple") && self.qual_tuple.is_some()
    }
    fn generic_tuple(&self) -> bool {
        self.tuple()
            && matches!(self.elements,Some(ErlangTerm::Atom(a))if a=="any")
            && self.qual_tuple.is_some_and(|v| {
                v.iter()
                    .all(|id| matches!(&self.arena.terms[id.0],ErlangTerm::Atom(a)if a=="any"))
            })
    }
}
fn exact(a: &RawFormTerm, b: &RawFormTerm) -> Result<bool, FormError> {
    let a_normal = TermOrderArena::new(a.arena())?;
    let b_normal = TermOrderArena::new(b.arena())?;
    Ok(a_normal
        .compare_map_keys(a.id(), &b_normal, b.id())?
        .is_eq())
}
fn type_wire(value: &Type) -> RawFormTerm {
    let mut b = ReasonBuilder::new();
    let root = b.native(&value.to_native_value());
    b.finish(root)
}
fn bounded(raw: &RawFormTerm) -> bool {
    let mut memo = vec![None; raw.arena().terms.len()];
    let mut todo = vec![(raw.id(), false)];
    while let Some((id, ready)) = todo.pop() {
        if memo[id.0].is_some() {
            continue;
        }
        let children = match &raw.arena().terms[id.0] {
            ErlangTerm::Tuple(v) => v.clone(),
            ErlangTerm::List { items, tail } => items.iter().copied().chain([*tail]).collect(),
            _ => Vec::new(),
        };
        if !ready {
            todo.push((id, true));
            for child in children.into_iter().rev() {
                if memo[child.0].is_none() {
                    todo.push((child, false));
                }
            }
            continue;
        }
        let mut depth = 1usize;
        let mut cost = 1usize;
        for child in children {
            let Some((d, n)) = memo[child.0] else {
                return false;
            };
            depth = depth.max(d + 1);
            cost = cost.saturating_add(n);
        }
        if depth > 128 || cost > 1_000_000 {
            return false;
        }
        memo[id.0] = Some((depth, cost));
    }
    true
}

fn child(raw: &RawFormTerm, id: TermId) -> RawFormTerm {
    RawFormTerm::new(raw.arena().clone(), id).expect("validated arena child")
}
fn record_elements(raw: &RawFormTerm) -> RawFormTerm {
    let ErlangTerm::Tuple(v) = raw.term() else {
        unreachable!()
    };
    child(raw, v[2])
}
fn raw_atom(raw: &RawFormTerm, atom: &str) -> bool {
    matches!(raw.term(), ErlangTerm::Atom(a) if a == atom)
}
fn raw_record(tag: &str, elements: &RawFormTerm, qualifier: &str) -> NativeTypeOperand {
    let structural = matches!(tag, "function" | "product" | "list");
    let mut b = ReasonBuilder::new();
    let c = b.atom("c");
    let tag = b.atom(tag);
    let elements = b.copy(elements);
    let qualifier = b.atom(qualifier);
    let id = b.tuple(vec![c, tag, elements, qualifier]);
    if structural {
        NativeTypeOperand::Raw(b.finish(id))
    } else {
        NativeTypeOperand::from_raw(b.finish(id))
    }
}
// Source ordsets:intersection traverses improper tails only when reached and
// swaps operands on its greater-than branch; equal numeric values retain the
// current left head, making both branch order and exact raw values observable.
#[derive(Clone)]
struct Cursor {
    raw: RawFormTerm,
    offset: usize,
}
enum Cell {
    Nil,
    Cons(RawFormTerm),
    Other,
}
impl Cursor {
    fn new(raw: RawFormTerm) -> Self {
        Self { raw, offset: 0 }
    }
    fn cell(&mut self) -> Cell {
        loop {
            match self.raw.term() {
                ErlangTerm::Nil => return Cell::Nil,
                ErlangTerm::List { items, tail } => {
                    if self.offset < items.len() {
                        return Cell::Cons(child(&self.raw, items[self.offset]));
                    }
                    self.raw = child(&self.raw, *tail);
                    self.offset = 0;
                }
                _ => return Cell::Other,
            }
        }
    }
    fn advance(&mut self) {
        self.offset += 1;
    }
}
fn set_intersection(a: RawFormTerm, b: RawFormTerm) -> Result<RawFormTerm, FormError> {
    if raw_atom(&a, "any") {
        return Ok(b);
    }
    if raw_atom(&b, "any") {
        return Ok(a);
    }
    let mut a = Cursor::new(a);
    let mut b = Cursor::new(b);
    let mut out = ReasonBuilder::new();
    let mut items = Vec::new();
    loop {
        let ac = a.cell();
        let bc = b.cell();
        match (ac, bc) {
            (Cell::Cons(x), Cell::Cons(y)) => {
                let an = TermOrderArena::new(x.arena())?;
                let bn = TermOrderArena::new(y.arena())?;
                match an.compare(x.id(), &bn, y.id())? {
                    std::cmp::Ordering::Less => a.advance(),
                    std::cmp::Ordering::Greater => {
                        b.advance();
                        std::mem::swap(&mut a, &mut b);
                    }
                    std::cmp::Ordering::Equal => {
                        items.push(out.copy(&x));
                        a.advance();
                        b.advance();
                    }
                }
            }
            (Cell::Nil, _) | (_, Cell::Nil) => break,
            _ => return Err(FormError::Native(NativeTransferError::FunctionClause)),
        }
    }
    let id = if items.is_empty() {
        out.atom("none")
    } else {
        out.list(items)
    };
    Ok(out.finish(id))
}
fn raw_set_inf(
    a: &RawFormTerm,
    b: &RawFormTerm,
    tag: &str,
) -> Result<NativeTypeOperand, FormError> {
    let set = set_intersection(record_elements(a), record_elements(b))?;
    if raw_atom(&set, "none") {
        Ok(NativeTypeOperand::Canonical(Type::none()))
    } else {
        Ok(raw_record(tag, &set, "unknown"))
    }
}
fn case_clause_pair(a: &RawFormTerm, b: &RawFormTerm) -> FormError {
    let mut r = ReasonBuilder::new();
    let tag = r.atom("case_clause");
    let a = r.copy(a);
    let b = r.copy(b);
    let pair = r.tuple(vec![a, b]);
    let root = r.tuple(vec![tag, pair]);
    FormError::ErrorReason(r.finish(root))
}
fn raw_number_inf(
    lhs: &NativeTypeOperand,
    rhs: &NativeTypeOperand,
    a: &RawFormTerm,
    b: &RawFormTerm,
) -> Result<NativeTypeOperand, FormError> {
    let x = Shape::new(a);
    let y = Shape::new(b);
    let xe = record_elements(a);
    let ye = record_elements(b);
    if y.qual_atom == Some("unknown") && raw_atom(&ye, "any") {
        return Ok(lhs.clone());
    }
    if x.qual_atom == Some("unknown") && raw_atom(&xe, "any") {
        return Ok(rhs.clone());
    }
    let xf = x.qual_atom == Some("float") && raw_atom(&xe, "any");
    let yf = y.qual_atom == Some("float") && raw_atom(&ye, "any");
    let xi = x.qual_atom == Some("integer");
    let yi = y.qual_atom == Some("integer");
    if (xf && yi) || (yf && xi) {
        return Ok(NativeTypeOperand::Canonical(Type::none()));
    }
    if xi && yi && raw_atom(&xe, "any") {
        return Ok(rhs.clone());
    }
    if xi && yi && raw_atom(&ye, "any") {
        return Ok(lhs.clone());
    }
    if xi && yi {
        if let (ErlangTerm::Tuple(xs), ErlangTerm::Tuple(ys)) = (xe.term(), ye.term())
            && xs.len() == 2
            && ys.len() == 2
            && raw_atom(&child(&xe, xs[0]), "int_set")
            && raw_atom(&child(&ye, ys[0]), "int_set")
        {
            let set = set_intersection(child(&xe, xs[1]), child(&ye, ys[1]))?;
            if raw_atom(&set, "none") {
                return Ok(NativeTypeOperand::Canonical(Type::none()));
            }
            let mut r = ReasonBuilder::new();
            let tag = r.atom("int_set");
            let set = r.copy(&set);
            let id = r.tuple(vec![tag, set]);
            return Ok(raw_record("number", &r.finish(id), "integer"));
        }
        let xr = range_parts(&xe);
        let yr = range_parts(&ye);
        let xs = set_parts(&xe);
        let ys = set_parts(&ye);
        if let (Some((xf, xt)), Some((yf, yt))) = (xr.clone(), yr.clone()) {
            let from = range_max(xf, yf)?;
            let to = range_min(xt, yt)?;
            return Ok(NativeTypeOperand::Canonical(Type::range(
                endpoint(&from)?,
                endpoint(&to)?,
            )?));
        }
        if let (Some(range), Some(set)) = (xr, ys) {
            return range_set_inf(range, set);
        }
        if let (Some(set), Some(range)) = (xs, yr) {
            return range_set_inf(range, set);
        }
    }
    Err(case_clause_pair(a, b))
}

fn normal_compare(a: &RawFormTerm, b: &RawFormTerm) -> Result<std::cmp::Ordering, FormError> {
    let an = TermOrderArena::new(a.arena())?;
    let bn = TermOrderArena::new(b.arena())?;
    Ok(an.compare(a.id(), &bn, b.id())?)
}
fn endpoint(raw: &RawFormTerm) -> Result<crate::lattice::Endpoint, FormError> {
    use crate::lattice::Endpoint;
    match raw.term() {
        ErlangTerm::Atom(a) if a == "neg_inf" => Ok(Endpoint::NegativeInfinity),
        ErlangTerm::Atom(a) if a == "pos_inf" => Ok(Endpoint::PositiveInfinity),
        ErlangTerm::Integer(n) => Ok(Endpoint::Finite(n.parse().expect("validated integer term"))),
        _ => Err(FormError::Native(NativeTransferError::FunctionClause)),
    }
}
fn range_parts(raw: &RawFormTerm) -> Option<(RawFormTerm, RawFormTerm)> {
    match raw.term() {
        ErlangTerm::Tuple(v) if v.len() == 3 && raw_atom(&child(raw, v[0]), "int_rng") => {
            Some((child(raw, v[1]), child(raw, v[2])))
        }
        _ => None,
    }
}
fn set_parts(raw: &RawFormTerm) -> Option<RawFormTerm> {
    match raw.term() {
        ErlangTerm::Tuple(v) if v.len() == 2 && raw_atom(&child(raw, v[0]), "int_set") => {
            Some(child(raw, v[1]))
        }
        _ => None,
    }
}
fn range_set_inf(
    (from, to): (RawFormTerm, RawFormTerm),
    set: RawFormTerm,
) -> Result<NativeTypeOperand, FormError> {
    let mut cursor = Cursor::new(set);
    let mut builder = ReasonBuilder::new();
    let mut kept = Vec::new();
    loop {
        match cursor.cell() {
            Cell::Nil => break,
            Cell::Other => {
                let mut b = ReasonBuilder::new();
                let tag = b.atom("bad_generator");
                let tail = b.copy(&cursor.raw);
                let root = b.tuple(vec![tag, tail]);
                return Err(FormError::ErrorReason(b.finish(root)));
            }
            Cell::Cons(x) => {
                let above = raw_atom(&from, "neg_inf") || !normal_compare(&x, &from)?.is_lt();
                let below = raw_atom(&to, "pos_inf") || !normal_compare(&x, &to)?.is_gt();
                if above && below {
                    kept.push(builder.copy(&x));
                }
                cursor.advance();
            }
        }
    }
    if kept.is_empty() {
        return Ok(NativeTypeOperand::Canonical(Type::none()));
    }
    let values = builder.list(kept);
    let tag = builder.atom("int_set");
    let root = builder.tuple(vec![tag, values]);
    Ok(raw_record("number", &builder.finish(root), "integer"))
}

fn range_min(x: RawFormTerm, y: RawFormTerm) -> Result<RawFormTerm, FormError> {
    if raw_atom(&x, "neg_inf") {
        return Ok(x);
    }
    if raw_atom(&y, "neg_inf") {
        return Ok(y);
    }
    if raw_atom(&x, "pos_inf") {
        return Ok(y);
    }
    if raw_atom(&y, "pos_inf") {
        return Ok(x);
    }
    if normal_compare(&x, &y)?.is_gt() {
        Ok(y)
    } else {
        Ok(x)
    }
}
fn range_max(x: RawFormTerm, y: RawFormTerm) -> Result<RawFormTerm, FormError> {
    if raw_atom(&x, "neg_inf") {
        return Ok(y);
    }
    if raw_atom(&y, "neg_inf") {
        return Ok(x);
    }
    if raw_atom(&x, "pos_inf") {
        return Ok(x);
    }
    if raw_atom(&y, "pos_inf") {
        return Ok(y);
    }
    if normal_compare(&x, &y)?.is_gt() {
        Ok(x)
    } else {
        Ok(y)
    }
}

type PairKey = (usize, TermId, usize, TermId);
struct MemoEntry {
    _left: NativeTypeOperand,
    _right: NativeTypeOperand,
    value: Option<NativeTypeOperand>,
}
enum InfWork {
    Save(PairKey),
    Visit(NativeTypeOperand, NativeTypeOperand),
    FunctionDomain(NativeTypeOperand, NativeTypeOperand),
    FunctionRange(NativeTypeOperand),
    Product(usize),
    ListTermination {
        left: NativeTypeOperand,
        right: NativeTypeOperand,
        left_size: RawFormTerm,
        right_size: RawFormTerm,
    },
    ListContents {
        termination: NativeTypeOperand,
        left_size: RawFormTerm,
        right_size: RawFormTerm,
    },
}
fn eval_inf(
    left: NativeTypeOperand,
    right: NativeTypeOperand,
) -> Result<NativeTypeOperand, FormError> {
    let mut work = vec![InfWork::Visit(left, right)];
    let mut values: Vec<NativeTypeOperand> = Vec::new();
    let mut memo = std::collections::BTreeMap::<PairKey, MemoEntry>::new();
    while let Some(next) = work.pop() {
        match next {
            InfWork::Save(key) => {
                memo.get_mut(&key).expect("active pair").value =
                    Some(values.last().expect("completed pair").clone());
            }
            InfWork::Visit(a, b) => {
                if let (NativeTypeOperand::Raw(ar), NativeTypeOperand::Raw(br)) = (&a, &b) {
                    let key = (
                        std::sync::Arc::as_ptr(ar.arena()) as usize,
                        ar.id(),
                        std::sync::Arc::as_ptr(br.arena()) as usize,
                        br.id(),
                    );
                    if let Some(value) = memo.get(&key).and_then(|x| x.value.clone()) {
                        values.push(value);
                        continue;
                    }
                    memo.insert(
                        key,
                        MemoEntry {
                            _left: a.clone(),
                            _right: b.clone(),
                            value: None,
                        },
                    );
                    work.push(InfWork::Save(key));
                }
                if let Some(value) = a.early_inf(&b)? {
                    values.push(value);
                    continue;
                }
                let ar = a.to_raw();
                let br = b.to_raw();
                let x = Shape::new(&ar);
                let y = Shape::new(&br);
                if x.matches("function", 2) && y.matches("function", 2) {
                    let ae = record_elements(&ar);
                    let be = record_elements(&br);
                    let ErlangTerm::Tuple(xs) = ae.term() else {
                        unreachable!()
                    };
                    let ErlangTerm::Tuple(ys) = be.term() else {
                        unreachable!()
                    };
                    work.push(InfWork::FunctionDomain(
                        operand_child(child(&ae, xs[1])),
                        operand_child(child(&be, ys[1])),
                    ));
                    work.push(InfWork::Visit(
                        operand_child(child(&ae, xs[0])),
                        operand_child(child(&be, ys[0])),
                    ));
                } else if x.tag == Some("product") && y.tag == Some("product") {
                    let xs = proper_list(record_elements(&ar))?;
                    let ys = proper_list(record_elements(&br))?;
                    if xs.len() != ys.len() {
                        values.push(NativeTypeOperand::Canonical(Type::none()));
                        continue;
                    }
                    work.push(InfWork::Product(xs.len()));
                    for (a, b) in xs.into_iter().zip(ys).rev() {
                        work.push(InfWork::Visit(operand_child(a), operand_child(b)));
                    }
                } else if x.matches("list", 2) && y.matches("list", 2) {
                    let ae = record_elements(&ar);
                    let be = record_elements(&br);
                    let ErlangTerm::Tuple(xs) = ae.term() else {
                        unreachable!()
                    };
                    let ErlangTerm::Tuple(ys) = be.term() else {
                        unreachable!()
                    };
                    work.push(InfWork::ListTermination {
                        left: operand_child(child(&ae, xs[0])),
                        right: operand_child(child(&be, ys[0])),
                        left_size: qualifier(&ar),
                        right_size: qualifier(&br),
                    });
                    work.push(InfWork::Visit(
                        operand_child(child(&ae, xs[1])),
                        operand_child(child(&be, ys[1])),
                    ));
                } else if (x.tag == Some("nil")
                    && y.matches("list", 2)
                    && y.qual_atom != Some("nonempty"))
                    || (y.tag == Some("nil")
                        && x.matches("list", 2)
                        && x.qual_atom != Some("nonempty"))
                {
                    let list = if x.tag == Some("nil") { br } else { ar };
                    let elems = record_elements(&list);
                    let ErlangTerm::Tuple(xs) = elems.term() else {
                        unreachable!()
                    };
                    let termination = structural_operand(operand_child(child(&elems, xs[1])))?;
                    work.push(InfWork::Visit(
                        NativeTypeOperand::Canonical(Type::nil()),
                        termination,
                    ));
                } else {
                    values.push(a.family_inf(&b)?);
                }
            }
            InfWork::FunctionDomain(a, b) => {
                let domain = values.pop().expect("domain child result");
                if raw_atom(&domain.to_raw(), "none") {
                    values.push(domain);
                } else {
                    work.push(InfWork::FunctionRange(domain));
                    work.push(InfWork::Visit(a, b));
                }
            }
            InfWork::FunctionRange(domain) => {
                let range = values.pop().expect("range child result");
                let mut b = ReasonBuilder::new();
                let d = b.copy(&domain.to_raw());
                let r = b.copy(&range.to_raw());
                let id = b.tuple(vec![d, r]);
                values.push(raw_record("function", &b.finish(id), "unknown"));
            }
            InfWork::ListTermination {
                left,
                right,
                left_size,
                right_size,
            } => {
                let termination = values.pop().expect("list termination result");
                if raw_atom(&termination.to_raw(), "none") {
                    values.push(termination);
                } else {
                    work.push(InfWork::ListContents {
                        termination,
                        left_size,
                        right_size,
                    });
                    work.push(InfWork::Visit(left, right));
                }
            }
            InfWork::ListContents {
                termination,
                left_size,
                right_size,
            } => {
                let contents = values.pop().expect("list contents result");
                if raw_atom(&contents.to_raw(), "none") {
                    values.push(NativeTypeOperand::Canonical(
                        if raw_atom(&left_size, "unknown") && raw_atom(&right_size, "unknown") {
                            Type::nil()
                        } else {
                            Type::none()
                        },
                    ));
                } else {
                    let size = if raw_atom(&left_size, "unknown")
                        && raw_atom(&right_size, "unknown")
                    {
                        left_size
                    } else if (raw_atom(&left_size, "unknown") && raw_atom(&right_size, "nonempty"))
                        || (raw_atom(&left_size, "nonempty")
                            && (raw_atom(&right_size, "unknown")
                                || raw_atom(&right_size, "nonempty")))
                    {
                        right_size_if_nonempty(left_size, right_size)
                    } else {
                        return Err(case_clause_pair(&left_size, &right_size));
                    };
                    let mut b = ReasonBuilder::new();
                    let c = b.copy(&contents.to_raw());
                    let t = b.copy(&termination.to_raw());
                    let id = b.tuple(vec![c, t]);
                    values.push(raw_record_with_qualifier("list", &b.finish(id), &size));
                }
            }
            InfWork::Product(n) => {
                let children = values.split_off(values.len() - n);
                let mut b = ReasonBuilder::new();
                let mut shared = std::collections::BTreeMap::new();
                let mut held = Vec::new();
                let fields = children
                    .into_iter()
                    .map(|x| {
                        let raw = x.to_raw();
                        let key = (std::sync::Arc::as_ptr(raw.arena()) as usize, raw.id());
                        let id = *shared.entry(key).or_insert_with(|| b.copy(&raw));
                        held.push(raw);
                        id
                    })
                    .collect();
                let id = b.list(fields);
                values.push(raw_record("product", &b.finish(id), "unknown"));
            }
        }
    }
    Ok(values.pop().expect("one completed native inf result"))
}
fn proper_list(raw: RawFormTerm) -> Result<Vec<RawFormTerm>, FormError> {
    let mut cursor = Cursor::new(raw);
    let mut items = Vec::new();
    loop {
        match cursor.cell() {
            Cell::Nil => return Ok(items),
            Cell::Cons(x) => {
                items.push(x);
                cursor.advance();
            }
            Cell::Other => return Err(FormError::Native(NativeTransferError::Badarg)),
        }
    }
}

fn qualifier(raw: &RawFormTerm) -> RawFormTerm {
    let ErlangTerm::Tuple(v) = raw.term() else {
        unreachable!()
    };
    child(raw, v[3])
}
fn right_size_if_nonempty(left: RawFormTerm, right: RawFormTerm) -> RawFormTerm {
    if raw_atom(&right, "nonempty") {
        right
    } else {
        left
    }
}
fn raw_record_with_qualifier(
    tag: &str,
    elements: &RawFormTerm,
    qualifier: &RawFormTerm,
) -> NativeTypeOperand {
    let structural = matches!(tag, "function" | "product" | "list");
    let mut b = ReasonBuilder::new();
    let c = b.atom("c");
    let tag = b.atom(tag);
    let e = b.copy(elements);
    let q = b.copy(qualifier);
    let id = b.tuple(vec![c, tag, e, q]);
    if structural {
        NativeTypeOperand::Raw(b.finish(id))
    } else {
        NativeTypeOperand::from_raw(b.finish(id))
    }
}
fn structural_operand(mut operand: NativeTypeOperand) -> Result<NativeTypeOperand, FormError> {
    loop {
        match operand {
            NativeTypeOperand::Canonical(t) => {
                return Ok(NativeTypeOperand::Canonical(t.structural()?));
            }
            NativeTypeOperand::Raw(ref r) => {
                let shape = Shape::new(r);
                if shape.matches("nominal", 2) {
                    let e = record_elements(r);
                    let ErlangTerm::Tuple(v) = e.term() else {
                        unreachable!()
                    };
                    operand = NativeTypeOperand::from_raw(child(&e, v[1]));
                    continue;
                }
                if matches!(
                    shape.tag,
                    Some(
                        "nominal_set"
                            | "list"
                            | "function"
                            | "product"
                            | "tuple"
                            | "tuple_set"
                            | "map"
                            | "union"
                    )
                ) {
                    return Err(FormError::Transport(
                        "native raw t_structural reached unimplemented structural clause".into(),
                    ));
                }
                return Ok(operand);
            }
        }
    }
}

fn operand_child(raw: RawFormTerm) -> NativeTypeOperand {
    let shape = Shape::new(&raw);
    if shape.matches("function", 2) || shape.matches("list", 2) || shape.tag == Some("product") {
        NativeTypeOperand::Raw(raw)
    } else {
        NativeTypeOperand::from_raw(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn shared_product(depth: usize, left: bool) -> RawFormTerm {
        let mut b = ReasonBuilder::new();
        let c = b.atom("c");
        let unknown = b.atom("weird");
        let leaf_tag = b.atom(if left { "var" } else { "unrecognized" });
        let payload = b.atom("payload");
        let mut at = b.tuple(vec![c, leaf_tag, payload, unknown]);
        let product = b.atom("product");
        for _ in 0..depth {
            let fields = b.list(vec![at, at]);
            at = b.tuple(vec![c, product, fields, unknown]);
        }
        b.finish(at)
    }
    #[test]
    fn shared_raw_pairs_evaluate_and_rebuild_without_expanding_dags() {
        let left = NativeTypeOperand::Raw(shared_product(80, true));
        let right = NativeTypeOperand::Raw(shared_product(80, false));
        let value = left.inf(&right).unwrap().to_raw();
        assert!(
            value.arena().terms.len() < 1000,
            "shared child copies must remain shared"
        );
        assert!(!NativeTypeOperand::Raw(value).is_impossible());
    }
}

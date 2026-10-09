pub use crate::variable_integer::VariableInteger;
use num_bigint::BigInt;
use num_traits::{One, Zero};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LatticeError {
    OpaqueSentinelSubtraction,
    FunctionClause,
    BadArg,
    NativeNonTerminating,
    CaseClause(Box<Type>),
    BadMatch(Box<Type>),
    BadMatchBoolean(bool),
    BadMatchNative(Box<NativeValue>),
    BadGenerators(Box<(Vec<Type>, Vec<Type>)>),
    BadGenerator(Box<Type>),
}
impl std::fmt::Display for LatticeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self{Self::BadGenerator(t)=>write!(f,"OTP bad_generator: {}",t.to_value()),Self::BadGenerators(tails)=>write!(f,"OTP bad_generators: {}",json!([tails.0.iter().map(Type::to_value).collect::<Vec<_>>(),tails.1.iter().map(Type::to_value).collect::<Vec<_>>()])),Self::BadMatchNative(v)=>write!(f,"OTP badmatch native: {}",v.to_value()),Self::BadMatchBoolean(v)=>write!(f,"OTP badmatch: {}",v),Self::BadMatch(t)=>write!(f,"OTP badmatch: {}",t.to_value()),Self::CaseClause(t)=>write!(f,"OTP case_clause: {}",t.to_value()),Self::NativeNonTerminating=>f.write_str("native_nonterminating: repeated active nominal operation pair"),Self::FunctionClause=>f.write_str("OTP function_clause"),Self::BadArg=>f.write_str("OTP badarg"),Self::OpaqueSentinelSubtraction=>f.write_str("OTP function_clause: opaque sentinel cannot be forced into structural union for subtraction")}
    }
}
impl std::error::Error for LatticeError {}
impl From<LatticeError> for String {
    fn from(e: LatticeError) -> Self {
        e.to_string()
    }
}

/// Constructor input outside the canonical runtime type domain. This is not an
/// Erlang exception: native t_bitstr/2 accepts signed integer parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitstringDomainError {
    pub unit: BigInt,
    pub base: BigInt,
}
impl std::fmt::Display for BitstringDomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "noncanonical bitstring parameters: unit={}, base={}",
            self.unit, self.base
        )
    }
}
impl std::error::Error for BitstringDomainError {}
impl From<BitstringDomainError> for String {
    fn from(error: BitstringDomainError) -> Self {
        error.to_string()
    }
}

/// Native exported constructors can produce records outside runtime invariants.
/// Such records are transportable, but cannot enter canonical type algebra.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawNativeType(NativeValue);
impl RawNativeType {
    pub fn to_native_value(&self) -> &NativeValue {
        &self.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeTypeOutcome {
    Canonical(Type),
    Raw(RawNativeType),
}
impl NativeTypeOutcome {
    pub fn to_native_value(&self) -> NativeValue {
        match self {
            Self::Canonical(t) => t.to_native_value(),
            Self::Raw(t) => t.0.clone(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Type(&'static Node);
impl std::ops::Deref for Type {
    type Target = Node;
    fn deref(&self) -> &Node {
        self.0
    }
}
impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        let same = std::ptr::eq(self.0, other.0);
        if verify_fast_paths() && !same {
            // Children are interned, so this compares one level only.
            assert!(self.0.repr != other.0.repr, "duplicate interned node: {self:?}");
        }
        same
    }
}
impl Eq for Type {}
impl std::hash::Hash for Type {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.0.hash)
    }
}
impl std::fmt::Debug for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Type").field(&self.0.repr).finish()
    }
}
/// `has_var()` holds.
const HAS_VAR: u8 = 1;
/// A variable is reachable through `append_children`.
const ANY_VAR: u8 = 2;
/// `contains_nominal()` holds.
const NOMINAL: u8 = 4;
/// Facts about a type that would otherwise need a walk over it, computed once
/// from the children's facts when the type is built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Info {
    flags: u8,
    /// The least depth for which `is_limited` holds; it holds for every
    /// greater depth too.
    limited_from: i32,
}
impl Info {
    fn of(repr: &Repr) -> Self {
        let deeper = |t: &Type| t.info.limited_from.saturating_add(1);
        let mut flags = 0;
        let mut from = 1;
        match repr {
            Repr::Any => return Self { flags: 0, limited_from: i32::MIN },
            Repr::None | Repr::Unit => {}
            Repr::Var(_) => flags = HAS_VAR | ANY_VAR,
            Repr::Opaque => flags = NOMINAL,
            Repr::Nominal(n) => {
                flags = NOMINAL | (n.inner.info.flags & (HAS_VAR | ANY_VAR));
                from = from.max(n.inner.info.limited_from);
            }
            Repr::NominalSet(n) => {
                flags = NOMINAL;
                let s = n.structural.info;
                if s.flags & HAS_VAR != 0 && n.nominals.iter().any(|m| m.inner.info.flags & HAS_VAR != 0) {
                    flags |= HAS_VAR;
                }
                flags |= s.flags & ANY_VAR;
                from = from.max(s.limited_from);
                for m in &n.nominals {
                    flags |= m.inner.info.flags & ANY_VAR;
                    from = from.max(m.inner.info.limited_from);
                }
            }
            Repr::Product(p) => {
                for t in p.iter() {
                    flags |= t.info.flags;
                    from = from.max(deeper(t));
                }
            }
            Repr::Families(s) => {
                if let Some(f) = s.functions() {
                    flags |= f.domain.info.flags | f.range.info.flags;
                    from = from.max(f.domain.info.limited_from).max(deeper(&f.range));
                }
                if let Some(Lists::List { elements, tail, .. }) = s.list() {
                    flags |= elements.info.flags | tail.info.flags;
                    from = from.max(deeper(elements));
                    if tail.info.limited_from > 1
                        && !matches!(tail.singleton_key(), Some(SingletonKey::Nil))
                    {
                        from = from.max(deeper(tail));
                    }
                }
                if let Some(Tuples::Set(groups)) = s.tuple() {
                    for v in groups.values().flatten() {
                        if v.tag.is_some() {
                            from = from.max(2);
                        }
                        for t in &v.elements {
                            flags |= t.info.flags;
                            from = from.max(deeper(t));
                        }
                    }
                }
                if let Some(m) = s.maps() {
                    flags |= m.default_key.info.flags | m.default_value.info.flags;
                    for e in &m.entries {
                        flags |= e.value.info.flags & NOMINAL;
                        from = from.max(2).max(deeper(&e.value));
                    }
                    from = from.max(deeper(&m.default_key)).max(deeper(&m.default_value));
                }
            }
        }
        Self { flags, limited_from: from }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Repr {
    Any,
    None,
    Unit,
    Families(FamilyStore),
    Product(Arc<Vec<Type>>),
    Var(VariableId),
    Opaque,
    Nominal(Arc<Nominal>),
    NominalSet(Arc<NominalSet>),
}
#[derive(Clone, Copy)]
enum CanonicalType {
    Atoms, Integers, Float, Number, Nil, Tuple, Map, Boolean,
    True, False, Infinity, Zero, One, MinusOne,
    Identifier, Pid, Port, Reference,
    Bitstring, Binary, NonemptyBitstring, NonemptyBinary,
    List, NonemptyList, MaybeImproperList, Cons, Function,
    Byte, Char, PositiveInteger, NonNegativeInteger, NegativeInteger,
}
const CANONICAL_TYPE_COUNT: usize = CanonicalType::NegativeInteger as usize + 1;
thread_local! {
    static CANONICAL_TYPES: [std::cell::OnceCell<Type>; CANONICAL_TYPE_COUNT] =
        const { [const { std::cell::OnceCell::new() }; CANONICAL_TYPE_COUNT] };
}
/// One hash-consed type: structurally equal types share a single node, so
/// equality and hashing look at the pointer. Node addresses never influence
/// ordering or output.
pub struct Node {
    hash: u64,
    info: Info,
    repr: Repr,
}
mod interner {
    use super::{Info, Node, Repr};
    use std::collections::HashMap;
    use std::hash::{Hash, Hasher};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

    /// Open addressing with slots that are only ever filled, as in
    /// `crate::atom`: lookups are atomic loads and threads never wait.
    struct Table<const SLOTS: usize> {
        slots: [AtomicPtr<Node>; SLOTS],
        overflow: Mutex<Option<HashMap<u64, Vec<&'static Node>>>>,
    }
    const PROBES: usize = 32;
    pub(super) static NODES: AtomicUsize = AtomicUsize::new(0);
    static TABLE: Table<{ 1 << 24 }> = Table {
        slots: [const { AtomicPtr::new(std::ptr::null_mut()) }; 1 << 24],
        overflow: Mutex::new(None),
    };

    pub(super) fn hash(repr: &Repr) -> u64 {
        let mut h = crate::fast_hash::FxHasher::default();
        repr.hash(&mut h);
        // FxHash leaves the low bits weak; fold the high bits in.
        let x = h.finish();
        x ^ (x >> 29) ^ (x >> 47)
    }
    pub(super) fn intern(repr: Repr) -> &'static Node {
        let hash = hash(&repr);
        let start = hash as usize;
        let mut fresh: Option<*mut Node> = None;
        let mut candidate = Some(repr);
        let slots = &TABLE.slots;
        for probe in 0..PROBES {
            let slot = &slots[(start.wrapping_add(probe)) & (slots.len() - 1)];
            let mut current = slot.load(Ordering::Acquire);
            if current.is_null() {
                let entry = *fresh.get_or_insert_with(|| {
                    let repr = candidate.take().unwrap();
                    let info = Info::of(&repr);
                    Box::into_raw(Box::new(Node { hash, info, repr }))
                });
                match slot.compare_exchange(std::ptr::null_mut(), entry, Ordering::AcqRel, Ordering::Acquire) {
                    Ok(_) => {
                        NODES.fetch_add(1, Ordering::Relaxed);
                        return unsafe { &*entry };
                    }
                    Err(winner) => current = winner,
                }
            }
            // Published nodes are leaked, so they stay valid forever.
            let existing: &'static Node = unsafe { &*current };
            if existing.hash == hash {
                let same = match (&candidate, fresh) {
                    (Some(r), _) => existing.repr == *r,
                    (None, Some(entry)) => existing.repr == unsafe { &*entry }.repr,
                    (None, None) => unreachable!(),
                };
                if same {
                    if let Some(entry) = fresh {
                        drop(unsafe { Box::from_raw(entry) });
                    }
                    return existing;
                }
            }
        }
        let node = match fresh {
            Some(entry) => unsafe { *Box::from_raw(entry) },
            None => {
                let repr = candidate.take().unwrap();
                Node { hash, info: Info::of(&repr), repr }
            }
        };
        let mut overflow = TABLE.overflow.lock().expect("type overflow table");
        let chain = overflow.get_or_insert_with(HashMap::new).entry(hash).or_default();
        if let Some(existing) = chain.iter().find(|n| n.repr == node.repr) {
            return existing;
        }
        let leaked: &'static Node = Box::leak(Box::new(node));
        NODES.fetch_add(1, Ordering::Relaxed);
        chain.push(leaked);
        leaked
    }
}
/// Per-thread caches of lattice results keyed by the operands' node
/// pointers. Only results that are a pure function of the operands are
/// stored: never errors, and never while an enclosing nominal operation is
/// active unless both operands are free of nominals (`ActivePairGuard` only
/// ever consults that stack for nominal operands).
mod memo {
    use super::{ACTIVE_PAIRS, NOMINAL, Node, Type};
    use std::cell::Cell;

    const BITS: u32 = 16;
    type Entry = Cell<(usize, usize, usize)>;
    pub(super) struct Table(std::thread::LocalKey<Box<[Entry]>>);
    thread_local! {
        static MEET_ENTRIES: Box<[Entry]> = (0..1 << BITS).map(|_| Cell::new((0, 0, 0))).collect();
    }
    pub(super) static MEET: Table = Table(MEET_ENTRIES);

    pub(super) fn pure(a: &Type, b: &Type) -> bool {
        (a.info.flags | b.info.flags) & NOMINAL == 0
            || ACTIVE_PAIRS.with(|pairs| pairs.borrow().is_empty())
    }
    fn key(a: &Type, b: &Type) -> (usize, usize, usize) {
        let (x, y) = (a.0 as *const Node as usize, b.0 as *const Node as usize);
        let h = (x ^ y.rotate_left(29)).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        (x, y, h >> (usize::BITS - BITS))
    }
    pub(super) fn get(table: &'static Table, a: &Type, b: &Type) -> Option<Type> {
        let (x, y, i) = key(a, b);
        let (ka, kb, r) = table.0.with(|t| t[i].get());
        // Stored results are interned nodes, which are never freed.
        (ka == x && kb == y).then(|| Type(unsafe { &*(r as *const Node) }))
    }
    pub(super) fn put(table: &'static Table, a: &Type, b: &Type, result: Type) {
        let (x, y, i) = key(a, b);
        table.0.with(|t| t[i].set((x, y, result.0 as *const Node as usize)));
    }
}

impl Type {
    fn new(repr: Repr) -> Self {
        static ANY: Node = Node { hash: 1, info: Info { flags: 0, limited_from: i32::MIN }, repr: Repr::Any };
        static NONE: Node = Node { hash: 2, info: Info { flags: 0, limited_from: 1 }, repr: Repr::None };
        static UNIT: Node = Node { hash: 3, info: Info { flags: 0, limited_from: 1 }, repr: Repr::Unit };
        static OPAQUE: Node = Node { hash: 4, info: Info { flags: NOMINAL, limited_from: 1 }, repr: Repr::Opaque };
        Self(match repr {
            Repr::Any => &ANY,
            Repr::None => &NONE,
            Repr::Unit => &UNIT,
            Repr::Opaque => &OPAQUE,
            repr => interner::intern(repr),
        })
    }
    fn integer_interval(from: Bound, to: Bound) -> Self {
        Self::scalar(None, Some(Numeric::Integer(Integers::Range(from, to))))
    }
    fn canonical(kind: CanonicalType) -> Self {
        CANONICAL_TYPES.with(|types| {
            types[kind as usize].get_or_init(|| Self::make_canonical(kind)).clone()
        })
    }
    fn make_canonical(kind: CanonicalType) -> Self {
        use CanonicalType::*;
        match kind {
            Atoms => Self::scalar(Some(self::Atoms::All), None),
            Integers => Self::scalar(None, Some(Numeric::Integer(self::Integers::All))),
            Float => Self::scalar(None, Some(Numeric::Float)),
            Number => Self::scalar(None, Some(Numeric::Number)),
            True | False | Infinity => Self::scalar(Some(self::Atoms::Set(BTreeSet::from([
                match kind { True => "true", False => "false", _ => "infinity" }.into(),
            ]))), None),
            Boolean => Self::scalar(Some(self::Atoms::Set(BTreeSet::from([
                "false".into(), "true".into(),
            ]))), None),
            Zero | One | MinusOne => Self::scalar(None, Some(Numeric::Integer(
                self::Integers::Set(BTreeSet::from([match kind { Zero => 0, One => 1, _ => -1 }.into()])),
            ))),
            Nil => Self::families(Slots { list: Some(Arc::new(Lists::Nil)), ..Slots::default() }),
            Tuple => Self::families(Slots { tuple: Some(Arc::new(Tuples::Any)), ..Slots::default() }),
            Map => Self::families(Slots { maps: Some(Arc::new(Maps {
                entries: Vec::new(), default_key: Box::new(Self::any()), default_value: Box::new(Self::any()),
            })), ..Slots::default() }),
            Identifier | Pid | Port | Reference => Self::families(Slots {
                identifiers: Some(Arc::new(if matches!(kind, Identifier) { Identifiers::All } else {
                    Identifiers::Set(BTreeSet::from([match kind { Pid => "pid", Port => "port", _ => "reference" }.into()]))
                })), ..Slots::default()
            }),
            Bitstring | Binary | NonemptyBitstring | NonemptyBinary => Self::families(Slots {
                bitstrings: Some(Arc::new(Bitstrings {
                    unit: if matches!(kind, Binary | NonemptyBinary) { 8.into() } else { 1.into() },
                    base: match kind { NonemptyBitstring => 1.into(), NonemptyBinary => 8.into(), _ => 0.into() },
                })), ..Slots::default()
            }),
            List | NonemptyList | MaybeImproperList | Cons => Self::families(Slots {
                list: Some(Arc::new(Lists::List {
                    elements: Box::new(Self::any()),
                    tail: Box::new(if matches!(kind, List | NonemptyList) { Self::nil() } else { Self::any() }),
                    nonempty: matches!(kind, NonemptyList | Cons),
                })), ..Slots::default()
            }),
            Function => Self::families(Slots { functions: Some(Arc::new(Functions {
                domain: Box::new(Self::any()), range: Box::new(Self::any()),
            })), ..Slots::default() }),
            Byte => Self::integer_interval(Bound::Finite(0.into()), Bound::Finite(255.into())),
            Char => Self::integer_interval(Bound::Finite(0.into()), Bound::Finite(0x10ffff.into())),
            PositiveInteger => Self::integer_interval(Bound::Finite(1.into()), Bound::PositiveInfinity),
            NonNegativeInteger => Self::integer_interval(Bound::Finite(0.into()), Bound::PositiveInfinity),
            NegativeInteger => Self::integer_interval(Bound::NegativeInfinity, Bound::Finite((-1).into())),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Atoms {
    All,
    Set(BTreeSet<crate::atom::Atom>),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Numeric {
    Float,
    Number,
    Integer(Integers),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Integers {
    All,
    Set(BTreeSet<BigInt>),
    Range(Bound, Bound),
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Bound {
    NegativeInfinity,
    Finite(BigInt),
    PositiveInfinity,
}

impl Bound {
    fn parse(value: &Value) -> Result<Self, String> {
        match value.as_str() {
            Some("neg_inf") => Ok(Self::NegativeInfinity),
            Some("pos_inf") => Ok(Self::PositiveInfinity),
            _ => integer(value).map(Self::Finite),
        }
    }
    fn value(&self) -> Value {
        match self {
            Self::NegativeInfinity => json!("neg_inf"),
            Self::PositiveInfinity => json!("pos_inf"),
            Self::Finite(n) => json!(n.to_string()),
        }
    }
    fn offset(&self, n: i32) -> Self {
        match self {
            Self::Finite(x) => Self::Finite(x + n),
            x => x.clone(),
        }
    }
}
fn integer(value: &Value) -> Result<BigInt, String> {
    let text = value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string());
    BigInt::from_str(&text).map_err(|_| format!("invalid integer {text}"))
}
fn int_set(set: BTreeSet<BigInt>) -> Option<Integers> {
    if set.is_empty() {
        None
    } else if set.len() <= 13 {
        Some(Integers::Set(set))
    } else {
        range(
            Bound::Finite(set.first().unwrap().clone()),
            Bound::Finite(set.last().unwrap().clone()),
        )
    }
}
fn checked_range(from: Bound, to: Bound) -> Result<Option<Integers>, LatticeError> {
    if matches!(
        (&from, &to),
        (Bound::PositiveInfinity, Bound::Finite(_)) | (Bound::Finite(_), Bound::NegativeInfinity)
    ) {
        return Err(LatticeError::FunctionClause);
    }
    Ok(range(from, to))
}

fn range(from: Bound, to: Bound) -> Option<Integers> {
    use Bound::*;
    let canonical = |lo: i32, hi: Bound| Some(Integers::Range(Finite(lo.into()), hi));
    match (&from, &to) {
        (PositiveInfinity, PositiveInfinity) => canonical(1, PositiveInfinity),
        (NegativeInfinity, NegativeInfinity) => {
            Some(Integers::Range(NegativeInfinity, Finite((-1).into())))
        }
        (NegativeInfinity, PositiveInfinity) => Some(Integers::All),
        (NegativeInfinity, Finite(y)) => {
            if y < &BigInt::zero() {
                Some(Integers::Range(NegativeInfinity, Finite((-1).into())))
            } else {
                Some(Integers::All)
            }
        }
        (Finite(x), PositiveInfinity) => {
            if x >= &BigInt::one() {
                canonical(1, PositiveInfinity)
            } else if x >= &BigInt::zero() {
                canonical(0, PositiveInfinity)
            } else {
                Some(Integers::All)
            }
        }
        (Finite(x), Finite(y)) if x <= y => {
            if y - x < BigInt::from(13) {
                let mut set = BTreeSet::new();
                let mut n = x.clone();
                while &n <= y {
                    set.insert(n.clone());
                    n += 1;
                }
                Some(Integers::Set(set))
            } else if x < &BigInt::zero() {
                if y < &BigInt::zero() {
                    Some(Integers::Range(NegativeInfinity, Finite((-1).into())))
                } else {
                    Some(Integers::All)
                }
            } else if y <= &BigInt::from(255) {
                canonical(if x >= &BigInt::one() { 1 } else { 0 }, Finite(255.into()))
            } else if y <= &BigInt::from(0x10ffff) {
                canonical(
                    if x >= &BigInt::one() { 1 } else { 0 },
                    Finite(0x10ffff.into()),
                )
            } else {
                canonical(if x >= &BigInt::one() { 1 } else { 0 }, PositiveInfinity)
            }
        }
        _ => None,
    }
}
impl Integers {
    fn bounds(&self) -> (Bound, Bound) {
        match self {
            Self::All => (Bound::NegativeInfinity, Bound::PositiveInfinity),
            Self::Set(s) => (
                Bound::Finite(s.first().unwrap().clone()),
                Bound::Finite(s.last().unwrap().clone()),
            ),
            Self::Range(a, b) => (a.clone(), b.clone()),
        }
    }
    fn contains(&self, n: &BigInt) -> bool {
        match self {
            Self::All => true,
            Self::Set(s) => s.contains(n),
            Self::Range(a, b) => {
                let n = Bound::Finite(n.clone());
                a <= &n && &n <= b
            }
        }
    }
    fn join(&self, rhs: &Self) -> Self {
        match (self, rhs) {
            (Self::All, _) | (_, Self::All) => Self::All,
            (Self::Set(a), Self::Set(b)) => int_set(a.union(b).cloned().collect()).unwrap(),
            _ => {
                let (a, b) = self.bounds();
                let (c, d) = rhs.bounds();
                range(a.min(c), b.max(d)).unwrap()
            }
        }
    }
    fn meet(&self, rhs: &Self) -> Option<Self> {
        match (self, rhs) {
            (Self::All, x) | (x, Self::All) => Some(x.clone()),
            (Self::Set(a), Self::Set(b)) => int_set(a.intersection(b).cloned().collect()),
            (Self::Set(a), b) | (b, Self::Set(a)) => {
                int_set(a.iter().filter(|x| b.contains(x)).cloned().collect())
            }
            _ => {
                let (a, b) = self.bounds();
                let (c, d) = rhs.bounds();
                range(a.max(c), b.min(d))
            }
        }
    }
    fn subtract(&self, rhs: &Self) -> Option<Self> {
        use Bound::*;
        match (self, rhs) {
            (_, Self::All) => None,
            (Self::All, _) => Some(Self::All),
            (Self::Set(a), Self::Set(b)) => int_set(a.difference(b).cloned().collect()),
            (Self::Range(a, b), Self::Set(s)) => {
                let lo = match a {
                    Finite(x) if s.contains(x) => a.offset(1),
                    _ => a.clone(),
                };
                let hi = match b {
                    Finite(x) if s.contains(x) => b.offset(-1),
                    _ => b.clone(),
                };
                if &lo == a && &hi == b {
                    Some(self.clone())
                } else {
                    range(lo, hi)
                }
            }
            (Self::Set(s), Self::Range(a, b)) => int_set(
                s.iter()
                    .filter(|x| match (a, b) {
                        (Finite(lo), Finite(hi)) => *x > lo && *x < hi,
                        (Finite(lo), _) => *x > lo,
                        _ => false,
                    })
                    .cloned()
                    .collect(),
            ),
            (Self::Range(a, b), Self::Range(_, _)) => match self.meet(rhs) {
                None => Some(self.clone()),
                Some(ref x) if x == self => None,
                Some(Self::Range(NegativeInfinity, hi)) => range(hi.offset(1), b.clone()),
                Some(Self::Range(lo, PositiveInfinity)) => range(a.clone(), lo.offset(-1)),
                Some(Self::Range(lo, hi)) => {
                    let left = range(a.clone(), lo.offset(-1));
                    let right = range(hi.offset(1), hi);
                    match (left, right) {
                        (Some(x), Some(y)) => Some(x.join(&y)),
                        (x, None) | (None, x) => x,
                    }
                }
                Some(_) => unreachable!("canonical range intersection is a range"),
            },
        }
    }
    fn value(&self) -> Value {
        match self {
            Self::All => json!({"kind":"integer"}),
            Self::Set(s) => {
                json!({"kind":"integers","values":s.iter().map(ToString::to_string).collect::<Vec<_>>()})
            }
            Self::Range(a, b) => json!({"kind":"range","from":a.value(),"to":b.value()}),
        }
    }
}
impl Atoms {
    fn join(&self, rhs: &Self) -> Self {
        match (self, rhs) {
            (Self::All, _) | (_, Self::All) => Self::All,
            (Self::Set(a), Self::Set(b)) => {
                let s: BTreeSet<_> = a.union(b).cloned().collect();
                if s.len() > 13 {
                    Self::All
                } else {
                    Self::Set(s)
                }
            }
        }
    }
    fn meet(&self, rhs: &Self) -> Option<Self> {
        match (self, rhs) {
            (Self::All, x) | (x, Self::All) => Some(x.clone()),
            (Self::Set(a), Self::Set(b)) => {
                let s: BTreeSet<_> = a.intersection(b).cloned().collect();
                if s.is_empty() {
                    None
                } else {
                    Some(Self::Set(s))
                }
            }
        }
    }
    fn meet_shared(x: &Arc<Self>, y: &Arc<Self>) -> Option<Arc<Self>> {
        match (&**x, &**y) {
            (Self::All, _) => Some(y.clone()),
            (_, Self::All) => Some(x.clone()),
            (a, b) => a.meet(b).map(|value| reuse(value, x, y)),
        }
    }
    fn subtract(&self, rhs: &Self) -> Option<Self> {
        match (self, rhs) {
            (_, Self::All) => None,
            (Self::All, _) => Some(Self::All),
            (Self::Set(a), Self::Set(b)) => {
                let s: BTreeSet<_> = a.difference(b).cloned().collect();
                if s.is_empty() {
                    None
                } else {
                    Some(Self::Set(s))
                }
            }
        }
    }
    fn value(&self) -> Value {
        match self {
            Self::All => json!({"kind":"atom"}),
            Self::Set(s) => json!({"kind":"atoms","values":s}),
        }
    }
}
impl Numeric {
    fn join(&self, rhs: &Self) -> Self {
        match (self, rhs) {
            (Self::Integer(a), Self::Integer(b)) => Self::Integer(a.join(b)),
            (Self::Float, Self::Float) => Self::Float,
            _ => Self::Number,
        }
    }
    fn meet(&self, rhs: &Self) -> Option<Self> {
        match (self, rhs) {
            (Self::Number, x) | (x, Self::Number) => Some(x.clone()),
            (Self::Float, Self::Float) => Some(Self::Float),
            (Self::Integer(a), Self::Integer(b)) => a.meet(b).map(Self::Integer),
            _ => None,
        }
    }
    fn meet_shared(x: &Arc<Self>, y: &Arc<Self>) -> Option<Arc<Self>> {
        match (&**x, &**y) {
            (Self::Number, _) | (Self::Integer(Integers::All), Self::Integer(_)) => Some(y.clone()),
            (_, Self::Number) | (Self::Integer(_), Self::Integer(Integers::All)) => Some(x.clone()),
            (a, b) => a.meet(b).map(|value| reuse(value, x, y)),
        }
    }
    fn subtract(&self, rhs: &Self) -> Option<Self> {
        match (self, rhs) {
            (_, Self::Number) => None,
            (Self::Float, Self::Float) => None,
            (Self::Number, Self::Float) => Some(Self::Integer(Integers::All)),
            (Self::Number, Self::Integer(Integers::All)) => Some(Self::Float),
            (Self::Integer(a), Self::Integer(b)) => a.subtract(b).map(Self::Integer),
            _ => Some(self.clone()),
        }
    }
    fn value(&self) -> Value {
        match self {
            Self::Number => json!({"kind":"number"}),
            Self::Float => json!({"kind":"float"}),
            Self::Integer(i) => i.value(),
        }
    }
}
/// The operand that already holds `value`, or a fresh allocation, so results
/// equal to an input keep sharing that input's storage.
fn reuse<T: PartialEq>(value: T, x: &Arc<T>, y: &Arc<T>) -> Arc<T> {
    if **x == value {
        x.clone()
    } else if **y == value {
        y.clone()
    } else {
        Arc::new(value)
    }
}
fn reuse_arc<T: PartialEq>(value: Arc<T>, x: &Arc<T>, y: &Arc<T>) -> Arc<T> {
    if value == *x {
        x.clone()
    } else if value == *y {
        y.clone()
    } else {
        value
    }
}
fn reuse_left<T: PartialEq>(value: T, x: &Arc<T>) -> Arc<T> {
    if **x == value { x.clone() } else { Arc::new(value) }
}
fn join_slot<T: PartialEq>(
    a: Option<&Arc<T>>,
    b: Option<&Arc<T>>,
    op: impl Fn(&T, &T) -> T,
) -> Option<Arc<T>> {
    match (a, b) {
        (Some(x), Some(y)) => Some(reuse(op(x, y), x, y)),
        (x, None) | (None, x) => x.cloned(),
    }
}
fn meet_slot<T>(
    a: Option<&Arc<T>>,
    b: Option<&Arc<T>>,
    op: impl Fn(&Arc<T>, &Arc<T>) -> Option<Arc<T>>,
) -> Option<Arc<T>> {
    match (a, b) {
        (Some(x), Some(y)) => op(x, y),
        _ => None,
    }
}
fn subtract_slot<T: PartialEq>(
    a: Option<&Arc<T>>,
    b: Option<&Arc<T>>,
    op: impl Fn(&T, &T) -> Option<T>,
) -> Option<Arc<T>> {
    match (a, b) {
        (Some(x), Some(y)) => op(x, y).map(|value| reuse_left(value, x)),
        (x, None) => x.cloned(),
        _ => None,
    }
}
fn checked_join_slot<T>(
    a: Option<&Arc<T>>,
    b: Option<&Arc<T>>,
    op: impl Fn(&Arc<T>, &Arc<T>) -> Result<Option<Arc<T>>, LatticeError>,
) -> Result<Option<Arc<T>>, LatticeError> {
    match (a, b) {
        (Some(x), Some(y)) => op(x, y),
        (x, None) | (None, x) => Ok(x.cloned()),
    }
}
fn checked_meet_slot<T>(
    a: Option<&Arc<T>>,
    b: Option<&Arc<T>>,
    op: impl Fn(&Arc<T>, &Arc<T>) -> Result<Option<Arc<T>>, LatticeError>,
) -> Result<Option<Arc<T>>, LatticeError> {
    match (a, b) {
        (Some(x), Some(y)) => op(x, y),
        _ => Ok(None),
    }
}
fn checked_subtract_slot<T>(
    a: Option<&Arc<T>>,
    b: Option<&Arc<T>>,
    op: impl Fn(&Arc<T>, &Arc<T>) -> Result<Option<Arc<T>>, LatticeError>,
) -> Result<Option<Arc<T>>, LatticeError> {
    match (a, b) {
        (Some(x), Some(y)) => op(x, y),
        (x, None) => Ok(x.cloned()),
        _ => Ok(None),
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
struct Slots {
    union_record: bool,
    atoms: Option<Arc<Atoms>>,
    bitstrings: Option<Arc<Bitstrings>>,
    functions: Option<Arc<Functions>>,
    identifiers: Option<Arc<Identifiers>>,
    list: Option<Arc<Lists>>,
    number: Option<Arc<Numeric>>,
    tuple: Option<Arc<Tuples>>,
    maps: Option<Arc<Maps>>,
}
#[derive(Clone, PartialEq, Eq, Hash)]
enum FamilyStore {
    Atoms(Arc<Atoms>),
    Bitstrings(Arc<Bitstrings>),
    Functions(Arc<Functions>),
    Identifiers(Arc<Identifiers>),
    Lists(Arc<Lists>),
    Numeric(Arc<Numeric>),
    Tuples(Arc<Tuples>),
    Maps(Arc<Maps>),
    Union(Arc<Slots>),
}
impl FamilyStore {
    fn raw(mut slots: Slots) -> Self {
        if !slots.union_record && slots.count() == 1 {
            if let Some(value) = slots.atoms.take() {
                return Self::Atoms(value);
            }
            if let Some(value) = slots.bitstrings.take() {
                return Self::Bitstrings(value);
            }
            if let Some(value) = slots.functions.take() {
                return Self::Functions(value);
            }
            if let Some(value) = slots.identifiers.take() {
                return Self::Identifiers(value);
            }
            if let Some(value) = slots.list.take() {
                return Self::Lists(value);
            }
            if let Some(value) = slots.number.take() {
                return Self::Numeric(value);
            }
            if let Some(value) = slots.tuple.take() {
                return Self::Tuples(value);
            }
            if let Some(value) = slots.maps.take() {
                return Self::Maps(value);
            }
        }
        Self::Union(Arc::new(slots))
    }
    fn atoms_owner(&self) -> Option<&Arc<Atoms>> {
        match self {
            Self::Atoms(value) => Some(value),
            Self::Union(slots) => slots.atoms.as_ref(),
            _ => None,
        }
    }
    fn atoms(&self) -> Option<&Atoms> {
        self.atoms_owner().map(Arc::as_ref)
    }
    fn bitstrings_owner(&self) -> Option<&Arc<Bitstrings>> {
        match self {
            Self::Bitstrings(value) => Some(value),
            Self::Union(slots) => slots.bitstrings.as_ref(),
            _ => None,
        }
    }
    fn bitstrings(&self) -> Option<&Bitstrings> {
        self.bitstrings_owner().map(Arc::as_ref)
    }
    fn functions_owner(&self) -> Option<&Arc<Functions>> {
        match self {
            Self::Functions(value) => Some(value),
            Self::Union(slots) => slots.functions.as_ref(),
            _ => None,
        }
    }
    fn functions(&self) -> Option<&Functions> {
        self.functions_owner().map(Arc::as_ref)
    }
    fn identifiers_owner(&self) -> Option<&Arc<Identifiers>> {
        match self {
            Self::Identifiers(value) => Some(value),
            Self::Union(slots) => slots.identifiers.as_ref(),
            _ => None,
        }
    }
    fn identifiers(&self) -> Option<&Identifiers> {
        self.identifiers_owner().map(Arc::as_ref)
    }
    fn list_owner(&self) -> Option<&Arc<Lists>> {
        match self {
            Self::Lists(value) => Some(value),
            Self::Union(slots) => slots.list.as_ref(),
            _ => None,
        }
    }
    fn list(&self) -> Option<&Lists> {
        self.list_owner().map(Arc::as_ref)
    }
    fn number_owner(&self) -> Option<&Arc<Numeric>> {
        match self {
            Self::Numeric(value) => Some(value),
            Self::Union(slots) => slots.number.as_ref(),
            _ => None,
        }
    }
    fn number(&self) -> Option<&Numeric> {
        self.number_owner().map(Arc::as_ref)
    }
    fn tuple_owner(&self) -> Option<&Arc<Tuples>> {
        match self {
            Self::Tuples(value) => Some(value),
            Self::Union(slots) => slots.tuple.as_ref(),
            _ => None,
        }
    }
    fn tuple(&self) -> Option<&Tuples> {
        self.tuple_owner().map(Arc::as_ref)
    }
    fn maps_owner(&self) -> Option<&Arc<Maps>> {
        match self {
            Self::Maps(value) => Some(value),
            Self::Union(slots) => slots.maps.as_ref(),
            _ => None,
        }
    }
    fn maps(&self) -> Option<&Maps> {
        self.maps_owner().map(Arc::as_ref)
    }
    fn union_record(&self) -> bool {
        matches!(self, Self::Union(slots) if slots.union_record)
    }
    fn count(&self) -> usize {
        match self {
            Self::Union(slots) => slots.count(),
            _ => 1,
        }
    }
    fn empty(&self) -> bool {
        self.count() == 0
    }
    fn into_slots(self) -> Slots {
        match self {
            Self::Atoms(value) => Slots {
                atoms: Some(value),
                ..Slots::default()
            },
            Self::Bitstrings(value) => Slots {
                bitstrings: Some(value),
                ..Slots::default()
            },
            Self::Functions(value) => Slots {
                functions: Some(value),
                ..Slots::default()
            },
            Self::Identifiers(value) => Slots {
                identifiers: Some(value),
                ..Slots::default()
            },
            Self::Lists(value) => Slots {
                list: Some(value),
                ..Slots::default()
            },
            Self::Numeric(value) => Slots {
                number: Some(value),
                ..Slots::default()
            },
            Self::Tuples(value) => Slots {
                tuple: Some(value),
                ..Slots::default()
            },
            Self::Maps(value) => Slots {
                maps: Some(value),
                ..Slots::default()
            },
            Self::Union(slots) => Arc::unwrap_or_clone(slots),
        }
    }
    fn to_slots(&self) -> Slots {
        self.clone().into_slots()
    }
    fn join(&self, b: &Self) -> Result<Slots, LatticeError> {
        Ok(Slots {
            union_record: false,
            atoms: join_slot(self.atoms_owner(), b.atoms_owner(), Atoms::join),
            bitstrings: join_slot(self.bitstrings_owner(), b.bitstrings_owner(), Bitstrings::join),
            functions: checked_join_slot(
                self.functions_owner(),
                b.functions_owner(),
                Functions::join_shared,
            )?,
            identifiers: join_slot(self.identifiers_owner(), b.identifiers_owner(), Identifiers::join),
            list: checked_join_slot(self.list_owner(), b.list_owner(), Lists::join_shared)?,
            number: join_slot(self.number_owner(), b.number_owner(), Numeric::join),
            tuple: checked_join_slot(self.tuple_owner(), b.tuple_owner(), Tuples::join_shared)?,
            maps: checked_join_slot(self.maps_owner(), b.maps_owner(), Maps::join_shared)?,
        })
    }
    fn meet(&self, b: &Self) -> Result<Slots, LatticeError> {
        Ok(Slots {
            union_record: false,
            atoms: meet_slot(self.atoms_owner(), b.atoms_owner(), Atoms::meet_shared),
            bitstrings: meet_slot(self.bitstrings_owner(), b.bitstrings_owner(), |x, y| {
                x.meet(y).map(|value| reuse(value, x, y))
            }),
            functions: checked_meet_slot(
                self.functions_owner(),
                b.functions_owner(),
                Functions::meet_shared,
            )?,
            identifiers: meet_slot(
                self.identifiers_owner(),
                b.identifiers_owner(),
                Identifiers::meet_shared,
            ),
            list: checked_meet_slot(self.list_owner(), b.list_owner(), Lists::meet_shared)?,
            number: meet_slot(self.number_owner(), b.number_owner(), Numeric::meet_shared),
            tuple: checked_meet_slot(self.tuple_owner(), b.tuple_owner(), Tuples::meet_shared)?,
            maps: checked_meet_slot(self.maps_owner(), b.maps_owner(), Maps::meet_shared)?,
        })
    }
    fn subtract(&self, b: &Self) -> Result<Slots, LatticeError> {
        Ok({
            let atoms = subtract_slot(self.atoms_owner(), b.atoms_owner(), Atoms::subtract);
            let bitstrings =
                subtract_slot(self.bitstrings_owner(), b.bitstrings_owner(), Bitstrings::subtract);
            let functions = checked_subtract_slot(
                self.functions_owner(),
                b.functions_owner(),
                Functions::subtract_shared,
            )?;
            let identifiers =
                subtract_slot(self.identifiers_owner(), b.identifiers_owner(), Identifiers::subtract);
            let list = checked_subtract_slot(self.list_owner(), b.list_owner(), Lists::subtract_shared)?;
            let number = subtract_slot(self.number_owner(), b.number_owner(), Numeric::subtract);
            let maps =
                checked_subtract_slot(self.maps_owner(), b.maps_owner(), Maps::subtract_shared)?;
            let tuple =
                checked_subtract_slot(self.tuple_owner(), b.tuple_owner(), Tuples::subtract_shared)?;
            Slots {
                union_record: false,
                atoms,
                bitstrings,
                functions,
                identifiers,
                list,
                number,
                tuple,
                maps,
            }
        })
    }
    fn limit_aux(&self, k: i64) -> Result<Slots, LatticeError> {
        Ok(Slots {
            list: self.list_owner().map(|l| Lists::limit_shared(l, k)).transpose()?,
            functions: self
                .functions_owner()
                .map(|f| Functions::limit_shared(f, k))
                .transpose()?,
            tuple: self
                .tuple_owner()
                .map(|t| Tuples::limit_shared(t, k))
                .transpose()?
                .flatten(),
            maps: self
                .maps_owner()
                .map(|m| Maps::limit_shared(m, k))
                .transpose()?
                .flatten(),
            ..self.rebuild_base()
        })
    }
    fn rebuild_base(&self) -> Slots {
        Slots {
            union_record: self.union_record(),
            atoms: self.atoms_owner().cloned(),
            bitstrings: self.bitstrings_owner().cloned(),
            functions: None,
            identifiers: self.identifiers_owner().cloned(),
            list: match self.list() {
                Some(Lists::Nil) => self.list_owner().cloned(),
                _ => None,
            },
            number: self.number_owner().cloned(),
            tuple: match self.tuple() {
                Some(Tuples::Any) => self.tuple_owner().cloned(),
                _ => None,
            },
            maps: None,
        }
    }
    /// Whether `Type::families(slots)` would rebuild exactly this store.
    fn is_families_of(&self, slots: &Slots) -> bool {
        self.same_fields(slots)
            && match self {
                Self::Union(s) => s.union_record && slots.count() > 1,
                _ => true,
            }
    }
    /// Whether `FamilyStore::raw(slots)` would rebuild exactly this store.
    fn is_raw_of(&self, slots: &Slots) -> bool {
        self.same_fields(slots)
            && match self {
                Self::Union(s) => s.union_record == slots.union_record && (s.union_record || slots.count() != 1),
                _ => !slots.union_record,
            }
    }
    fn same_fields(&self, slots: &Slots) -> bool {
        fn same<T>(a: Option<&Arc<T>>, b: &Option<Arc<T>>) -> bool {
            match (a, b) {
                (None, None) => true,
                (Some(x), Some(y)) => Arc::ptr_eq(x, y),
                _ => false,
            }
        }
        same(self.atoms_owner(), &slots.atoms)
            && same(self.bitstrings_owner(), &slots.bitstrings)
            && same(self.functions_owner(), &slots.functions)
            && same(self.identifiers_owner(), &slots.identifiers)
            && same(self.list_owner(), &slots.list)
            && same(self.number_owner(), &slots.number)
            && same(self.tuple_owner(), &slots.tuple)
            && same(self.maps_owner(), &slots.maps)
    }
    fn slot_values(&self) -> Vec<serde_json::Value> {
        self.to_slots().slot_values()
    }
}
impl std::fmt::Debug for FamilyStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Slots")
            .field("union_record", &self.union_record())
            .field("atoms", &self.atoms())
            .field("bitstrings", &self.bitstrings())
            .field("functions", &self.functions())
            .field("identifiers", &self.identifiers())
            .field("list", &self.list())
            .field("number", &self.number())
            .field("tuple", &self.tuple())
            .field("maps", &self.maps())
            .finish()
    }
}

impl FamilyStore {
    fn shares_storage(&self, rhs: &Self) -> bool {
        match (self, rhs) {
            (Self::Atoms(a), Self::Atoms(b)) => Arc::ptr_eq(a, b),
            (Self::Bitstrings(a), Self::Bitstrings(b)) => Arc::ptr_eq(a, b),
            (Self::Functions(a), Self::Functions(b)) => Arc::ptr_eq(a, b),
            (Self::Identifiers(a), Self::Identifiers(b)) => Arc::ptr_eq(a, b),
            (Self::Lists(a), Self::Lists(b)) => Arc::ptr_eq(a, b),
            (Self::Numeric(a), Self::Numeric(b)) => Arc::ptr_eq(a, b),
            (Self::Tuples(a), Self::Tuples(b)) => Arc::ptr_eq(a, b),
            (Self::Maps(a), Self::Maps(b)) => Arc::ptr_eq(a, b),
            (Self::Union(a), Self::Union(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}
impl Slots {
    fn count(&self) -> usize {
        [
            self.atoms.is_some(),
            self.bitstrings.is_some(),
            self.functions.is_some(),
            self.identifiers.is_some(),
            self.list.is_some(),
            self.number.is_some(),
            self.tuple.is_some(),
            self.maps.is_some(),
        ]
        .into_iter()
        .filter(|v| *v)
        .count()
    }
    fn empty(&self) -> bool {
        self.atoms.is_none()
            && self.bitstrings.is_none()
            && self.functions.is_none()
            && self.identifiers.is_none()
            && self.list.is_none()
            && self.number.is_none()
            && self.tuple.is_none()
            && self.maps.is_none()
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Identifiers {
    All,
    Set(BTreeSet<String>),
}
impl Identifiers {
    fn join(&self, b: &Self) -> Self {
        match (self, b) {
            (Self::All, _) | (_, Self::All) => Self::All,
            (Self::Set(a), Self::Set(b)) => Self::Set(a.union(b).cloned().collect()),
        }
    }
    fn meet(&self, b: &Self) -> Option<Self> {
        match (self, b) {
            (Self::All, x) | (x, Self::All) => Some(x.clone()),
            (Self::Set(a), Self::Set(b)) => {
                let s: BTreeSet<_> = a.intersection(b).cloned().collect();
                if s.is_empty() {
                    None
                } else {
                    Some(Self::Set(s))
                }
            }
        }
    }
    fn meet_shared(x: &Arc<Self>, y: &Arc<Self>) -> Option<Arc<Self>> {
        match (&**x, &**y) {
            (Self::All, _) => Some(y.clone()),
            (_, Self::All) => Some(x.clone()),
            (a, b) => a.meet(b).map(|value| reuse(value, x, y)),
        }
    }
    fn subtract(&self, b: &Self) -> Option<Self> {
        match (self, b) {
            (_, Self::All) => None,
            (Self::All, _) => Some(Self::All),
            (Self::Set(a), Self::Set(b)) => {
                let s: BTreeSet<_> = a.difference(b).cloned().collect();
                if s.is_empty() {
                    None
                } else {
                    Some(Self::Set(s))
                }
            }
        }
    }
    fn value(&self) -> Value {
        match self {
            Self::All => json!({"kind":"identifier"}),
            Self::Set(s) => json!({"kind":"identifiers","values":s}),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Lists {
    Nil,
    List {
        elements: Box<Type>,
        tail: Box<Type>,
        nonempty: bool,
    },
}
impl Lists {
    /// A list node with these fields, reusing `x` or `y` when one already
    /// holds exactly them.
    fn shared(elements: Type, tail: Type, nonempty: bool, x: &Arc<Self>, y: &Arc<Self>) -> Arc<Self> {
        let holds = |l: &Self| {
            matches!(l, Self::List { elements: e, tail: t, nonempty: n }
                if *n == nonempty && **e == elements && **t == tail)
        };
        if holds(x) {
            x.clone()
        } else if holds(y) {
            y.clone()
        } else {
            Arc::new(Self::List {
                elements: Box::new(elements),
                tail: Box::new(tail),
                nonempty,
            })
        }
    }
    fn join_shared(x: &Arc<Self>, y: &Arc<Self>) -> Result<Option<Arc<Self>>, LatticeError> {
        Ok(Some(match (&**x, &**y) {
            (Self::Nil, Self::Nil) => x.clone(),
            (Self::Nil, Self::List { elements, tail, .. })
            | (Self::List { elements, tail, .. }, Self::Nil) => Self::shared(
                (**elements).clone(),
                tail.join(&Type::nil())?,
                false,
                x,
                y,
            ),
            (
                Self::List {
                    elements: a,
                    tail: t,
                    nonempty: n,
                },
                Self::List {
                    elements: b,
                    tail: u,
                    nonempty: m,
                },
            ) => {
                let elements = a.join(b)?;
                Self::shared(elements, t.join(u)?, *n && *m, x, y)
            }
        }))
    }
    fn meet_shared(x: &Arc<Self>, y: &Arc<Self>) -> Result<Option<Arc<Self>>, LatticeError> {
        Ok({
            match (&**x, &**y) {
                (Self::Nil, Self::Nil) => Some(x.clone()),
                (Self::Nil, Self::List { nonempty: true, .. })
                | (Self::List { nonempty: true, .. }, Self::Nil) => None,
                (Self::Nil, Self::List { tail, .. }) | (Self::List { tail, .. }, Self::Nil) => {
                    if tail.structural()?.meet(&Type::nil())?.impossible() {
                        None
                    } else if matches!(**x, Self::Nil) {
                        Some(x.clone())
                    } else {
                        Some(y.clone())
                    }
                }
                (
                    Self::List {
                        elements: a,
                        tail: t,
                        nonempty: n,
                    },
                    Self::List {
                        elements: b,
                        tail: u,
                        nonempty: m,
                    },
                ) => {
                    let tail = t.meet(u)?;
                    if tail == Type::none() {
                        return Ok(None);
                    }
                    let elements = a.meet(b)?;
                    if elements == Type::none() {
                        if !n && !m { Some(Arc::new(Self::Nil)) } else { None }
                    } else {
                        Some(Self::shared(elements, tail, *n || *m, x, y))
                    }
                }
            }
        })
    }
    fn subtract_shared(x: &Arc<Self>, y: &Arc<Self>) -> Result<Option<Arc<Self>>, LatticeError> {
        Ok({
            match (&**x, &**y) {
                (Self::Nil, Self::Nil) => None,
                (Self::Nil, Self::List { nonempty: true, .. }) => Some(x.clone()),
                (Self::Nil, Self::List { .. }) => None,
                (
                    Self::List {
                        elements,
                        tail,
                        nonempty: false,
                    },
                    Self::Nil,
                ) if **tail == Type::nil() => Some(Arc::new(Self::List {
                    elements: elements.clone(),
                    tail: tail.clone(),
                    nonempty: true,
                })),
                (Self::List { .. }, Self::Nil) => Some(x.clone()),
                (
                    Self::List {
                        elements: a,
                        tail: t,
                        nonempty: n,
                    },
                    Self::List {
                        elements: b,
                        tail: u,
                        nonempty: m,
                    },
                ) => {
                    if a.is_subtype(b)? && t.is_subtype(u)? {
                        if !n && *m { Some(Arc::new(Self::Nil)) } else { None }
                    } else {
                        Some(x.clone())
                    }
                }
            }
        })
    }
    fn limit_shared(x: &Arc<Self>, k: i64) -> Result<Arc<Self>, LatticeError> {
        Ok({
            match &**x {
                Self::Nil => x.clone(),
                Self::List {
                    elements,
                    tail,
                    nonempty,
                } => {
                    let elements = elements.limit_aux(k - 1)?;
                    let tail = if **tail == Type::nil() {
                        Type::nil()
                    } else {
                        tail.limit_aux((k - 1).max(1))?
                    };
                    Self::shared(elements, tail, *nonempty, x, x)
                }
            }
        })
    }
    fn value(&self) -> Value {
        match self {
            Self::Nil => json!({"kind":"nil"}),
            Self::List {
                elements,
                tail,
                nonempty,
            } => {
                json!({"kind":"list","elements":elements.to_value(),"tail":tail.to_value(),"nonempty":nonempty})
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TupleVariant {
    elements: Vec<Type>,
    tag: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Tuples {
    Any,
    Set(BTreeMap<usize, Vec<TupleVariant>>),
}
impl Tuples {
    fn construct(elements: Vec<Type>) -> Result<Option<Self>, LatticeError> {
        if elements.iter().any(Type::impossible) {
            return Ok(None);
        }
        let first = elements.first().map(Type::shallow_structural).transpose()?;
        let tags = first
            .as_ref()
            .and_then(|first| match &first.repr {
                Repr::Families(slots) => {
                    if !slots.union_record()
                        && slots.count() == usize::from(slots.atoms().is_some())
                        && let Some(Atoms::Set(atoms)) = slots.atoms()
                        && atoms.len() <= 5
                    {
                        Some(atoms.iter().cloned().map(Some).collect::<Vec<_>>())
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .unwrap_or_else(|| vec![None]);
        let split = tags.len() > 1;
        let variants = tags
            .into_iter()
            .map(|tag| {
                let mut elements = elements.clone();
                if split && let Some(t) = &tag {
                    elements[0] = Type::atom(t)
                }
                TupleVariant { elements, tag: tag.map(String::from) }
            })
            .collect();
        Ok(Some(Self::Set(BTreeMap::from([(
            elements.len(),
            variants,
        )]))))
    }
    fn join_shared(x: &Arc<Self>, y: &Arc<Self>) -> Result<Option<Arc<Self>>, LatticeError> {
        match (&**x, &**y) {
            (Self::Any, _) => Ok(Some(x.clone())),
            (_, Self::Any) => Ok(Some(y.clone())),
            (a, b) => Ok(Some(reuse(a.join(b)?, x, y))),
        }
    }
    fn join(&self, b: &Self) -> Result<Self, LatticeError> {
        Ok({
            let (Self::Set(a), Self::Set(b)) = (self, b) else {
                return Ok(Self::Any);
            };
            let mut groups = a.clone();
            for (arity, variants) in b {
                groups.entry(*arity).or_default().extend(variants.clone());
            }
            if groups.len() > 8 {
                return Ok(Self::Any);
            }
            for variants in groups.values_mut() {
                let has_any = variants.iter().any(|v| v.tag.is_none());
                let tag_count = variants
                    .iter()
                    .filter_map(|v| v.tag.clone())
                    .collect::<BTreeSet<_>>()
                    .len();
                if has_any || tag_count > 5 {
                    let mut elements = variants[0].elements.clone();
                    for v in &variants[1..] {
                        for (a, b) in elements.iter_mut().zip(&v.elements) {
                            *a = a.join(b)?
                        }
                    }
                    *variants = vec![TupleVariant {
                        elements,
                        tag: None,
                    }];
                } else {
                    let mut bytag: BTreeMap<String, Vec<Type>> = BTreeMap::new();
                    for v in variants.drain(..) {
                        let tag = v.tag.unwrap();
                        if let Some(es) = bytag.get_mut(&tag) {
                            for (a, b) in es.iter_mut().zip(v.elements) {
                                *a = a.join(&b)?
                            }
                        } else {
                            bytag.insert(tag, v.elements);
                        }
                    }
                    *variants = bytag
                        .into_iter()
                        .map(|(tag, elements)| TupleVariant {
                            elements,
                            tag: Some(tag),
                        })
                        .collect();
                }
            }
            Self::Set(groups)
        })
    }
    fn meet_shared(x: &Arc<Self>, y: &Arc<Self>) -> Result<Option<Arc<Self>>, LatticeError> {
        Ok({
            let (Self::Set(a), Self::Set(b)) = (&**x, &**y) else {
                return Ok(Some(if matches!(**x, Self::Any) {
                    y.clone()
                } else {
                    x.clone()
                }));
            };
            // Native plain tuple meet ignores tags retained by opacity marking.
            let plain_pair = a.len() == 1
                && b.len() == 1
                && a.values().next().is_some_and(|v| v.len() == 1)
                && b.values().next().is_some_and(|v| v.len() == 1);
            let mut out = Type::none();
            for (arity, left) in a {
                if let Some(right) = b.get(arity) {
                    for x in left {
                        for y in right {
                            if !plain_pair && x.tag.is_some() && y.tag.is_some() && x.tag != y.tag {
                                continue;
                            }
                            let elements = x
                                .elements
                                .iter()
                                .zip(&y.elements)
                                .map(|(a, b)| a.meet(b))
                                .collect::<Result<Vec<_>, _>>()?;
                            out = out.join(&Type::tuple(elements)?)?;
                        }
                    }
                }
            }
            out.into_tuple_slot().map(|t| reuse_arc(t, x, y))
        })
    }
    fn subtract_one(a: &TupleVariant, b: &TupleVariant) -> Result<Type, LatticeError> {
        let deltas = a
            .elements
            .iter()
            .zip(&b.elements)
            .map(|(x, y)| x.subtract(y))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(
            match deltas.iter().filter(|x| **x != Type::none()).count() {
                0 => Type::none(),
                1 => Type::tuple(
                    a.elements
                        .iter()
                        .zip(deltas)
                        .map(|(a, b)| if b == Type::none() { a.clone() } else { b })
                        .collect(),
                )?,
                _ => Type::tuple(a.elements.clone())?,
            },
        )
    }
    fn subtract_shared(x: &Arc<Self>, b: &Arc<Self>) -> Result<Option<Arc<Self>>, LatticeError> {
        if matches!(**b, Self::Any) {
            return Ok(None);
        }
        let Self::Set(a) = &**x else {
            return Ok(Some(x.clone()));
        };
        let Self::Set(b) = &**b else { unreachable!() };
        let mut out = Type::none();
        for (arity, variants) in a {
            for v in variants {
                let value = if let Some(right) = b.get(arity) {
                    right
                        .iter()
                        .map(|r| Self::subtract_one(v, r))
                        .collect::<Result<Vec<_>, _>>()?
                        .into_iter()
                        .try_fold(Type::any(), |a, b| a.meet(&b))?
                } else {
                    Type::tuple(v.elements.clone())?
                };
                out = out.join(&value)?
            }
        }
        Ok(out.into_tuple_slot().map(|t| reuse_arc(t, x, x)))
    }
    fn limit_shared(x: &Arc<Self>, k: i64) -> Result<Option<Arc<Self>>, LatticeError> {
        Ok({
            match &**x {
                Self::Any => Some(x.clone()),
                Self::Set(groups) => {
                    let mut out = Type::none();
                    for (arity, variants) in groups {
                        for v in variants {
                            let tuple = if k == 1 {
                                Type::tuple_arity(*arity)
                            } else {
                                Type::tuple(
                                    v.elements
                                        .iter()
                                        .map(|x| x.limit_aux(k - 1))
                                        .collect::<Result<Vec<_>, _>>()?,
                                )?
                            };
                            out = out.join(&tuple)?
                        }
                    }
                    out.into_tuple_slot().map(|t| reuse_arc(t, x, x))
                }
            }
        })
    }
    fn value(&self) -> Value {
        match self {
            Self::Any => json!({"kind":"tuple_any"}),
            Self::Set(groups) => {
                let values:Vec<_>=groups.values().flat_map(|v|v.iter()).map(|v|json!({"kind":"tuple","elements":v.elements.iter().map(Type::to_value).collect::<Vec<_>>()})).collect();
                if values.len() == 1 {
                    values.into_iter().next().unwrap()
                } else {
                    json!({"kind":"tuple_set","values":values})
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Bitstrings {
    unit: BigInt,
    base: BigInt,
}
fn gcd(mut a: BigInt, mut b: BigInt) -> BigInt {
    while !b.is_zero() {
        let r = &a % &b;
        a = b;
        b = r;
    }
    a
}
fn modulo(a: BigInt, b: &BigInt) -> BigInt {
    let r = a % b;
    if r < BigInt::zero() { r + b } else { r }
}
fn inverse(a: &BigInt, m: &BigInt) -> BigInt {
    let (mut old_r, mut r) = (a.clone(), m.clone());
    let (mut old_s, mut s) = (BigInt::one(), BigInt::zero());
    while !r.is_zero() {
        let q = &old_r / &r;
        (old_r, r) = (r.clone(), old_r - q.clone() * r);
        (old_s, s) = (s.clone(), old_s - q * s);
    }
    modulo(old_s, m)
}
impl Bitstrings {
    fn construct(unit: BigInt, base: BigInt) -> Result<Self, String> {
        if unit < BigInt::zero() || base < BigInt::zero() {
            return Err("bitstring unit/base must be nonnegative".into());
        }
        let base = if !unit.is_zero() && base >= &unit * 9 {
            base % &unit + &unit * 8
        } else {
            base
        };
        Ok(Self { unit, base })
    }
    fn join(&self, b: &Self) -> Self {
        let difference = if self.base >= b.base {
            &self.base - &b.base
        } else {
            &b.base - &self.base
        };
        Self::construct(
            gcd(gcd(self.unit.clone(), b.unit.clone()), difference),
            self.base.clone().min(b.base.clone()),
        )
        .unwrap()
    }
    fn meet(&self, b: &Self) -> Option<Self> {
        if self == b {
            return Some(self.clone());
        }
        if self.unit.is_zero() && b.unit.is_zero() {
            return None;
        }
        if b.unit.is_zero() {
            return if b.base >= self.base && (&b.base - &self.base) % &self.unit == BigInt::zero() {
                Some(b.clone())
            } else {
                None
            };
        }
        if self.unit.is_zero() {
            return b.meet(self);
        }
        let g = gcd(self.unit.clone(), b.unit.clone());
        let difference = &b.base - &self.base;
        if &difference % &g != BigInt::zero() {
            return None;
        }
        let m = &b.unit / &g;
        let steps = modulo(difference / &g * inverse(&(&self.unit / &g), &m), &m);
        let period = &self.unit * &b.unit / &g;
        let mut base = &self.base + &self.unit * steps;
        let minimum = self.base.clone().max(b.base.clone());
        if base < minimum {
            base += ((&minimum - &base + &period - 1) / &period) * &period;
        }
        Some(Self::construct(period, base).unwrap())
    }
    fn subtract(&self, b: &Self) -> Option<Self> {
        let Some(overlap) = self.meet(b) else {
            return Some(self.clone());
        };
        if self == &overlap {
            return None;
        }
        if overlap.unit.is_zero() && self.base == overlap.base {
            return Some(Self::construct(self.unit.clone(), &self.base + &self.unit).unwrap());
        }
        if self.unit == overlap.unit {
            return Some(if &self.base + &self.unit != overlap.base {
                Self::construct(BigInt::zero(), self.base.clone()).unwrap()
            } else {
                self.clone()
            });
        }
        if &self.unit * 2 == overlap.unit {
            if self.base == overlap.base {
                return Some(Self::construct(overlap.unit, &self.base + &self.unit).unwrap());
            }
            if &self.base + &self.unit == overlap.base {
                return Some(Self::construct(overlap.unit, self.base.clone()).unwrap());
            }
        }
        Some(self.clone())
    }
    fn value(&self) -> Value {
        json!({"kind":"bitstring","unit":self.unit.to_string(),"base":self.base.to_string()})
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Functions {
    domain: Box<Type>,
    range: Box<Type>,
}
impl Functions {
    fn shared(domain: Type, range: Type, x: &Arc<Self>, y: &Arc<Self>) -> Arc<Self> {
        let holds = |f: &Self| *f.domain == domain && *f.range == range;
        if holds(x) {
            x.clone()
        } else if holds(y) {
            y.clone()
        } else {
            Arc::new(Self {
                domain: Box::new(domain),
                range: Box::new(range),
            })
        }
    }
    fn join_shared(x: &Arc<Self>, y: &Arc<Self>) -> Result<Option<Arc<Self>>, LatticeError> {
        let domain = x.domain.join(&y.domain)?;
        Ok(Some(Self::shared(domain, x.range.join(&y.range)?, x, y)))
    }
    fn meet_shared(x: &Arc<Self>, y: &Arc<Self>) -> Result<Option<Arc<Self>>, LatticeError> {
        Ok({
            let domain = x.domain.meet(&y.domain)?;
            if domain == Type::none() {
                None
            } else {
                Some(Self::shared(domain, x.range.meet(&y.range)?, x, y))
            }
        })
    }
    fn subtract_shared(x: &Arc<Self>, y: &Arc<Self>) -> Result<Option<Arc<Self>>, LatticeError> {
        Ok({
            if Self::meet_shared(x, y)?.is_some_and(|m| m == *x) {
                None
            } else {
                Some(x.clone())
            }
        })
    }
    fn limit_shared(x: &Arc<Self>, k: i64) -> Result<Arc<Self>, LatticeError> {
        let domain = x.domain.limit_aux(k)?;
        Ok(Self::shared(domain, x.range.limit_aux(k - 1)?, x, x))
    }
    fn value(&self) -> Value {
        let domain = match &self.domain.repr {
            Repr::Any => Value::Null,
            Repr::Product(es) => json!(es.iter().map(Type::to_value).collect::<Vec<_>>()),
            _ => unreachable!("function domain is a product or any"),
        };
        json!({"kind":"function","domain":domain,"range":self.range.to_value()})
    }
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VariableId {
    Integer(VariableInteger),
    Atom(String),
}
impl VariableId {
    fn parse(value: &Value) -> Result<Self, String> {
        if let Some(atom) = value["atom"].as_str() {
            Ok(Self::Atom(atom.to_owned()))
        } else if !value["integer"].is_null() {
            Ok(Self::Integer(integer(&value["integer"])?.into()))
        } else {
            Err("var id must contain atom or integer".into())
        }
    }
    fn value(&self) -> Value {
        match self {
            Self::Atom(a) => json!({"atom":a}),
            Self::Integer(n) => json!({"integer":n.to_string()}),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Opacity {
    Opaque,
    Transparent,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NominalName {
    Symbol(String),
    Defined {
        module: String,
        name: String,
        arity: usize,
        opacity: Opacity,
    },
}
impl NominalName {
    fn value(&self) -> Value {
        match self {
            Self::Symbol(s) => json!({"symbol":s}),
            Self::Defined {
                module,
                name,
                arity,
                opacity,
            } => {
                json!({"module":module,"name":name,"arity":arity,"opacity":if *opacity==Opacity::Opaque{"opaque"}else{"transparent"}})
            }
        }
    }
    fn from_value(v: &Value) -> Result<Self, String> {
        if let Some(s) = v["symbol"].as_str() {
            return Ok(Self::Symbol(s.into()));
        }
        Ok(Self::Defined {
            module: v["module"].as_str().ok_or("nominal module missing")?.into(),
            name: v["name"].as_str().ok_or("nominal name missing")?.into(),
            arity: usize::try_from(v["arity"].as_u64().ok_or("nominal arity missing")?)
                .map_err(|_| "nominal arity exceeds usize")?,
            opacity: match v["opacity"].as_str() {
                Some("opaque") => Opacity::Opaque,
                Some("transparent") => Opacity::Transparent,
                _ => return Err("nominal opacity must be opaque or transparent".into()),
            },
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Nominal {
    name: NominalName,
    inner: Box<Type>,
}
impl Nominal {
    fn as_type(&self) -> Type {
        Type::new(Repr::Nominal(Arc::new(self.clone())))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct NominalSet {
    nominals: Vec<Nominal>,
    structural: Box<Type>,
}
impl NominalSet {
    fn raw(nominals: Vec<Nominal>, structural: Type) -> Type {
        Type::new(Repr::NominalSet(Arc::new(Self {
            nominals,
            structural: Box::new(structural),
        })))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    Join,
    Meet,
    Subtract,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct ActivePair {
    operation: Operation,
    a: Type,
    b: Type,
}
thread_local! {static ACTIVE_PAIRS:std::cell::RefCell<Vec<ActivePair>>=const{std::cell::RefCell::new(Vec::new())};}
struct ActivePairGuard;
impl ActivePairGuard {
    fn enter(operation: Operation, a: &Type, b: &Type) -> Result<Option<Self>, LatticeError> {
        if !matches!(a.repr, Repr::Nominal(_) | Repr::NominalSet(_))
            && !matches!(b.repr, Repr::Nominal(_) | Repr::NominalSet(_))
        {
            return Ok(None);
        }
        ACTIVE_PAIRS.with(|pairs| {
            let mut pairs = pairs.borrow_mut();
            if pairs.iter().any(|p| {
                p.operation == operation
                    && ((&p.a == a && &p.b == b)
                        || (operation != Operation::Subtract && &p.a == b && &p.b == a))
            }) {
                return Err(LatticeError::NativeNonTerminating);
            }
            pairs.push(ActivePair {
                operation,
                a: a.clone(),
                b: b.clone(),
            });
            Ok(Some(Self))
        })
    }
}
impl Drop for ActivePairGuard {
    fn drop(&mut self) {
        ACTIVE_PAIRS.with(|pairs| {
            pairs.borrow_mut().pop();
        });
    }
}

impl Type {
    pub fn nominal(name: NominalName, inner: Self) -> Self {
        if inner.is_impossible() {
            Self::none()
        } else {
            Self::new(Repr::Nominal(Arc::new(Nominal {
                name,
                inner: Box::new(inner),
            })))
        }
    }
    fn raw_nominal(name: NominalName, inner: Self) -> Self {
        Self::new(Repr::Nominal(Arc::new(Nominal {
            name,
            inner: Box::new(inner),
        })))
    }
    fn normalized_nominals(mut remaining: Vec<Self>, other: Self) -> Result<Self, LatticeError> {
        let mut done = Vec::new();
        loop {
            if other.is_any() {
                return Ok(Self::any());
            }
            let Some(t) = remaining.first().cloned() else {
                return Ok(match done.len() {
                    0 => other,
                    1 if other.is_none() => done.pop().unwrap(),
                    _ => {
                        let mut ns = Vec::new();
                        for t in done {
                            let Repr::Nominal(n) = &t.repr else {
                                return Err(LatticeError::FunctionClause);
                            };
                            ns.push((**n).clone())
                        }
                        self::NominalSet::raw(ns, other)
                    }
                });
            };
            remaining.remove(0);
            if t.is_none() {
                continue;
            }
            if other.is_none() || t.meet(&other)?.is_none() {
                done.push(t)
            } else {
                let structural = t.join(&other)?;
                done.extend(remaining);
                let set = Self::from_nominal_types(done, Self::none())?;
                return set.join(&structural);
            }
        }
    }
    fn from_nominal_types(types: Vec<Self>, other: Self) -> Result<Self, LatticeError> {
        let mut ns = Vec::new();
        for t in types {
            if let Repr::Nominal(n) = &t.repr {
                ns.push((**n).clone())
            } else if !t.is_none() {
                return Err(LatticeError::FunctionClause);
            }
        }
        Ok(self::NominalSet::raw(ns, other))
    }
    fn nominal_absorb(ns: &[Nominal], sup: &Nominal) -> Result<Self, LatticeError> {
        let mut acc = Vec::new();
        for (i, n) in ns.iter().enumerate() {
            match n.as_type().meet(&sup.as_type())?.repr {
                Repr::Nominal(_) => {
                    acc.extend_from_slice(&ns[i + 1..]);
                    return self::NominalSet::raw(acc, Self::none())
                        .join(&n.as_type().join(&sup.as_type())?);
                }
                Repr::None => acc.push(n.clone()),
                _ => return Err(LatticeError::FunctionClause),
            }
        }
        let mut inserted = false;
        for n in &mut acc {
            if n.name == sup.name {
                *n.inner = n.inner.join(&sup.inner)?;
                inserted = true;
                break;
            }
        }
        if !inserted {
            acc.push(sup.clone())
        }
        acc.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(self::NominalSet::raw(acc, Self::none()))
    }
    fn nominal_join(&self, rhs: &Self) -> Result<Self, LatticeError> {
        use Repr::*;
        Ok(match (&self.repr, &rhs.repr) {
            (Nominal(a), Nominal(b)) if a.name == b.name => {
                Self::raw_nominal(a.name.clone(), a.inner.join(&b.inner)?)
            }
            (Nominal(a), Nominal(b)) => match (&a.inner.repr, &b.inner.repr) {
                (Nominal(x), Nominal(y)) => {
                    let sup = a.inner.join(&b.inner)?;
                    match &sup.repr {
                        Nominal(n) if n.name == x.name && n.name == b.name => {
                            Self::raw_nominal(b.name.clone(), sup)
                        }
                        Nominal(n) if n.name == y.name && n.name == a.name => {
                            Self::raw_nominal(a.name.clone(), sup)
                        }
                        Nominal(_) => sup,
                        NominalSet(ns) if ns.structural.is_none() => Self::nominal_pair(a, b),
                        _ => return Err(LatticeError::FunctionClause),
                    }
                }
                (Nominal(x), NominalSet(_)) => Self::raw_nominal(
                    a.name.clone(),
                    self::NominalSet::raw(vec![x.as_ref().clone()], Self::none()),
                )
                .join(rhs)?,
                (NominalSet(_), Nominal(_)) => rhs.nominal_join(self)?,
                (Nominal(x), _) => {
                    let sup = a.inner.join(rhs)?;
                    match &sup.repr {
                        NominalSet(ns) if ns.structural.is_none() => Self::nominal_pair(a, b),
                        Nominal(n) if n.name == b.name => sup,
                        Nominal(n) if n.name == x.name => Self::raw_nominal(a.name.clone(), sup),
                        _ => return Err(LatticeError::FunctionClause),
                    }
                }
                (_, Nominal(_)) => rhs.nominal_join(self)?,
                (NominalSet(x), NominalSet(y)) => {
                    let mut sup = Self::raw_nominal(a.name.clone(), *x.structural.clone())
                        .join(&Self::raw_nominal(b.name.clone(), *y.structural.clone()))?;
                    for n in &x.nominals {
                        sup = Self::raw_nominal(a.name.clone(), n.as_type()).join(&sup)?
                    }
                    for n in &y.nominals {
                        sup = Self::raw_nominal(b.name.clone(), n.as_type()).join(&sup)?
                    }
                    sup
                }
                (NominalSet(x), _) => {
                    let expanded = x
                        .nominals
                        .iter()
                        .map(|n| self::Nominal {
                            name: a.name.clone(),
                            inner: Box::new(n.as_type()),
                        })
                        .collect::<Vec<_>>();
                    Self::nominal_absorb(&expanded, b)?
                        .join(&Self::raw_nominal(a.name.clone(), *x.structural.clone()))?
                }
                (_, NominalSet(_)) => rhs.nominal_join(self)?,
                _ => Self::nominal_pair(a, b),
            },
            (NominalSet(a), NominalSet(b)) => {
                let mut sup = a.structural.join(&b.structural)?;
                for n in &a.nominals {
                    sup = n.as_type().join(&sup)?
                }
                for n in &b.nominals {
                    sup = n.as_type().join(&sup)?
                }
                sup
            }
            (NominalSet(a), Nominal(b)) if a.structural.is_none() => {
                Self::nominal_absorb(&a.nominals, b)?
            }
            (NominalSet(a), Nominal(b)) => self::NominalSet::raw(a.nominals.clone(), Self::none())
                .join(&b.as_type())?
                .join(&a.structural)?,
            (Nominal(_), NominalSet(_)) => rhs.nominal_join(self)?,
            (Nominal(a), _) => {
                let inf = a.inner.meet(rhs)?;
                if inf.is_impossible() {
                    self::NominalSet::raw(vec![a.as_ref().clone()], rhs.clone())
                } else {
                    a.inner.join(rhs)?
                }
            }
            (_, Nominal(_)) => rhs.nominal_join(self)?,
            (NominalSet(a), _) => Self::normalized_nominals(
                a.nominals.iter().map(self::Nominal::as_type).collect(),
                a.structural.join(rhs)?,
            )?,
            (_, NominalSet(_)) => rhs.nominal_join(self)?,
            _ => return Err(LatticeError::FunctionClause),
        })
    }
    fn nominal_pair(a: &Nominal, b: &Nominal) -> Self {
        if a.name < b.name {
            self::NominalSet::raw(vec![a.clone(), b.clone()], Self::none())
        } else {
            self::NominalSet::raw(vec![b.clone(), a.clone()], Self::none())
        }
    }
    fn nominal_cartesian(a: &[Self], b: &[Self]) -> Result<Self, LatticeError> {
        let mut rows = Vec::new();
        for x in a {
            let mut cells = Vec::new();
            for y in b {
                let inf = x.meet(y)?;
                if !inf.is_none() {
                    cells.push(inf);
                }
            }
            let mut row = Self::none();
            for cell in cells.into_iter().rev() {
                row = cell.join(&row)?;
            }
            if !row.is_none() {
                rows.push(row);
            }
        }
        let mut all = Self::none();
        for row in rows.into_iter().rev() {
            all = row.join(&all)?;
        }
        Ok(all)
    }
    fn nominal_meet(&self, rhs: &Self) -> Result<Self, LatticeError> {
        use Repr::*;
        Ok(match (&self.repr, &rhs.repr) {
            (Nominal(a), Nominal(b)) if a.name == b.name => {
                Self::nominal(a.name.clone(), a.inner.meet(&b.inner)?)
            }
            (Nominal(a), Nominal(b)) => match (&a.inner.repr, &b.inner.repr) {
                (Nominal(x), Nominal(y)) => {
                    let inf = a.inner.meet(&b.inner)?;
                    match &inf.repr {
                        Nominal(n) if n.name == x.name && n.name == b.name => {
                            Self::raw_nominal(a.name.clone(), inf)
                        }
                        Nominal(n) if n.name == y.name && n.name == a.name => {
                            Self::raw_nominal(b.name.clone(), inf)
                        }
                        _ => Self::none(),
                    }
                }
                (NominalSet(x), NominalSet(y)) => {
                    let expand = |name: &NominalName, s: &self::NominalSet| {
                        let mut ts = vec![Self::raw_nominal(name.clone(), *s.structural.clone())];
                        ts.extend(
                            s.nominals
                                .iter()
                                .map(|n| Self::raw_nominal(name.clone(), n.as_type())),
                        );
                        ts
                    };
                    let inf = Self::nominal_cartesian(&expand(&a.name, x), &expand(&b.name, y))?;
                    match &inf.repr {
                        Nominal(n) if n.name == a.name || n.name == b.name => inf,
                        Repr::None => inf,
                        _ => return Err(LatticeError::CaseClause(Box::new(inf))),
                    }
                }
                (Nominal(x), NominalSet(_)) => Self::raw_nominal(
                    a.name.clone(),
                    self::NominalSet::raw(vec![x.as_ref().clone()], Self::none()),
                )
                .meet(rhs)?,
                (NominalSet(_), Nominal(y)) => self.meet(&Self::raw_nominal(
                    b.name.clone(),
                    self::NominalSet::raw(vec![y.as_ref().clone()], Self::none()),
                ))?,
                (NominalSet(_), _) | (Nominal(_), _) => {
                    self.meet(&Self::raw_nominal(a.name.clone(), rhs.clone()))?
                }
                (_, NominalSet(_)) | (_, Nominal(_)) => rhs.nominal_meet(self)?,
                _ => Self::none(),
            },
            (NominalSet(a), NominalSet(b)) => {
                Self::nominal_cartesian(&Self::nominal_components(a), &Self::nominal_components(b))?
            }
            (NominalSet(a), _) => {
                Self::nominal_cartesian(&Self::nominal_components(a), std::slice::from_ref(rhs))?
            }
            (_, NominalSet(_)) => rhs.nominal_meet(self)?,
            (Nominal(a), _) => Self::nominal(a.name.clone(), a.inner.meet(rhs)?),
            (_, Nominal(_)) => rhs.nominal_meet(self)?,
            _ => return Err(LatticeError::FunctionClause),
        })
    }
    fn nominal_components(s: &NominalSet) -> Vec<Self> {
        let mut ts = vec![*s.structural.clone()];
        ts.extend(s.nominals.iter().map(self::Nominal::as_type));
        ts
    }
    fn nominal_subtract(&self, rhs: &Self) -> Result<Self, LatticeError> {
        use Repr::*;
        Ok(match (&self.repr, &rhs.repr) {
            (NominalSet(a), NominalSet(b)) => {
                let mut rows = Vec::new();
                for x in Self::nominal_components(a) {
                    let mut row = x;
                    for y in Self::nominal_components(b) {
                        row = row.subtract(&y)?;
                    }
                    if !row.is_none() {
                        rows.push(row);
                    }
                }
                let mut all = Self::none();
                for row in rows.into_iter().rev() {
                    all = row.join(&all)?;
                }
                all
            }
            (NominalSet(_), Nominal(b)) => self.subtract(&self::NominalSet::raw(
                vec![b.as_ref().clone()],
                Self::none(),
            ))?,
            (NominalSet(a), _) => {
                let overlap = self.meet(rhs)?;
                match overlap.repr {
                    NominalSet(_) | Nominal(_) => self.subtract(&overlap)?,
                    _ => Self::normalized_nominals(
                        a.nominals.iter().map(self::Nominal::as_type).collect(),
                        a.structural.subtract(&overlap)?,
                    )?,
                }
            }
            (_, NominalSet(b)) => self.subtract(&b.structural)?,
            (Nominal(a), Nominal(b)) if a.name == b.name => {
                Self::nominal(a.name.clone(), a.inner.subtract(&b.inner)?)
            }
            (Nominal(a), _) => {
                let overlap = self.meet(rhs)?;
                match &overlap.repr {
                    Nominal(n) if n.name == a.name => self.subtract(&overlap)?,
                    _ => self.clone(),
                }
            }
            (_, Nominal(_)) => self.clone(),
            _ => return Err(LatticeError::FunctionClause),
        })
    }
    pub fn shallow_structural(&self) -> Result<Self, LatticeError> {
        match &self.repr {
            Repr::Nominal(n) => n.inner.shallow_structural(),
            Repr::NominalSet(ns) => {
                let mut s = *ns.structural.clone();
                for n in &ns.nominals {
                    s = s.join(&n.inner)?
                }
                s.shallow_structural()
            }
            _ => Ok(self.clone()),
        }
    }
    pub fn is_opaque(&self, module: &str) -> bool {
        match &self.repr {
            Repr::Nominal(n) => match &n.name {
                NominalName::Defined {
                    module: m,
                    opacity: Opacity::Opaque,
                    ..
                } => m != module,
                _ => false,
            },
            Repr::NominalSet(ns) if ns.structural.is_none() => {
                ns.nominals.iter().any(|n| n.as_type().is_opaque(module))
            }
            _ => false,
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SingletonKey<'a> {
    Atom(&'a str),
    Nil,
    Integer(&'a BigInt),
}
impl Type {
    fn singleton_key(&self) -> Option<SingletonKey<'_>> {
        let s = self.direct_slots()?;
        if s.count() != 1 {
            return None;
        }
        if let Some(Atoms::Set(xs)) = s.atoms() {
            if xs.len() == 1 {
                return Some(SingletonKey::Atom(xs.first().unwrap()));
            }
        }
        if matches!(s.list(), Some(Lists::Nil)) {
            return Some(SingletonKey::Nil);
        }
        if let Some(Numeric::Integer(Integers::Set(xs))) = s.number() {
            if xs.len() == 1 {
                return Some(SingletonKey::Integer(xs.first().unwrap()));
            }
        }
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MapKey {
    Atom(String),
    Nil,
    Integer(BigInt),
}
impl MapKey {
    pub fn as_type(&self) -> Type {
        match self {
            Self::Atom(a) => Type::atom(a),
            Self::Nil => Type::nil(),
            Self::Integer(n) => Type::integer(n.clone()),
        }
    }
    pub fn from_type(t: &Type) -> Result<Self, String> {
        if let Some(key) = t.singleton_key() {
            return Ok(match key {
                SingletonKey::Atom(a) => Self::Atom(a.to_owned()),
                SingletonKey::Nil => Self::Nil,
                SingletonKey::Integer(n) => Self::Integer(n.clone()),
            });
        }
        match t.view() {
            TypeView::Families(_) if t == &Type::nil() => Ok(Self::Nil),
            TypeView::Families(s) if s.atoms.is_some() => match s.atoms {
                Some(AtomsView::Set(a)) if a.len() == 1 && t == &Type::atoms(a.iter().cloned()) => {
                    Ok(Self::Atom(a.first().unwrap().to_string()))
                }
                _ => Err(
                    "OTP badarg: map exact key must be native singleton atom, integer, or nil"
                        .into(),
                ),
            },
            TypeView::Families(s) if s.number.is_some() => match s.number {
                Some(NumberView::Integer(IntegerView::Set(n)))
                    if n.len() == 1 && t == &Type::integers(n.iter().cloned()) =>
                {
                    Ok(Self::Integer(n.first().unwrap().clone()))
                }
                _ => Err(
                    "OTP badarg: map exact key must be native singleton atom, integer, or nil"
                        .into(),
                ),
            },
            _ => Err(
                "OTP badarg: map exact key must be native singleton atom, integer, or nil".into(),
            ),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Presence {
    Mandatory,
    Optional,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MapEntry {
    pub key: MapKey,
    pub presence: Presence,
    pub value: Type,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapTypeEntry {
    pub key: Type,
    pub presence: Presence,
    pub value: Type,
}
type AlignedMapPair = (MapKey, Presence, Type, Presence, Type);
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Maps {
    entries: Vec<MapEntry>,
    default_key: Box<Type>,
    default_value: Box<Type>,
}
impl Maps {
    fn canonical(entries: Vec<MapEntry>, dk: Type, dv: Type) -> Result<Option<Self>, LatticeError> {
        Self::canonical_types(
            entries
                .into_iter()
                .map(|e| MapTypeEntry {
                    key: e.key.as_type(),
                    presence: e.presence,
                    value: e.value,
                })
                .collect(),
            dk,
            dv,
        )
    }
    fn insert_types(entries: &mut Vec<MapTypeEntry>, e: MapTypeEntry) -> Result<(), LatticeError> {
        let mut index = 0;
        while index < entries.len() {
            match e.key.native_record_order(&entries[index].key) {
                std::cmp::Ordering::Equal => return Err(LatticeError::BadArg),
                std::cmp::Ordering::Greater => index += 1,
                std::cmp::Ordering::Less => break,
            }
        }
        entries.insert(index, e);
        Ok(())
    }
    fn needs_merge_type(k: &Type, dk: &Type) -> Result<bool, LatticeError> {
        let s = k.direct_slots();
        if let Some(Numeric::Integer(Integers::Set(v))) = s.and_then(|s| s.number()) {
            if v.len() != 1 {
                return Err(LatticeError::BadMatchNative(Box::new(NativeValue::List(
                    v.iter().cloned().map(NativeValue::Integer).collect(),
                ))));
            }
            let n = v.first().unwrap();
            return Ok(!Type::integer(n + 1).meet(dk)?.is_none()
                || !Type::integer(n - 1).meet(dk)?.is_none());
        }
        if s.is_some_and(|s| s.atoms().is_some()) {
            return Ok(!Type::all_atoms().meet(dk)?.is_none());
        }
        if k == &Type::nil() {
            return Type::proper_list(Type::any(), true)
                .meet(dk)?
                .predicate(TypePredicate::Cons);
        }
        Ok(false)
    }
    fn canonical_types(
        mut entries: Vec<MapTypeEntry>,
        mut dk: Type,
        mut dv: Type,
    ) -> Result<Option<Self>, LatticeError> {
        for e in &entries {
            dk = dk.subtract(&e.key)?;
        }
        if dk.is_impossible() || dv.is_impossible() {
            dk = Type::none();
            dv = Type::none();
        }
        if let Ok(key) = MapKey::from_type(&dk) {
            Self::insert_types(
                &mut entries,
                MapTypeEntry {
                    key: key.as_type(),
                    presence: Presence::Optional,
                    value: dv,
                },
            )?;
            dk = Type::none();
            dv = Type::none();
        }
        loop {
            let mut changed = false;
            let mut keep = Vec::new();
            for e in entries {
                let key = e.key.clone();
                if e.presence == Presence::Optional && e.value.is_none() {
                    let diff = dk.subtract(&key)?;
                    if key.is_subtype(&dk)? && dk == diff {
                        keep.push(e)
                    } else {
                        dk = diff
                    }
                } else if e.presence == Presence::Optional && e.value == dv {
                    if key.is_subtype(&dk)? {
                    } else if Self::needs_merge_type(&e.key, &dk)? {
                        dk = key.join(&dk)?;
                        changed = true;
                    } else {
                        keep.push(e)
                    }
                } else {
                    keep.push(e)
                }
            }
            entries = keep;
            if !changed {
                break;
            }
        }
        if entries
            .windows(2)
            .any(|es| es[0].key.native_record_order(&es[1].key) != std::cmp::Ordering::Less)
        {
            return Err(LatticeError::BadArg);
        }
        let entries = entries
            .into_iter()
            .map(|e| {
                Ok(MapEntry {
                    key: MapKey::from_type(&e.key).map_err(|_| LatticeError::BadArg)?,
                    presence: e.presence,
                    value: e.value,
                })
            })
            .collect::<Result<Vec<_>, LatticeError>>()?;
        if entries
            .iter()
            .any(|e| e.presence == Presence::Mandatory && e.value.is_none())
        {
            return Ok(None);
        }
        Ok(Some(Self {
            entries,
            default_key: Box::new(dk),
            default_value: Box::new(dv),
        }))
    }
    fn store(entries: &mut Vec<MapEntry>, e: MapEntry) {
        match entries.binary_search_by(|x| x.key.cmp(&e.key)) {
            Ok(i) => entries[i] = e,
            Err(i) => entries.insert(i, e),
        }
    }
    fn other(&self, k: &MapKey) -> Result<(Presence, Type), LatticeError> {
        let v = if k.as_type().meet(&self.default_key)?.is_none() {
            Type::none()
        } else {
            *self.default_value.clone()
        };
        Ok((Presence::Optional, v))
    }
    fn aligned(&self, b: &Self) -> Result<Vec<AlignedMapPair>, LatticeError> {
        let keys: BTreeSet<_> = self
            .entries
            .iter()
            .chain(&b.entries)
            .map(|e| e.key.clone())
            .collect();
        keys.into_iter()
            .map(|k| {
                let entry = |m: &Self| match m.entries.binary_search_by(|e| e.key.cmp(&k)) {
                    Ok(i) => Ok((m.entries[i].presence, m.entries[i].value.clone())),
                    Err(_) => m.other(&k),
                };
                let (p, x) = entry(self)?;
                let (q, y) = entry(b)?;
                Ok((k, p, x, q, y))
            })
            .collect()
    }
    fn join_shared(x: &Arc<Self>, y: &Arc<Self>) -> Result<Option<Arc<Self>>, LatticeError> {
        Ok(x.join(y)?.map(|m| reuse(m, x, y)))
    }
    fn meet_shared(x: &Arc<Self>, y: &Arc<Self>) -> Result<Option<Arc<Self>>, LatticeError> {
        Ok(x.meet(y)?.map(|m| reuse(m, x, y)))
    }
    fn subtract_shared(x: &Arc<Self>, y: &Arc<Self>) -> Result<Option<Arc<Self>>, LatticeError> {
        if !x.default_key.is_subtype(&y.default_key)?
            || !x.default_value.is_subtype(&y.default_value)?
        {
            return Ok(Some(x.clone()));
        }
        let mut constraints = Vec::new();
        for (key, p, a, q, b) in x.aligned(y)? {
            let d = a.subtract(&b)?;
            if p == Presence::Optional && q == Presence::Mandatory {
                constraints.push(MapEntry {
                    key,
                    presence: Presence::Optional,
                    value: d,
                })
            } else if !d.is_none() {
                constraints.push(MapEntry {
                    key,
                    presence: Presence::Mandatory,
                    value: d,
                })
            }
        }
        match constraints.len() {
            0 => Ok(None),
            1 => {
                let mut entries = x.entries.clone();
                Self::store(&mut entries, constraints.pop().unwrap());
                Ok(Self::canonical(
                    entries,
                    *x.default_key.clone(),
                    *x.default_value.clone(),
                )?
                .map(|m| reuse_left(m, x)))
            }
            _ => Ok(Some(x.clone())),
        }
    }
    fn limit_shared(x: &Arc<Self>, k: i64) -> Result<Option<Arc<Self>>, LatticeError> {
        Ok(x.limit_aux(k)?.map(|m| reuse_left(m, x)))
    }
    fn join(&self, b: &Self) -> Result<Option<Self>, LatticeError> {
        let entries = self
            .aligned(b)?
            .into_iter()
            .map(|(key, p, x, q, y)| {
                Ok(MapEntry {
                    key,
                    presence: if p == q { p } else { Presence::Optional },
                    value: x.join(&y)?,
                })
            })
            .collect::<Result<Vec<_>, LatticeError>>()?;
        Self::canonical(
            entries,
            self.default_key.join(&b.default_key)?,
            self.default_value.join(&b.default_value)?,
        )
    }
    fn meet(&self, b: &Self) -> Result<Option<Self>, LatticeError> {
        let entries = self
            .aligned(b)?
            .into_iter()
            .map(|(key, p, x, q, y)| {
                Ok(MapEntry {
                    key,
                    presence: if p == Presence::Optional && q == Presence::Optional {
                        Presence::Optional
                    } else {
                        Presence::Mandatory
                    },
                    value: x.meet(&y)?,
                })
            })
            .collect::<Result<Vec<_>, LatticeError>>()?;
        Self::canonical(
            entries,
            self.default_key.meet(&b.default_key)?,
            self.default_value.meet(&b.default_value)?,
        )
    }
    fn limit_aux(&self, k: i64) -> Result<Option<Self>, LatticeError> {
        let mut dk = *self.default_key.clone();
        let mut dv = *self.default_value.clone();
        let mut entries = Vec::new();
        for e in &self.entries {
            let key = e.key.as_type().limit_aux(k - 1)?;
            let value = e.value.limit_aux(k - 1)?;
            if key == e.key.as_type() {
                entries.push(MapEntry {
                    key: e.key.clone(),
                    presence: e.presence,
                    value,
                })
            } else {
                dk = dk.join(&key)?;
                dv = dv.join(&value)?
            }
        }
        Self::canonical(entries, dk.limit_aux(k - 1)?, dv.limit_aux(k - 1)?)
    }
    fn value(&self) -> Value {
        json!({"kind":"map","entries":self.entries.iter().map(|e|json!({"key":e.key.as_type().to_value(),"presence":if e.presence==Presence::Mandatory{"mandatory"}else{"optional"},"value":e.value.to_value()})).collect::<Vec<_>>(),"default_key":self.default_key.to_value(),"default_value":self.default_value.to_value()})
    }
    fn get(&self, key: &Type) -> Result<Type, LatticeError> {
        if let Ok(k) = MapKey::from_type(key)
            && let Ok(i) = self.entries.binary_search_by(|e| e.key.cmp(&k))
        {
            return Ok(self.entries[i].value.clone());
        }
        let mut result = if self.default_key.meet(key)?.is_impossible() {
            Type::none()
        } else {
            *self.default_value.clone()
        };
        if MapKey::from_type(key).is_err() {
            for e in &self.entries {
                if !e.key.as_type().meet(key)?.is_impossible() {
                    result = result.join(&e.value)?
                }
            }
        }
        Ok(result)
    }
    fn is_key(&self, key: &Type) -> Result<Type, LatticeError> {
        if let Ok(k) = MapKey::from_type(key)
            && let Ok(i) = self.entries.binary_search_by(|e| e.key.cmp(&k))
        {
            let e = &self.entries[i];
            return Ok(match (e.presence, e.value.is_none()) {
                (Presence::Mandatory, _) => Type::atom("true"),
                (Presence::Optional, true) => Type::atom("false"),
                _ => Type::atoms(["false", "true"]),
            });
        }
        let mut overlap = !self.default_key.meet(key)?.is_impossible();
        if MapKey::from_type(key).is_err() {
            for e in &self.entries {
                overlap |= !e.value.is_none() && !e.key.as_type().meet(key)?.is_impossible()
            }
        }
        Ok(if overlap {
            Type::atoms(["false", "true"])
        } else {
            Type::atom("false")
        })
    }
    fn put(&self, key: Type, value: Type) -> Result<Option<Self>, LatticeError> {
        if key.is_impossible() || value.is_impossible() {
            return Ok(None);
        }
        let mut entries = self.entries.clone();
        let (mut dk, mut dv) = (*self.default_key.clone(), *self.default_value.clone());
        if let Ok(key) = MapKey::from_type(&key) {
            Self::store(
                &mut entries,
                MapEntry {
                    key,
                    presence: Presence::Mandatory,
                    value,
                },
            );
        } else {
            for e in &mut entries {
                if !e.key.as_type().meet(&key)?.is_none() {
                    e.value = e.value.join(&value)?
                }
            }
            dk = dk.join(&key)?;
            dv = dv.join(&value)?
        }
        Self::canonical(entries, dk, dv)
    }
    fn remove(&self, key: &Type) -> Result<Option<Self>, LatticeError> {
        let Ok(key) = MapKey::from_type(key) else {
            return Ok(Some(self.clone()));
        };
        let Ok(i) = self.entries.binary_search_by(|e| e.key.cmp(&key)) else {
            return Ok(Some(self.clone()));
        };
        let mut es = self.entries.clone();
        es.remove(i);
        Self::canonical(es, *self.default_key.clone(), *self.default_value.clone())
    }
}
impl Type {
    fn map_ref(&self) -> Result<Option<&Maps>, LatticeError> {
        if self.is_impossible() {
            return Ok(None);
        }
        if let Repr::Families(s) = &self.repr {
            if !s.union_record() && s.count() == usize::from(s.maps().is_some()) {
                return Ok(s.maps());
            }
        }
        Err(LatticeError::FunctionClause)
    }
    fn from_map(maps: Option<Maps>) -> Self {
        Self::families(Slots {
            maps: maps.map(Arc::new),
            ..Slots::default()
        })
    }
    pub fn map_pairwise_merge<F>(
        &self,
        rhs: &Self,
        mut callback: F,
    ) -> Result<Vec<MapEntry>, LatticeError>
    where
        F: FnMut(
            &MapKey,
            Presence,
            &Type,
            Presence,
            &Type,
        ) -> Result<Option<(Presence, Type)>, LatticeError>,
    {
        let left = self.shallow_structural()?;
        let right = rhs.shallow_structural()?;
        let a = left.map_ref()?.ok_or(LatticeError::FunctionClause)?;
        let b = right.map_ref()?.ok_or(LatticeError::FunctionClause)?;
        let mut out = Vec::new();
        for (key, p, x, q, y) in a.aligned(b)? {
            if let Some((presence, value)) = callback(&key, p, &x, q, &y)? {
                out.push(MapEntry {
                    key,
                    presence,
                    value,
                })
            }
        }
        Ok(out)
    }
    pub fn map_get(&self, key: &Self) -> Result<Self, LatticeError> {
        let map = self.shallow_structural()?;
        match map.map_ref()? {
            Some(m) => m.get(&key.shallow_structural()?),
            None => Ok(Self::none()),
        }
    }
    pub fn map_is_key(&self, key: &Self) -> Result<Self, LatticeError> {
        let map = self.shallow_structural()?;
        match map.map_ref()? {
            Some(m) => m.is_key(&key.shallow_structural()?),
            None => Ok(Self::none()),
        }
    }
    pub fn map_put(&self, key: Self, value: Self) -> Result<Self, LatticeError> {
        let map = self.shallow_structural()?;
        Ok(match map.map_ref()? {
            Some(m) => Self::from_map(m.put(key, value)?),
            None => Self::none(),
        })
    }
    pub fn map_remove(&self, key: &Self) -> Result<Self, LatticeError> {
        let map = self.shallow_structural()?;
        Ok(match map.map_ref()? {
            Some(m) => Self::from_map(m.remove(key)?),
            None => Self::none(),
        })
    }
    pub fn map_update(&self, key: Self, value: Self) -> Result<Self, LatticeError> {
        if self.is_impossible() || !Self::atom("true").is_subtype(&self.map_is_key(&key)?)? {
            return Ok(Self::none());
        }
        self.map_put(key, value)
    }
}

impl Type {
    pub fn map_types(
        entries: Vec<MapTypeEntry>,
        default_key: Self,
        default_value: Self,
    ) -> Result<Self, LatticeError> {
        Ok(Self::families(Slots {
            maps: Maps::canonical_types(entries, default_key, default_value)?.map(Arc::new),
            ..Slots::default()
        }))
    }
    pub fn map(
        entries: Vec<MapEntry>,
        default_key: Self,
        default_value: Self,
    ) -> Result<Self, LatticeError> {
        let maps = Maps::canonical(entries, default_key, default_value)?;
        Ok(Self::families(Slots {
            maps: maps.map(Arc::new),
            ..Slots::default()
        }))
    }
    pub fn map_any() -> Self {
        Self::canonical(CanonicalType::Map)
    }
    pub fn any() -> Self {
        Self::new(Repr::Any)
    }
    pub fn none() -> Self {
        Self::new(Repr::None)
    }
    pub fn unit() -> Self {
        Self::new(Repr::Unit)
    }
    fn families(mut slots: Slots) -> Self {
        slots.union_record = slots.count() > 1;
        if slots.empty() {
            Self::none()
        } else {
            Self::new(Repr::Families(FamilyStore::raw(slots)))
        }
    }
    fn scalar(atoms: Option<Atoms>, number: Option<Numeric>) -> Self {
        Self::families(Slots {
            atoms: atoms.map(Arc::new),
            number: number.map(Arc::new),
            ..Slots::default()
        })
    }
    pub fn all_atoms() -> Self {
        Self::canonical(CanonicalType::Atoms)
    }
    pub fn all_integers() -> Self {
        Self::canonical(CanonicalType::Integers)
    }
    pub fn float() -> Self {
        Self::canonical(CanonicalType::Float)
    }
    pub fn number() -> Self {
        Self::canonical(CanonicalType::Number)
    }
    pub fn atoms<A: Into<crate::atom::Atom>>(values: impl IntoIterator<Item = A>) -> Self {
        Self::scalar(
            match values.into_iter().map(Into::into).collect::<BTreeSet<_>>() {
                v if v.is_empty() => None,
                v if v.len() > 13 => Some(Atoms::All),
                v => Some(Atoms::Set(v)),
            },
            None,
        )
    }
    pub fn integers(values: impl IntoIterator<Item = BigInt>) -> Self {
        Self::scalar(
            None,
            int_set(values.into_iter().collect()).map(Numeric::Integer),
        )
    }
    pub fn range(from: Endpoint, to: Endpoint) -> Result<Self, LatticeError> {
        Ok(Self::scalar(
            None,
            checked_range(from.into(), to.into())?.map(Numeric::Integer),
        ))
    }
    pub fn range_strings(from: &str, to: &str) -> Result<Self, String> {
        Ok(Self::scalar(
            None,
            checked_range(Bound::parse(&json!(from))?, Bound::parse(&json!(to))?)?
                .map(Numeric::Integer),
        ))
    }
    pub fn atom(name: impl Into<crate::atom::Atom>) -> Self {
        let name: crate::atom::Atom = name.into();
        match name.as_str() {
            "true" => return Self::canonical(CanonicalType::True),
            "false" => return Self::canonical(CanonicalType::False),
            "infinity" => return Self::canonical(CanonicalType::Infinity),
            _ => {}
        }
        Self::scalar(Some(Atoms::Set(BTreeSet::from([name]))), None)
    }
    pub fn integer(value: BigInt) -> Self {
        match num_traits::ToPrimitive::to_i64(&value) {
            Some(0) => return Self::canonical(CanonicalType::Zero),
            Some(1) => return Self::canonical(CanonicalType::One),
            Some(-1) => return Self::canonical(CanonicalType::MinusOne),
            _ => {}
        }
        Self::scalar(
            None,
            Some(Numeric::Integer(Integers::Set(BTreeSet::from([value])))),
        )
    }

    pub fn nil() -> Self {
        Self::canonical(CanonicalType::Nil)
    }
    pub fn list(elements: Self, tail: Self, nonempty: bool) -> Self {
        if elements.is_any() {
            if tail.is_any() { return Self::canonical(if nonempty { CanonicalType::Cons } else { CanonicalType::MaybeImproperList }); }
            if matches!(&tail.repr, Repr::Families(s) if !s.union_record() && s.count() == 1 && matches!(s.list(), Some(Lists::Nil))) {
                return Self::canonical(if nonempty { CanonicalType::NonemptyList } else { CanonicalType::List });
            }
        }
        if elements.impossible() || tail.impossible() {
            Self::none()
        } else {
            Self::families(Slots {
                list: Some(Arc::new(Lists::List {
                    elements: Box::new(elements),
                    tail: Box::new(tail),
                    nonempty,
                })),
                ..Slots::default()
            })
        }
    }
    pub fn proper_list(elements: Self, nonempty: bool) -> Self {
        if elements.impossible() {
            if nonempty { Self::none() } else { Self::nil() }
        } else {
            Self::list(elements, Self::nil(), nonempty)
        }
    }
    pub fn cons(head: Self, tail: Self) -> Result<Self, LatticeError> {
        Ok({
            if head.impossible() || tail.impossible() {
                return Ok(Self::none());
            }
            if let Repr::Families(slots) = &tail.repr {
                if !slots.union_record()
                    && slots.count() == usize::from(slots.list().is_some())
                    && let Some(Lists::List {
                        elements,
                        tail: termination,
                        ..
                    }) = slots.list()
                {
                    return Ok(Self::list(head.join(elements)?, *termination.clone(), true));
                }
            }
            let list_part = tail
                .meet(&Self::list(Self::any(), Self::any(), false))?
                .shallow_structural()?;
            let (elements, termination) = match list_part.list_domain() {
                Some(Lists::List {
                    elements,
                    tail: termination,
                    ..
                }) => (
                    head.join(elements)?,
                    tail.subtract(&Self::list(Self::any(), Self::any(), false))?
                        .join(termination)?,
                ),
                _ => (head, tail),
            };
            Self::list(elements, termination, true)
        })
    }
    fn list_domain(&self) -> Option<&Lists> {
        match &self.repr {
            Repr::Families(s) => s.list(),
            _ => None,
        }
    }
    /// The tuple family slot of a family value, moved out without copying.
    fn into_tuple_slot(self) -> Option<Arc<Tuples>> {
        match &self.repr {
            Repr::Families(s) => s.tuple_owner().cloned(),
            _ => None,
        }
    }
    fn impossible(&self) -> bool {
        matches!(self.repr, Repr::None | Repr::Unit)
    }
    pub fn tuple_any() -> Self {
        Self::canonical(CanonicalType::Tuple)
    }
    pub fn tuple_arity(arity: usize) -> Self {
        if arity > 1024 {
            Self::tuple_any()
        } else {
            Self::from_tuples(Some(Tuples::Set(BTreeMap::from([(
                arity,
                vec![TupleVariant {
                    elements: vec![Self::any(); arity],
                    tag: None,
                }],
            )]))))
        }
    }
    pub fn tuple(elements: Vec<Self>) -> Result<Self, LatticeError> {
        Ok(Self::from_tuples(Tuples::construct(elements)?))
    }
    fn from_tuples(tuple: Option<Tuples>) -> Self {
        Self::families(Slots {
            tuple: tuple.map(Arc::new),
            ..Slots::default()
        })
    }
    pub fn identifier(kinds: Option<Vec<String>>) -> Result<Self, String> {
        let canonical = match &kinds {
            None => Some(CanonicalType::Identifier),
            Some(xs) if xs.len() == 1 => match xs[0].as_str() {
                "pid" => Some(CanonicalType::Pid), "port" => Some(CanonicalType::Port), "reference" => Some(CanonicalType::Reference), _ => None,
            },
            _ => None,
        };
        if let Some(kind) = canonical { return Ok(Self::canonical(kind)); }
        let identifiers = match kinds {
            None => Some(Identifiers::All),
            Some(values) => {
                let mut kinds = BTreeSet::new();
                for value in values {
                    if !["pid", "port", "reference"].contains(&value.as_str()) {
                        return Err(format!("unsupported identifier kind {value}"));
                    }
                    kinds.insert(value);
                }
                if kinds.is_empty() {
                    None
                } else {
                    Some(Identifiers::Set(kinds))
                }
            }
        };
        Ok(Self::families(Slots {
            identifiers: identifiers.map(Arc::new),
            ..Slots::default()
        }))
    }
    pub fn bitstring(unit: BigInt, base: BigInt) -> Result<Self, BitstringDomainError> {
        let canonical = match (num_traits::ToPrimitive::to_u64(&unit), num_traits::ToPrimitive::to_u64(&base)) {
            (Some(1), Some(0)) => Some(CanonicalType::Bitstring),
            (Some(8), Some(0)) => Some(CanonicalType::Binary),
            (Some(1), Some(1)) => Some(CanonicalType::NonemptyBitstring),
            (Some(8), Some(8)) => Some(CanonicalType::NonemptyBinary),
            _ => None,
        };
        if let Some(kind) = canonical { return Ok(Self::canonical(kind)); }
        if unit < BigInt::zero() || base < BigInt::zero() {
            return Err(BitstringDomainError { unit, base });
        }
        Ok(Self::families(Slots {
            bitstrings: Some(Arc::new(
                Bitstrings::construct(unit, base).expect("validated nonnegative parameters"),
            )),
            ..Slots::default()
        }))
    }
    /// Exact integer-input t_bitstr/2 boundary, including out-of-spec records.
    pub fn native_bitstring(unit: BigInt, base: BigInt) -> NativeTypeOutcome {
        if unit >= BigInt::zero() && base >= BigInt::zero() {
            return NativeTypeOutcome::Canonical(
                Self::bitstring(unit, base).expect("nonnegative parameters"),
            );
        }
        let base = if !unit.is_zero() && base >= &unit * 9 {
            base % &unit + &unit * 8
        } else {
            base
        };
        NativeTypeOutcome::Raw(RawNativeType(NativeValue::Tuple(vec![
            NativeValue::Atom("c".into()),
            NativeValue::Atom("binary".into()),
            NativeValue::Tuple(vec![NativeValue::Integer(unit), NativeValue::Integer(base)]),
            NativeValue::Atom("unknown".into()),
        ])))
    }
    pub fn binary() -> Self {
        Self::bitstring(8.into(), 0.into()).unwrap()
    }
    pub fn function(domain: Option<Vec<Self>>, range: Self) -> Self {
        if domain.is_none() && range.is_any() { return Self::canonical(CanonicalType::Function); }
        let domain = domain
            .map(|es| Self::raw_product(es))
            .unwrap_or_else(Self::any);
        Self::families(Slots {
            functions: Some(Arc::new(Functions {
                domain: Box::new(domain),
                range: Box::new(range),
            })),
            ..Slots::default()
        })
    }
    fn raw_product(elements: Vec<Self>) -> Self {
        Self::new(Repr::Product(Arc::new(elements)))
    }
    /// `raw_product(elements)`, reusing `a` or `b` when it is a product of
    /// exactly these elements.
    fn product_sharing(elements: Vec<Self>, a: &Self, b: &Self) -> Self {
        let holds = |t: &Self| {
            matches!(&t.repr, Repr::Product(p)
                if p.len() == elements.len() && p.iter().zip(&elements).all(|(x, y)| x.same(y)))
        };
        if holds(a) {
            a.clone()
        } else if holds(b) {
            b.clone()
        } else {
            Self::raw_product(elements)
        }
    }
    /// `families(slots)`, reusing `a` or `b` when it already stores exactly
    /// these slots.
    fn families_sharing(slots: Slots, a: &Self, b: &Self) -> Self {
        let shared = [a, b].into_iter().find(|t| {
            matches!(&t.repr, Repr::Families(store) if store.is_families_of(&slots))
        });
        match shared {
            Some(t) => {
                if verify_fast_paths() {
                    assert_eq!(*t, Self::families(slots), "families sharing");
                }
                t.clone()
            }
            None => Self::families(slots),
        }
    }
    /// Whether both values share storage, which implies equality; cheap,
    /// but may miss equal values stored separately.
    fn same(&self, other: &Self) -> bool {
        use Repr::*;
        match (&self.repr, &other.repr) {
            (Any, Any) | (None, None) | (Unit, Unit) | (Opaque, Opaque) => true,
            (Var(a), Var(b)) => a == b,
            (Product(a), Product(b)) => Arc::ptr_eq(a, b),
            (Nominal(a), Nominal(b)) => Arc::ptr_eq(a, b),
            (NominalSet(a), NominalSet(b)) => Arc::ptr_eq(a, b),
            (Families(a), Families(b)) => a.shares_storage(b),
            _ => false,
        }
    }
    pub fn product(elements: Vec<Self>) -> Self {
        if elements.len() == 1 {
            elements.into_iter().next().unwrap()
        } else {
            Self::raw_product(elements)
        }
    }
    pub fn opaque_sentinel() -> Self {
        Self::new(Repr::Opaque)
    }
    pub fn join(&self, rhs: &Self) -> Result<Self, LatticeError> {
        let _active = ActivePairGuard::enter(Operation::Join, self, rhs)?;
        Ok({
            use Repr::*;
            match (&self.repr, &rhs.repr) {
                (Any, _) | (_, Any) => Self::any(),
                (None, _) => rhs.clone(),
                (_, None) => self.clone(),
                (Unit, _) => rhs.clone(),
                (_, Unit) => self.clone(),
                _ if self == rhs => self.clone(),
                (Opaque, _) => rhs.clone(),
                (_, Opaque) => self.clone(),
                (Var(_), _) | (_, Var(_)) => Self::any(),
                (Product(a), Product(b)) if a.len() == b.len() => Self::product_sharing(
                    a.iter()
                        .zip(b.iter())
                        .map(|(x, y)| x.join(y))
                        .collect::<Result<Vec<_>, _>>()?,
                    self,
                    rhs,
                ),
                (Product(_), _) | (_, Product(_)) => Self::any(),
                (Nominal(_) | NominalSet(_), _) | (_, Nominal(_) | NominalSet(_)) => {
                    self.nominal_join(rhs)?
                }
                (Families(a), Families(b)) => Self::families_sharing(a.join(b)?, self, rhs),
            }
        })
    }
    pub fn meet(&self, rhs: &Self) -> Result<Self, LatticeError> {
        use Repr::*;
        if matches!(self.repr, Var(_) | Any | None | Unit)
            || matches!(rhs.repr, Var(_) | Any | None | Unit)
            || self == rhs
            || !memo::pure(self, rhs)
        {
            return self.meet_uncached(rhs);
        }
        if let Some(hit) = memo::get(&memo::MEET, self, rhs) {
            if verify_fast_paths() {
                assert_eq!(Ok(hit), self.meet_uncached(rhs), "meet memo: {self:?} /\\ {rhs:?}");
            }
            return Ok(hit);
        }
        let result = self.meet_uncached(rhs)?;
        memo::put(&memo::MEET, self, rhs, result);
        Ok(result)
    }
    fn meet_uncached(&self, rhs: &Self) -> Result<Self, LatticeError> {
        let _active = ActivePairGuard::enter(Operation::Meet, self, rhs)?;
        Ok({
            use Repr::*;
            match (&self.repr, &rhs.repr) {
                (Var(_), Var(_)) => Self::any(),
                (Var(_), _) => rhs.clone(),
                (_, Var(_)) => self.clone(),
                (Any, _) => rhs.clone(),
                (_, Any) => self.clone(),
                (None, _) | (_, None) => Self::none(),
                (Unit, _) | (_, Unit) => Self::unit(),
                _ if self == rhs => self.clone(),
                (Nominal(_) | NominalSet(_), _) | (_, Nominal(_) | NominalSet(_)) => {
                    self.nominal_meet(rhs)?
                }
                (Product(a), Product(b)) if a.len() == b.len() => Self::product_sharing(
                    a.iter()
                        .zip(b.iter())
                        .map(|(x, y)| x.meet(y))
                        .collect::<Result<Vec<_>, _>>()?,
                    self,
                    rhs,
                ),
                (Product(_), _) | (_, Product(_)) | (Opaque, _) | (_, Opaque) => Self::none(),
                (Families(a), Families(b)) => Self::families_sharing(a.meet(b)?, self, rhs),
            }
        })
    }
    pub fn subtract(&self, rhs: &Self) -> Result<Self, LatticeError> {
        let _active = ActivePairGuard::enter(Operation::Subtract, self, rhs)?;
        use Repr::*;
        Ok(match (&self.repr, &rhs.repr) {
            (_, Any) => Self::none(),
            (_, Var(_)) => self.clone(),
            (Any, _) => Self::any(),
            (Var(_), _) => self.clone(),
            (_, Unit) => self.clone(),
            (Unit, _) => Self::unit(),
            (None, _) => Self::none(),
            (_, None) => self.clone(),
            (Nominal(_) | NominalSet(_), _) | (_, Nominal(_) | NominalSet(_)) => {
                self.nominal_subtract(rhs)?
            }
            (Product(a), Product(b)) if a.len() == b.len() => {
                let delta = a
                    .iter()
                    .zip(b.iter())
                    .map(|(x, y)| x.subtract(y))
                    .collect::<Result<Vec<_>, _>>()?;
                match delta.iter().filter(|x| **x != Self::none()).count() {
                    0 => Self::none(),
                    1 => Self::product(
                        a.iter()
                            .zip(delta)
                            .map(|(a, b)| if b == Self::none() { a.clone() } else { b })
                            .collect(),
                    ),
                    _ => self.clone(),
                }
            }
            (Product(_), _) | (_, Product(_)) => self.clone(),
            (Families(a), Families(b)) => Self::families_sharing(a.subtract(b)?, self, self),
            (Opaque, _) | (_, Opaque) => return Err(LatticeError::OpaqueSentinelSubtraction),
        })
    }
    pub fn is_subtype(&self, rhs: &Self) -> Result<bool, LatticeError> {
        Ok(self == &self.meet(rhs)?)
    }
    pub fn limit(&self, depth: i64) -> Result<Self, LatticeError> {
        if self.is_limited(depth) {
            Ok(self.clone())
        } else {
            self.limit_aux(depth)
        }
    }
    fn limit_aux(&self, depth: i64) -> Result<Self, LatticeError> {
        Ok({
            if depth <= 0 {
                return Ok(Self::any());
            }
            match &self.repr {
                Repr::Nominal(n) => Self::raw_nominal(n.name.clone(), n.inner.limit_aux(depth)?),
                Repr::NominalSet(ns) => Self::normalized_nominals(
                    ns.nominals
                        .iter()
                        .map(|n| n.as_type().limit_aux(depth))
                        .collect::<Result<Vec<_>, _>>()?,
                    ns.structural.limit_aux(depth)?,
                )?,
                Repr::Families(s) => {
                    let limited = s.limit_aux(depth)?;
                    if s.union_record() {
                        if s.is_raw_of(&limited) {
                            self.clone()
                        } else {
                            Self::new(Repr::Families(FamilyStore::raw(limited)))
                        }
                    } else {
                        Self::families_sharing(limited, self, self)
                    }
                }
                Repr::Product(es) => Self::product_sharing(
                    es.iter()
                        .map(|e| e.limit_aux(depth - 1))
                        .collect::<Result<Vec<_>, _>>()?,
                    self,
                    self,
                ),
                _ => self.clone(),
            }
        })
    }
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let kind = value["kind"].as_str().ok_or("missing kind")?;
        let values = || {
            value["values"]
                .as_array()
                .ok_or("values must be array".to_owned())
        };
        Ok(match kind {
            "product_raw" => Self::raw_product(
                values()?
                    .iter()
                    .map(Self::from_value)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            "union_raw" => Self::from_union_record(values()?)?,
            "nominal_raw" => Self::raw_nominal(
                NominalName::from_value(&value["name"])?,
                Self::from_value(&value["inner"])?,
            ),
            "nominal" => Self::nominal(
                NominalName::from_value(&value["name"])?,
                Self::from_value(&value["inner"])?,
            ),
            "nominal_set" => Self::from_nominal_types(
                value["nominals"]
                    .as_array()
                    .ok_or("missing nominals")?
                    .iter()
                    .map(Self::from_value)
                    .collect::<Result<Vec<_>, _>>()?,
                Self::from_value(&value["structural"])?,
            )?,
            "map_any" => Self::map_any(),
            "map" => Self::map_types(
                value["entries"]
                    .as_array()
                    .ok_or("missing map entries")?
                    .iter()
                    .map(|e| {
                        Ok(MapTypeEntry {
                            key: Self::from_value(&e["key"])?,
                            presence: match e["presence"].as_str() {
                                Some("mandatory") => Presence::Mandatory,
                                Some("optional") => Presence::Optional,
                                _ => {
                                    return Err("map presence must be mandatory or optional".into());
                                }
                            },
                            value: Self::from_value(&e["value"])?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?,
                Self::from_value(&value["default_key"])?,
                Self::from_value(&value["default_value"])?,
            )?,
            "any" => Self::any(),
            "none" => Self::none(),
            "unit" => Self::unit(),
            "atom" => Self::scalar(Some(Atoms::All), None),
            "atoms" => {
                let atoms = values()?
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .map(crate::atom::Atom::new)
                            .ok_or("atom must be string".to_owned())
                    })
                    .collect::<Result<BTreeSet<_>, _>>()?;
                Self::scalar(
                    if atoms.is_empty() {
                        None
                    } else if atoms.len() > 13 {
                        Some(Atoms::All)
                    } else {
                        Some(Atoms::Set(atoms))
                    },
                    None,
                )
            }
            "integer" => Self::scalar(None, Some(Numeric::Integer(Integers::All))),
            "integers" => {
                let ints = values()?
                    .iter()
                    .map(integer)
                    .collect::<Result<BTreeSet<_>, _>>()?;
                Self::scalar(None, int_set(ints).map(Numeric::Integer))
            }
            "range" => Self::scalar(
                None,
                checked_range(Bound::parse(&value["from"])?, Bound::parse(&value["to"])?)?
                    .map(Numeric::Integer),
            ),
            "float" => Self::scalar(None, Some(Numeric::Float)),
            "number" => Self::scalar(None, Some(Numeric::Number)),
            "bitstring" => Self::bitstring(integer(&value["unit"])?, integer(&value["base"])?)?,
            "binary" => Self::binary(),
            "function" => Self::function(
                if value["domain"].is_null() {
                    None
                } else {
                    Some(
                        value["domain"]
                            .as_array()
                            .ok_or("function domain must be array or null")?
                            .iter()
                            .map(Self::from_value)
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                },
                Self::from_value(&value["range"])?,
            ),
            "product" => Self::product(
                values()?
                    .iter()
                    .map(Self::from_value)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            "var" => Self::new(Repr::Var(VariableId::parse(&value["id"])?)),
            "opaque" => Self::opaque_sentinel(),
            "identifier" => Self::identifier(None)?,
            "identifiers" => Self::identifier(Some(
                values()?
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .map(str::to_owned)
                            .ok_or("identifier must be string".to_owned())
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ))?,
            "nil" => Self::nil(),
            "list" => Self::list(
                Self::from_value(&value["elements"])?,
                Self::from_value(&value["tail"])?,
                value["nonempty"].as_bool().ok_or("missing nonempty")?,
            ),
            "proper_list" => Self::proper_list(
                Self::from_value(&value["elements"])?,
                value["nonempty"].as_bool().unwrap_or(false),
            ),
            "cons" => Self::cons(
                Self::from_value(&value["head"])?,
                Self::from_value(&value["tail"])?,
            )?,
            "tuple_any" => Self::tuple_any(),
            "tuple_arity" => {
                Self::tuple_arity(value["arity"].as_u64().ok_or("invalid arity")? as usize)
            }
            "tuple" => Self::tuple(
                value["elements"]
                    .as_array()
                    .ok_or("missing tuple elements")?
                    .iter()
                    .map(Self::from_value)
                    .collect::<Result<Vec<_>, _>>()?,
            )?,
            "tuple_set" => values()?.iter().try_fold(Self::none(), |acc, v| {
                acc.join(&Self::from_value(v)?).map_err(|e| e.to_string())
            })?,
            "union" => values()?.iter().try_fold(Self::none(), |acc, v| {
                acc.join(&Self::from_value(v)?).map_err(|e| e.to_string())
            })?,
            _ => return Err(format!("unsupported type family {kind}")),
        })
    }

    pub fn to_value(&self) -> Value {
        match &self.repr {
            Repr::Nominal(n) => {
                json!({"kind":if n.inner.is_impossible(){"nominal_raw"}else{"nominal"},"name":n.name.value(),"inner":n.inner.to_value()})
            }
            Repr::NominalSet(ns) => {
                json!({"kind":"nominal_set","nominals":ns.nominals.iter().map(|n|n.as_type().to_value()).collect::<Vec<_>>(),"structural":ns.structural.to_value()})
            }
            Repr::Any => json!({"kind":"any"}),
            Repr::None => json!({"kind":"none"}),
            Repr::Unit => json!({"kind":"unit"}),
            Repr::Opaque => json!({"kind":"opaque"}),
            Repr::Var(id) => json!({"kind":"var","id":id.value()}),
            Repr::Product(es) => {
                json!({"kind":if es.len()==1 {"product_raw"}else{"product"},"values":es.iter().map(Type::to_value).collect::<Vec<_>>()})
            }
            Repr::Families(s) => {
                if s.union_record() && s.count() < 2 {
                    return json!({"kind":"union_raw","values":s.slot_values()});
                }
                let mut values = Vec::new();
                if let Some(a) = s.atoms() {
                    values.push(a.value())
                }
                if let Some(b) = s.bitstrings() {
                    values.push(b.value())
                }
                if let Some(f) = s.functions() {
                    values.push(f.value())
                }
                if let Some(i) = s.identifiers() {
                    values.push(i.value())
                }
                if let Some(l) = s.list() {
                    values.push(l.value())
                }
                if let Some(n) = s.number() {
                    values.push(n.value())
                }
                if let Some(t) = s.tuple() {
                    values.push(t.value())
                }
                if let Some(m) = s.maps() {
                    values.push(m.value());
                }
                if values.len() == 1 {
                    values.pop().unwrap()
                } else {
                    json!({"kind":"union","values":values})
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum TypeView<'a> {
    Nominal(NominalView<'a>),
    NominalSet(NominalSetView<'a>),
    Any,
    None,
    Unit,
    OpaqueSentinel,
    Variable(VariableView<'a>),
    Product(&'a [Type]),
    Families(SlotsView<'a>),
}
#[derive(Clone, Copy, Debug)]
pub struct NominalView<'a> {
    pub name: &'a NominalName,
    pub inner: &'a Type,
}
#[derive(Clone, Copy, Debug)]
pub struct NominalSetView<'a> {
    nominals: &'a [Nominal],
    pub structural: &'a Type,
}
impl<'a> NominalSetView<'a> {
    pub fn iter(self) -> impl Iterator<Item = NominalView<'a>> + 'a {
        self.nominals.iter().map(|n| NominalView {
            name: &n.name,
            inner: &n.inner,
        })
    }
}
#[derive(Clone, Copy, Debug)]
pub struct SlotsView<'a> {
    pub is_union_record: bool,
    pub atoms: Option<AtomsView<'a>>,
    pub bitstrings: Option<BitstringView<'a>>,
    pub functions: Option<FunctionView<'a>>,
    pub identifiers: Option<IdentifiersView<'a>>,
    pub list: Option<ListView<'a>>,
    pub number: Option<NumberView<'a>>,
    pub tuples: Option<TuplesView<'a>>,
    pub maps: Option<MapView<'a>>,
}
#[derive(Clone, Copy, Debug)]
pub struct MapView<'a> {
    pub entries: &'a [MapEntry],
    pub default_key: &'a Type,
    pub default_value: &'a Type,
}
#[derive(Clone, Copy, Debug)]
pub enum AtomsView<'a> {
    All,
    Set(&'a BTreeSet<crate::atom::Atom>),
}
#[derive(Clone, Copy, Debug)]
pub enum IntegerView<'a> {
    All,
    Set(&'a BTreeSet<BigInt>),
    Range {
        from: EndpointView<'a>,
        to: EndpointView<'a>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Endpoint {
    NegativeInfinity,
    Finite(BigInt),
    PositiveInfinity,
}
impl From<Endpoint> for Bound {
    fn from(e: Endpoint) -> Self {
        match e {
            Endpoint::NegativeInfinity => Self::NegativeInfinity,
            Endpoint::Finite(n) => Self::Finite(n),
            Endpoint::PositiveInfinity => Self::PositiveInfinity,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum EndpointView<'a> {
    NegativeInfinity,
    Finite(&'a BigInt),
    PositiveInfinity,
}
#[derive(Clone, Copy, Debug)]
pub enum NumberView<'a> {
    Float,
    Number,
    Integer(IntegerView<'a>),
}
#[derive(Clone, Copy, Debug)]
pub struct BitstringView<'a> {
    pub unit: &'a BigInt,
    pub base: &'a BigInt,
}
#[derive(Clone, Copy, Debug)]
pub struct FunctionView<'a> {
    pub domain: Option<&'a [Type]>,
    pub range: &'a Type,
}
#[derive(Clone, Copy, Debug)]
pub enum IdentifiersView<'a> {
    All,
    Set(&'a BTreeSet<String>),
}
#[derive(Clone, Copy, Debug)]
pub enum ListView<'a> {
    Nil,
    List {
        elements: &'a Type,
        tail: &'a Type,
        nonempty: bool,
    },
}
#[derive(Clone, Copy, Debug)]
pub enum TuplesView<'a> {
    Any,
    Alternatives(TupleAlternativesView<'a>),
}
#[derive(Clone, Copy, Debug)]
pub struct TupleAlternativesView<'a>(&'a BTreeMap<usize, Vec<TupleVariant>>);
#[derive(Clone, Copy, Debug)]
pub struct TupleAlternativeView<'a> {
    pub elements: &'a [Type],
    pub tag: Option<&'a str>,
}
impl<'a> TupleAlternativesView<'a> {
    pub fn iter(self) -> impl Iterator<Item = TupleAlternativeView<'a>> + 'a {
        self.0
            .values()
            .flat_map(|vs| vs.iter())
            .map(|v| TupleAlternativeView {
                elements: &v.elements,
                tag: v.tag.as_deref(),
            })
    }
    pub fn arities(self) -> impl Iterator<Item = usize> + 'a {
        self.0.keys().copied()
    }
}
#[derive(Clone, Copy, Debug)]
pub enum VariableView<'a> {
    Atom(&'a str),
    Integer(&'a VariableInteger),
}
impl Bound {
    fn view(&self) -> EndpointView<'_> {
        match self {
            Self::NegativeInfinity => EndpointView::NegativeInfinity,
            Self::PositiveInfinity => EndpointView::PositiveInfinity,
            Self::Finite(n) => EndpointView::Finite(n),
        }
    }
}
impl Type {
    pub fn view(&self) -> TypeView<'_> {
        match &self.repr {
            Repr::Nominal(n) => TypeView::Nominal(NominalView {
                name: &n.name,
                inner: &n.inner,
            }),
            Repr::NominalSet(ns) => TypeView::NominalSet(NominalSetView {
                nominals: &ns.nominals,
                structural: &ns.structural,
            }),
            Repr::Any => TypeView::Any,
            Repr::None => TypeView::None,
            Repr::Unit => TypeView::Unit,
            Repr::Opaque => TypeView::OpaqueSentinel,
            Repr::Var(id) => TypeView::Variable(match id {
                VariableId::Atom(a) => VariableView::Atom(a),
                VariableId::Integer(n) => VariableView::Integer(n),
            }),
            Repr::Product(es) => TypeView::Product(es),
            Repr::Families(s) => TypeView::Families(s.view()),
        }
    }
    pub fn is_any(&self) -> bool {
        matches!(self.repr, Repr::Any)
    }
    pub fn is_none(&self) -> bool {
        matches!(self.repr, Repr::None)
    }
    pub fn is_unit(&self) -> bool {
        matches!(self.repr, Repr::Unit)
    }
    pub fn is_impossible(&self) -> bool {
        self.is_none() || self.is_unit()
    }
    pub fn variable_atom(name: impl Into<String>) -> Self {
        Self::new(Repr::Var(VariableId::Atom(name.into())))
    }
    pub fn variable_id(id: VariableId) -> Self {
        Self::new(Repr::Var(id))
    }
    pub(crate) fn variable_ref(&self) -> Option<&VariableId> {
        match &self.repr {
            Repr::Var(id) => Some(id),
            _ => None,
        }
    }
    pub fn variable_integer(id: BigInt) -> Self {
        Self::variable_id(VariableId::Integer(id.into()))
    }
}

impl serde::Serialize for Type {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(&self.to_value(), serializer)
    }
}

impl<'de> serde::Deserialize<'de> for Type {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = <Value as serde::Deserialize>::deserialize(deserializer)?;
        Self::from_value(&value).map_err(serde::de::Error::custom)
    }
}

impl FamilyStore {
    fn view(&self) -> SlotsView<'_> {
        let s = self;
        SlotsView {
            is_union_record: s.union_record(),
            maps: s.maps().map(|m| MapView {
                entries: &m.entries,
                default_key: &m.default_key,
                default_value: &m.default_value,
            }),
            atoms: s.atoms().map(|a| match a {
                Atoms::All => AtomsView::All,
                Atoms::Set(xs) => AtomsView::Set(xs),
            }),
            bitstrings: s.bitstrings().map(|b| BitstringView {
                unit: &b.unit,
                base: &b.base,
            }),
            functions: s.functions().map(|f| FunctionView {
                domain: match &f.domain.repr {
                    Repr::Any => None,
                    Repr::Product(es) => Some(es),
                    _ => unreachable!("function domain invariant"),
                },
                range: &f.range,
            }),
            identifiers: s.identifiers().map(|i| match i {
                Identifiers::All => IdentifiersView::All,
                Identifiers::Set(xs) => IdentifiersView::Set(xs),
            }),
            list: s.list().map(|l| match l {
                Lists::Nil => ListView::Nil,
                Lists::List {
                    elements,
                    tail,
                    nonempty,
                } => ListView::List {
                    elements,
                    tail,
                    nonempty: *nonempty,
                },
            }),
            number: s.number().map(|n| match n {
                Numeric::Float => NumberView::Float,
                Numeric::Number => NumberView::Number,
                Numeric::Integer(i) => NumberView::Integer(match i {
                    Integers::All => IntegerView::All,
                    Integers::Set(xs) => IntegerView::Set(xs),
                    Integers::Range(a, b) => IntegerView::Range {
                        from: a.view(),
                        to: b.view(),
                    },
                }),
            }),
            tuples: s.tuple().map(|t| match t {
                Tuples::Any => TuplesView::Any,
                Tuples::Set(xs) => TuplesView::Alternatives(TupleAlternativesView(xs)),
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpacityConflict {
    None,
    ExpectedOpaque,
    ExpectedTransparent,
}
impl Type {
    pub fn structural(&self) -> Result<Self, LatticeError> {
        match &self.repr {
            Repr::Nominal(n) => n.inner.structural(),
            Repr::NominalSet(_) => self.shallow_structural()?.structural(),
            _ => self.transform_children(&|t| t.structural()),
        }
    }
    /// Whether any nominal (named, possibly opaque) type or the opaque
    /// sentinel occurs in this type. Allocation-free.
    pub fn contains_nominal(&self) -> bool {
        let fast = self.info.flags & NOMINAL != 0;
        if verify_fast_paths() {
            assert_eq!(fast, self.contains_nominal_walk(), "contains_nominal: {self:?}");
        }
        fast
    }
    fn contains_nominal_walk(&self) -> bool {
        let mut pending = vec![self];
        while let Some(t) = pending.pop() {
            match &t.repr {
                Repr::Nominal(_) | Repr::NominalSet(_) | Repr::Opaque => return true,
                Repr::Product(p) => pending.extend(p.iter()),
                Repr::Families(slots) => {
                    if let Some(Functions { domain, range }) = slots.functions() {
                        pending.push(domain);
                        pending.push(range);
                    }
                    if let Some(Lists::List { elements, tail, .. }) = slots.list() {
                        pending.push(elements);
                        pending.push(tail);
                    }
                    if let Some(Tuples::Set(groups)) = slots.tuple() {
                        for variants in groups.values() {
                            for v in variants {
                                pending.extend(v.elements.iter());
                            }
                        }
                    }
                    if let Some(m) = slots.maps() {
                        pending.extend(m.entries.iter().map(|e| &e.value));
                        pending.push(&m.default_key);
                        pending.push(&m.default_value);
                    }
                }
                _ => {}
            }
        }
        false
    }
    fn blind(&self, direction: &Self, module: &str) -> Result<Self, LatticeError> {
        match &self.repr {
            Repr::Nominal(n) => match &n.name {
                NominalName::Defined {
                    module: m, opacity, ..
                } => Ok(Self::nominal(
                    n.name.clone(),
                    if *opacity == Opacity::Transparent || m == module {
                        n.inner.blind(direction, module)?
                    } else {
                        direction.clone()
                    },
                )),
                _ => Ok(self.clone()),
            },
            Repr::NominalSet(ns) => Self::normalized_nominals(
                ns.nominals
                    .iter()
                    .map(|n| n.as_type().blind(direction, module))
                    .collect::<Result<Vec<_>, _>>()?,
                ns.structural.blind(direction, module)?,
            ),
            _ => self.transform_children(&|t| t.blind(direction, module)),
        }
    }
    fn transform_children<F>(&self, f: &F) -> Result<Self, LatticeError>
    where
        F: Fn(&Self) -> Result<Self, LatticeError>,
    {
        Ok(match &self.repr {
            Repr::Product(p) => Self::raw_product(p.iter().map(f).collect::<Result<Vec<_>, _>>()?),
            Repr::Families(slots) => {
                if slots.functions().is_none()
                    && !matches!(slots.list(), Some(Lists::List { .. }))
                    && !matches!(slots.tuple(), Some(Tuples::Set(_)))
                    && slots.maps().is_none()
                    && !slots.empty()
                    && slots.union_record() == (slots.count() > 1)
                {
                    return Ok(self.clone());
                }
                let mut s = slots.rebuild_base();
                if let Some(x) = slots.functions_owner() {
                    let domain = f(&x.domain)?;
                    s.functions = Some(Functions::shared(domain, f(&x.range)?, x, x));
                }
                if let Some(x) = slots.list_owner()
                    && let Lists::List {
                        elements,
                        tail,
                        nonempty,
                    } = &**x
                {
                    let elements = f(elements)?;
                    s.list = Some(Lists::shared(elements, f(tail)?, *nonempty, x, x));
                }
                if let Some(x) = slots.tuple_owner()
                    && let Tuples::Set(groups) = &**x
                {
                    let mut out = BTreeMap::new();
                    for (arity, variants) in groups {
                        let mut vs = Vec::new();
                        for v in variants {
                            vs.push(TupleVariant {
                                elements: v
                                    .elements
                                    .iter()
                                    .map(f)
                                    .collect::<Result<Vec<_>, _>>()?,
                                tag: v.tag.clone(),
                            });
                        }
                        out.insert(*arity, vs);
                    }
                    s.tuple = Some(reuse_left(Tuples::Set(out), x));
                }
                if let Some(x) = slots.maps_owner() {
                    let entries = x
                        .entries
                        .iter()
                        .map(|e| {
                            Ok(MapEntry {
                                key: e.key.clone(),
                                presence: e.presence,
                                value: f(&e.value)?,
                            })
                        })
                        .collect::<Result<Vec<_>, LatticeError>>()?;
                    s.maps = Maps::canonical(entries, f(&x.default_key)?, f(&x.default_value)?)?
                        .map(|m| reuse_left(m, x));
                }
                Self::families_sharing(s, self, self)
            }
            _ => self.clone(),
        })
    }
    pub fn opacity_conflict(
        &self,
        required: &Self,
        module: &str,
    ) -> Result<OpacityConflict, LatticeError> {
        // Without nominal or opaque parts, `blind` returns its input, so there
        // is no conflict; skip the meet and both rebuilds.
        if !self.contains_nominal() && !required.contains_nominal() {
            if verify_fast_paths() {
                let slow = self.opacity_conflict_full(required, module)?;
                assert_eq!(slow, OpacityConflict::None, "opacity fast path: {self:?} vs {required:?}");
            }
            return Ok(OpacityConflict::None);
        }
        self.opacity_conflict_full(required, module)
    }
    fn opacity_conflict_full(
        &self,
        required: &Self,
        module: &str,
    ) -> Result<OpacityConflict, LatticeError> {
        let direction = if self.meet(required)?.is_impossible() {
            Self::any()
        } else {
            Self::opaque_sentinel()
        };
        let required_blind = required.blind(&direction, module)?;
        let given_blind = self.blind(&direction, module)?;
        let required_unchanged = required_blind == *required;
        if required_unchanged && given_blind == *self {
            return Ok(OpacityConflict::None);
        }
        let error = if required_unchanged {
            OpacityConflict::ExpectedTransparent
        } else {
            OpacityConflict::ExpectedOpaque
        };
        let impossible = given_blind.meet(&required_blind)?.is_impossible();
        Ok(
            if (impossible && !direction.is_any()) || (!impossible && direction.is_any()) {
                error
            } else {
                OpacityConflict::None
            },
        )
    }
}

impl Type {
    pub fn elements(&self) -> Result<Vec<Self>, LatticeError> {
        use Repr::*;
        Ok(match &self.repr {
            None | Unit => vec![],
            Any | Product(_) | Nominal(_) => vec![self.clone()],
            Var(_) => vec![Self::any()],
            Opaque => return Err(LatticeError::FunctionClause),
            NominalSet(ns) => {
                let mut es = ns.structural.elements()?;
                es.extend(ns.nominals.iter().map(self::Nominal::as_type));
                es
            }
            Families(s) => {
                let mut out = Vec::new();
                if let Some(a) = s.atoms() {
                    match a {
                        Atoms::All => out.push(Self::all_atoms()),
                        Atoms::Set(xs) => out.extend(xs.iter().map(Self::atom)),
                    }
                }
                if let Some(b) = s.bitstrings() {
                    out.push(Self::families(Slots {
                        bitstrings: Some(Arc::new(b.clone())),
                        ..Slots::default()
                    }))
                }
                if let Some(f) = s.functions() {
                    out.push(Self::families(Slots {
                        functions: Some(Arc::new(f.clone())),
                        ..Slots::default()
                    }))
                }
                if let Some(i) = s.identifiers() {
                    match i {
                        Identifiers::All => out.push(
                            Self::identifier(Option::None).map_err(|_| LatticeError::BadArg)?,
                        ),
                        Identifiers::Set(xs) => {
                            for x in xs {
                                out.push(
                                    Self::identifier(Some(vec![x.clone()]))
                                        .map_err(|_| LatticeError::BadArg)?,
                                );
                            }
                        }
                    }
                }
                if let Some(l) = s.list() {
                    out.push(Self::families(Slots {
                        list: Some(Arc::new(l.clone())),
                        ..Slots::default()
                    }))
                }
                if let Some(n) = s.number() {
                    match n {
                        Numeric::Number => {
                            out.push(Self::float());
                            out.push(Self::all_integers())
                        }
                        Numeric::Integer(Integers::Set(xs)) => {
                            out.extend(xs.iter().cloned().map(Self::integer))
                        }
                        _ => out.push(Self::scalar(Option::None, Some(n.clone()))),
                    }
                }
                if let Some(t) = s.tuple() {
                    match t {
                        Tuples::Any => out.push(Self::tuple_any()),
                        Tuples::Set(groups) => {
                            for (arity, vs) in groups {
                                for v in vs {
                                    out.push(Self::from_tuples(Some(Tuples::Set(BTreeMap::from(
                                        [(*arity, vec![v.clone()])],
                                    )))));
                                }
                            }
                        }
                    }
                }
                if let Some(m) = s.maps() {
                    out.push(Self::from_map(Some(m.clone())))
                }
                out
            }
        })
    }
}

impl Slots {
    fn slot_values(&self) -> Vec<Value> {
        vec![
            self.atoms.as_deref().map(Atoms::value),
            self.bitstrings.as_deref().map(Bitstrings::value),
            self.functions.as_deref().map(Functions::value),
            self.identifiers.as_deref().map(Identifiers::value),
            self.list.as_deref().map(Lists::value),
            self.number.as_deref().map(Numeric::value),
            self.tuple.as_deref().map(Tuples::value),
            self.maps.as_deref().map(Maps::value),
        ]
        .into_iter()
        .map(|v| v.unwrap_or_else(|| Type::none().to_value()))
        .collect()
    }
}
impl Type {
    fn from_union_record(values: &[Value]) -> Result<Self, String> {
        if values.len() != 8 {
            return Err("native union record requires eight ordered slots".into());
        }
        let mut slots = Slots::default();
        for (index, value) in values.iter().enumerate() {
            let t = Self::from_value(value)?;
            if t.is_none() {
                continue;
            }
            let Repr::Families(s) = &t.repr else {
                return Err("native union slot must be a family or none".into());
            };
            if s.union_record() || s.count() != 1 {
                return Err("native union slot must contain exactly one direct family".into());
            }
            let mut expected = s.to_slots();
            match index {
                0 => slots.atoms = expected.atoms.take(),
                1 => slots.bitstrings = expected.bitstrings.take(),
                2 => slots.functions = expected.functions.take(),
                3 => slots.identifiers = expected.identifiers.take(),
                4 => slots.list = expected.list.take(),
                5 => slots.number = expected.number.take(),
                6 => slots.tuple = expected.tuple.take(),
                7 => slots.maps = expected.maps.take(),
                _ => unreachable!(),
            };
            if !expected.empty() {
                return Err("native union family is in the wrong slot".into());
            }
        }
        slots.union_record = true;
        Ok(Self::new(Repr::Families(FamilyStore::raw(slots))))
    }
}

impl Type {
    fn append_children<'a>(&'a self, pending: &mut Vec<&'a Type>) {
        match &self.repr {
            Repr::Nominal(n) => pending.push(&n.inner),
            Repr::NominalSet(n) => {
                pending.extend(n.nominals.iter().map(|v| v.inner.as_ref()));
                pending.push(&n.structural);
            }
            Repr::Product(p) => pending.extend(p.iter()),
            Repr::Families(s) => {
                if let Some(f) = s.functions() {
                    pending.extend([f.domain.as_ref(), f.range.as_ref()]);
                }
                if let Some(Lists::List { elements, tail, .. }) = s.list() {
                    pending.extend([elements.as_ref(), tail.as_ref()]);
                }
                if let Some(Tuples::Set(groups)) = s.tuple() {
                    for variant in groups.values().flatten() {
                        pending.extend(&variant.elements);
                    }
                }
                if let Some(map) = s.maps() {
                    pending.extend([map.default_key.as_ref(), map.default_value.as_ref()]);
                }
            }
            _ => (),
        }
    }
    pub fn is_limited(&self, depth: i64) -> bool {
        let fast = depth >= i64::from(self.info.limited_from);
        if verify_fast_paths() {
            assert_eq!(fast, self.is_limited_walk(depth), "is_limited({depth}): {self:?}");
        }
        fast
    }
    fn is_limited_walk(&self, depth: i64) -> bool {
        if self.is_any() {
            return true;
        }
        if depth <= 0 {
            return false;
        }
        match &self.repr {
            Repr::Nominal(n) => n.inner.is_limited_walk(depth),
            Repr::NominalSet(n) => {
                n.structural.is_limited_walk(depth)
                    && n.nominals.iter().all(|n| n.inner.is_limited_walk(depth))
            }
            Repr::Product(p) => p.iter().all(|t| t.is_limited_walk(depth - 1)),
            Repr::Families(s) => {
                let functions = s
                    .functions()
                    .as_deref()
                    .is_none_or(|f| f.domain.is_limited_walk(depth) && f.range.is_limited_walk(depth - 1));
                let list = s.list().is_none_or(|l| match l {
                    Lists::Nil => true,
                    Lists::List { elements, tail, .. } => {
                        elements.is_limited_walk(depth - 1)
                            && (matches!(tail.singleton_key(), Some(SingletonKey::Nil))
                                || tail.is_limited_walk((depth - 1).max(1)))
                    }
                });
                let tuple = s.tuple().is_none_or(|t| match t {
                    Tuples::Any => true,
                    Tuples::Set(g) => g.values().flatten().all(|v| {
                        (depth != 1 || v.tag.is_none())
                            && v.elements.iter().all(|t| t.is_limited_walk(depth - 1))
                    }),
                });
                let map = s.maps().is_none_or(|m| {
                    m.entries
                        .iter()
                        .all(|e| depth > 1 && e.value.is_limited_walk(depth - 1))
                        && m.default_key.is_limited_walk(depth - 1)
                        && m.default_value.is_limited_walk(depth - 1)
                });
                functions && list && tuple && map
            }
            _ => true,
        }
    }
    pub fn substitute(&self, bindings: &BTreeMap<VariableId, Type>) -> Result<Self, LatticeError> {
        self.substitute_with(&|id| bindings.get(id))
    }
    pub(crate) fn substitute_with<'a>(
        &self,
        lookup: &impl Fn(&VariableId) -> Option<&'a Type>,
    ) -> Result<Self, LatticeError> {
        if self.has_var() {
            self.substitute_aux(lookup)
        } else {
            Ok(self.clone())
        }
    }
    fn substitute_aux<'a>(
        &self,
        lookup: &impl Fn(&VariableId) -> Option<&'a Type>,
    ) -> Result<Self, LatticeError> {
        Ok(match &self.repr {
            Repr::Var(id) => lookup(id).cloned().unwrap_or_else(Self::any),
            Repr::Nominal(n) => Self::raw_nominal(n.name.clone(), n.inner.substitute_aux(lookup)?),
            Repr::NominalSet(n) => Self::normalized_nominals(
                n.nominals
                    .iter()
                    .map(|n| n.as_type().substitute_aux(lookup))
                    .collect::<Result<Vec<_>, _>>()?,
                n.structural.substitute_aux(lookup)?,
            )?,
            Repr::Product(p) => Self::raw_product(
                p.iter()
                    .map(|t| t.substitute_aux(lookup))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Repr::Families(slots) => {
                let mut s = slots.rebuild_base();
                if let Some(f) = slots.functions() {
                    s.functions = Some(Arc::new(Functions {
                        domain: Box::new(f.domain.substitute_aux(lookup)?),
                        range: Box::new(f.range.substitute_aux(lookup)?),
                    }));
                }
                if let Some(Lists::List {
                    elements,
                    tail,
                    nonempty,
                }) = slots.list()
                {
                    let head = elements.substitute_aux(lookup)?;
                    if head.is_impossible() {
                        s.list = None;
                    } else {
                        let end = tail.substitute_aux(lookup)?;
                        if end == Self::nil() || end.is_any() {
                            s.list = Some(Arc::new(Lists::List {
                                elements: Box::new(head),
                                tail: Box::new(end),
                                nonempty: *nonempty,
                            }));
                        } else {
                            let cons = Self::cons(head, end)?;
                            let Repr::Families(c) = &cons.repr else {
                                return Err(LatticeError::BadMatch(Box::new(cons)));
                            };
                            let Some(Lists::List { elements, tail, .. }) = c.list() else {
                                return Err(LatticeError::BadMatch(Box::new(cons)));
                            };
                            s.list = Some(Arc::new(Lists::List {
                                elements: elements.clone(),
                                tail: tail.clone(),
                                nonempty: *nonempty,
                            }));
                        }
                    }
                }
                if let Some(Tuples::Set(groups)) = slots.tuple() {
                    let mut combined = Self::none();
                    for variant in groups.values().flatten() {
                        let t = Self::tuple(
                            variant
                                .elements
                                .iter()
                                .map(|t| t.substitute_aux(lookup))
                                .collect::<Result<Vec<_>, _>>()?,
                        )?;
                        combined = combined.join(&t)?;
                    }
                    s.tuple = match &combined.repr {
                        Repr::Families(t) => t.tuple_owner().cloned(),
                        _ => None,
                    };
                }
                if let Some(m) = slots.maps() {
                    s.maps = Maps::canonical(
                        m.entries
                            .iter()
                            .map(|e| {
                                Ok(MapEntry {
                                    key: e.key.clone(),
                                    presence: e.presence,
                                    value: e.value.substitute_aux(lookup)?,
                                })
                            })
                            .collect::<Result<Vec<_>, LatticeError>>()?,
                        m.default_key.substitute_aux(lookup)?,
                        m.default_value.substitute_aux(lookup)?,
                    )?
                    .map(Arc::new);
                }
                if slots.union_record() {
                    Self::new(Repr::Families(FamilyStore::raw(s)))
                } else {
                    Self::families(s)
                }
            }
            _ => self.clone(),
        })
    }
    pub fn has_var(&self) -> bool {
        let fast = self.info.flags & HAS_VAR != 0;
        if verify_fast_paths() {
            assert_eq!(fast, self.has_var_walk(), "has_var: {self:?}");
        }
        fast
    }
    fn has_var_walk(&self) -> bool {
        match &self.repr {
            Repr::Var(_) => true,
            Repr::Nominal(n) => n.inner.has_var_walk(),
            Repr::NominalSet(n) => {
                n.structural.has_var_walk() && n.nominals.iter().any(|n| n.inner.has_var_walk())
            }
            Repr::Product(p) => p.iter().any(Type::has_var_walk),
            Repr::Families(s) => {
                s.functions()
                    .as_deref()
                    .is_some_and(|f| f.domain.has_var_walk() || f.range.has_var_walk())
                    || s.list().is_some_and(|list| match list {
                        Lists::Nil => false,
                        Lists::List { elements, tail, .. } => elements.has_var_walk() || tail.has_var_walk(),
                    })
                    || s.tuple().is_some_and(|tuple| match tuple {
                        Tuples::Any => false,
                        Tuples::Set(groups) => groups
                            .values()
                            .flatten()
                            .any(|variant| variant.elements.iter().any(Type::has_var_walk)),
                    })
                    || s.maps()
                        .as_deref()
                        .is_some_and(|map| map.default_key.has_var_walk() || map.default_value.has_var_walk())
            }
            _ => false,
        }
    }
    pub fn collect_var_names(&self) -> BTreeSet<VariableId> {
        let found = self.collect_var_names_skipping();
        if verify_fast_paths() {
            let mut all = BTreeSet::new();
            let mut pending = vec![self];
            while let Some(t) = pending.pop() {
                if let Repr::Var(id) = &t.repr {
                    all.insert(id.clone());
                }
                t.append_children(&mut pending);
            }
            assert_eq!(found, all, "collect_var_names: {self:?}");
        }
        found
    }
    fn collect_var_names_skipping(&self) -> BTreeSet<VariableId> {
        let mut found = BTreeSet::new();
        let mut pending = vec![self];
        if self.info.flags & ANY_VAR == 0 {
            pending.clear();
        }
        while let Some(t) = pending.pop() {
            if t.info.flags & ANY_VAR == 0 {
                continue;
            }
            if let Repr::Var(id) = &t.repr {
                found.insert(id.clone());
            }
            t.append_children(&mut pending);
        }
        found
    }
}

impl Type {
    fn bitstring_ref(&self) -> Result<&Bitstrings, LatticeError> {
        if let Repr::Families(s) = &self.repr {
            if !s.union_record()
                && s.count() == usize::from(s.bitstrings().is_some())
                && let Some(b) = s.bitstrings()
            {
                return Ok(b);
            }
        }
        Err(LatticeError::FunctionClause)
    }
    pub fn bitstr_concat(&self, rhs: &Self) -> Result<Self, LatticeError> {
        let bitstr =
            Self::bitstring(BigInt::one(), BigInt::zero()).map_err(|_| LatticeError::BadArg)?;
        let a = bitstr.meet(self)?.structural()?;
        let b = bitstr.meet(rhs)?.structural()?;
        a.bitstr_concat_raw(&b)
    }
    pub fn bitstr_match(&self, subject: &Self) -> Result<Self, LatticeError> {
        let bitstr =
            Self::bitstring(BigInt::one(), BigInt::zero()).map_err(|_| LatticeError::BadArg)?;
        let a = bitstr.meet(self)?.structural()?;
        let b = bitstr.meet(subject)?.structural()?;
        a.bitstr_match_raw(&b)
    }
    fn bitstr_concat_raw(&self, rhs: &Self) -> Result<Self, LatticeError> {
        if self.is_none() || rhs.is_none() {
            return Ok(Self::none());
        }
        let (a, b) = (self.bitstring_ref()?, rhs.bitstring_ref()?);
        Self::bitstring(gcd(a.unit.clone(), b.unit.clone()), &a.base + &b.base)
            .map_err(|_| LatticeError::BadArg)
    }
    fn bitstr_match_raw(&self, subject: &Self) -> Result<Self, LatticeError> {
        if self.is_none() || subject.is_none() {
            return Ok(Self::none());
        }
        let (a, b) = (self.bitstring_ref()?, subject.bitstring_ref()?);
        let (unit, base) = if a.unit.is_zero() && b.unit.is_zero() {
            if a.base > b.base {
                return Ok(Self::none());
            }
            (BigInt::zero(), &b.base - &a.base)
        } else if a.unit.is_zero() {
            (
                b.unit.clone(),
                if a.base <= b.base {
                    &b.base - &a.base
                } else {
                    modulo(&b.base - &a.base, &b.unit)
                },
            )
        } else if b.unit.is_zero() && a.base > b.base {
            return Ok(Self::none());
        } else {
            let unit = gcd(a.unit.clone(), b.unit.clone());
            let base = modulo(&b.base - &a.base, &unit);
            (unit, base)
        };
        Self::bitstring(unit, base).map_err(|_| LatticeError::BadArg)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnificationValue {
    Type(Type),
    MalformedNominalSet { nominal: Type, structural: Type },
}
impl UnificationValue {
    pub fn limit(&self, depth: i64) -> Result<Type, LatticeError> {
        match self {
            Self::Type(t) => t.limit(depth),
            Self::MalformedNominalSet { .. } if depth <= 0 => Ok(Type::any()),
            Self::MalformedNominalSet {
                nominal,
                structural,
            } => {
                if structural.is_limited(depth) {
                    Err(LatticeError::FunctionClause)
                } else {
                    Err(LatticeError::BadGenerator(Box::new(nominal.clone())))
                }
            }
        }
    }
    pub fn to_value(&self) -> Value {
        match self {
            Self::Type(t) => t.to_value(),
            Self::MalformedNominalSet {
                nominal,
                structural,
            } => {
                json!({"kind":"unification_nominal_set","nominal":nominal.to_value(),"structural":structural.to_value()})
            }
        }
    }
    fn is_nominal_set(&self) -> bool {
        matches!(
            self,
            Self::MalformedNominalSet { .. }
        ) || matches!(self, Self::Type(t) if matches!(t.repr, Repr::NominalSet(_))
        )
    }
}
impl From<Type> for UnificationValue {
    fn from(t: Type) -> Self {
        Self::Type(t)
    }
}
pub type UnificationBindings = BTreeMap<VariableId, UnificationValue>;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnificationError {
    Mismatch {
        left: Box<UnificationValue>,
        right: Box<UnificationValue>,
    },
    NativeError(LatticeError),
    NativeNonTerminating,
}
impl std::fmt::Display for UnificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mismatch { left, right } => write!(
                f,
                "OTP throw mismatch: {}",
                json!([left.to_value(), right.to_value()])
            ),
            Self::NativeError(e) => e.fmt(f),
            Self::NativeNonTerminating => {
                f.write_str("native_nonterminating: repeated active unification pair and bindings")
            }
        }
    }
}
impl std::error::Error for UnificationError {}
impl From<LatticeError> for UnificationError {
    fn from(e: LatticeError) -> Self {
        if e == LatticeError::NativeNonTerminating {
            Self::NativeNonTerminating
        } else {
            Self::NativeError(e)
        }
    }
}
#[derive(Default)]
struct Unifier {
    bindings: UnificationBindings,
}
/// A borrowed `UnificationValue`, so unifying the parts of a type doesn't
/// clone them. `Other` never holds `UnificationValue::Type`, so the derived
/// equality agrees with `UnificationValue`'s.
#[derive(Clone, Copy, PartialEq)]
enum Val<'a> {
    Type(&'a Type),
    Other(&'a UnificationValue),
}
impl<'a> From<&'a UnificationValue> for Val<'a> {
    fn from(v: &'a UnificationValue) -> Self {
        match v {
            UnificationValue::Type(t) => Self::Type(t),
            other => Self::Other(other),
        }
    }
}
impl<'a> Val<'a> {
    fn type_ref(self) -> Option<&'a Type> {
        match self {
            Self::Type(t) => Some(t),
            Self::Other(_) => None,
        }
    }
    fn is_nominal_set(self) -> bool {
        match self {
            Self::Type(t) => matches!(t.repr, Repr::NominalSet(_)),
            Self::Other(v) => v.is_nominal_set(),
        }
    }
    fn to_owned(self) -> UnificationValue {
        match self {
            Self::Type(t) => UnificationValue::Type(t.clone()),
            Self::Other(v) => v.clone(),
        }
    }
}
/// A pair being unified by an enclosing call, linked to its caller's pair.
/// Only absent-key inserts change bindings; `len` identifies each snapshot.
struct Active<'a> {
    a: Val<'a>,
    b: Val<'a>,
    len: usize,
    up: Option<&'a Active<'a>>,
}
impl Type {
    pub fn unify_bindings(&self, rhs: &Self) -> Result<UnificationBindings, UnificationError> {
        let mut u = Unifier::default();
        u.pair(Val::Type(self), Val::Type(rhs), None)?;
        Ok(u.bindings)
    }
    fn direct_slots(&self) -> Option<&FamilyStore> {
        match &self.repr {
            Repr::Families(s) if !s.union_record() => Some(s),
            _ => None,
        }
    }
}
impl Unifier {
    fn mismatch(a: Val, b: Val) -> UnificationError {
        UnificationError::Mismatch {
            left: Box::new(a.to_owned()),
            right: Box::new(b.to_owned()),
        }
    }
    fn pair(&mut self, a: Val, b: Val, up: Option<&Active>) -> Result<(), UnificationError> {
        let len = self.bindings.len();
        let mut active = up;
        while let Some(x) = active {
            if x.len == len && x.a == a && x.b == b {
                return Err(UnificationError::NativeNonTerminating);
            }
            active = x.up;
        }
        self.pair_aux(a, b, &Active { a, b, len, up })
    }
    fn types(&mut self, a: &Type, b: &Type, up: &Active) -> Result<(), UnificationError> {
        self.pair(Val::Type(a), Val::Type(b), Some(up))
    }
    fn lists(&mut self, a: &[Type], b: &[Type], up: &Active) -> Result<(), UnificationError> {
        if a.len() != b.len() {
            return Err(LatticeError::FunctionClause.into());
        }
        for (a, b) in a.iter().zip(b.iter()) {
            self.types(a, b, up)?;
        }
        Ok(())
    }
    fn pair_aux(&mut self, a: Val, b: Val, up: &Active) -> Result<(), UnificationError> {
        let ta = a.type_ref();
        let tb = b.type_ref();
        let va = ta.and_then(|t| {
            if let Repr::Var(v) = &t.repr {
                Some(v)
            } else {
                None
            }
        });
        let vb = tb.and_then(|t| {
            if let Repr::Var(v) = &t.repr {
                Some(v)
            } else {
                None
            }
        });
        if let (Some(x), Some(y)) = (va, vb) {
            if x == y {
                return Ok(());
            }
            let bx = self.bindings.get(x).cloned();
            let by = self.bindings.get(y).cloned();
            return match (bx, by) {
                (Some(x), Some(y)) => self.pair((&x).into(), (&y).into(), Some(up)),
                (Some(x), None) => self.pair((&x).into(), b, Some(up)),
                (None, Some(y)) => self.pair(a, (&y).into(), Some(up)),
                (None, None) => {
                    self.bindings.insert(x.clone(), a.to_owned());
                    self.bindings.insert(y.clone(), a.to_owned());
                    Ok(())
                }
            };
        }
        if let Some(id) = va {
            if let Some(bound) = self.bindings.get(id).cloned() {
                return self.pair((&bound).into(), b, Some(up));
            }
            self.bindings.insert(id.clone(), b.to_owned());
            return Ok(());
        }
        if let Some(id) = vb {
            if let Some(bound) = self.bindings.get(id).cloned() {
                return self.pair((&bound).into(), a, Some(up));
            }
            self.bindings.insert(id.clone(), a.to_owned());
            return Ok(());
        }
        if let (Some(ta), Some(tb)) = (ta, tb) {
            let sa = ta.direct_slots();
            let sb = tb.direct_slots();
            if let (Some(fa), Some(fb)) = (
                sa.and_then(|s| s.functions()),
                sb.and_then(|s| s.functions()),
            ) {
                self.types(&fa.domain, &fb.domain, up)?;
                return self.types(&fa.range, &fb.range, up);
            }
            if let (Repr::Nominal(na), Repr::Nominal(nb)) = (&ta.repr, &tb.repr) {
                return if na.name == nb.name {
                    self.types(&na.inner, &nb.inner, up)
                } else {
                    Err(Self::mismatch(a, b))
                };
            }
            if let (Repr::NominalSet(na), Repr::NominalSet(nb)) = (&ta.repr, &tb.repr)
                && let (Some(ha), Some(hb)) = (na.nominals.first(), nb.nominals.first())
            {
                self.types(&ha.as_type(), &hb.as_type(), up)?;
                if na.nominals.len() == 1 && nb.nominals.len() == 1 {
                    return self.types(&na.structural, &nb.structural, up);
                }
                return self.types(
                    &NominalSet::raw(na.nominals[1..].to_vec(), *na.structural.clone()),
                    &NominalSet::raw(nb.nominals[1..].to_vec(), *nb.structural.clone()),
                    up,
                );
            }
        }
        if ta.is_some_and(|t| matches!(t.repr, Repr::Nominal(_))) && b.is_nominal_set() {
            return self.pair(b, a, Some(up));
        }
        if a.is_nominal_set()
            && tb.is_some_and(|t| matches!(t.repr, Repr::Nominal(_)))
        {
            let malformed = UnificationValue::MalformedNominalSet {
                nominal: tb.unwrap().clone(),
                structural: Type::none(),
            };
            return self.pair(a, Val::Other(&malformed), Some(up));
        }
        if let Some(Repr::NominalSet(n)) = ta.map(|t| &t.repr)
            && let Some(head) = n.nominals.first()
        {
            self.pair(
                Val::Type(&head.inner.join(&n.structural)?),
                b,
                Some(up),
            )?;
            if n.nominals.len() == 1 {
                return Ok(());
            }
            let rest = NominalSet::raw(n.nominals[1..].to_vec(), *n.structural.clone());
            return self.pair(Val::Type(&rest), b, Some(up));
        }
        if b.is_nominal_set() {
            return self.pair(b, a, Some(up));
        }
        if let Some(Repr::Nominal(n)) = ta.map(|t| &t.repr) {
            return self.pair(Val::Type(&n.inner), b, Some(up));
        }
        if tb.is_some_and(|t| matches!(t.repr, Repr::Nominal(_))) {
            return self.pair(b, a, Some(up));
        }
        if let (Some(ta), Some(tb)) = (ta, tb) {
            let sa = ta.direct_slots();
            let sb = tb.direct_slots();
            if let (
                Some(Lists::List {
                    elements: ea,
                    tail: xa,
                    nonempty: na,
                }),
                Some(Lists::List {
                    elements: eb,
                    tail: xb,
                    nonempty: nb,
                }),
            ) = (sa.and_then(|s| s.list()), sb.and_then(|s| s.list()))
                && na == nb
            {
                self.types(ea, eb, up)?;
                return self.types(xa, xb, up);
            }
            if let (Repr::Product(pa), Repr::Product(pb)) = (&ta.repr, &tb.repr) {
                return self.lists(pa, pb, up);
            }
            if let (Some(Tuples::Set(ga)), Some(Tuples::Set(gb))) =
                (sa.and_then(|s| s.tuple()), sb.and_then(|s| s.tuple()))
            {
                let direct_a = ga.len() == 1 && ga.values().next().unwrap().len() == 1;
                let direct_b = gb.len() == 1 && gb.values().next().unwrap().len() == 1;
                if direct_a && direct_b && ga.keys().next() == gb.keys().next() {
                    return self.lists(
                        &ga.values().next().unwrap()[0].elements,
                        &gb.values().next().unwrap()[0].elements,
                        up,
                    );
                }
                if !direct_a && !direct_b {
                    let types = |g: &BTreeMap<usize, Vec<TupleVariant>>| {
                        g.iter()
                            .flat_map(|(arity, vs)| {
                                vs.iter().map(move |v| {
                                    Type::from_tuples(Some(Tuples::Set(BTreeMap::from([(
                                        *arity,
                                        vec![v.clone()],
                                    )]))))
                                })
                            })
                            .collect::<Vec<_>>()
                    };
                    return match self.lists(&types(ga), &types(gb), up) {
                        Err(UnificationError::NativeNonTerminating) => {
                            Err(UnificationError::NativeNonTerminating)
                        }
                        Err(_) => Err(Self::mismatch(a, b)),
                        ok => ok,
                    };
                }
                if ga.len() == 1 && gb.len() == 1 && ga.keys().next() == gb.keys().next() {
                    let sup = |vs: &[TupleVariant]| -> Result<Vec<Type>, LatticeError> {
                        let mut e = vs[0].elements.clone();
                        for v in &vs[1..] {
                            for (a, b) in e.iter_mut().zip(&v.elements) {
                                *a = a.join(b)?;
                            }
                        }
                        Ok(e)
                    };
                    return self.lists(
                        &sup(ga.values().next().unwrap())?,
                        &sup(gb.values().next().unwrap())?,
                        up,
                    );
                }
            }
            if let (Some(ma), Some(mb)) = (sa.and_then(|s| s.maps()), sb.and_then(|s| s.maps())) {
                self.types(&ma.default_key, &mb.default_key, up)?;
                self.types(&ma.default_value, &mb.default_value, up)?;
                for (_, _, x, _, y) in ma.aligned(mb)?.into_iter().rev() {
                    self.types(&x, &y, up)?;
                }
                return Ok(());
            }
        }
        if a == b {
            Ok(())
        } else {
            Err(Self::mismatch(a, b))
        }
    }
}

#[path = "type_api.rs"]
mod type_api;
pub use type_api::{BuiltinType, Known, TypeGetter, TypePredicate, TypeValue};

#[path = "native_order.rs"]
mod native_order;
pub use native_order::NativeValue;

#[cfg(test)]
mod tests {
    use super::*;
    fn corpus() -> Vec<Type> {
        let mut values = vec![
            Type::none(),
            Type::any(),
            Type::all_atoms(),
            Type::all_integers(),
            Type::float(),
            Type::number(),
        ];
        for n in 0..16 {
            values.push(Type::atoms((0..n).map(|x| format!("a{x}"))));
            values.push(Type::integers((0..n).map(BigInt::from)));
        }
        for (a, b) in [
            ("neg_inf", "-1"),
            ("1", "pos_inf"),
            ("0", "255"),
            ("0", "1114111"),
            ("-20", "20"),
        ] {
            values.push(Type::range_strings(a, b).unwrap());
        }
        values.push(Type::integer(BigInt::from(10).pow(100)));
        values.push(Type::atom("ok").join(&Type::all_integers()).unwrap());
        values
    }
    #[test]
    fn checked_constructor_boundaries_preserve_native_and_domain_distinctions() {
        assert_eq!(
            Type::range(Endpoint::PositiveInfinity, Endpoint::Finite(0.into())),
            Err(LatticeError::FunctionClause)
        );
        assert_eq!(
            Type::range(Endpoint::Finite(0.into()), Endpoint::NegativeInfinity),
            Err(LatticeError::FunctionClause)
        );
        assert_eq!(
            Type::range(Endpoint::PositiveInfinity, Endpoint::NegativeInfinity),
            Ok(Type::none())
        );
        let bad = Type::bitstring((-1).into(), 0.into()).unwrap_err();
        assert_eq!(bad.unit, BigInt::from(-1));
        assert_eq!(bad.base, BigInt::zero());
        let raw = Type::native_bitstring((-1).into(), 0.into());
        assert!(matches!(raw, NativeTypeOutcome::Raw(_)));
        assert_eq!(
            raw.to_native_value(),
            NativeValue::Tuple(vec![
                NativeValue::Atom("c".into()),
                NativeValue::Atom("binary".into()),
                NativeValue::Tuple(vec![
                    NativeValue::Integer((-1).into()),
                    NativeValue::Integer((-8).into())
                ]),
                NativeValue::Atom("unknown".into())
            ])
        );
        assert!(matches!(
            Type::native_bitstring(8.into(), 0.into()),
            NativeTypeOutcome::Canonical(_)
        ));
    }

    #[test]
    fn scalar_algebra() {
        let values = corpus();
        for a in &values {
            assert_eq!(a.join(a).unwrap(), *a);
            assert_eq!(a.meet(a).unwrap(), *a);
            assert_eq!(a.join(&Type::none()).unwrap(), *a);
            assert_eq!(a.meet(&Type::any()).unwrap(), *a);
            assert_eq!(Type::from_value(&a.to_value()).unwrap(), *a);
            for b in &values {
                assert_eq!(a.join(b).unwrap(), b.join(a).unwrap());
                assert_eq!(a.meet(b).unwrap(), b.meet(a).unwrap());
                assert_eq!(a.join(&a.meet(b).unwrap()).unwrap(), *a);
                assert_eq!(a.meet(&a.join(b).unwrap()).unwrap(), *a);
                assert!(a.is_subtype(&a.join(b).unwrap()).unwrap());
                assert!(a.meet(b).unwrap().is_subtype(a).unwrap());
            }
        }
        let mut seed = 283u64;
        for _ in 0..20000 {
            let mut next = || {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                &values[(seed >> 32) as usize % values.len()]
            };
            let (a, b, c) = (next(), next(), next());
            assert_eq!(
                a.join(&b.join(c).unwrap()).unwrap(),
                a.join(b).unwrap().join(c).unwrap()
            );
            assert_eq!(
                a.meet(&b.meet(c).unwrap()).unwrap(),
                a.meet(b).unwrap().meet(c).unwrap()
            );
        }
    }
    #[test]
    fn thresholds_and_exceptional_unit() {
        let thirteen = Type::atoms((0..13).map(|x| format!("a{x}")));
        assert_ne!(thirteen, Type::all_atoms());
        assert_eq!(
            thirteen.join(&Type::atom("extra")).unwrap(),
            Type::all_atoms()
        );
        assert_eq!(
            Type::range_strings("0", "12").unwrap(),
            Type::integers((0..13).map(BigInt::from))
        );
        assert_eq!(
            Type::range_strings("0", "13").unwrap(),
            Type::range_strings("0", "255").unwrap()
        );
        assert_eq!(Type::unit().join(&Type::none()).unwrap(), Type::unit());
        assert_eq!(Type::none().join(&Type::unit()).unwrap(), Type::unit());
        assert_eq!(Type::unit().meet(&Type::all_atoms()).unwrap(), Type::unit());
        assert_eq!(Type::unit().meet(&Type::none()).unwrap(), Type::none());
        assert_eq!(Type::none().limit(0).unwrap(), Type::any());
        assert_eq!(Type::unit().limit(0).unwrap(), Type::any());
    }
    #[test]
    fn exact_upstream_subtraction() {
        let a = Type::integers([-2, -1, 0, 1, 2].into_iter().map(BigInt::from));
        assert_eq!(
            a.subtract(&Type::range_strings("neg_inf", "-1").unwrap())
                .unwrap(),
            Type::none()
        );
        assert_eq!(
            a.subtract(&Type::range_strings("1", "pos_inf").unwrap())
                .unwrap(),
            Type::integer(2.into())
        );
    }
    fn structural_corpus() -> Vec<Type> {
        let a = Type::atom("a");
        let b = Type::atom("b");
        let integer = Type::all_integers();
        let mut values = corpus();
        values.extend([
            Type::nil(),
            Type::tuple_any(),
            Type::identifier(None).unwrap(),
        ]);
        for ids in [
            vec![],
            vec!["pid"],
            vec!["port"],
            vec!["reference"],
            vec!["pid", "port", "reference"],
        ] {
            values.push(
                Type::identifier(Some(ids.into_iter().map(str::to_owned).collect())).unwrap(),
            );
        }
        for elements in [Type::any(), a.clone(), integer.clone(), Type::nil()] {
            values.push(Type::proper_list(elements.clone(), false));
            values.push(Type::proper_list(elements.clone(), true));
            values.push(Type::list(
                elements.clone(),
                Type::nil().join(&b).unwrap(),
                false,
            ));
            values.push(Type::list(elements, b.clone(), true));
        }
        for n in 0..10 {
            values.push(Type::tuple_arity(n));
        }
        for tag in [
            a.clone(),
            b.clone(),
            Type::all_atoms(),
            Type::atoms((0..6).map(|n| format!("tag{n}"))),
        ] {
            values.push(Type::tuple(vec![tag, integer.clone()]).unwrap());
        }
        let tagged = Type::tuple(vec![a.clone(), Type::integer(1.into())])
            .unwrap()
            .join(&Type::tuple(vec![b.clone(), Type::integer(2.into())]).unwrap())
            .unwrap();
        values.push(tagged.clone());
        values.push(Type::proper_list(tagged.clone(), false));
        values.push(Type::tuple(vec![tagged, Type::proper_list(integer, false)]).unwrap());
        values
    }
    #[test]
    fn structural_algebra_and_checked_serde() {
        let values = structural_corpus();
        for a in &values {
            let wire = serde_json::to_string(a).unwrap();
            assert_eq!(serde_json::from_str::<Type>(&wire).unwrap(), *a);
            assert_eq!(a.meet(a).unwrap(), *a);
            assert_eq!(a.join(a).unwrap(), *a);
            for b in &values {
                assert_eq!(a.join(b).unwrap(), b.join(a).unwrap());
                assert_eq!(a.meet(b).unwrap(), b.meet(a).unwrap());
                assert_eq!(a.join(&a.meet(b).unwrap()).unwrap(), *a);
                assert_eq!(a.meet(&a.join(b).unwrap()).unwrap(), *a);
                assert!(a.is_subtype(&a.join(b).unwrap()).unwrap());
                assert!(a.meet(b).unwrap().is_subtype(a).unwrap());
            }
            for k in [0, 1, 2, 3, 4] {
                let limited = a.limit(k).unwrap();
                assert!(a.is_subtype(&limited).unwrap());
                assert_eq!(limited.limit(k).unwrap(), limited);
            }
        }
        let mut seed = 28302u64;
        for _ in 0..30000 {
            let mut next = || {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                &values[(seed >> 32) as usize % values.len()]
            };
            let (a, b, c) = (next(), next(), next());
            assert_eq!(
                a.join(&b.join(c).unwrap()).unwrap(),
                a.join(b).unwrap().join(c).unwrap()
            );
            assert_eq!(
                a.meet(&b.meet(c).unwrap()).unwrap(),
                a.meet(b).unwrap().meet(c).unwrap()
            );
        }
    }
    #[test]
    fn tagged_tuple_widening_and_correlation() {
        let build = |n: i32| {
            Type::tuple(vec![Type::atom(format!("tag{n}")), Type::integer(n.into())]).unwrap()
        };
        let five = (0..5)
            .map(build)
            .fold(Type::none(), |a, b| a.join(&b).unwrap());
        assert_eq!(
            five.meet(&Type::tuple(vec![Type::atom("tag1"), Type::integer(2.into())]).unwrap())
                .unwrap(),
            Type::none()
        );
        let six = five.join(&build(5)).unwrap();
        assert_ne!(
            six.meet(&Type::tuple(vec![Type::atom("tag1"), Type::integer(2.into())]).unwrap())
                .unwrap(),
            Type::none()
        );
        let eight = (0..8)
            .map(Type::tuple_arity)
            .fold(Type::none(), |a, b| a.join(&b).unwrap());
        assert_ne!(eight, Type::tuple_any());
        assert_eq!(
            eight.join(&Type::tuple_arity(8)).unwrap(),
            Type::tuple_any()
        );
        assert_eq!(Type::tuple_arity(1025), Type::tuple_any());
        assert_eq!(five.limit(1).unwrap(), Type::tuple_arity(2));
    }
    #[test]
    fn list_termination_and_cons_collapse() {
        let a = Type::atom("a");
        let b = Type::atom("b");
        let integer = Type::all_integers();
        assert_eq!(Type::proper_list(Type::none(), false), Type::nil());
        assert_eq!(Type::proper_list(Type::none(), true), Type::none());
        let proper = Type::proper_list(integer.clone(), false);
        assert_eq!(
            proper.subtract(&Type::nil()).unwrap(),
            Type::proper_list(integer.clone(), true)
        );
        assert_eq!(
            proper.meet(&Type::proper_list(a.clone(), false)).unwrap(),
            Type::nil()
        );
        let improper = Type::list(integer.clone(), b.clone(), true);
        assert_eq!(improper.meet(&proper).unwrap(), Type::none());
        assert_eq!(
            improper.limit(1).unwrap(),
            Type::list(Type::any(), b.clone(), true)
        );
        assert_eq!(
            Type::cons(a.clone(), proper).unwrap(),
            Type::proper_list(a.join(&integer).unwrap(), true)
        );
        assert_eq!(Type::nil().subtract(&improper).unwrap(), Type::nil());
        let mixed = Type::list(integer.clone(), Type::nil(), false)
            .join(&b)
            .unwrap();
        assert_eq!(
            Type::cons(a.clone(), mixed).unwrap(),
            Type::list(
                a.join(&integer).unwrap(),
                Type::nil().join(&b).unwrap(),
                true
            )
        );
    }
    #[test]
    fn marker_subtraction_is_a_typed_error() {
        let marker = Type::opaque_sentinel();
        assert_eq!(
            marker.subtract(&Type::atom("a")),
            Err(LatticeError::OpaqueSentinelSubtraction)
        );
        assert_eq!(
            Type::atom("a").subtract(&marker),
            Err(LatticeError::OpaqueSentinelSubtraction)
        );
        assert_eq!(
            Type::tuple(vec![marker])
                .unwrap()
                .subtract(&Type::tuple(vec![Type::atom("a")]).unwrap()),
            Err(LatticeError::OpaqueSentinelSubtraction)
        );
        assert!(matches!(
            Type::opaque_sentinel().view(),
            TypeView::OpaqueSentinel
        ));
    }
    #[test]
    fn callable_views_and_lattice() {
        let mut values = structural_corpus();
        for (u, b) in [
            (0, 0),
            (0, 8),
            (1, 0),
            (8, 0),
            (8, 8),
            (16, 7),
            (85, 7),
            (6442450944i64, 1456),
        ] {
            values.push(Type::bitstring(u.into(), b.into()).unwrap());
        }
        let bases = [
            Type::any(),
            Type::none(),
            Type::atom("a"),
            Type::all_integers(),
            Type::proper_list(Type::all_atoms(), false),
        ];
        for domain in [
            None,
            Some(vec![]),
            Some(vec![Type::any()]),
            Some(vec![Type::all_integers()]),
            Some(vec![Type::atom("a"), Type::all_integers()]),
        ] {
            for result in &bases {
                values.push(Type::function(domain.clone(), result.clone()));
            }
        }
        values.push(Type::product(vec![]));
        values.push(Type::product(vec![Type::any(), Type::any()]));
        values.push(Type::product(vec![Type::none(), Type::all_integers()]));
        for a in &values {
            assert_eq!(
                serde_json::from_str::<Type>(&serde_json::to_string(a).unwrap()).unwrap(),
                *a
            );
            for b in &values {
                assert_eq!(a.join(b).unwrap(), b.join(a).unwrap());
                assert_eq!(a.meet(b).unwrap(), b.meet(a).unwrap());
                assert_eq!(a.join(&a.meet(b).unwrap()).unwrap(), *a);
                assert_eq!(a.meet(&a.join(b).unwrap()).unwrap(), *a);
            }
        }
        let mut seed = 28303u64;
        for _ in 0..30000 {
            let mut next = || {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                &values[(seed >> 32) as usize % values.len()]
            };
            let (a, b, c) = (next(), next(), next());
            assert_eq!(
                a.join(&b.join(c).unwrap()).unwrap(),
                a.join(b).unwrap().join(c).unwrap()
            );
            assert_eq!(
                a.meet(&b.meet(c).unwrap()).unwrap(),
                a.meet(b).unwrap().meet(c).unwrap()
            );
        }
        let binary = Type::binary();
        let TypeView::Families(slots) = binary.view() else {
            panic!("binary must have slots")
        };
        assert_eq!(*slots.bitstrings.unwrap().unit, BigInt::from(8));
        let function = Type::function(Some(vec![Type::all_integers()]), Type::atom("a"));
        let TypeView::Families(slots) = function.view() else {
            panic!("function must have slots")
        };
        assert_eq!(
            slots.functions.unwrap().domain.unwrap(),
            [Type::all_integers()]
        );
        assert_eq!(*slots.functions.unwrap().range, Type::atom("a"));
        let tuples = Type::tuple(vec![
            Type::atoms(["a", "b"]),
            Type::all_integers(),
        ])
        .unwrap();
        let TypeView::Families(slots) = tuples.view() else {
            panic!("tuple must have slots")
        };
        let TuplesView::Alternatives(alts) = slots.tuples.unwrap() else {
            panic!("expected alternatives")
        };
        assert_eq!(alts.iter().count(), 2);
        assert_eq!(alts.arities().collect::<Vec<_>>(), [2]);
        let var = Type::variable_atom("x");
        assert_eq!(var.meet(&var).unwrap(), Type::any());
        assert!(!var.is_subtype(&var).unwrap());
    }
    #[test]
    fn map_canonical_algebra_transfers_and_checked_errors() {
        let entry = |name: &str, presence, value| MapEntry {
            key: MapKey::Atom(name.into()),
            presence,
            value,
        };
        let mut maps = vec![
            Type::map_any(),
            Type::map(vec![], Type::none(), Type::none()).unwrap(),
        ];
        for name in ["a", "b"] {
            for presence in [Presence::Mandatory, Presence::Optional] {
                for value in [
                    Type::any(),
                    Type::integer(1.into()),
                    Type::atoms(["a", "b"]),
                ] {
                    maps.push(
                        Type::map(
                            vec![entry(name, presence, value)],
                            Type::none(),
                            Type::none(),
                        )
                        .unwrap(),
                    );
                }
            }
        }
        maps.push(
            Type::map(
                vec![entry("a", Presence::Optional, Type::none())],
                Type::all_atoms(),
                Type::any(),
            )
            .unwrap(),
        );
        for a in &maps {
            assert_eq!(Type::from_value(&a.to_value()).unwrap(), *a);
            for b in &maps {
                assert_eq!(a.join(b), b.join(a));
                assert_eq!(a.meet(b), b.meet(a));
                assert_eq!(a.join(&a.meet(b).unwrap()).unwrap(), *a);
                assert_eq!(a.meet(&a.join(b).unwrap()).unwrap(), *a);
            }
        }
        let required = &maps[2];
        assert_eq!(required.map_get(&Type::atom("a")).unwrap(), Type::any());
        assert_eq!(
            required.map_is_key(&Type::atom("a")).unwrap(),
            Type::atom("true")
        );
        assert_eq!(
            required.map_is_key(&Type::unit()).unwrap(),
            Type::atom("false")
        );
        assert_eq!(required.limit(1).unwrap(), Type::map_any());
        let TypeView::Families(slots) = required.view() else {
            panic!()
        };
        let map = slots.maps.unwrap();
        assert_eq!(map.entries.len(), 1);
        assert!(map.default_key.is_none());
        assert!(map.default_value.is_none());
        let marker_map = Type::map(vec![], Type::opaque_sentinel(), Type::any()).unwrap();
        assert_eq!(
            marker_map.join(required),
            Err(LatticeError::OpaqueSentinelSubtraction)
        );
        assert_eq!(
            Type::map(
                vec![entry("a", Presence::Mandatory, Type::any())],
                Type::opaque_sentinel(),
                Type::any()
            ),
            Err(LatticeError::OpaqueSentinelSubtraction)
        );
        assert_eq!(
            Type::map(
                vec![
                    entry("b", Presence::Mandatory, Type::any()),
                    entry("a", Presence::Mandatory, Type::any())
                ],
                Type::none(),
                Type::none()
            ),
            Err(LatticeError::BadArg)
        );
        let mandatory_unit = Type::map(
            vec![entry("a", Presence::Mandatory, Type::unit())],
            Type::none(),
            Type::none(),
        )
        .unwrap();
        assert!(!mandatory_unit.is_none());
    }
    #[test]
    fn nominal_identity_nesting_visibility_and_native_errors() {
        let name = |s: &str| NominalName::Symbol(s.into());
        let alpha = name("alpha");
        let beta = name("beta");
        let gamma = name("gamma");
        let a = Type::nominal(alpha.clone(), Type::all_atoms());
        let b = Type::nominal(beta.clone(), Type::all_atoms());
        assert_eq!(a.meet(&b).unwrap(), Type::none());
        assert_eq!(a.meet(&Type::all_atoms()).unwrap(), a);
        assert_eq!(a.join(&Type::all_atoms()).unwrap(), Type::all_atoms());
        assert_eq!(a.elements().unwrap(), vec![a.clone()]);
        let lhs = Type::nominal(
            alpha.clone(),
            Type::nominal(beta.clone(), Type::nominal(gamma.clone(), Type::any())),
        );
        let rhs = Type::nominal(
            beta.clone(),
            Type::nominal(gamma.clone(), Type::all_atoms()),
        );
        assert_eq!(lhs.meet(&rhs).unwrap(), Type::nominal(alpha.clone(), rhs));
        assert_eq!(
            lhs.meet(&Type::nominal(
                alpha.clone(),
                Type::nominal(gamma, Type::all_atoms())
            ))
            .unwrap(),
            Type::nominal(
                alpha.clone(),
                Type::nominal(
                    beta.clone(),
                    Type::nominal(name("gamma"), Type::all_atoms())
                )
            )
        );
        let opaque = Type::nominal(
            NominalName::Defined {
                module: "defining".into(),
                name: "t".into(),
                arity: 0,
                opacity: Opacity::Opaque,
            },
            Type::atom("a"),
        );
        assert!(opaque.is_opaque("external"));
        assert!(!opaque.is_opaque("defining"));
        assert_eq!(
            Type::atom("a")
                .opacity_conflict(&opaque, "external")
                .unwrap(),
            OpacityConflict::ExpectedOpaque
        );
        assert_eq!(
            opaque
                .opacity_conflict(&Type::atom("a"), "external")
                .unwrap(),
            OpacityConflict::ExpectedTransparent
        );
        assert_eq!(
            Type::atom("a")
                .opacity_conflict(&opaque, "defining")
                .unwrap(),
            OpacityConflict::None
        );
        assert_eq!(opaque.structural().unwrap(), Type::atom("a"));
        let tuple = Type::tuple(vec![opaque.clone(), Type::all_integers()]).unwrap();
        let TypeView::Families(slots) = tuple.view() else {
            panic!()
        };
        let Some(TuplesView::Alternatives(vs)) = slots.tuples else {
            panic!()
        };
        let v = vs.iter().next().unwrap();
        assert_eq!(v.tag, Some("a"));
        assert_eq!(v.elements[0], opaque);
        let set = a.join(&b).unwrap();
        let left = Type::nominal(alpha.clone(), set.clone());
        let right = Type::nominal(beta.clone(), set);
        assert!(matches!(
            left.meet(&right),
            Err(LatticeError::CaseClause(_))
        ));
        let raw = Type::raw_nominal(alpha, Type::none());
        assert_eq!(Type::from_value(&raw.to_value()).unwrap(), raw);
        let command:Value=serde_json::from_str(r##"{"op": "join", "a": {"kind": "nominal", "name": {"symbol": "beta"}, "inner": {"kind": "union", "values": [{"kind": "nominal", "name": {"symbol": "beta"}, "inner": {"kind": "atom"}}, {"kind": "tuple_any"}]}}, "b": {"kind": "union", "values": [{"kind": "nominal", "name": {"module": "mod_b", "name": "t", "arity": 0, "opacity": "opaque"}, "inner": {"kind": "atom"}}, {"kind": "nominal", "name": {"symbol": "alpha"}, "inner": {"kind": "any"}}]}}"##).unwrap();
        let left = Type::from_value(&command["a"]).unwrap();
        let right = Type::from_value(&command["b"]).unwrap();
        for _ in 0..2 {
            assert_eq!(left.join(&right), Err(LatticeError::NativeNonTerminating));
        }
        assert_eq!(a.join(&a).unwrap(), a);
    }
    #[test]
    fn native_solver_records_and_substitution_shortcuts() {
        let none = Type::none();
        let raw=Type::from_value(&json!({"kind":"union_raw","values":[none.to_value(),none.to_value(),none.to_value(),none.to_value(),none.to_value(),none.to_value(),none.to_value(),none.to_value()]})).unwrap();
        assert_ne!(raw, none);
        assert_eq!(raw.limit(1).unwrap(), raw);
        assert_eq!(raw.structural().unwrap(), none);
        assert_eq!(Type::from_value(&raw.to_value()).unwrap(), raw);
        let x = Type::variable_atom("x");
        let bindings = BTreeMap::from([(VariableId::Atom("x".into()), Type::none())]);
        let m = Type::map(
            vec![MapEntry {
                key: MapKey::Atom("a".into()),
                presence: Presence::Mandatory,
                value: x.clone(),
            }],
            none.clone(),
            none.clone(),
        )
        .unwrap();
        assert!(!m.has_var());
        assert!(m.collect_var_names().is_empty());
        assert_eq!(m.substitute(&bindings).unwrap(), m);
        let tuple = Type::tuple(vec![x.clone(), m]).unwrap();
        assert_eq!(tuple.substitute(&bindings).unwrap(), none);
        let list = Type::list(Type::any(), x, false);
        assert_eq!(
            list.substitute(&bindings),
            Err(LatticeError::BadMatch(Box::new(none)))
        );
    }
    #[test]
    fn native_unification_bindings_errors_and_malformed_solver_values() {
        let x = Type::variable_atom("x");
        let y = Type::variable_atom("y");
        assert!(x.unify_bindings(&x).unwrap().is_empty());
        let pair = x.unify_bindings(&y).unwrap();
        assert_eq!(
            pair.get(&VariableId::Atom("x".into())),
            Some(&UnificationValue::Type(x.clone()))
        );
        assert_eq!(
            pair.get(&VariableId::Atom("y".into())),
            Some(&UnificationValue::Type(x.clone()))
        );
        let a = Type::tuple(vec![x.clone(), Type::atom("a")]).unwrap();
        let b = Type::tuple(vec![y.clone(), y.clone()]).unwrap();
        assert_eq!(
            a.unify_bindings(&b),
            Err(UnificationError::NativeNonTerminating)
        );
        let type_ = Type::nominal(NominalName::Symbol("a".into()), x)
            .join(&Type::nominal(NominalName::Symbol("b".into()), y))
            .unwrap();
        let inf = type_
            .meet(&Type::nominal(
                NominalName::Symbol("a".into()),
                Type::atom("ok"),
            ))
            .unwrap();
        let bindings = type_.unify_bindings(&inf).unwrap();
        assert_eq!(bindings.len(), 2);
        assert!(bindings.values().all(|v|matches!(v,UnificationValue::MalformedNominalSet{nominal,structural} if nominal==&inf && structural.is_none())));
        assert!(matches!(
            Type::product(vec![]).unify_bindings(&Type::product(vec![Type::any(), Type::any()])),
            Err(UnificationError::NativeError(LatticeError::FunctionClause))
        ));
    }
    #[test]
    fn rejects_unsupported_families_and_malformed_numbers() {
        {
            let family = "unknown_family";
            assert!(
                Type::from_value(&json!({"kind":family}))
                    .unwrap_err()
                    .contains("unsupported")
            );
        }
        assert!(Type::from_value(&json!({"kind":"integers","values":[1.5]})).is_err());
        assert!(Type::range_strings("pos_inf", "0").is_err());
        assert!(Type::range_strings("0", "neg_inf").is_err());
    }
}

#[path = "runtime_term.rs"]
mod runtime_term;
pub use runtime_term::RuntimeTerm;

#[path = "presentation.rs"]
mod presentation;
pub use presentation::{PresentationContext, PresentationRecordField};

#[cfg(test)]
mod tuple_opacity_tests {
    use super::*;
    #[test]
    fn plain_tuple_meet_ignores_cached_tags_after_opacity_marking() {
        let given = Type::tuple(vec![Type::atom("false")]).unwrap();
        let required = Type::tuple(vec![Type::nominal(
            NominalName::Defined {
                module: "other".into(),
                name: "wrapped".into(),
                arity: 0,
                opacity: Opacity::Opaque,
            },
            Type::atom("true"),
        )])
        .unwrap();
        assert_eq!(
            given.opacity_conflict(&required, "fixture").unwrap(),
            OpacityConflict::ExpectedOpaque
        );
    }
}

#[cfg(test)]
mod family_sharing_tests {
    use super::*;

    fn slots(value: &Type) -> &FamilyStore {
        let Repr::Families(slots) = &value.repr else {
            panic!("expected family")
        };
        slots
    }

    #[test]
    fn cloned_family_shares_storage_and_functional_map_edits_are_isolated() {
        let original = Type::map_types(
            vec![MapTypeEntry {
                key: Type::atom("entry"),
                presence: Presence::Mandatory,
                value: Type::tuple(vec![
                    Type::atom("tag"),
                    Type::list(Type::integer(7.into()), Type::nil(), true),
                ])
                .unwrap(),
            }],
            Type::none(),
            Type::none(),
        )
        .unwrap();
        let before = original.to_value();
        let copy = original.clone();
        assert!(slots(&original).shares_storage(slots(&copy)));
        let edited = copy
            .map_update(Type::atom("entry"), Type::atom("changed"))
            .unwrap();
        assert!(!slots(&original).shares_storage(slots(&edited)));
        assert_eq!(
            edited.map_get(&Type::atom("entry")).unwrap(),
            Type::atom("changed")
        );
        assert_eq!(original.to_value(), before);
        assert_eq!(copy.to_value(), before);
        let removed = edited.map_remove(&Type::atom("entry")).unwrap();
        assert_eq!(
            removed.map_is_key(&Type::atom("entry")).unwrap(),
            Type::atom("false")
        );
        assert_eq!(original.to_value(), before);
    }

    #[test]
    fn cloned_tuple_substitution_owned_extraction_does_not_change_original() {
        let original =
            Type::tuple(vec![Type::atom("tag"), Type::variable_integer(1.into())]).unwrap();
        let copy = original.clone();
        let before = original.to_value();
        let substituted = copy
            .substitute(&BTreeMap::from([(
                VariableId::Integer(1.into()),
                Type::integer(9.into()),
            )]))
            .unwrap();
        assert_eq!(
            substituted,
            Type::tuple(vec![Type::atom("tag"), Type::integer(9.into())]).unwrap()
        );
        assert_eq!(original.to_value(), before);
        assert_eq!(copy.to_value(), before);
        assert_eq!(original.limit(1).unwrap(), Type::tuple_arity(2));
        assert_eq!(original.to_value(), before);
    }

    #[test]
    fn raw_union_record_roundtrip_substitution_and_validation_are_preserved() {
        let tuple = Type::tuple(vec![Type::variable_integer(1.into())]).unwrap();
        let mut values = vec![Type::none().to_value(); 8];
        values[6] = tuple.to_value();
        let wire = json!({"kind":"union_raw", "values": values});
        let original = Type::from_value(&wire).unwrap();
        let copy = original.clone();
        assert!(slots(&original).shares_storage(slots(&copy)));
        assert!(slots(&original).union_record());
        assert_eq!(original.to_value(), wire);
        assert_eq!(
            serde_json::from_str::<Type>(&serde_json::to_string(&original).unwrap()).unwrap(),
            original
        );
        let replaced = copy
            .substitute(&BTreeMap::from([(
                VariableId::Integer(1.into()),
                Type::atom("bound"),
            )]))
            .unwrap();
        assert!(slots(&replaced).union_record());
        let mut expected = vec![Type::none().to_value(); 8];
        expected[6] = Type::tuple(vec![Type::atom("bound")]).unwrap().to_value();
        assert_eq!(
            replaced.to_value(),
            json!({"kind":"union_raw", "values": expected})
        );
        assert_eq!(original.to_value(), wire);
        let mut misplaced = vec![Type::none().to_value(); 8];
        misplaced[0] = tuple.to_value();
        assert_eq!(
            Type::from_union_record(&misplaced).unwrap_err(),
            "native union family is in the wrong slot"
        );
    }
}

#[cfg(test)]
mod borrowed_presence_proof_tests {
    use super::*;
    #[test]
    fn all_raw_family_presence_and_union_flags_preserve_domains() {
        let families = [
            Type::all_atoms(),
            Type::bitstring(1.into(), 0.into()).unwrap(),
            Type::function(None, Type::any()),
            Type::builtin(BuiltinType::Pid).unwrap(),
            Type::proper_list(Type::any(), false),
            Type::number(),
            Type::tuple_any(),
            Type::map_any(),
        ];
        let all = families
            .into_iter()
            .try_fold(Type::none(), |a, b| a.join(&b))
            .unwrap();
        let Repr::Families(prototype) = all.repr.clone() else {
            panic!("family prototype")
        };
        for mask in 0..256 {
            for union_record in [false, true] {
                let mut s = prototype.to_slots();
                if mask & 1 == 0 {
                    s.atoms = None;
                }
                if mask & 2 == 0 {
                    s.bitstrings = None;
                }
                if mask & 4 == 0 {
                    s.functions = None;
                }
                if mask & 8 == 0 {
                    s.identifiers = None;
                }
                if mask & 16 == 0 {
                    s.list = None;
                }
                if mask & 32 == 0 {
                    s.number = None;
                }
                if mask & 64 == 0 {
                    s.tuple = None;
                }
                if mask & 128 == 0 {
                    s.maps = None;
                }
                s.union_record = union_record;
                for domain in [0, 1, 4, 7] {
                    let mut rest = s.clone();
                    let present = match domain {
                        0 => {
                            rest.atoms = None;
                            s.atoms.is_some()
                        }
                        1 => {
                            rest.bitstrings = None;
                            s.bitstrings.is_some()
                        }
                        4 => {
                            rest.list = None;
                            s.list.is_some()
                        }
                        7 => {
                            rest.maps = None;
                            s.maps.is_some()
                        }
                        _ => unreachable!(),
                    };
                    assert_eq!(
                        !s.union_record && rest.empty(),
                        !s.union_record && s.count() == usize::from(present),
                        "mask={mask} union={union_record} domain={domain}"
                    );
                }
                let t = Type::new(Repr::Families(FamilyStore::raw(s.clone())));
                let mut rest = s.clone();
                rest.maps = None;
                let expected = if !union_record && rest.empty() {
                    Ok(s.maps.as_deref())
                } else {
                    Err(LatticeError::FunctionClause)
                };
                assert_eq!(t.map_ref(), expected);
                let mut rest = s.clone();
                rest.bitstrings = None;
                assert_eq!(
                    t.bitstring_ref().is_ok(),
                    !union_record && rest.empty() && s.bitstrings.is_some()
                );
            }
        }
        assert_eq!(Type::none().map_ref(), Ok(None));
        assert_eq!(Type::unit().map_ref(), Ok(None));
    }
}

#[cfg(test)]
mod immutable_transform_tests {
    use super::*;
    #[test]
    fn normalized_leaf_families_share_but_raw_flags_and_empty_slots_normalize() {
        let scalar = [
            Type::all_atoms(),
            Type::bitstring(1.into(), 0.into()).unwrap(),
            Type::builtin(BuiltinType::Pid).unwrap(),
            Type::nil(),
            Type::number(),
            Type::tuple_any(),
            Type::all_atoms().join(&Type::number()).unwrap(),
        ];
        for t in scalar {
            let Repr::Families(s) = &t.repr else { panic!() };
            let transformed = t
                .transform_children(&|_| panic!("scalar leaf called callback"))
                .unwrap();
            let Repr::Families(shared) = &transformed.repr else {
                panic!()
            };
            assert!(s.shares_storage(shared));
            let mut raw = s.to_slots();
            raw.union_record = !raw.union_record;
            let raw = Type::new(Repr::Families(FamilyStore::raw(raw)));
            assert_eq!(
                raw.transform_children(&|_| panic!("raw leaf called callback"))
                    .unwrap(),
                t
            );
        }
        let empty = Type::new(Repr::Families(FamilyStore::raw(Slots::default())));
        assert_eq!(empty.structural().unwrap(), Type::none());
    }
    #[test]
    fn child_callbacks_and_malformed_map_errors_still_execute() {
        let function = Type::function(Some(vec![Type::atom("domain")]), Type::atom("range"));
        let count = std::cell::Cell::new(0);
        assert_eq!(
            function
                .transform_children(&|t| {
                    count.set(count.get() + 1);
                    if count.get() == 2 {
                        Err(LatticeError::BadArg)
                    } else {
                        Ok(t.clone())
                    }
                })
                .unwrap_err(),
            LatticeError::BadArg
        );
        assert_eq!(count.get(), 2);
        let malformed = Type::new(Repr::Families(FamilyStore::raw(Slots {
            maps: Some(Arc::new(Maps {
                entries: vec![
                    MapEntry {
                        key: MapKey::Atom("z".into()),
                        presence: Presence::Mandatory,
                        value: Type::atom("last"),
                    },
                    MapEntry {
                        key: MapKey::Atom("a".into()),
                        presence: Presence::Mandatory,
                        value: Type::atom("first"),
                    },
                ],
                default_key: Box::new(Type::none()),
                default_value: Box::new(Type::none()),
            })),
            ..Slots::default()
        })));
        let log = std::cell::RefCell::new(Vec::new());
        assert_eq!(
            malformed
                .transform_children(&|t| {
                    log.borrow_mut().push(t.clone());
                    Ok(t.clone())
                })
                .unwrap_err(),
            LatticeError::BadArg
        );
        assert_eq!(
            *log.borrow(),
            vec![
                Type::atom("last"),
                Type::atom("first"),
                Type::none(),
                Type::none()
            ]
        );
        // The nominal-free fast path never rebuilds, so only the full
        // algorithm reaches the malformed map.
        assert_eq!(
            Type::any()
                .opacity_conflict_full(&malformed, "external")
                .unwrap_err(),
            LatticeError::BadArg
        );
        assert_eq!(
            Type::atom("same")
                .opacity_conflict(&Type::atom("same"), "external")
                .unwrap(),
            OpacityConflict::None
        );
        assert_eq!(
            Type::atom("a")
                .opacity_conflict(&Type::atom("b"), "external")
                .unwrap(),
            OpacityConflict::None
        );
    }
}

#[cfg(test)]
mod singleton_key_tests {
    use super::*;
    #[test]
    fn borrowed_singleton_keys_preserve_payload_and_native_record_order() {
        let mut keys = vec![MapKey::Nil];
        keys.extend(["", "\0", "a", "é", "😀"].map(|a| MapKey::Atom(a.into())));
        keys.extend(
            [
                "-999999999999999999999999999999999999",
                "-9223372036854775809",
                "-1",
                "0",
                "1",
                "9223372036854775808",
                "18446744073709551615",
                "999999999999999999999999999999999999",
            ]
            .map(|n| MapKey::Integer(n.parse().unwrap())),
        );
        for key in &keys {
            assert_eq!(MapKey::from_type(&key.as_type()).unwrap(), *key);
        }
        for a in &keys {
            for b in &keys {
                let a = a.as_type();
                let b = b.as_type();
                assert_eq!(
                    a.native_record_order(&b),
                    a.to_native_value().cmp(&b.to_native_value())
                );
            }
        }
    }
    #[test]
    fn raw_union_mixed_and_non_singleton_shapes_keep_original_fallback() {
        let atom = Type::atom("a");
        let Repr::Families(s) = &atom.repr else { panic!() };
        let mut union = s.to_slots();
        union.union_record = true;
        let union = Type::new(Repr::Families(FamilyStore::raw(union)));
        let mut mixed = s.to_slots();
        mixed.number = Some(Arc::new(Numeric::Integer(Integers::Set([1.into()].into()))));
        let mixed = Type::new(Repr::Families(FamilyStore::raw(mixed)));
        let range = Type::new(Repr::Families(FamilyStore::raw(Slots {
            number: Some(Arc::new(Numeric::Integer(Integers::Range(
                Bound::Finite(1.into()),
                Bound::Finite(1.into()),
            )))),
            ..Slots::default()
        })));
        let candidates = [
            union,
            mixed,
            range,
            Type::all_atoms(),
            Type::atoms(["a", "b"]),
            Type::nominal(NominalName::Symbol("wrapper".into()), atom.clone()),
            Type::variable_integer((-7).into()),
            Type::product(vec![atom.clone(), Type::nil()]),
            Type::any(),
        ];
        for t in &candidates {
            assert!(t.singleton_key().is_none());
            assert_eq!(
                MapKey::from_type(t).unwrap_err(),
                "OTP badarg: map exact key must be native singleton atom, integer, or nil"
            );
            for other in candidates.iter().chain(std::iter::once(&atom)) {
                assert_eq!(
                    t.native_record_order(other),
                    t.to_native_value().cmp(&other.to_native_value())
                );
                assert_eq!(
                    other.native_record_order(t),
                    other.to_native_value().cmp(&t.to_native_value())
                );
            }
        }
    }
}

#[cfg(test)]
mod depth_query_tests {
    use super::*;

    #[test]
    fn nil_tail_recognition_preserves_raw_union_and_mixed_family_flags() {
        let nil = Type::nil();
        assert!(matches!(nil.singleton_key(), Some(SingletonKey::Nil)));
        let Repr::Families(original) = &nil.repr else {
            unreachable!()
        };
        let mut raw = original.to_slots();
        raw.union_record = true;
        assert!(!matches!(
            Type::new(Repr::Families(FamilyStore::raw(raw))).singleton_key(),
            Some(SingletonKey::Nil)
        ));
        let mut mixed = original.to_slots();
        mixed.atoms = Some(Arc::new(Atoms::Set(BTreeSet::new())));
        assert!(!matches!(
            Type::new(Repr::Families(FamilyStore::raw(mixed))).singleton_key(),
            Some(SingletonKey::Nil)
        ));
        assert!(Type::proper_list(Type::any(), false).is_limited(1));
        assert!(!Type::proper_list(Type::atom("element"), false).is_limited(1));
    }

    #[test]
    fn map_keys_require_depth_two_without_restricting_empty_maps() {
        assert!(Type::map_any().is_limited(1));
        for key in [
            Type::atom("\0é"),
            Type::nil(),
            Type::integer("-999999999999999999999999999999999999".parse().unwrap()),
        ] {
            let map = Type::map_types(
                vec![MapTypeEntry {
                    key,
                    presence: Presence::Mandatory,
                    value: Type::any(),
                }],
                Type::any(),
                Type::any(),
            )
            .unwrap();
            assert!(!map.is_limited(1));
            assert!(map.is_limited(2));
        }
    }
}

#[cfg(test)]
mod immutable_nominal_payload_tests {
    use super::*;

    #[test]
    fn cloned_nominal_payloads_share_immutable_storage_and_owned_extraction_preserves_values() {
        let source = Type::nominal(NominalName::Symbol("identity".into()), Type::atom("a"));
        let alias = source.clone();
        let (Repr::Nominal(original), Repr::Nominal(shared)) = (&source.repr, &alias.repr) else {
            unreachable!()
        };
        assert!(Arc::ptr_eq(original, shared));
        let set = Type::from_nominal_types(vec![source.clone()], Type::integer(7.into())).unwrap();
        let set_alias = set.clone();
        let (Repr::NominalSet(original_set), Repr::NominalSet(shared_set)) = (&set.repr, &set_alias.repr)
        else {
            unreachable!()
        };
        assert!(Arc::ptr_eq(original_set, shared_set));
        assert_eq!(original_set.nominals[0], **original);
        assert_eq!(source, alias);
        let unique = Type::from_nominal_types(
            vec![Type::nominal(
                NominalName::Symbol("identity".into()),
                Type::atom("a"),
            )],
            Type::integer(7.into()),
        )
        .unwrap();
        assert_eq!(set, unique);
        assert_eq!(set.to_value(), unique.to_value());
        assert_eq!(set.to_native_value(), unique.to_native_value());
    }

    #[test]
    fn nominal_absorption_mutates_only_the_owned_result() {
        let source = Type::nominal(NominalName::Symbol("identity".into()), Type::atom("a"));
        let alias = source.clone();
        let Repr::Nominal(original) = &source.repr else {
            unreachable!()
        };
        let sup = Nominal {
            name: original.name.clone(),
            inner: Box::new(Type::atom("b")),
        };
        let output = Type::nominal_absorb(&[original.as_ref().clone()], &sup).unwrap();
        let Repr::NominalSet(result) = &output.repr else {
            unreachable!()
        };
        assert_eq!(
            *result.nominals[0].inner,
            Type::atoms(["a", "b"])
        );
        assert_eq!(*original.inner, Type::atom("a"));
        assert_eq!(source, alias);
        assert_eq!(source.to_value(), alias.to_value());
    }
}

#[cfg(test)]
mod borrowed_query_contract_tests {
    use super::*;

    #[test]
    fn nominal_set_and_gate_is_distinct_from_variable_collection() {
        for structural_var in [false, true] {
            for member_var in [false, true] {
                let structural = if structural_var {
                    Type::variable_atom("structural")
                } else {
                    Type::any()
                };
                let inner = if member_var {
                    Type::variable_atom("member")
                } else {
                    Type::any()
                };
                let ty = NominalSet::raw(
                    vec![Nominal {
                        name: NominalName::Symbol("n".into()),
                        inner: Box::new(inner),
                    }],
                    structural,
                );
                assert_eq!(ty.has_var(), structural_var && member_var);
                let expected = [(structural_var, "structural"), (member_var, "member")]
                    .into_iter()
                    .filter(|(present, _)| *present)
                    .map(|(_, name)| VariableId::Atom(name.into()))
                    .collect();
                assert_eq!(ty.collect_var_names(), expected);
                assert_eq!(ty.clone().collect_var_names(), ty.collect_var_names());
            }
        }
    }

    #[test]
    fn borrowed_queries_preserve_compound_values_and_shared_snapshots() {
        let x = Type::variable_atom("x");
        let variants = [
            Type::nominal(NominalName::Symbol("n".into()), x.clone()),
            Type::product(vec![Type::any(), x.clone()]),
            Type::function(Some(vec![x.clone()]), Type::any()),
            Type::function(Some(vec![Type::any()]), x.clone()),
            Type::list(x.clone(), Type::nil(), false),
            Type::list(Type::any(), x.clone(), false),
            Type::tuple(vec![Type::any(), x.clone()]).unwrap(),
            Type::map(vec![], x.clone(), Type::any()).unwrap(),
            Type::map(vec![], Type::any(), x).unwrap(),
        ];
        for ty in variants {
            let snapshot = ty.clone();
            let wire = ty.to_value();
            assert!(ty.has_var());
            assert_eq!(
                ty.collect_var_names(),
                BTreeSet::from([VariableId::Atom("x".into())])
            );
            assert_eq!(snapshot, ty);
            assert_eq!(ty.to_value(), wire);
        }
    }
}

#[cfg(test)]
mod rebuild_base_tests {
    use super::*;
    #[test]
    fn rebuild_base_preserves_scalar_payloads_sentinels_and_raw_flags() {
        let source = Slots {
            union_record: true,
            atoms: Some(Arc::new(Atoms::Set(BTreeSet::from(["leaf".into()])))),
            bitstrings: Some(Arc::new(Bitstrings {
                unit: 8.into(),
                base: 3.into(),
            })),
            identifiers: Some(Arc::new(Identifiers::Set(BTreeSet::from(["pid".into()])))),
            number: Some(Arc::new(Numeric::Integer(Integers::Set(BTreeSet::from([
                42.into(),
            ]))))),
            list: Some(Arc::new(Lists::Nil)),
            tuple: Some(Arc::new(Tuples::Any)),
            functions: Some(Arc::new(Functions {
                domain: Box::new(Type::any()),
                range: Box::new(Type::any()),
            })),
            maps: Some(Arc::new(Maps {
                entries: vec![],
                default_key: Box::new(Type::any()),
                default_value: Box::new(Type::none()),
            })),
        };
        let base = FamilyStore::raw(source.clone()).rebuild_base();
        assert!(base.union_record);
        assert_eq!(base.atoms, source.atoms);
        assert_eq!(base.bitstrings, source.bitstrings);
        assert_eq!(base.identifiers, source.identifiers);
        assert_eq!(base.number, source.number);
        assert_eq!(base.list.as_deref(), Some(&Lists::Nil));
        assert_eq!(base.tuple.as_deref(), Some(&Tuples::Any));
        assert!(base.functions.is_none());
        assert!(base.maps.is_none());
    }
    #[test]
    fn substitution_still_skips_tail_lookup_after_impossible_head() {
        let t = Type::new(Repr::Families(FamilyStore::raw(Slots {
            list: Some(Arc::new(Lists::List {
                elements: Box::new(Type::variable_atom("head")),
                tail: Box::new(Type::variable_atom("tail")),
                nonempty: true,
            })),
            ..Slots::default()
        })));
        let none = Type::none();
        let trace = std::cell::RefCell::new(Vec::new());
        let result = t
            .substitute_aux(&|id| {
                trace.borrow_mut().push(id.clone());
                match id {
                    VariableId::Atom(name) if name == "head" => Some(&none),
                    _ => panic!("unreachable tail lookup"),
                }
            })
            .unwrap();
        assert_eq!(result, Type::none());
        assert_eq!(*trace.borrow(), vec![VariableId::Atom("head".into())]);
    }
}

#[cfg(test)]
mod family_store_roundtrip_tests {
    use super::*;

    #[test]
    fn raw_family_flags_roundtrip_and_debug_and_equality_remain_transparent() {
        let fixtures = [
            Type::atoms(["a", "b"]),
            Type::binary(),
            Type::function(
                Some(vec![Type::variable_integer(42.into())]),
                Type::all_atoms(),
            ),
            Type::builtin(BuiltinType::Pid).unwrap(),
            Type::proper_list(Type::variable_integer(42.into()), true),
            Type::number(),
            Type::tuple(vec![Type::atom("tag"), Type::all_integers()]).unwrap(),
            Type::map_any()
                .map_put(Type::atom("entry"), Type::all_integers())
                .unwrap(),
        ];
        let mut template = Slots::default();
        for typ in fixtures {
            let Repr::Families(store) = typ.repr.clone() else {
                panic!("fixture is not a family")
            };
            let fields = store.into_slots();
            if fields.atoms.is_some() {
                template.atoms = fields.atoms;
            }
            if fields.bitstrings.is_some() {
                template.bitstrings = fields.bitstrings;
            }
            if fields.functions.is_some() {
                template.functions = fields.functions;
            }
            if fields.identifiers.is_some() {
                template.identifiers = fields.identifiers;
            }
            if fields.list.is_some() {
                template.list = fields.list;
            }
            if fields.number.is_some() {
                template.number = fields.number;
            }
            if fields.tuple.is_some() {
                template.tuple = fields.tuple;
            }
            if fields.maps.is_some() {
                template.maps = fields.maps;
            }
        }
        let mut rows = Vec::new();
        for mask in 0..256 {
            for union_record in [false, true] {
                let mut slots = template.clone();
                slots.union_record = union_record;
                if mask & 1 == 0 {
                    slots.atoms = None;
                }
                if mask & 2 == 0 {
                    slots.bitstrings = None;
                }
                if mask & 4 == 0 {
                    slots.functions = None;
                }
                if mask & 8 == 0 {
                    slots.identifiers = None;
                }
                if mask & 16 == 0 {
                    slots.list = None;
                }
                if mask & 32 == 0 {
                    slots.number = None;
                }
                if mask & 64 == 0 {
                    slots.tuple = None;
                }
                if mask & 128 == 0 {
                    slots.maps = None;
                }
                let store = FamilyStore::raw(slots.clone());
                assert_eq!(store.to_slots(), slots);
                assert_eq!(store.clone().into_slots(), slots);
                assert_eq!(format!("{store:?}"), format!("{slots:?}"));
                rows.push((slots, store));
            }
        }
        for (left_slots, left_store) in &rows {
            for (right_slots, right_store) in &rows {
                assert_eq!(left_store == right_store, left_slots == right_slots);
            }
        }
    }
}

#[cfg(test)]
mod canonical_constructor_tests {
    use super::*;
    fn same_owner(a: &FamilyStore, b: &FamilyStore) -> bool {
        macro_rules! owner { ($($variant:ident),*) => { match (a,b) {
            $((FamilyStore::$variant(a),FamilyStore::$variant(b))=>Arc::ptr_eq(a,b),)*
            _=>false,
        } } }
        owner!(Atoms, Bitstrings, Functions, Identifiers, Lists, Numeric, Tuples, Maps, Union)
    }

    #[test]
    fn all_cached_factories_keep_exact_raw_shapes_debug_and_wire() {
        let keys = [CanonicalType::Atoms,CanonicalType::Integers,CanonicalType::Float,CanonicalType::Number,CanonicalType::Nil,CanonicalType::Tuple,CanonicalType::Map,CanonicalType::Boolean,CanonicalType::True,CanonicalType::False,CanonicalType::Infinity,CanonicalType::Zero,CanonicalType::One,CanonicalType::MinusOne,CanonicalType::Identifier,CanonicalType::Pid,CanonicalType::Port,CanonicalType::Reference,CanonicalType::Bitstring,CanonicalType::Binary,CanonicalType::NonemptyBitstring,CanonicalType::NonemptyBinary,CanonicalType::List,CanonicalType::NonemptyList,CanonicalType::MaybeImproperList,CanonicalType::Cons,CanonicalType::Function,CanonicalType::Byte,CanonicalType::Char,CanonicalType::PositiveInteger,CanonicalType::NonNegativeInteger,CanonicalType::NegativeInteger];
        for key in keys {
            let raw = Type::make_canonical(key);
            let shared = Type::canonical(key);
            assert_eq!(shared,raw);
            assert_eq!(format!("{shared:?}"),format!("{raw:?}"));
            assert_eq!(shared.to_value(),raw.to_value());
            let Repr::Families(a)=&shared.repr else {panic!()};
            let Repr::Families(b)=&Type::canonical(key).repr else {panic!()};
            assert!(same_owner(a,b));
        }
    }
    #[test]
    fn atom_conversion_effect_occurs_once_on_every_cached_call() {
        struct Input<'a>(&'a std::cell::Cell<usize>);
        impl From<Input<'_>> for crate::atom::Atom {
            fn from(input:Input<'_>)->Self {input.0.set(input.0.get()+1);crate::atom::Atom::new("true")}
        }
        let count=std::cell::Cell::new(0);
        for expected in 1..=4 {assert_eq!(Type::atom(Input(&count)),Type::atom("true"));assert_eq!(count.get(),expected);}
    }
    #[test]
    fn malformed_and_noncanonical_inputs_keep_original_boundaries() {
        for (u,b) in [(-1,0),(1,-1),(-8,-2)] {
            assert_eq!(Type::bitstring(u.into(),b.into()),Err(BitstringDomainError{unit:u.into(),base:b.into()}));
        }
        assert_eq!(Type::identifier(Some(vec!["pid".into(),"bad".into(),"worse".into()])),Err("unsupported identifier kind bad".into()));
        assert_ne!(Type::identifier(None).unwrap(),Type::identifier(Some(vec!["pid".into(),"port".into(),"reference".into()])).unwrap());
        assert_eq!(Type::identifier(Some(vec![])).unwrap(),Type::none());
        assert_ne!(Type::tuple_any(),Type::tuple_arity(0));
        assert_ne!(Type::function(None,Type::any()),Type::function(Some(vec![]),Type::any()));
        let raw_nil=Type::new(Repr::Families(FamilyStore::raw(Slots{union_record:true,list:Some(Arc::new(Lists::Nil)),..Slots::default()})));
        let list=Type::list(Type::any(),raw_nil.clone(),false);
        let Repr::Families(slots)=&list.repr else {panic!()};
        let Some(Lists::List{tail,..})=slots.list() else {panic!()};
        assert_eq!(**tail,raw_nil);
        assert_ne!(list,Type::proper_list(Type::any(),false));
        assert_eq!(Type::list(Type::unit(),Type::any(),false),Type::none());
    }
    #[test]
    fn owned_extraction_mutation_cannot_change_cached_owner() {
        let original=Type::nil();
        let Repr::Families(s)=original.repr.clone() else {panic!()};
        let mut owned=s.into_slots();
        owned.list=None;
        owned.atoms=Some(Arc::new(Atoms::All));
        assert_eq!(Type::nil(),original);
        assert_ne!(Type::new(Repr::Families(FamilyStore::raw(owned))),original);
    }
    #[test]
    fn per_thread_owners_remain_equal_and_outlive_thread_cache() {
        let here=Type::nil();
        let there=std::thread::spawn(Type::nil).join().unwrap();
        assert_eq!(here,there);
        let (Repr::Families(a),Repr::Families(b))=(&here.repr,&there.repr)else{panic!()};
        assert!(same_owner(a,b), "interning shares one node across threads");
        assert_eq!(there.to_value(),here.to_value());
    }
}

/// `RUST_DIALYZER_VERIFY_FASTPATH=1` computes the full result next to each
/// fast path and panics on any difference.
pub(crate) fn verify_fast_paths() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RUST_DIALYZER_VERIFY_FASTPATH").is_some_and(|v| v == "1"))
}

#[cfg(test)]
mod cached_info_tests {
    use super::*;
    #[test]
    fn cached_facts_agree_with_walks() {
        let v = Type::variable_integer(1.into());
        let nominal = Type::nominal(NominalName::Symbol("n".into()), Type::tuple(vec![v.clone()]).unwrap());
        let map = Type::map_types(
            vec![MapTypeEntry { key: Type::atom("k"), presence: Presence::Mandatory, value: v.clone() }],
            Type::all_atoms(),
            Type::list(Type::any(), Type::any(), false),
        )
        .unwrap();
        let tagged = Type::tuple(vec![Type::atoms(["a", "b"]), Type::integer(3.into())]).unwrap();
        let deep = Type::list(Type::list(tagged.clone(), Type::atom("end"), true), Type::nil(), false);
        let types = [
            Type::any(),
            Type::none(),
            v.clone(),
            Type::opaque_sentinel(),
            nominal.clone(),
            Type::from_nominal_types(vec![nominal.clone()], v.clone()).unwrap(),
            map,
            tagged,
            deep,
            Type::product(vec![Type::any(), v.clone()]),
            Type::function(Some(vec![v.clone()]), Type::tuple_arity(3)),
            Type::list(Type::any(), Type::list(Type::atom("x"), Type::nil(), false), true),
        ];
        for t in &types {
            assert_eq!(t.has_var(), t.has_var_walk(), "{t:?}");
            assert_eq!(t.contains_nominal(), t.contains_nominal_walk(), "{t:?}");
            for depth in -2..8 {
                assert_eq!(t.is_limited(depth), t.is_limited_walk(depth), "{depth} {t:?}");
            }
        }
    }
}

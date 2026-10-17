use super::*;
use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeValue {
    Integer(BigInt),
    Atom(String),
    Tuple(Vec<NativeValue>),
    #[cfg(otp_domain = "29")]
    Map(BTreeMap<NativeValue, NativeValue>),
    Nil,
    List(Vec<NativeValue>),
}
impl NativeValue {
    fn rank(&self) -> u8 {
        match self {
            Self::Integer(_) => 0,
            Self::Atom(_) => 1,
            Self::Tuple(_) => 2,
            #[cfg(otp_domain = "29")]
            Self::Map(_) => 3,
            Self::Nil => 4,
            Self::List(_) => 5,
        }
    }
    fn atom(s: &str) -> Self {
        Self::Atom(s.into())
    }
    fn list(v: Vec<Self>) -> Self {
        if v.is_empty() {
            Self::Nil
        } else {
            Self::List(v)
        }
    }
    fn tuple(v: Vec<Self>) -> Self {
        Self::Tuple(v)
    }
    fn c(tag: &str, e: Self, q: Self) -> Self {
        Self::tuple(vec![Self::atom("c"), Self::atom(tag), e, q])
    }
    fn unknown(tag: &str, e: Self) -> Self {
        Self::c(tag, e, Self::atom("unknown"))
    }
    pub fn to_value(&self) -> Value {
        match self {
            Self::Integer(n) => json!({"term":"integer","value":n.to_string()}),
            Self::Atom(a) => json!({"term":"atom","value":a}),
            Self::Tuple(t) => {
                json!({"term":"tuple","values":t.iter().map(Self::to_value).collect::<Vec<_>>()})
            }
            Self::Nil => json!({"term":"nil"}),
            #[cfg(otp_domain = "29")]
            Self::Map(entries) => json!({"term":"map","entries":entries.iter().map(|(k,v)|vec![k.to_value(),v.to_value()]).collect::<Vec<_>>()}),
            Self::List(t) => {
                json!({"term":"list","values":t.iter().map(Self::to_value).collect::<Vec<_>>()})
            }
        }
    }
}
impl Ord for NativeValue {
    fn cmp(&self, rhs: &Self) -> Ordering {
        self.rank()
            .cmp(&rhs.rank())
            .then_with(|| match (self, rhs) {
                (Self::Integer(a), Self::Integer(b)) => a.cmp(b),
                (Self::Atom(a), Self::Atom(b)) => a.cmp(b),
                (Self::Tuple(a), Self::Tuple(b)) => a.len().cmp(&b.len()).then_with(|| a.cmp(b)),
                (Self::List(a), Self::List(b)) => a.cmp(b),
                #[cfg(otp_domain = "29")]
                (Self::Map(a), Self::Map(b)) => a.len().cmp(&b.len())
                    .then_with(|| a.keys().cmp(b.keys()))
                    .then_with(|| a.values().cmp(b.values())),
                _ => Ordering::Equal,
            })
    }
}
impl PartialOrd for NativeValue {
    fn partial_cmp(&self, rhs: &Self) -> Option<Ordering> {
        Some(self.cmp(rhs))
    }
}
impl Bound {
    fn native_value(&self) -> NativeValue {
        match self {
            Self::NegativeInfinity => NativeValue::atom("neg_inf"),
            Self::PositiveInfinity => NativeValue::atom("pos_inf"),
            Self::Finite(n) => NativeValue::Integer(n.clone()),
        }
    }
}
impl NominalName {
    fn native_value(&self) -> NativeValue {
        match self {
            Self::Symbol(s) => NativeValue::Atom(s.clone()),
            Self::Defined {
                module,
                name,
                arity,
                opacity,
            } => NativeValue::tuple(vec![
                NativeValue::Atom(module.clone()),
                NativeValue::Atom(name.clone()),
                NativeValue::Integer((*arity).into()),
                NativeValue::atom(match opacity {
                    Opacity::Opaque => "opaque",
                    Opacity::Transparent => "transparent",
                }),
            ]),
        }
    }
}
impl Type {
    /// Native variable records have tag `var`, after every other type tag.
    pub(crate) fn variable_order(&self, rhs: &Self) -> Option<Ordering> {
        let order = match (self.variable_ref(), rhs.variable_ref()) {
            (Some(a), Some(b)) => a.cmp(b),
            (Some(_), None) => Ordering::Greater,
            (None, Some(_)) => Ordering::Less,
            (None, None) => return None,
        };
        if verify_fast_paths() {
            assert_eq!(order, self.native_value().cmp(&rhs.native_value()), "variable record order");
        }
        Some(order)
    }
    pub fn to_native_value(&self) -> NativeValue {
        self.native_value()
    }
    pub fn native_record_order(&self, rhs: &Self) -> Ordering {
        // Equal types have equal native values.
        if self == rhs {
            return Ordering::Equal;
        }
        if let Some(order) = self.variable_order(rhs) {
            return order;
        }
        if let (Some(a), Some(b)) = (self.singleton_key(), rhs.singleton_key()) {
            return a.cmp(&b);
        }
        let order = self.native_order_key().cmp(&rhs.native_order_key());
        if verify_fast_paths() {
            assert_eq!(order, self.native_value().cmp(&rhs.native_value()), "record order key");
        }
        order
    }
    fn native_value(&self) -> NativeValue {
        use NativeValue as N;
        match &self.repr {
            Repr::Any => N::atom("any"),
            Repr::None => N::atom("none"),
            Repr::Unit => N::atom("unit"),
            Repr::Opaque => N::atom("opaque"),
            Repr::Var(id) => N::unknown(
                "var",
                match id {
                    VariableId::Atom(a) => N::Atom(a.clone()),
                    VariableId::Integer(n) => N::Integer(n.to_bigint()),
                },
            ),
            Repr::Product(p) => N::unknown(
                "product",
                N::list(p.iter().map(Self::native_value).collect()),
            ),
            Repr::Nominal(n) => N::unknown(
                "nominal",
                N::tuple(vec![n.name.native_value(), n.inner.native_value()]),
            ),
            Repr::NominalSet(n) => N::unknown(
                "nominal_set",
                N::tuple(vec![
                    N::list(
                        n.nominals
                            .iter()
                            .map(|n| n.as_type().native_value())
                            .collect(),
                    ),
                    n.structural.native_value(),
                ]),
            ),
            Repr::Families(s) => {
                if s.union_record() {
                    return N::unknown(
                        "union",
                        N::list(
                            s.components_with_none()
                                .iter()
                                .map(Self::native_value)
                                .collect(),
                        ),
                    );
                }
                if let Some(a) = s.atoms() {
                    return N::unknown(
                        "atom",
                        match a {
                            Atoms::All => N::atom("any"),
                            Atoms::Set(v) => N::list(v.iter().map(|a| N::Atom(a.to_string())).collect()),
                        },
                    );
                }
                if let Some(b) = s.bitstrings() {
                    return N::unknown(
                        "binary",
                        N::tuple(vec![N::Integer(b.unit.clone()), N::Integer(b.base.clone())]),
                    );
                }
                if let Some(f) = s.functions() {
                    return N::unknown(
                        "function",
                        N::tuple(vec![f.domain.native_value(), f.range.native_value()]),
                    );
                }
                if let Some(i) = s.identifiers() {
                    return N::unknown(
                        "identifier",
                        match i {
                            Identifiers::All => N::atom("any"),
                            Identifiers::Set(v) => {
                                N::list(v.iter().cloned().map(N::Atom).collect())
                            }
                        },
                    );
                }
                if let Some(l) = s.list() {
                    return match l {
                        Lists::Nil => N::unknown("nil", N::Nil),
                        Lists::List {
                            elements,
                            tail,
                            nonempty,
                        } => N::c(
                            "list",
                            N::tuple(vec![elements.native_value(), tail.native_value()]),
                            N::atom(if *nonempty { "nonempty" } else { "unknown" }),
                        ),
                    };
                }
                if let Some(n) = s.number() {
                    let (e, q) = match n {
                        Numeric::Float => (N::atom("any"), "float"),
                        Numeric::Number => (N::atom("any"), "unknown"),
                        Numeric::Integer(i) => (
                            match i {
                                Integers::All => N::atom("any"),
                                Integers::Set(v) => N::tuple(vec![
                                    N::atom("int_set"),
                                    N::list(v.iter().cloned().map(N::Integer).collect()),
                                ]),
                                Integers::Range(a, b) => N::tuple(vec![
                                    N::atom("int_rng"),
                                    a.native_value(),
                                    b.native_value(),
                                ]),
                            },
                            "integer",
                        ),
                    };
                    return N::c("number", e, N::atom(q));
                }
                if let Some(t) = s.tuple() {
                    return match t {
                        Tuples::Any => N::c(
                            "tuple",
                            N::atom("any"),
                            N::tuple(vec![N::atom("any"), N::atom("any")]),
                        ),
                        Tuples::Set(g) => {
                            let variant = |arity: usize, v: &TupleVariant| {
                                N::c(
                                    "tuple",
                                    N::list(v.elements.iter().map(Self::native_value).collect()),
                                    N::tuple(vec![
                                        N::Integer(arity.into()),
                                        v.tag
                                            .as_ref()
                                            .map(|tag| Self::atom(tag).native_value())
                                            .unwrap_or_else(|| N::atom("any")),
                                    ]),
                                )
                            };
                            if g.len() == 1 && g.values().next().unwrap().len() == 1 {
                                variant(*g.keys().next().unwrap(), &g.values().next().unwrap()[0])
                            } else {
                                N::unknown(
                                    "tuple_set",
                                    N::list(
                                        g.iter()
                                            .map(|(arity, vs)| {
                                                N::tuple(vec![
                                                    N::Integer((*arity).into()),
                                                    N::list(
                                                        vs.iter()
                                                            .map(|v| variant(*arity, v))
                                                            .collect(),
                                                    ),
                                                ])
                                            })
                                            .collect(),
                                    ),
                                )
                            }
                        }
                    };
                }
                if let Some(m) = s.maps() {
                    return N::unknown(
                        "map",
                        N::tuple(vec![
                            N::list(
                                m.entries
                                    .iter()
                                    .map(|e| {
                                        N::tuple(vec![
                                            e.key.as_type().native_value(),
                                            N::atom(match e.presence {
                                                Presence::Mandatory => "mandatory",
                                                Presence::Optional => "optional",
                                            }),
                                            e.value.native_value(),
                                        ])
                                    })
                                    .collect(),
                            ),
                            m.default_key.native_value(),
                            m.default_value.native_value(),
                        ]),
                    );
                }
                #[cfg(otp_domain = "29")]
                if let Some(records) = s.view().records {
                    return records_native_value(records);
                }
                unreachable!("direct canonical family is nonempty")
            }
        }
    }
}
impl FamilyStore {
    pub(super) fn components_with_none(&self) -> Vec<Type> {
        vec![
            Type::families(Slots {
                atoms: self.atoms_owner().cloned(),
                ..Slots::default()
            }),
            Type::families(Slots {
                bitstrings: self.bitstrings_owner().cloned(),
                ..Slots::default()
            }),
            Type::families(Slots {
                functions: self.functions_owner().cloned(),
                ..Slots::default()
            }),
            Type::families(Slots {
                identifiers: self.identifiers_owner().cloned(),
                ..Slots::default()
            }),
            Type::families(Slots {
                list: self.list_owner().cloned(),
                ..Slots::default()
            }),
            Type::families(Slots {
                number: self.number_owner().cloned(),
                ..Slots::default()
            }),
            Type::families(Slots {
                tuple: self.tuple_owner().cloned(),
                ..Slots::default()
            }),
            Type::families(Slots {
                maps: self.maps_owner().cloned(),
                ..Slots::default()
            }),
            #[cfg(otp_domain = "29")]
            Type::families(Slots {
                records: self.records_owner().cloned(),
                ..Slots::default()
            }),
        ]
    }
}

#[cfg(otp_domain = "29")]
fn records_native_value(records: RecordsView<'_>) -> NativeValue {
    fn record(r: &NativeRecord) -> NativeValue {
        use NativeValue as N;
        let name = match &r.name {
            NativeRecordName::Any => N::atom("nil"),
            NativeRecordName::Named { module, name } =>
                N::tuple(vec![N::atom(module), N::atom(name)]),
            NativeRecordName::Atom(name) => N::atom(name),
        };
        let fields = r.fields.iter().map(|(key, field)| {
            let value = match field {
                RecordField::Missing => N::atom("missing"),
                RecordField::Present(value) =>
                    N::tuple(vec![N::atom("present"), value.native_value()]),
            };
            (N::atom(key), value)
        }).collect();
        N::unknown("record", N::tuple(vec![name, N::Map(fields)]))
    }
    match records {
        RecordsView::Record(r) => record(r),
        RecordsView::Set(rs) => NativeValue::unknown(
            "record_set", NativeValue::list(rs.iter().map(record).collect())),
    }
}

/// A term's place in the order of `NativeValue`, encoded once as bytes so
/// that comparing two terms is a `memcmp`. The encoding is prefix-free, so
/// encodings concatenate without separators and still compare element by
/// element. Class tags leave room for the classes Erlang orders between
/// them (reference, fun, port, pid; map; bitstring), which no `NativeValue`
/// takes.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeOrderKey(std::sync::Arc<[u8]>);

const KEY_INTEGER: u8 = 0x10;
const KEY_ATOM: u8 = 0x20;
const KEY_TUPLE: u8 = 0x60;
#[cfg(otp_domain = "29")]
const KEY_MAP: u8 = 0x70;
const KEY_NIL: u8 = 0x80;
const KEY_LIST: u8 = 0x90;
const KEY_NEGATIVE: u8 = 0x01;
const KEY_ZERO: u8 = 0x02;
const KEY_POSITIVE: u8 = 0x03;
const KEY_MORE: u8 = 0x01;
const KEY_END: u8 = 0x00;

#[derive(Default)]
struct KeyWriter(Vec<u8>);
impl KeyWriter {
    /// An order-preserving, prefix-free length: one byte below 255, else
    /// 255 then four big-endian bytes. `invert` reverses its order.
    fn length(&mut self, n: usize, invert: bool) {
        let mask = if invert { 0xff } else { 0 };
        if n < 255 {
            self.0.push(n as u8 ^ mask);
        } else {
            let n = u32::try_from(n).expect("order key length fits in 32 bits");
            self.0.push(0xff ^ mask);
            self.0.extend(n.to_be_bytes().map(|b| b ^ mask));
        }
    }
    /// Sign, then the magnitude's length and big-endian bytes (no leading
    /// zeros), all inverted for negative numbers.
    fn magnitude(&mut self, negative: bool, magnitude: &[u8]) {
        self.0.push(KEY_INTEGER);
        if magnitude.is_empty() {
            self.0.push(KEY_ZERO);
            return;
        }
        self.0.push(if negative { KEY_NEGATIVE } else { KEY_POSITIVE });
        self.length(magnitude.len(), negative);
        let mask = if negative { 0xff } else { 0 };
        self.0.extend(magnitude.iter().map(|b| b ^ mask));
    }
    fn small(&mut self, n: i64) {
        let bytes = n.unsigned_abs().to_be_bytes();
        let skip = bytes.iter().take_while(|b| **b == 0).count();
        self.magnitude(n < 0, &bytes[skip..]);
    }
    fn integer(&mut self, n: &BigInt) {
        use num_traits::ToPrimitive;
        match n.to_i64() {
            Some(n) => self.small(n),
            None => {
                let (sign, bytes) = n.to_bytes_be();
                self.magnitude(sign == num_bigint::Sign::Minus, &bytes);
            }
        }
    }
    /// Text bytes with 0 escaped as 0,255 and ended by 0,0, so a proper
    /// prefix orders first, as in byte-wise string order.
    fn atom(&mut self, s: &str) {
        self.0.push(KEY_ATOM);
        for &b in s.as_bytes() {
            self.0.push(b);
            if b == 0 {
                self.0.push(0xff);
            }
        }
        self.0.extend([0, 0]);
    }
    /// Tuples order by size first, then element-wise.
    fn tuple(&mut self, size: usize) {
        self.0.push(KEY_TUPLE);
        self.length(size, false);
    }
    fn nil(&mut self) {
        self.0.push(KEY_NIL);
    }
    /// `NativeValue::list`: an empty list is nil. Each element follows a
    /// continuation byte and the list ends with a smaller one, so a proper
    /// prefix orders first.
    fn list<I: IntoIterator>(&mut self, items: I, mut item: impl FnMut(&mut Self, I::Item)) {
        let mut items = items.into_iter().peekable();
        if items.peek().is_none() {
            return self.nil();
        }
        self.0.push(KEY_LIST);
        for value in items {
            self.0.push(KEY_MORE);
            item(self, value);
        }
        self.0.push(KEY_END);
    }
    /// `NativeValue::c(tag, e, q)`: `{c, Tag, E, Q}`.
    fn c(&mut self, tag: &str, e: impl FnOnce(&mut Self), q: impl FnOnce(&mut Self)) {
        self.tuple(4);
        self.atom("c");
        self.atom(tag);
        e(self);
        q(self);
    }
    fn unknown(&mut self, tag: &str, e: impl FnOnce(&mut Self)) {
        self.c(tag, e, |w| w.atom("unknown"))
    }
    fn finish(self) -> NativeOrderKey {
        NativeOrderKey(self.0.into())
    }
}
impl NativeValue {
    pub fn order_key(&self) -> NativeOrderKey {
        let mut w = KeyWriter::default();
        self.write_key(&mut w);
        w.finish()
    }
    fn write_key(&self, w: &mut KeyWriter) {
        match self {
            Self::Integer(n) => w.integer(n),
            Self::Atom(a) => w.atom(a),
            Self::Tuple(t) => {
                w.tuple(t.len());
                t.iter().for_each(|v| v.write_key(w));
            }
            Self::Nil => w.nil(),
            #[cfg(otp_domain = "29")]
            Self::Map(entries) => {
                w.0.push(KEY_MAP);
                w.length(entries.len(), false);
                for key in entries.keys() { key.write_key(w); }
                for value in entries.values() { value.write_key(w); }
            }
            // Unlike `list`, the constructor keeps an empty `List` apart from nil.
            Self::List(t) => {
                w.0.push(KEY_LIST);
                for v in t {
                    w.0.push(KEY_MORE);
                    v.write_key(w);
                }
                w.0.push(KEY_END);
            }
        }
    }
}
impl Bound {
    fn write_key(&self, w: &mut KeyWriter) {
        match self {
            Self::NegativeInfinity => w.atom("neg_inf"),
            Self::PositiveInfinity => w.atom("pos_inf"),
            Self::Finite(n) => w.integer(n),
        }
    }
}
impl NominalName {
    fn write_key(&self, w: &mut KeyWriter) {
        match self {
            Self::Symbol(s) => w.atom(s),
            Self::Defined {
                module,
                name,
                arity,
                opacity,
            } => {
                w.tuple(4);
                w.atom(module);
                w.atom(name);
                w.integer(&(*arity).into());
                w.atom(match opacity {
                    Opacity::Opaque => "opaque",
                    Opacity::Transparent => "transparent",
                });
            }
        }
    }
}
impl Nominal {
    fn write_key(&self, w: &mut KeyWriter) {
        w.unknown("nominal", |w| {
            w.tuple(2);
            self.name.write_key(w);
            self.inner.write_key(w);
        })
    }
}
impl Type {
    /// The bytes of `to_native_value`'s order, written without building the tree.
    /// Keys are a pure function of the interned node, so each thread keeps
    /// the recent ones by node address.
    pub fn native_order_key(&self) -> NativeOrderKey {
        const BITS: u32 = 16;
        type Slot = Option<(usize, NativeOrderKey)>;
        thread_local! {
            static KEYS: std::cell::RefCell<Box<[Slot]>> =
                std::cell::RefCell::new((0..1 << BITS).map(|_| None).collect());
        }
        let address = self.0 as *const Node as usize;
        let slot = (address.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> (usize::BITS - BITS)) as usize;
        let cached = KEYS.with(|keys| match &keys.borrow()[slot] {
            Some((at, key)) if *at == address => Some(key.clone()),
            _ => None,
        });
        if let Some(key) = cached {
            if verify_fast_paths() {
                assert_eq!(key, self.native_order_key_uncached(), "cached order key: {self:?}");
            }
            return key;
        }
        let key = self.native_order_key_uncached();
        KEYS.with(|keys| keys.borrow_mut()[slot] = Some((address, key.clone())));
        key
    }
    fn native_order_key_uncached(&self) -> NativeOrderKey {
        let mut w = KeyWriter::default();
        self.write_key(&mut w);
        let key = w.finish();
        if verify_fast_paths() {
            assert_eq!(key, self.native_value().order_key(), "order key diverged: {self:?}");
        }
        key
    }
    fn write_key(&self, w: &mut KeyWriter) {
        match &self.repr {
            Repr::Any => w.atom("any"),
            Repr::None => w.atom("none"),
            Repr::Unit => w.atom("unit"),
            Repr::Opaque => w.atom("opaque"),
            Repr::Var(id) => w.unknown("var", |w| match id {
                VariableId::Atom(a) => w.atom(a),
                VariableId::Integer(n) => match n.small() {
                    Some(n) => w.small(n),
                    None => w.integer(&n.to_bigint()),
                },
            }),
            Repr::Product(p) => w.unknown("product", |w| w.list(p.iter(), |w, t| t.write_key(w))),
            Repr::Nominal(n) => n.write_key(w),
            Repr::NominalSet(n) => w.unknown("nominal_set", |w| {
                w.tuple(2);
                w.list(n.nominals.iter(), |w, n| n.write_key(w));
                n.structural.write_key(w);
            }),
            Repr::Families(s) => {
                if s.union_record() {
                    return w.unknown("union", |w| {
                        w.list(s.components_with_none(), |w, t| t.write_key(w))
                    });
                }
                if let Some(a) = s.atoms() {
                    return w.unknown("atom", |w| match a {
                        Atoms::All => w.atom("any"),
                        Atoms::Set(v) => w.list(v.iter(), |w, a| w.atom(a.as_str())),
                    });
                }
                if let Some(b) = s.bitstrings() {
                    return w.unknown("binary", |w| {
                        w.tuple(2);
                        w.integer(&b.unit);
                        w.integer(&b.base);
                    });
                }
                if let Some(f) = s.functions() {
                    return w.unknown("function", |w| {
                        w.tuple(2);
                        f.domain.write_key(w);
                        f.range.write_key(w);
                    });
                }
                if let Some(i) = s.identifiers() {
                    return w.unknown("identifier", |w| match i {
                        Identifiers::All => w.atom("any"),
                        Identifiers::Set(v) => w.list(v.iter(), |w, a| w.atom(a)),
                    });
                }
                if let Some(l) = s.list() {
                    return match l {
                        Lists::Nil => w.unknown("nil", KeyWriter::nil),
                        Lists::List {
                            elements,
                            tail,
                            nonempty,
                        } => w.c(
                            "list",
                            |w| {
                                w.tuple(2);
                                elements.write_key(w);
                                tail.write_key(w);
                            },
                            |w| w.atom(if *nonempty { "nonempty" } else { "unknown" }),
                        ),
                    };
                }
                if let Some(n) = s.number() {
                    let q = match n {
                        Numeric::Float => "float",
                        Numeric::Number => "unknown",
                        Numeric::Integer(_) => "integer",
                    };
                    return w.c(
                        "number",
                        |w| match n {
                            Numeric::Float | Numeric::Number | Numeric::Integer(Integers::All) => {
                                w.atom("any")
                            }
                            Numeric::Integer(Integers::Set(v)) => {
                                w.tuple(2);
                                w.atom("int_set");
                                w.list(v.iter(), KeyWriter::integer);
                            }
                            Numeric::Integer(Integers::Range(a, b)) => {
                                w.tuple(3);
                                w.atom("int_rng");
                                a.write_key(w);
                                b.write_key(w);
                            }
                        },
                        |w| w.atom(q),
                    );
                }
                if let Some(t) = s.tuple() {
                    return match t {
                        Tuples::Any => w.c(
                            "tuple",
                            |w| w.atom("any"),
                            |w| {
                                w.tuple(2);
                                w.atom("any");
                                w.atom("any");
                            },
                        ),
                        Tuples::Set(g) => {
                            let variant = |w: &mut KeyWriter, arity: usize, v: &TupleVariant| {
                                w.c(
                                    "tuple",
                                    |w| w.list(v.elements.iter(), |w, t| t.write_key(w)),
                                    |w| {
                                        w.tuple(2);
                                        w.integer(&arity.into());
                                        match &v.tag {
                                            Some(tag) => Self::atom(tag.as_str()).write_key(w),
                                            None => w.atom("any"),
                                        }
                                    },
                                )
                            };
                            if g.len() == 1 && g.values().next().unwrap().len() == 1 {
                                variant(w, *g.keys().next().unwrap(), &g.values().next().unwrap()[0])
                            } else {
                                w.unknown("tuple_set", |w| {
                                    w.list(g.iter(), |w, (arity, vs)| {
                                        w.tuple(2);
                                        w.integer(&(*arity).into());
                                        w.list(vs.iter(), |w, v| variant(w, *arity, v));
                                    })
                                })
                            }
                        }
                    };
                }
                if let Some(m) = s.maps() {
                    return w.unknown("map", |w| {
                        w.tuple(3);
                        w.list(m.entries.iter(), |w, e| {
                            w.tuple(3);
                            e.key.as_type().write_key(w);
                            w.atom(match e.presence {
                                Presence::Mandatory => "mandatory",
                                Presence::Optional => "optional",
                            });
                            e.value.write_key(w);
                        });
                        m.default_key.write_key(w);
                        m.default_value.write_key(w);
                    });
                }
                #[cfg(otp_domain = "29")]
                if let Some(records) = s.view().records {
                    return records_native_value(records).write_key(w);
                }
                unreachable!("direct canonical family is nonempty")
            }
        }
    }
}

#[cfg(test)]
mod order_key_tests {
    use super::*;
    use NativeValue as N;

    fn ints() -> Vec<BigInt> {
        let mut v: Vec<BigInt> = [
            0i64, 1, -1, 2, -2, 127, 128, -128, 255, 256, -255, -256, 65535, 65536,
            i64::MAX, i64::MIN, i64::MAX - 1, i64::MIN + 1,
        ]
        .into_iter()
        .map(BigInt::from)
        .collect();
        let big: BigInt = BigInt::from(1) << 70usize;
        let huge: BigInt = BigInt::from(1) << 2100usize;
        for b in [big, huge] {
            let one = BigInt::from(1);
            v.extend([b.clone(), -b.clone(), &b + &one, -(&b + &one), &b - &one]);
        }
        v.push(BigInt::from(i64::MAX) + 1);
        v.push(BigInt::from(i64::MIN) - 1);
        v
    }

    fn shapes() -> Vec<NativeValue> {
        let atoms = ["", "a", "ab", "b", "a\0", "a\0b", "\0", "\u{e9}", "z", "any"];
        let mut base: Vec<NativeValue> = ints().into_iter().map(N::Integer).collect();
        base.extend(atoms.iter().map(|a| N::Atom((*a).into())));
        base.extend([N::Nil, N::List(vec![]), N::Tuple(vec![])]);
        let mut all = base.clone();
        let small: Vec<_> = base.iter().step_by(3).cloned().collect();
        for a in &small {
            all.push(N::List(vec![a.clone()]));
            all.push(N::Tuple(vec![a.clone()]));
            for b in &small {
                all.push(N::List(vec![a.clone(), b.clone()]));
                all.push(N::Tuple(vec![a.clone(), b.clone()]));
                all.push(N::Tuple(vec![N::List(vec![a.clone()]), b.clone()]));
                all.push(N::List(vec![N::Tuple(vec![a.clone(), b.clone()]), N::Nil]));
            }
        }
        all.push(N::Tuple((0..300).map(|i| N::Integer(i.into())).collect()));
        all.push(N::Tuple((0..254).map(|i| N::Integer(i.into())).collect()));
        all.push(N::Tuple((0..255).map(|i| N::Integer(i.into())).collect()));
        all.push(N::List((0..300).map(|i| N::Atom(format!("x{i}"))).collect()));
        all
    }

    #[test]
    fn byte_keys_order_like_native_values() {
        let all = shapes();
        let keys: Vec<_> = all.iter().map(NativeValue::order_key).collect();
        for (a, ka) in all.iter().zip(&keys) {
            for (b, kb) in all.iter().zip(&keys) {
                assert_eq!(ka.cmp(kb), a.cmp(b), "{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn type_keys_match_native_value_keys() {
        let mut types = vec![
            Type::any(),
            Type::nil(),
            Type::atom("ok"),
            Type::atoms(["a", "b", "c"]),
            Type::integer((-5).into()),
            Type::integer(BigInt::from(1) << 80usize),
            Type::integer(-(BigInt::from(1) << 80usize)),
            Type::variable_integer(7.into()),
            Type::variable_integer(-(BigInt::from(1) << 90usize)),
            Type::variable_atom("X"),
        ];
        let tuples: Vec<_> = types
            .iter()
            .flat_map(|a| types.iter().map(move |b| Type::tuple(vec![a.clone(), b.clone()]).unwrap()))
            .collect();
        types.extend(tuples);
        types.push(Type::function(Some(vec![Type::atom("x")]), Type::nil()));
        let all: Vec<_> = types.iter().map(|t| (t.native_order_key(), t.native_value())).collect();
        for (typ_a, (ka, a)) in types.iter().zip(&all) {
            assert_eq!(*ka, a.order_key());
            for (typ_b, (kb, b)) in types.iter().zip(&all) {
                assert_eq!(ka.cmp(kb), a.cmp(b));
                assert_eq!(typ_a.native_record_order(typ_b), a.cmp(b));
                if let Some(order) = typ_a.variable_order(typ_b) {
                    assert_eq!(order, a.cmp(b));
                }
            }
        }
    }
}

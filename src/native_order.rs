use super::*;
use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeValue {
    Integer(BigInt),
    Atom(String),
    Tuple(Vec<NativeValue>),
    Nil,
    List(Vec<NativeValue>),
}
impl NativeValue {
    fn rank(&self) -> u8 {
        match self {
            Self::Integer(_) => 0,
            Self::Atom(_) => 1,
            Self::Tuple(_) => 2,
            Self::Nil => 3,
            Self::List(_) => 4,
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
    pub fn to_native_value(&self) -> NativeValue {
        self.native_value()
    }
    pub fn native_record_order(&self, rhs: &Self) -> Ordering {
        if let (Some(a), Some(b)) = (self.singleton_key(), rhs.singleton_key()) {
            return a.cmp(&b);
        }
        self.native_value().cmp(&rhs.native_value())
    }
    fn native_value(&self) -> NativeValue {
        use NativeValue as N;
        match &self.0 {
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
                if s.union_record {
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
                if let Some(a) = &s.atoms {
                    return N::unknown(
                        "atom",
                        match a {
                            Atoms::All => N::atom("any"),
                            Atoms::Set(v) => N::list(v.iter().cloned().map(N::Atom).collect()),
                        },
                    );
                }
                if let Some(b) = &s.bitstrings {
                    return N::unknown(
                        "binary",
                        N::tuple(vec![N::Integer(b.unit.clone()), N::Integer(b.base.clone())]),
                    );
                }
                if let Some(f) = &s.functions {
                    return N::unknown(
                        "function",
                        N::tuple(vec![f.domain.native_value(), f.range.native_value()]),
                    );
                }
                if let Some(i) = &s.identifiers {
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
                if let Some(l) = &s.list {
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
                if let Some(n) = &s.number {
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
                if let Some(t) = &s.tuple {
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
                if let Some(m) = &s.maps {
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
                unreachable!("direct canonical family is nonempty")
            }
        }
    }
}
impl Slots {
    pub(super) fn components_with_none(&self) -> Vec<Type> {
        vec![
            Type::families(Slots {
                atoms: self.atoms.clone(),
                ..Slots::default()
            }),
            Type::families(Slots {
                bitstrings: self.bitstrings.clone(),
                ..Slots::default()
            }),
            Type::families(Slots {
                functions: self.functions.clone(),
                ..Slots::default()
            }),
            Type::families(Slots {
                identifiers: self.identifiers.clone(),
                ..Slots::default()
            }),
            Type::families(Slots {
                list: self.list.clone(),
                ..Slots::default()
            }),
            Type::families(Slots {
                number: self.number.clone(),
                ..Slots::default()
            }),
            Type::families(Slots {
                tuple: self.tuple.clone(),
                ..Slots::default()
            }),
            Type::families(Slots {
                maps: self.maps.clone(),
                ..Slots::default()
            }),
        ]
    }
}

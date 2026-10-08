use super::*;

#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypePredicate {
    Any,
    None,
    Unit,
    Impossible,
    NoneOrUnit,
    Atom,
    Boolean,
    Binary,
    Bitstring,
    Function,
    Identifier,
    Pid,
    Port,
    Reference,
    Number,
    Float,
    Integer,
    Byte,
    Char,
    Cons,
    Nil,
    List,
    MaybeImproperList,
    String,
    Map,
    Tuple,
    NonNegativeInteger,
    Variable,
    Singleton,
    Opaque,
}
#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinType {
    Any,
    None,
    Unit,
    Atom,
    Binary,
    NonemptyBinary,
    Bitstring,
    NonemptyBitstring,
    Boolean,
    Byte,
    Char,
    Function,
    Identifier,
    Pid,
    Port,
    Reference,
    Number,
    Float,
    Integer,
    PositiveInteger,
    NonNegativeInteger,
    NegativeInteger,
    Fixnum,
    PositiveFixnum,
    NonNegativeFixnum,
    Cons,
    Nil,
    List,
    NonemptyList,
    String,
    NonemptyString,
    MaybeImproperList,
    Map,
    Tuple,
    Arity,
    Mfa,
    Module,
    Node,
    Iolist,
    Iodata,
    BitstringList,
    Timeout,
}
impl Type {
    pub fn builtin(kind: BuiltinType) -> Result<Self, LatticeError> {
        use BuiltinType::*;
        Ok(match kind {
            Any => Self::any(),
            None => Self::none(),
            Unit => Self::unit(),
            Atom | Module | Node => Self::all_atoms(),
            Binary => Self::bitstring(8.into(), 0.into()).map_err(|_| LatticeError::BadArg)?,
            NonemptyBinary => {
                Self::bitstring(8.into(), 8.into()).map_err(|_| LatticeError::BadArg)?
            }
            Bitstring => Self::bitstring(1.into(), 0.into()).map_err(|_| LatticeError::BadArg)?,
            NonemptyBitstring => {
                Self::bitstring(1.into(), 1.into()).map_err(|_| LatticeError::BadArg)?
            }
            Boolean => Self::canonical(CanonicalType::Boolean),
            Byte | Arity => {
                Self::canonical(CanonicalType::Byte)
            }
            Char => Self::canonical(CanonicalType::Char),
            Function => Self::function(std::option::Option::None, Self::any()),
            Identifier => {
                Self::canonical(CanonicalType::Identifier)
            }
            Pid => Self::canonical(CanonicalType::Pid),
            Port => {
                Self::canonical(CanonicalType::Port)
            }
            Reference => Self::canonical(CanonicalType::Reference),
            Number => Self::number(),
            Float => Self::float(),
            Integer | Fixnum => Self::all_integers(),
            PositiveInteger | PositiveFixnum => {
                Self::canonical(CanonicalType::PositiveInteger)
            }
            NonNegativeInteger | NonNegativeFixnum => {
                Self::canonical(CanonicalType::NonNegativeInteger)
            }
            NegativeInteger => {
                Self::canonical(CanonicalType::NegativeInteger)
            }
            Cons => Self::list(Self::any(), Self::any(), true),
            Nil => Self::nil(),
            List => Self::proper_list(Self::any(), false),
            NonemptyList => Self::proper_list(Self::any(), true),
            String => Self::proper_list(Self::builtin(Char)?, false),
            NonemptyString => Self::proper_list(Self::builtin(Char)?, true),
            MaybeImproperList => Self::list(Self::any(), Self::any(), false),
            Map => Self::map_any(),
            Tuple => Self::tuple_any(),
            Mfa => Self::tuple(vec![
                Self::all_atoms(),
                Self::all_atoms(),
                Self::builtin(Arity)?,
            ])?,
            Iolist | BitstringList => {
                let terminal = Self::builtin(if matches!(kind, Iolist) {
                    Binary
                } else {
                    Bitstring
                })?;
                let tail = terminal.join(&Self::nil())?;
                let nested = Self::list(Self::any(), tail.clone(), false);
                Self::list(
                    nested.join(&terminal)?.join(&Self::builtin(Byte)?)?,
                    tail,
                    false,
                )
            }
            Iodata => Self::builtin(Iolist)?.join(&Self::builtin(Binary)?)?,
            Timeout => Self::builtin(NonNegativeInteger)?.join(&Self::atom("infinity"))?,
        })
    }
    pub fn predicate(&self, predicate: TypePredicate) -> Result<bool, LatticeError> {
        use TypePredicate::*;
        let direct = matches!(
            predicate,
            None | NoneOrUnit
                | Unit
                | Impossible
                | Identifier
                | Byte
                | Char
                | List
                | String
                | NonNegativeInteger
                | Variable
                | Opaque
        );
        let structural = if direct {
            std::option::Option::None
        } else {
            Some(self.shallow_structural()?)
        };
        let t = structural.as_ref().unwrap_or(self);
        let s = t.direct_slots();
        let number = s.and_then(|s| s.number());
        let list = s.and_then(|s| s.list());
        Ok(match predicate {
            Any => t.is_any(),
            None => t.is_none(),
            Unit => t.is_unit(),
            Impossible | NoneOrUnit => t.is_impossible(),
            Atom => s.is_some_and(|s| s.atoms().is_some()),
            Boolean => {
                matches!(s.and_then(|s|s.atoms()),Some(Atoms::Set(a)) if !a.is_empty()&&a.iter().all(|v|v=="false"||v=="true"))
            }
            Binary => {
                matches!(s.and_then(|s|s.bitstrings()),Some(b) if (&b.unit%8u8).is_zero()&&(&b.base%8u8).is_zero())
            }
            Bitstring => s.is_some_and(|s| s.bitstrings().is_some()),
            Function => s.is_some_and(|s| s.functions().is_some()),
            Identifier => s.is_some_and(|s| s.identifiers().is_some()),
            Pid | Port | Reference => {
                let name = match predicate {
                    Pid => "pid",
                    Port => "port",
                    _ => "reference",
                };
                matches!(s.and_then(|s|s.identifiers()),Some(Identifiers::Set(set))if set.len()==1&&set.contains(name))
            }
            Number => number.is_some(),
            Float => matches!(number, Some(Numeric::Float)),
            Integer => matches!(number, Some(Numeric::Integer(_))),
            Byte | Char => {
                let max = BigInt::from(if matches!(predicate, Byte) {
                    255
                } else {
                    0x10ffff
                });
                match number {
                    Some(Numeric::Integer(Integers::Set(v))) => {
                        v.first().is_some_and(|n| n >= &BigInt::zero())
                            && v.last().is_some_and(|n| n <= &max)
                    }
                    Some(Numeric::Integer(Integers::Range(Bound::Finite(a), Bound::Finite(b)))) => {
                        a >= &BigInt::zero() && b <= &max
                    }
                    _ => false,
                }
            }
            Cons => matches!(list, Some(Lists::List { nonempty: true, .. })),
            Nil => matches!(list, Some(Lists::Nil)),
            List => {
                matches!(list, Some(Lists::Nil))
                    || matches!(list,Some(Lists::List{tail,..})if tail.as_ref()==&Self::nil())
            }
            MaybeImproperList => list.is_some(),
            String => {
                matches!(list,Some(Lists::List{elements,tail,..})if tail.as_ref()==&Self::nil()&&elements.predicate(Char)?)
            }
            Map => s.is_some_and(|s| s.maps().is_some()),
            Tuple => s.is_some_and(|s| s.tuple().is_some()),
            NonNegativeInteger => {
                matches!(number, Some(Numeric::Integer(_)))
                    && t.is_subtype(&Self::builtin(BuiltinType::NonNegativeInteger)?)?
            }
            Variable => matches!(t.0, Repr::Var(_)),
            Singleton => {
                matches!(list, Some(Lists::Nil))
                    || matches!(s.and_then(|s|s.atoms()),Some(Atoms::Set(v))if v.len()==1)
                    || matches!(number,Some(Numeric::Integer(Integers::Set(v)))if v.len()==1)
                    || matches!(number,Some(Numeric::Integer(Integers::Range(a,b)))if a==b)
            }
            Opaque => matches!(
                &t.0,
                Repr::Nominal(n) if matches!(&n.name, NominalName::Defined {opacity: Opacity::Opaque, ..})
            ),
        })
    }
    pub fn is_atom_literal(&self, name: &str) -> Result<bool, LatticeError> {
        let t = self.shallow_structural()?;
        Ok(
            matches!(t.direct_slots().and_then(|s|s.atoms()),Some(Atoms::Set(v))if v.len()==1&&v.iter().any(|a|*a==name)),
        )
    }
    pub fn same_opaque(&self, rhs: &Self) -> Result<bool, LatticeError> {
        match (&self.0, &rhs.0) {
            (Repr::Nominal(a), Repr::Nominal(b))
                if matches!(
                    &a.name,
                    NominalName::Defined {
                        opacity: Opacity::Opaque,
                        ..
                    }
                ) && a.name == b.name =>
            {
                Ok(true)
            }
            (Repr::Nominal(a), Repr::Nominal(b))
                if matches!(
                    &a.name,
                    NominalName::Defined {
                        opacity: Opacity::Opaque,
                        ..
                    }
                ) && matches!(
                    &b.name,
                    NominalName::Defined {
                        opacity: Opacity::Opaque,
                        ..
                    }
                ) =>
            {
                Ok(false)
            }
            _ => Err(LatticeError::FunctionClause),
        }
    }
}

#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeGetter {
    AtomValues,
    NumberValues,
    NumberMin,
    NumberMax,
    BitstringUnit,
    BitstringBase,
    FunctionArgs,
    FunctionArity,
    FunctionRange,
    ConsHead,
    ConsTail,
    ListElements,
    ListTermination,
    TupleArgs,
    TupleSize,
    TupleSizes,
    TupleSubtypes,
    MapEntries,
    MapDefaultKey,
    MapDefaultValue,
    VariableName,
    NominalModule,
    ToTypeList,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Known<T> {
    Unknown,
    Known(T),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeValue {
    Type(Type),
    Types(Known<Vec<Type>>),
    Atoms(Known<Vec<String>>),
    Integers(Known<Vec<BigInt>>),
    Arity(Known<usize>),
    Arities(Known<Vec<usize>>),
    Endpoint(Endpoint),
    BitLength(BigInt),
    Entries(Vec<MapEntry>),
    Variable(VariableId),
    Module(String),
}
impl TypeValue {
    pub fn to_value(&self) -> Value {
        let known = |v: Option<Value>| v.unwrap_or(Value::Null);
        match self {
            Self::Type(t) => json!({"kind":"type","value":t.to_value()}),
            Self::Types(t) => {
                json!({"kind":"types","values":known(match t{Known::Unknown=>None,Known::Known(v)=>Some(json!(v.iter().map(Type::to_value).collect::<Vec<_>>()))})})
            }
            Self::Atoms(t) => {
                json!({"kind":"atoms","values":known(match t{Known::Unknown=>None,Known::Known(v)=>Some(json!(v))})})
            }
            Self::Integers(t) => {
                json!({"kind":"integers","values":known(match t{Known::Unknown=>None,Known::Known(v)=>Some(json!(v.iter().map(ToString::to_string).collect::<Vec<_>>()))})})
            }
            Self::Arity(t) => {
                json!({"kind":"arity","value":known(match t{Known::Unknown=>None,Known::Known(v)=>Some(json!(v))})})
            }
            Self::Arities(t) => {
                json!({"kind":"arities","values":known(match t{Known::Unknown=>None,Known::Known(v)=>Some(json!(v))})})
            }
            Self::Endpoint(v) => {
                json!({"kind":"endpoint","value":match v{Endpoint::NegativeInfinity=>"neg_inf".to_owned(),Endpoint::PositiveInfinity=>"pos_inf".to_owned(),Endpoint::Finite(n)=>n.to_string()}})
            }
            Self::BitLength(v) => json!({"kind":"integer","value":v.to_string()}),
            Self::Entries(es) => {
                json!({"kind":"entries","values":es.iter().map(|e|json!({"key":e.key.as_type().to_value(),"presence":match e.presence{Presence::Mandatory=>"mandatory",Presence::Optional=>"optional"},"value":e.value.to_value()})).collect::<Vec<_>>()})
            }
            Self::Variable(id) => json!({"kind":"variable","value":id.value()}),
            Self::Module(v) => json!({"kind":"atom","value":v}),
        }
    }
}
impl Type {
    pub fn get(&self, getter: TypeGetter) -> Result<TypeValue, LatticeError> {
        use TypeGetter::*;
        let direct = matches!(
            getter,
            BitstringUnit | BitstringBase | VariableName | NominalModule | ToTypeList
        );
        let structural = if direct {
            None
        } else {
            Some(self.shallow_structural()?)
        };
        let t = structural.as_ref().unwrap_or(self);
        let s = t.direct_slots();
        let bad = || LatticeError::FunctionClause;
        Ok(match getter {
            AtomValues => {
                let atoms = if s.is_some_and(|s| s.atoms().is_some()) {
                    t.clone()
                } else {
                    t.meet(&Self::all_atoms())?
                };
                let Some(a) = atoms.direct_slots().and_then(|s| s.atoms()) else {
                    return Err(LatticeError::BadMatch(Box::new(atoms)));
                };
                TypeValue::Atoms(match a {
                    Atoms::All => Known::Unknown,
                    Atoms::Set(v) => Known::Known(v.iter().map(|a| a.to_string()).collect()),
                })
            }
            NumberValues => {
                let number = if s.is_some_and(|s| s.number().is_some()) {
                    t.clone()
                } else {
                    let inf = t.meet(&Self::number())?;
                    if inf.is_none() {
                        return Err(LatticeError::BadMatchBoolean(true));
                    }
                    if inf == *t {
                        return Err(LatticeError::NativeNonTerminating);
                    }
                    inf
                };
                let Some(n) = number.direct_slots().and_then(|s| s.number()) else {
                    return Err(bad());
                };
                TypeValue::Integers(match n {
                    Numeric::Integer(Integers::Set(v)) => Known::Known(v.iter().cloned().collect()),
                    _ => Known::Unknown,
                })
            }
            NumberMin | NumberMax => {
                let n = s.and_then(|s| s.number()).ok_or_else(bad)?;
                let minimum = matches!(getter, NumberMin);
                let endpoint = match n {
                    Numeric::Integer(Integers::Range(a, b)) => if minimum { a } else { b }.clone(),
                    Numeric::Integer(Integers::Set(v)) => {
                        Bound::Finite(if minimum { v.first() } else { v.last() }.unwrap().clone())
                    }
                    _ => {
                        if minimum {
                            Bound::NegativeInfinity
                        } else {
                            Bound::PositiveInfinity
                        }
                    }
                };
                TypeValue::Endpoint(match endpoint {
                    Bound::NegativeInfinity => Endpoint::NegativeInfinity,
                    Bound::PositiveInfinity => Endpoint::PositiveInfinity,
                    Bound::Finite(n) => Endpoint::Finite(n),
                })
            }
            BitstringUnit | BitstringBase => {
                let b = s.and_then(|s| s.bitstrings()).ok_or_else(bad)?;
                TypeValue::BitLength(if matches!(getter, BitstringUnit) {
                    b.unit.clone()
                } else {
                    b.base.clone()
                })
            }
            FunctionArgs | FunctionArity | FunctionRange => {
                let f = s.and_then(|s| s.functions()).ok_or_else(bad)?;
                if matches!(getter, FunctionRange) {
                    TypeValue::Type(*f.range.clone())
                } else {
                    match (&f.domain.0, getter) {
                        (Repr::Any, FunctionArgs) => TypeValue::Types(Known::Unknown),
                        (Repr::Any, FunctionArity) => TypeValue::Arity(Known::Unknown),
                        (Repr::Product(p), FunctionArgs) => {
                            TypeValue::Types(Known::Known(p.as_ref().clone()))
                        }
                        (Repr::Product(p), FunctionArity) => {
                            TypeValue::Arity(Known::Known(p.len()))
                        }
                        _ => return Err(bad()),
                    }
                }
            }
            ConsHead | ConsTail | ListElements | ListTermination => {
                let l = s.and_then(|s| s.list()).ok_or_else(bad)?;
                TypeValue::Type(match (getter, l) {
                    (ListElements, Lists::Nil) => Self::none(),
                    (ListTermination, Lists::Nil) => Self::nil(),
                    (ListElements, Lists::List { elements, .. })
                    | (
                        ConsHead,
                        Lists::List {
                            elements,
                            nonempty: true,
                            ..
                        },
                    ) => *elements.clone(),
                    (ListTermination, Lists::List { tail, .. }) => *tail.clone(),
                    (
                        ConsTail,
                        Lists::List {
                            tail,
                            nonempty: true,
                            ..
                        },
                    ) => tail.join(t)?,
                    _ => return Err(bad()),
                })
            }
            TupleArgs | TupleSize | TupleSizes | TupleSubtypes => {
                let tuples = s.and_then(|s| s.tuple()).ok_or_else(bad)?;
                match (getter, tuples) {
                    (TupleSizes, Tuples::Any) => TypeValue::Arities(Known::Unknown),
                    (TupleSubtypes, Tuples::Any) => TypeValue::Types(Known::Unknown),
                    (_, Tuples::Any) => return Err(bad()),
                    (TupleSizes, Tuples::Set(g)) => {
                        TypeValue::Arities(Known::Known(g.keys().copied().collect()))
                    }
                    (TupleSubtypes, Tuples::Set(g)) => TypeValue::Types(Known::Known(
                        g.iter()
                            .flat_map(|(arity, vs)| {
                                vs.iter().map(move |v| {
                                    Self::from_tuples(Some(Tuples::Set(BTreeMap::from([(
                                        *arity,
                                        vec![v.clone()],
                                    )]))))
                                })
                            })
                            .collect(),
                    )),
                    (TupleArgs, Tuples::Set(g))
                        if g.len() == 1 && g.values().next().unwrap().len() == 1 =>
                    {
                        TypeValue::Types(Known::Known(
                            g.values().next().unwrap()[0].elements.clone(),
                        ))
                    }
                    (TupleSize, Tuples::Set(g))
                        if g.len() == 1 && g.values().next().unwrap().len() == 1 =>
                    {
                        TypeValue::Arity(Known::Known(*g.keys().next().unwrap()))
                    }
                    _ => return Err(bad()),
                }
            }
            MapEntries | MapDefaultKey | MapDefaultValue => {
                let m = s.and_then(|s| s.maps()).ok_or_else(bad)?;
                match getter {
                    MapEntries => TypeValue::Entries(m.entries.clone()),
                    MapDefaultKey => TypeValue::Type(*m.default_key.clone()),
                    _ => TypeValue::Type(*m.default_value.clone()),
                }
            }
            VariableName => {
                let Repr::Var(id) = &t.0 else {
                    return Err(bad());
                };
                TypeValue::Variable(id.clone())
            }
            NominalModule => {
                let Repr::Nominal(n) = &t.0 else {
                    return Err(bad());
                };
                let NominalName::Defined { module, .. } = &n.name else {
                    return Err(bad());
                };
                TypeValue::Module(module.clone())
            }
            ToTypeList => TypeValue::Types(Known::Known(match &t.0 {
                Repr::Product(p) => p.as_ref().clone(),
                _ => vec![t.clone()],
            })),
        })
    }
    pub fn lift_list_to_pos_empty(&self) -> Result<Self, LatticeError> {
        let t = self.shallow_structural()?;
        match t.direct_slots().and_then(|s| s.list()) {
            Some(Lists::Nil) => Ok(Self::nil()),
            Some(Lists::List { elements, tail, .. }) => {
                Ok(Self::list(*elements.clone(), *tail.clone(), false))
            }
            _ => Err(LatticeError::FunctionClause),
        }
    }
    pub fn supremum(types: &[Self]) -> Result<Self, LatticeError> {
        if types.iter().any(Self::is_any) {
            return Ok(Self::any());
        }
        let Some(first) = types.first() else {
            return Ok(Self::none());
        };
        types[1..]
            .iter()
            .try_fold(first.clone(), |acc, t| t.join(&acc))
    }
    pub fn infimum(types: &[Self]) -> Result<Self, LatticeError> {
        let Some(first) = types.first() else {
            return Ok(Self::none());
        };
        let mut acc = first.clone();
        for t in &types[1..] {
            acc = acc.meet(t)?;
            if acc.is_none() {
                break;
            }
        }
        Ok(acc)
    }
    pub fn meet_lists(left: &[Self], right: &[Self]) -> Result<Vec<Self>, LatticeError> {
        let common = left.len().min(right.len());
        let mut out = Vec::with_capacity(common);
        for index in 0..common {
            out.push(left[index].meet(&right[index])?);
        }
        if left.len() != right.len() {
            return Err(LatticeError::BadGenerators(Box::new((
                left[common..].to_vec(),
                right[common..].to_vec(),
            ))));
        }
        Ok(out)
    }
    pub fn function_with_arity(arity: i64, range: Self) -> Result<Self, LatticeError> {
        if !(0..=255).contains(&arity) {
            return Err(LatticeError::FunctionClause);
        }
        Ok(Self::function(
            Some(vec![Self::any(); arity as usize]),
            range,
        ))
    }
    pub fn map_pairs(pairs: &[(Self, Self)]) -> Result<Self, LatticeError> {
        pairs.iter().try_fold(Self::map_any(), |map, (key, value)| {
            map.map_put(key.clone(), value.clone())
        })
    }
    pub fn substitute_all_variables(&self) -> Result<Self, LatticeError> {
        self.substitute(&BTreeMap::new())
    }
    pub fn subtract_types(&self, types: &[Self]) -> Result<Self, LatticeError> {
        types
            .iter()
            .try_fold(self.clone(), |acc, t| acc.subtract(t))
    }
    pub fn bitstr_concat_types(types: &[Self]) -> Result<Self, LatticeError> {
        types.iter().try_fold(
            Self::bitstring(0.into(), 0.into()).map_err(|_| LatticeError::BadArg)?,
            |acc, t| acc.bitstr_concat(t),
        )
    }
    pub fn any_none(types: &[Self]) -> bool {
        types.iter().any(Self::is_none)
    }
    pub fn any_none_or_unit(types: &[Self]) -> bool {
        types.iter().any(Self::is_impossible)
    }
    pub fn is_native_type(&self) -> bool {
        !matches!(self.0, Repr::Opaque)
    }
}

impl Type {
    pub fn widen_to_number(&self) -> Result<Self, LatticeError> {
        Ok(match &self.0 {
            Repr::Opaque => return Err(LatticeError::FunctionClause),
            Repr::Nominal(n) => Self::raw_nominal(n.name.clone(), n.inner.widen_to_number()?),
            Repr::NominalSet(n) => Self::normalized_nominals(
                n.nominals
                    .iter()
                    .map(|n| n.as_type().widen_to_number())
                    .collect::<Result<Vec<_>, _>>()?,
                n.structural.widen_to_number()?,
            )?,
            Repr::Product(p) => Self::raw_product(
                p.iter()
                    .map(Self::widen_to_number)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Repr::Families(slots) => {
                let mut s = slots.rebuild_base();
                if let Some(f) = slots.functions() {
                    s.functions = Some(Arc::new(Functions {
                        domain: Box::new(f.domain.widen_to_number()?),
                        range: Box::new(f.range.widen_to_number()?),
                    }));
                }
                if let Some(Lists::List {
                    elements,
                    tail,
                    nonempty,
                }) = slots.list()
                {
                    s.list = Some(Arc::new(Lists::List {
                        elements: Box::new(elements.widen_to_number()?),
                        tail: Box::new(tail.widen_to_number()?),
                        nonempty: *nonempty,
                    }));
                }
                if slots.number().is_some() {
                    s.number = Some(Arc::new(Numeric::Number));
                }
                if let Some(Tuples::Set(g)) = slots.tuple() {
                    let variants = g
                        .iter()
                        .map(|(arity, vs)| {
                            Ok((
                                *arity,
                                vs.iter()
                                    .map(|v| {
                                        Ok(TupleVariant {
                                            elements: v
                                                .elements
                                                .iter()
                                                .map(Self::widen_to_number)
                                                .collect::<Result<Vec<_>, LatticeError>>()?,
                                            tag: v.tag.clone(),
                                        })
                                    })
                                    .collect::<Result<Vec<_>, LatticeError>>()?,
                            ))
                        })
                        .collect::<Result<BTreeMap<_, _>, LatticeError>>()?;
                    let mut tuples = Self::none();
                    for (arity, vs) in variants {
                        for v in vs {
                            tuples = Self::from_tuples(Some(Tuples::Set(BTreeMap::from([(
                                arity,
                                vec![v],
                            )]))))
                            .join(&tuples)?;
                        }
                    }
                    s.tuple = match tuples.0 {
                        Repr::Families(t) => t.into_slots().tuple,
                        _ => None,
                    };
                }
                if let Some(m) = slots.maps() {
                    s.maps = Maps::canonical_types(
                        m.entries
                            .iter()
                            .map(|e| {
                                Ok(MapTypeEntry {
                                    key: e.key.as_type().widen_to_number()?,
                                    presence: e.presence,
                                    value: e.value.widen_to_number()?,
                                })
                            })
                            .collect::<Result<Vec<_>, LatticeError>>()?,
                        m.default_key.widen_to_number()?,
                        m.default_value.widen_to_number()?,
                    )?
                    .map(Arc::new);
                }
                if slots.union_record() {
                    Self(Repr::Families(FamilyStore::raw(s)))
                } else {
                    Self::families(s)
                }
            }
            _ => self.clone(),
        })
    }
}

impl Endpoint {
    pub fn minimum(&self, other: &Self) -> Self {
        let left = Bound::from(self.clone());
        let right = Bound::from(other.clone());
        if left <= right {
            self.clone()
        } else {
            other.clone()
        }
    }
    pub fn maximum(&self, other: &Self) -> Self {
        let left = Bound::from(self.clone());
        let right = Bound::from(other.clone());
        if left <= right {
            other.clone()
        } else {
            self.clone()
        }
    }
}

#[cfg(test)]
mod api_tests {
    use super::*;
    #[test]
    fn native_zip_error_preserves_unconsumed_tails() {
        let left = vec![Type::any(), Type::atom("tail")];
        assert_eq!(
            Type::meet_lists(&left, &[Type::none()]),
            Err(LatticeError::BadGenerators(Box::new((
                vec![Type::atom("tail")],
                vec![]
            ))))
        );
    }
    #[test]
    fn optional_non_singleton_keys_can_normalize_away_before_native_validation() {
        let key = Type::atoms(["a", "b"]);
        let map = Type::map_types(
            vec![MapTypeEntry {
                key,
                presence: Presence::Optional,
                value: Type::any(),
            }],
            Type::all_atoms(),
            Type::any(),
        )
        .unwrap();
        assert!(
            matches!(map.get(TypeGetter::MapEntries).unwrap(),TypeValue::Entries(entries) if entries.is_empty())
        );
    }
    #[test]
    fn otp28_runtime_maps_distinguish_signed_float_zero() {
        let term = RuntimeTerm::map(vec![
            (RuntimeTerm::float(0.0).unwrap(), RuntimeTerm::atom("a")),
            (RuntimeTerm::float(-0.0).unwrap(), RuntimeTerm::atom("b")),
        ]);
        let map = Type::from_runtime_term(&term).unwrap();
        assert_eq!(
            map.get(TypeGetter::MapDefaultValue).unwrap().to_value(),
            TypeValue::Type(Type::atoms(["a", "b"])).to_value()
        );
    }
}

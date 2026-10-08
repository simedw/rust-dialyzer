use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresentationRecordField {
    pub name: String,
    pub declared_type: Type,
}
#[derive(Clone, Debug)]
pub struct PresentationContext {
    records: BTreeMap<(String, usize), Vec<PresentationRecordField>>,
    feature_keywords: BTreeSet<String>,
}
impl Default for PresentationContext {
    fn default() -> Self {
        Self {
            records: BTreeMap::new(),
            feature_keywords: ["maybe".into(), "else".into()].into(),
        }
    }
}
impl PresentationContext {
    pub fn with_feature_keywords(keywords: impl IntoIterator<Item = String>) -> Self {
        Self {
            feature_keywords: keywords.into_iter().collect(),
            ..Self::default()
        }
    }
    pub fn insert_record(&mut self, name: String, fields: Vec<PresentationRecordField>) {
        self.records.insert((name, fields.len()), fields);
    }
    pub fn atom_to_string(&self, atom: &str) -> String {
        atom_string(atom, false, &self.feature_keywords)
    }
    pub(crate) fn atom_latin1(&self, atom: &str) -> String {
        atom_string(atom, true, &self.feature_keywords)
    }
    pub fn quoted_atom(&self, atom: &str) -> String {
        quoted(atom, false)
    }
    pub fn qualified_name(&self, module: &str, name: &str) -> String {
        format!("{}:{}", self.atom_latin1(module), self.atom_to_string(name))
    }
    pub fn type_to_string(&self, value: &Type) -> Result<String, LatticeError> {
        print_type(value, self)
    }
}
fn quoted(value: &str, latin1: bool) -> String {
    let mut out = String::from("'");
    for c in value.chars() {
        match c {
            '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{b}' => out.push_str("\\v"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\u{1b}' => out.push_str("\\e"),
            '\u{7f}' => out.push_str("\\d"),
            c if (32..=126).contains(&(c as u32))
                || (160..=255).contains(&(c as u32))
                || (!latin1 && c as u32 >= 160) =>
            {
                out.push(c)
            }
            c if latin1 && c as u32 >= 255 => out.push_str(&format!("\\x{{{:X}}}", c as u32)),
            c => out.push_str(&format!("\\{:03o}", c as u32)),
        }
    }
    out.push('\'');
    out
}
fn atom_string(value: &str, latin1: bool, features: &BTreeSet<String>) -> String {
    let reserved = matches!(
        value,
        "after"
            | "begin"
            | "case"
            | "try"
            | "cond"
            | "catch"
            | "andalso"
            | "orelse"
            | "end"
            | "fun"
            | "if"
            | "let"
            | "of"
            | "receive"
            | "when"
            | "bnot"
            | "not"
            | "div"
            | "rem"
            | "band"
            | "and"
            | "bor"
            | "bxor"
            | "bsl"
            | "bsr"
            | "or"
            | "xor"
    ) || features.contains(value);
    let lower = |c: char| c.is_ascii_lowercase() || ('ß'..='ÿ').contains(&c) && c != '÷';
    let name = |c: char| {
        lower(c)
            || c.is_ascii_uppercase()
            || ('À'..='Þ').contains(&c) && c != '×'
            || c.is_ascii_digit()
            || c == '_'
            || c == '@'
    };
    let mut chars = value.chars();
    let unquoted = !reserved && chars.next().is_some_and(lower) && chars.all(name);
    if unquoted {
        value.into()
    } else {
        quoted(value, latin1)
    }
}
fn comma(types: &[Type], ctx: &PresentationContext) -> Result<String, LatticeError> {
    types
        .iter()
        .map(|t| {
            if t.is_any() {
                Ok("_".into())
            } else {
                print_type(t, ctx)
            }
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|p| p.join(","))
}
fn union(
    types: impl IntoIterator<Item = Type>,
    ctx: &PresentationContext,
) -> Result<String, LatticeError> {
    types
        .into_iter()
        .map(|t| print_type(&t, ctx))
        .collect::<Result<Vec<_>, _>>()
        .map(|p| p.join(" | "))
}
fn bound_string(bound: &Bound) -> String {
    match bound {
        Bound::Finite(n) => n.to_string(),
        Bound::NegativeInfinity => "neg_inf".into(),
        Bound::PositiveInfinity => "pos_inf".into(),
    }
}
fn print_tuple(tuple: &TupleVariant, ctx: &PresentationContext) -> Result<String, LatticeError> {
    if let Some(tag) = &tuple.tag
        && let Some(fields) = ctx.records.get(&(tag.clone(), tuple.elements.len() - 1))
    {
        let mut strings = Vec::new();
        for (actual, field) in tuple.elements[1..].iter().zip(fields) {
            let skip = actual.is_any()
                || (actual.is_atom_literal("undefined")?
                    && !actual.meet(&field.declared_type)?.is_none());
            if !skip {
                strings.push(format!(
                    "{}::{}",
                    ctx.atom_to_string(&field.name),
                    print_type(actual, ctx)?
                ));
            }
        }
        return Ok(format!(
            "#{}{{{}}}",
            ctx.atom_to_string(tag),
            strings.join(",")
        ));
    }
    Ok(format!("{{{}}}", comma(&tuple.elements, ctx)?))
}
fn print_type(t: &Type, ctx: &PresentationContext) -> Result<String, LatticeError> {
    Ok(match &t.0 {
        Repr::Any => "any()".into(),
        Repr::None => "none()".into(),
        Repr::Unit => "no_return()".into(),
        Repr::Opaque => return Err(LatticeError::FunctionClause),
        Repr::Var(VariableId::Atom(a)) => a.clone(),
        Repr::Var(VariableId::Integer(n)) => format!("var({n})"),
        Repr::Product(types) => format!("<{}>", comma(types, ctx)?),
        Repr::Nominal(n) => match &n.name {
            NominalName::Symbol(_) => return Err(LatticeError::FunctionClause),
            NominalName::Defined {
                module,
                name,
                arity,
                opacity,
            } => {
                let args = vec!["_"; *arity].join(",");
                let named = format!(
                    "{}:{}({args})",
                    ctx.atom_latin1(module),
                    ctx.atom_to_string(name)
                );
                if matches!(n.inner.0, Repr::Opaque) || *opacity == Opacity::Opaque {
                    named
                } else {
                    format!("({named} :: {})", print_type(&n.inner, ctx)?)
                }
            }
        },
        Repr::NominalSet(n) => union(
            std::iter::once(*n.structural.clone())
                .chain(n.nominals.iter().map(Nominal::as_type))
                .filter(|t| !t.is_none()),
            ctx,
        )?,
        Repr::Families(s) => {
            if s.union_record() || s.count() != 1 {
                return union(
                    s.components_with_none()
                        .into_iter()
                        .filter(|t| !t.is_none()),
                    ctx,
                );
            }
            if let Some(atoms) = s.atoms() {
                match atoms {
                    Atoms::All => "atom()".into(),
                    Atoms::Set(values)
                        if values.len() == 2
                            && values.iter().any(|a| *a == "true")
                            && values.iter().any(|a| *a == "false") =>
                    {
                        "boolean()".into()
                    }
                    Atoms::Set(values) => values
                        .iter()
                        .map(|a| quoted(a, false))
                        .collect::<Vec<_>>()
                        .join(" | "),
                }
            } else if let Some(b) = s.bitstrings() {
                match (b.unit.to_string().as_str(), b.base.to_string().as_str()) {
                    ("0", "0") => "<<>>".into(),
                    ("8", "0") => "binary()".into(),
                    ("8", "8") => "nonempty_binary()".into(),
                    ("1", "0") => "bitstring()".into(),
                    ("1", "1") => "nonempty_bitstring()".into(),
                    ("0", _) => format!("<<_:{}>>", b.base),
                    (_, "0") => format!("<<_:_*{}>>", b.unit),
                    _ => format!("<<_:{},_:_*{}>>", b.base, b.unit),
                }
            } else if let Some(f) = s.functions() {
                match &f.domain.0 {
                    Repr::Any if f.range.is_any() => "fun()".into(),
                    Repr::Any => format!("fun((...) -> {})", print_type(&f.range, ctx)?),
                    Repr::Product(args) => format!(
                        "fun(({}) -> {})",
                        comma(args, ctx)?,
                        print_type(&f.range, ctx)?
                    ),
                    _ => return Err(LatticeError::FunctionClause),
                }
            } else if let Some(ids) = s.identifiers() {
                match ids {
                    Identifiers::All => "identifier()".into(),
                    Identifiers::Set(set) => set
                        .iter()
                        .map(|n| format!("{n}()"))
                        .collect::<Vec<_>>()
                        .join(" | "),
                }
            } else if let Some(list) = s.list() {
                match list {
                    Lists::Nil => "[]".into(),
                    Lists::List {
                        elements,
                        tail,
                        nonempty,
                    } => {
                        let content = print_type(elements, ctx)?;
                        if **tail == Type::nil() {
                            let char_type = Type::builtin(BuiltinType::Char)?;
                            if **elements == char_type {
                                if *nonempty {
                                    "nonempty_string()".into()
                                } else {
                                    "string()".into()
                                }
                            } else if *nonempty {
                                format!("[{content},...]")
                            } else {
                                format!("[{content}]")
                            }
                        } else if tail.is_any() {
                            if *nonempty {
                                "nonempty_maybe_improper_list()".into()
                            } else {
                                "maybe_improper_list()".into()
                            }
                        } else {
                            let prefix = if *nonempty {
                                if Type::nil().is_subtype(tail)? {
                                    "nonempty_maybe_improper_list"
                                } else {
                                    "nonempty_improper_list"
                                }
                            } else {
                                "maybe_improper_list"
                            };
                            format!("{prefix}({content},{})", print_type(tail, ctx)?)
                        }
                    }
                }
            } else if let Some(number) = s.number() {
                match number {
                    Numeric::Float => "float()".into(),
                    Numeric::Number => "number()".into(),
                    Numeric::Integer(Integers::All) => "integer()".into(),
                    Numeric::Integer(Integers::Set(set)) => set
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(" | "),
                    Numeric::Integer(Integers::Range(from, to)) => match (from, to) {
                        (Bound::Finite(a), Bound::Finite(b))
                            if a == &0.into() && b == &255.into() =>
                        {
                            "byte()".into()
                        }
                        (Bound::Finite(a), Bound::Finite(b))
                            if a == &0.into() && b == &0x10ffff.into() =>
                        {
                            "char()".into()
                        }
                        (Bound::Finite(a), Bound::PositiveInfinity) if a == &1.into() => {
                            "pos_integer()".into()
                        }
                        (Bound::Finite(a), Bound::PositiveInfinity) if a == &0.into() => {
                            "non_neg_integer()".into()
                        }
                        (Bound::NegativeInfinity, Bound::Finite(b)) if b == &(-1).into() => {
                            "neg_integer()".into()
                        }
                        _ => format!("{}..{}", bound_string(from), bound_string(to)),
                    },
                }
            } else if let Some(tuple) = s.tuple() {
                match tuple {
                    Tuples::Any => "tuple()".into(),
                    Tuples::Set(rows) => {
                        let alternatives = rows.values().flatten().collect::<Vec<_>>();
                        alternatives
                            .iter()
                            .map(|t| print_tuple(t, ctx))
                            .collect::<Result<Vec<_>, _>>()?
                            .join(" | ")
                    }
                }
            } else if let Some(map) = s.maps() {
                if map.entries.is_empty() && map.default_key.is_any() && map.default_value.is_any()
                {
                    "map()".into()
                } else {
                    let mut pairs = map
                        .entries
                        .iter()
                        .map(|e| (e.key.as_type(), e.presence, e.value.clone()))
                        .collect::<Vec<_>>();
                    if !map.default_key.is_none() || !map.default_value.is_none() {
                        pairs.push((
                            *map.default_key.clone(),
                            Presence::Optional,
                            *map.default_value.clone(),
                        ));
                    }
                    let mut mandatory = Vec::new();
                    let mut optional = Vec::new();
                    for (key, p, value) in pairs {
                        let k = if key.is_any() {
                            "_".into()
                        } else {
                            print_type(&key, ctx)?
                        };
                        let v = if value.is_any() {
                            "_".into()
                        } else {
                            print_type(&value, ctx)?
                        };
                        if p == Presence::Mandatory {
                            mandatory.push(format!("{k}:={v}"))
                        } else {
                            optional.push(format!("{k}=>{v}"))
                        }
                    }
                    mandatory.extend(optional);
                    format!("#{{{}}}", mandatory.join(", "))
                }
            } else {
                return Err(LatticeError::FunctionClause);
            }
        }
    })
}
impl Type {
    pub fn to_native_string(&self) -> Result<String, LatticeError> {
        PresentationContext::default().type_to_string(self)
    }
    pub fn record_field_diffs(
        &self,
        ctx: &PresentationContext,
    ) -> Result<(Vec<usize>, String), LatticeError> {
        let s = self.direct_slots().ok_or(LatticeError::FunctionClause)?;
        if s.union_record() || s.count() != 1 {
            return Err(LatticeError::FunctionClause);
        }
        let Some(Tuples::Set(rows)) = s.tuple() else {
            return Err(LatticeError::FunctionClause);
        };
        let mut variants = rows.values().flatten();
        let tuple = variants.next().ok_or(LatticeError::FunctionClause)?;
        if variants.next().is_some() {
            return Err(LatticeError::FunctionClause);
        }
        let tag = tuple.tag.as_ref().ok_or_else(|| {
            LatticeError::BadMatchNative(Box::new(NativeValue::Atom("unknown".into())))
        })?;
        let fields = ctx
            .records
            .get(&(tag.clone(), tuple.elements.len() - 1))
            .ok_or_else(|| {
                LatticeError::BadMatchNative(Box::new(NativeValue::Atom("error".into())))
            })?;
        let mut positions = Vec::new();
        let mut diffs = Vec::new();
        for (i, (actual, field)) in tuple.elements[1..].iter().zip(fields).enumerate() {
            if actual.meet(&field.declared_type)?.is_none() {
                positions.push(i + 1);
                diffs.push(format!(
                    "{}::{}",
                    ctx.atom_to_string(&field.name),
                    print_type(&field.declared_type, ctx)?
                ));
            }
        }
        Ok((positions, diffs.join(" and ")))
    }
}

impl Type {
    pub fn abstract_records(&self, ctx: &PresentationContext) -> Result<Self, LatticeError> {
        match &self.0 {
            Repr::Product(v) => Ok(Self::raw_product(
                v.iter()
                    .map(|t| t.abstract_records(ctx))
                    .collect::<Result<_, _>>()?,
            )),
            Repr::Nominal(n) => {
                let inner = n.inner.abstract_records(ctx)?;
                if matches!(n.inner.0, Repr::Nominal(_) | Repr::NominalSet(_))
                    && !matches!(inner.0, Repr::Nominal(_) | Repr::NominalSet(_))
                {
                    Ok(Self::any())
                } else {
                    Ok(Self::raw_nominal(n.name.clone(), inner))
                }
            }
            Repr::NominalSet(n) => Self::normalized_nominals(
                n.nominals
                    .iter()
                    .map(|n| n.as_type().abstract_records(ctx))
                    .collect::<Result<_, _>>()?,
                n.structural.abstract_records(ctx)?,
            ),
            Repr::Families(s) if s.union_record() || s.count() != 1 => {
                let mut components = Vec::new();
                macro_rules! family {
                    ($field:ident, $owner:ident) => {
                        if let Some(value) = s.$owner() {
                            let one = Slots {
                                $field: Some(value.clone()),
                                ..Slots::default()
                            };
                            components.push(
                                Self(Repr::Families(FamilyStore::raw(one)))
                                    .abstract_records(ctx)?,
                            );
                        }
                    };
                }
                family!(atoms, atoms_owner);
                family!(bitstrings, bitstrings_owner);
                family!(functions, functions_owner);
                family!(identifiers, identifiers_owner);
                family!(list, list_owner);
                family!(number, number_owner);
                family!(tuple, tuple_owner);
                family!(maps, maps_owner);
                Self::supremum(&components)
            }
            Repr::Families(s) => {
                if let Some(Lists::List {
                    elements,
                    tail,
                    nonempty,
                }) = s.list()
                {
                    let elements = elements.abstract_records(ctx)?;
                    if elements.is_none() {
                        return Ok(Self::none());
                    }
                    let tail = tail.abstract_records(ctx)?;
                    let (elements, tail) = if tail == Self::nil() || tail.is_any() {
                        (elements, tail)
                    } else {
                        let v = Self::cons(elements, tail)?;
                        let Some(Lists::List { elements, tail, .. }) =
                            v.direct_slots().and_then(|s| s.list())
                        else {
                            return Err(LatticeError::BadMatch(Box::new(v)));
                        };
                        ((**elements).clone(), (**tail).clone())
                    };
                    return Ok(Self(Repr::Families(FamilyStore::raw(Slots {
                        list: Some(Arc::new(Lists::List {
                            elements: Box::new(elements),
                            tail: Box::new(tail),
                            nonempty: *nonempty,
                        })),
                        ..Slots::default()
                    }))));
                }
                if let Some(f) = s.functions() {
                    return Ok(Self(Repr::Families(FamilyStore::raw(Slots {
                        functions: Some(Arc::new(Functions {
                            domain: Box::new(f.domain.abstract_records(ctx)?),
                            range: Box::new(f.range.abstract_records(ctx)?),
                        })),
                        ..Slots::default()
                    }))));
                }
                if let Some(Tuples::Set(rows)) = s.tuple() {
                    let mut types = Vec::new();
                    for tuple in rows.values().flatten() {
                        if let Some(fields) = tuple.tag.as_ref().and_then(|tag| {
                            ctx.records.get(&(tag.clone(), tuple.elements.len() - 1))
                        }) {
                            let mut elements = vec![tuple.elements[0].clone()];
                            elements.extend(fields.iter().map(|f| f.declared_type.clone()));
                            types.push(Self::tuple(elements)?);
                        } else {
                            types.push(Self::tuple(
                                tuple
                                    .elements
                                    .iter()
                                    .map(|t| t.abstract_records(ctx))
                                    .collect::<Result<_, _>>()?,
                            )?);
                        }
                    }
                    return Self::supremum(&types);
                }
                Ok(self.clone())
            }
            _ => Ok(self.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_atom_literal_and_name_printers_have_distinct_quoting_rules() {
        let ctx = PresentationContext::default();
        assert_eq!(ctx.atom_to_string("simple"), "simple");
        assert_eq!(ctx.quoted_atom("simple"), "'simple'");
        assert_eq!(ctx.atom_to_string("maybe"), "'maybe'");
        assert_eq!(ctx.qualified_name("雪", "雪"), "'\\x{96EA}':'雪'");
    }
    #[test]
    fn malformed_unification_limit_follows_native_short_circuit_then_generator_error() {
        let nominal = Type::raw_nominal(NominalName::Symbol("a".into()), Type::any());
        let shallow = UnificationValue::MalformedNominalSet {
            nominal: nominal.clone(),
            structural: Type::none(),
        };
        assert_eq!(shallow.limit(0), Ok(Type::any()));
        assert_eq!(shallow.limit(5), Err(LatticeError::FunctionClause));
        let deep = UnificationValue::MalformedNominalSet {
            nominal: nominal.clone(),
            structural: Type::tuple(vec![Type::integer(1.into())]).unwrap(),
        };
        assert_eq!(
            deep.limit(1),
            Err(LatticeError::BadGenerator(Box::new(nominal)))
        );
    }
}

use super::*;
use crate::atom::Atom;
use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NativeRecordName {
    Any,
    Named { module: Atom, name: Atom },
    Atom(Atom),
}
impl NativeRecordName {
    fn normalized(self) -> Self {
        match self {
            Self::Atom(a) if a == "nil" => Self::Any,
            other => other,
        }
    }
    fn atom(&self) -> Option<&str> {
        match self {
            Self::Any => Some("nil"),
            Self::Atom(a) => Some(a),
            Self::Named { .. } => None,
        }
    }
    fn value(&self) -> Value {
        match self {
            Self::Any => Value::Null,
            Self::Atom(a) => json!(a.as_ref()),
            Self::Named { module, name } => json!({"module":module.as_ref(),"name":name.as_ref()}),
        }
    }
    pub fn from_value(value: &Value) -> Result<Self, String> {
        Ok(if value.is_null() {
            Self::Any
        } else if let Some(a) = value.as_str() {
            Self::Atom(a.into()).normalized()
        } else {
            Self::Named {
                module: value["module"].as_str().ok_or("record module must be atom")?.into(),
                name: value["name"].as_str().ok_or("record name must be atom")?.into(),
            }
        })
    }
}
impl Ord for NativeRecordName {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.atom(), other.atom()) {
            (Some(a), Some(b)) => a.cmp(b),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => match (self, other) {
                (Self::Named { module: am, name: an }, Self::Named { module: bm, name: bn }) =>
                    am.cmp(bm).then_with(|| an.cmp(bn)),
                _ => unreachable!(),
            },
        }
    }
}
impl PartialOrd for NativeRecordName {
    fn partial_cmp(&self, rhs: &Self) -> Option<Ordering> { Some(self.cmp(rhs)) }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RecordField { Present(Type), Missing }

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NativeRecord {
    pub name: NativeRecordName,
    pub fields: BTreeMap<Atom, RecordField>,
}

#[derive(Clone, Copy, Debug)]
pub enum RecordsView<'a> { Record(&'a NativeRecord), Set(&'a [NativeRecord]) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordTest { True, False, Unknown }

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum Records { Record(NativeRecord), Set(Vec<NativeRecord>) }

impl NativeRecord {
    fn any() -> Self { Self { name: NativeRecordName::Any, fields: BTreeMap::new() } }
    pub(super) fn value(&self) -> Value {
        json!({"kind":"record","name":self.name.value(),"fields":self.fields.iter().map(|(key,field)| {
            match field {
                RecordField::Present(t) => json!({"key":key.as_ref(),"presence":"present","value":t.to_value()}),
                RecordField::Missing => json!({"key":key.as_ref(),"presence":"missing"}),
            }
        }).collect::<Vec<_>>()})
    }
    fn parse(value: &Value) -> Result<Self, String> {
        if value["kind"] != "record" { return Err("record_set member must be record".into()); }
        let name = NativeRecordName::from_value(value.get("name").ok_or("record name missing")?)?;
        let mut fields = BTreeMap::new();
        for field in value["fields"].as_array().ok_or("record fields must be array")? {
            let key: Atom = field["key"].as_str().ok_or("record field key must be atom")?.into();
            let value = match field["presence"].as_str() {
                Some("present") => RecordField::Present(Type::from_value(&field["value"])?),
                Some("missing") => RecordField::Missing,
                _ => return Err("record field presence must be present or missing".into()),
            };
            if fields.insert(key, value).is_some() { return Err("duplicate record field".into()); }
        }
        Ok(Self { name, fields })
    }
    fn join(&self, rhs: &Self) -> Result<Self, LatticeError> {
        if self == rhs { return Ok(self.clone()); }
        let name = if self.name == rhs.name { self.name.clone() } else { NativeRecordName::Any };
        let mut fields = BTreeMap::new();
        for (key, left) in &self.fields {
            match (left, rhs.fields.get(key)) {
                (RecordField::Present(a), Some(RecordField::Present(b))) => {
                    let sup = a.join(b)?;
                    if !sup.is_none() { fields.insert(key.clone(), RecordField::Present(sup)); }
                }
                (RecordField::Missing, Some(RecordField::Missing)) => {
                    fields.insert(key.clone(), RecordField::Missing);
                }
                _ => {}
            }
        }
        Ok(Self { name, fields })
    }
    fn meet(&self, rhs: &Self) -> Result<Option<Self>, LatticeError> {
        if self == rhs { return Ok(Some(self.clone())); }
        let name = match (&self.name, &rhs.name) {
            (a, b) if a == b => a.clone(),
            (NativeRecordName::Any, name) | (name, NativeRecordName::Any) => name.clone(),
            _ => return Ok(None),
        };
        let mut fields = self.fields.clone();
        for (key, right) in &rhs.fields {
            let value = match (fields.get(key), right) {
                (Some(RecordField::Present(a)), RecordField::Present(b)) => {
                    let inf = a.meet(b)?;
                    if inf.is_none() { return Ok(None); }
                    RecordField::Present(inf)
                }
                (Some(RecordField::Missing), RecordField::Present(_))
                | (Some(RecordField::Present(_)), RecordField::Missing) => return Ok(None),
                (_, value) => value.clone(),
            };
            fields.insert(key.clone(), value);
        }
        Ok(Some(Self { name, fields }))
    }
    fn transform(&self, f: &impl Fn(&Type) -> Result<Type, LatticeError>) -> Result<Self, LatticeError> {
        let mut fields = BTreeMap::new();
        for (key, field) in &self.fields {
            if let RecordField::Present(t) = field {
                fields.insert(key.clone(), RecordField::Present(f(t)?));
            }
        }
        Ok(Self { name: self.name.clone(), fields })
    }
    fn limited_from(&self) -> i32 {
        let base = if self.name == NativeRecordName::Any { 1 } else { 2 };
        self.values().fold(base, |depth, t| depth.max(t.info.limited_from.saturating_add(1)))
    }
    pub(super) fn values(&self) -> impl Iterator<Item = &Type> {
        self.fields.values().filter_map(|field| match field {
            RecordField::Present(t) => Some(t), RecordField::Missing => None,
        })
    }
    fn limited(&self, k: i64) -> bool {
        (k != 1 || self.name == NativeRecordName::Any)
            && self.values().all(|t| t.is_limited_walk(k - 1))
    }
    fn limit(&self, k: i64) -> Result<Self, LatticeError> {
        if k == 1 { Ok(Self::any()) }
        else { self.transform(&|t| t.limit_aux(k - 1)) }
    }
    pub fn type_to_string(&self, ctx: &PresentationContext) -> Result<String, LatticeError> {
        if self.name == NativeRecordName::Any && self.fields.is_empty() { return Ok("record()".into()); }
        let prefix = match &self.name {
            NativeRecordName::Any => "_#".to_owned(),
            NativeRecordName::Named { module, name } => format!("#{}", ctx.qualified_name(module, name)),
            NativeRecordName::Atom(name) => format!("#{}", ctx.atom_to_string(name)),
        };
        let fields = self.fields.iter().filter_map(|(key, value)| match value {
            RecordField::Present(t) => Some(ctx.type_to_string(t).map(|s| format!("{}={s}", ctx.atom_to_string(key)))),
            RecordField::Missing => None,
        }).collect::<Result<Vec<_>, _>>()?;
        Ok(format!("{prefix}{{{}}}", fields.join(", ")))
    }
}

impl Records {
    pub(super) fn view(&self) -> RecordsView<'_> {
        match self { Self::Record(r) => RecordsView::Record(r), Self::Set(rs) => RecordsView::Set(rs) }
    }
    pub(super) fn rows(&self) -> &[NativeRecord] {
        match self { Self::Record(r) => std::slice::from_ref(r), Self::Set(rs) => rs }
    }
    pub(super) fn value(&self) -> Value {
        match self {
            Self::Record(r) => r.value(),
            Self::Set(rs) => json!({"kind":"record_set","values":rs.iter().map(NativeRecord::value).collect::<Vec<_>>()}),
        }
    }
    pub(super) fn from_value(value: &Value) -> Result<Self, String> {
        match value["kind"].as_str() {
            Some("record") => Ok(Self::Record(NativeRecord::parse(value)?)),
            Some("record_set") => Ok(Self::Set(value["values"].as_array().ok_or("record_set values must be array")?
                .iter().map(NativeRecord::parse).collect::<Result<_, _>>()?)),
            _ => Err("record or record_set expected".into()),
        }
    }
    fn collapse(mut rows: Vec<NativeRecord>) -> Option<Self> {
        match rows.len() { 0 => None, 1 => Some(Self::Record(rows.pop().unwrap())), _ => Some(Self::Set(rows)) }
    }
    fn join(&self, rhs: &Self) -> Result<Self, LatticeError> {
        if self == rhs { return Ok(self.clone()); }
        if let (Self::Record(a), Self::Record(b)) = (self, rhs) {
            return if a.name == b.name || a.name == NativeRecordName::Any || b.name == NativeRecordName::Any {
                Ok(Self::Record(a.join(b)?))
            } else if a.name < b.name { Ok(Self::Set(vec![a.clone(), b.clone()])) }
            else { Ok(Self::Set(vec![b.clone(), a.clone()])) };
        }
        if matches!(self, Self::Set(rs) if rs.len()<2) || matches!(rhs, Self::Set(rs) if rs.len()<2) {
            return Err(LatticeError::NativeNonTerminating);
        }
        // erl_types calls record_set_merge(Set, [Record]) in both mixed cases.
        let (mut a, mut b) = if matches!(self, Self::Record(_)) { (rhs.rows(), self.rows()) }
                            else { (self.rows(), rhs.rows()) };
        let mut out: Vec<NativeRecord> = Vec::new();
        while !a.is_empty() || !b.is_empty() {
            match (a.first(), b.first()) {
                (Some(x), Some(y)) if x.name == NativeRecordName::Any => {
                    out = vec![x.join(y)?]; a = &[]; b = &b[1..];
                }
                (Some(x), Some(y)) if y.name == NativeRecordName::Any => {
                    out = vec![y.join(x)?]; a = &a[1..]; b = &[];
                }
                (Some(x), Some(y)) if x.name == y.name => {
                    out.push(x.join(y)?); a = &a[1..]; b = &b[1..];
                }
                (Some(x), Some(y)) if x.name < y.name => { out.push(x.clone()); a = &a[1..]; }
                (Some(_), Some(y)) => { out.push(y.clone()); b = &b[1..]; }
                (Some(x), None) | (None, Some(x)) => {
                    if out.len() == 1 && out[0].name == NativeRecordName::Any { out[0] = x.join(&out[0])?; }
                    else { out.push(x.clone()); }
                    if a.is_empty() { b = &b[1..]; } else { a = &a[1..]; }
                }
                (None, None) => unreachable!(),
            }
        }
        Self::collapse(out).ok_or(LatticeError::FunctionClause)
    }
    fn meet(&self, rhs: &Self) -> Result<Option<Self>, LatticeError> {
        if self == rhs { return Ok(Some(self.clone())); }
        if let (Self::Record(a), Self::Record(b)) = (self, rhs) {
            return Ok(a.meet(b)?.map(Self::Record));
        }
        let (mut a, mut b) = if matches!(rhs, Self::Record(_)) { (rhs.rows(), self.rows()) }
                            else { (self.rows(), rhs.rows()) };
        let mut out = Vec::new();
        while let (Some(x), Some(y)) = (a.first(), b.first()) {
            if x.name != y.name && x.name != NativeRecordName::Any && y.name != NativeRecordName::Any {
                if x.name < y.name { a = &a[1..]; } else { b = &b[1..]; }
            } else if a.len() == 1 && x.name == NativeRecordName::Any {
                if let Some(inf) = x.meet(y)? { out.push(inf); }
                b = &b[1..];
            } else {
                if let Some(inf) = x.meet(y)? { out.push(inf); }
                a = &a[1..]; b = &b[1..];
            }
        }
        Ok(Self::collapse(out))
    }
    pub(super) fn join_shared(a: &Arc<Self>, b: &Arc<Self>) -> Result<Option<Arc<Self>>, LatticeError> {
        Ok(Some(reuse(a.join(b)?, a, b)))
    }
    pub(super) fn meet_shared(a: &Arc<Self>, b: &Arc<Self>) -> Result<Option<Arc<Self>>, LatticeError> {
        Ok(a.meet(b)?.map(|v| reuse(v, a, b)))
    }
    pub(super) fn subtract_shared(a: &Arc<Self>, b: &Arc<Self>) -> Result<Option<Arc<Self>>, LatticeError> {
        match (&**a, &**b) {
            (Self::Record(_), _) => Ok(Some(a.clone())),
            (Self::Set(rs), Self::Record(other)) if !rs.is_empty() => {
                let mut kept = Vec::new();
                for record in rs {
                    if record.meet(other)?.as_ref() != Some(record) { kept.push(record.clone()); }
                }
                Ok(Self::collapse(kept).map(|v| reuse_left(v, a)))
            }
            _ => Err(LatticeError::NativeNonTerminating),
        }
    }
    pub(super) fn transform(&self, f: &impl Fn(&Type) -> Result<Type, LatticeError>) -> Result<Self, LatticeError> {
        Ok(match self {
            Self::Record(r) => Self::Record(r.transform(f)?),
            Self::Set(rs) => Self::Set(rs.iter().map(|r| r.transform(f)).collect::<Result<_, _>>()?),
        })
    }
    pub(super) fn limit_shared(value: &Arc<Self>, k: i64) -> Result<Arc<Self>, LatticeError> {
        let out = match &**value {
            Self::Record(r) => Self::Record(r.limit(k)?),
            Self::Set(_) if k == 1 => Self::Record(NativeRecord::any()),
            Self::Set(rs) => {
                let rows = rs.iter().map(|r| r.limit(k)).collect::<Result<Vec<_>, _>>()?;
                if rows.iter().any(|r| *r == NativeRecord::any()) { Self::Record(NativeRecord::any()) }
                else { Self::Set(rows) }
            }
        };
        Ok(reuse_left(out, value))
    }
    pub(super) fn limited_from(&self) -> i32 {
        self.rows().iter().fold(if matches!(self, Self::Set(_)) { 2 } else { 1 }, |k, r| k.max(r.limited_from()))
    }
    pub(super) fn limited(&self, k: i64) -> bool {
        (!matches!(self, Self::Set(_)) || k != 1) && self.rows().iter().all(|r| r.limited(k))
    }
    pub(super) fn nominal(&self) -> bool {
        self.rows().iter().flat_map(NativeRecord::values).any(Type::contains_nominal)
    }
    pub(super) fn blinding_changes(&self) -> bool {
        self.rows().iter().flat_map(|r| r.fields.values()).any(|field| match field {
            RecordField::Missing => true,
            RecordField::Present(t) => t.info.flags & BLINDING_CHANGES != 0,
        })
    }
}

impl Type {
    pub fn native_record(name: NativeValue, fields: Vec<(NativeValue, Type)>) -> NativeTypeOutcome {
        let name = match name { NativeValue::Nil => NativeValue::Atom("nil".into()), other => other };
        let fields = fields.into_iter().collect::<BTreeMap<_, _>>();
        let canonical_name = match &name {
            NativeValue::Atom(a) => Some(NativeRecordName::Atom(a.into()).normalized()),
            NativeValue::Tuple(parts) => match parts.as_slice() {
                [NativeValue::Atom(module), NativeValue::Atom(name)] =>
                    Some(NativeRecordName::Named { module: module.into(), name: name.into() }),
                _ => None,
            },
            _ => None,
        };
        if let Some(name) = canonical_name
            && fields.keys().all(|key| matches!(key, NativeValue::Atom(_)))
        {
            return NativeTypeOutcome::Canonical(Self::record(name, fields.into_iter().map(|(key, value)| {
                let NativeValue::Atom(key) = key else { unreachable!() };
                (key.into(), RecordField::Present(value))
            }).collect()));
        }
        NativeTypeOutcome::Raw(RawNativeType(NativeValue::Tuple(vec![
            NativeValue::Atom("c".into()), NativeValue::Atom("record".into()),
            NativeValue::Tuple(vec![name, NativeValue::Map(fields.into_iter().map(|(key, value)| {
                (key, NativeValue::Tuple(vec![NativeValue::Atom("present".into()), value.to_native_value()]))
            }).collect())]),
            NativeValue::Atom("unknown".into()),
        ])))
    }
    pub fn record_any() -> Self { Self::record(NativeRecordName::Any, BTreeMap::new()) }
    pub fn record(name: NativeRecordName, fields: BTreeMap<Atom, RecordField>) -> Self {
        Self::from_records(Records::Record(NativeRecord { name: name.normalized(), fields }))
    }
    pub fn record_set(mut records: Vec<NativeRecord>) -> Result<Self, String> {
        for record in &mut records { record.name = record.name.clone().normalized(); }
        Ok(Self::from_records(Records::Set(records)))
    }
    pub(super) fn from_records(records: Records) -> Self {
        Self::families(Slots { records: Some(Arc::new(records)), ..Slots::default() })
    }
    pub fn is_native_record(&self) -> bool {
        matches!(self.direct_slots().and_then(FamilyStore::records), Some(Records::Record(_)))
    }
    pub fn is_native_record_named(&self, name: &NativeValue) -> RecordTest {
        let Some(Records::Record(record)) = self.direct_slots().and_then(FamilyStore::records) else {
            return RecordTest::False;
        };
        match name {
            NativeValue::Atom(n) => match &record.name {
                NativeRecordName::Named { name, .. } if name.as_ref() == n => RecordTest::True,
                _ => RecordTest::Unknown,
            },
            _ => RecordTest::False,
        }
    }
    pub fn is_native_record_qualified(&self, module: &NativeValue, name: &NativeValue) -> RecordTest {
        match (self.direct_slots().and_then(FamilyStore::records), module, name) {
            (Some(Records::Record(NativeRecord { name: NativeRecordName::Named { module: m, name: n }, .. })),
             NativeValue::Atom(want_m), NativeValue::Atom(want_n)) => {
                if m.as_ref() == want_m && n.as_ref() == want_n { RecordTest::True } else { RecordTest::Unknown }
            }
            _ => RecordTest::False,
        }
    }
    pub fn record_get(&self, key: &str) -> Result<Type, LatticeError> {
        let Some(Records::Record(record)) = self.direct_slots().and_then(FamilyStore::records) else {
            return Err(LatticeError::FunctionClause);
        };
        Ok(match record.fields.get(&Atom::from(key)) {
            Some(RecordField::Present(t)) => *t,
            Some(RecordField::Missing) => Self::none(),
            None => Self::any(),
        })
    }
    pub fn record_replace(&self, selector: Option<&NativeRecordName>, updates: &[(Atom, Type)]) -> Result<Self, LatticeError> {
        if updates.is_empty() { return Ok(*self); }
        let Some(records) = self.direct_slots().and_then(FamilyStore::records) else { return Ok(*self); };
        let update = |r: &NativeRecord| {
            let mut out = r.clone();
            if selector.is_none_or(|name| name.clone().normalized() == r.name) {
                for (key, value) in updates { out.fields.insert(key.clone(), RecordField::Present(*value)); }
            }
            out
        };
        Ok(Self::from_records(match records {
            Records::Record(r) => Records::Record(update(r)),
            Records::Set(rs) => Records::Set(rs.iter().map(update).collect()),
        }))
    }
    pub fn native_record_field_diffs(&self, actual: &Self, ctx: &PresentationContext) -> Result<String, LatticeError> {
        let one = |t: &Self| match t.direct_slots().and_then(FamilyStore::records) {
            Some(Records::Record(r)) => Some(r.clone()), _ => None,
        };
        let expected = one(self).ok_or(LatticeError::FunctionClause)?;
        let actual = one(actual).ok_or(LatticeError::FunctionClause)?;
        if expected.name != actual.name {
            return match &expected.name {
                NativeRecordName::Named { module, name } =>
                    Ok(format!("#{}:{}{{}}", ctx.atom_to_string(module), ctx.atom_to_string(name))),
                other => Err(LatticeError::BadMatchNative(Box::new(NativeValue::Atom(other.atom().expect("non-qualified record name").into())))),
            };
        }
        let mut differences = Vec::new();
        for (key, value) in &actual.fields {
            if let (Some(RecordField::Present(decl)), RecordField::Present(found)) = (expected.fields.get(key), value)
                && decl.meet(found)?.is_none()
            {
                differences.push(format!("{}::{}", ctx.atom_to_string(key), PresentationContext::default().type_to_string(decl)?));
            }
        }
        differences.reverse();
        Ok(differences.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(n: &str) -> NativeRecordName {
        NativeRecordName::Named { module: "m".into(), name: n.into() }
    }
    fn record(n: &str, fields: &[(&str, RecordField)]) -> Type {
        Type::record(name(n), fields.iter().map(|(k, v)| ((*k).into(), v.clone())).collect())
    }
    fn present(t: Type) -> RecordField { RecordField::Present(t) }

    #[test]
    fn ninth_family_roundtrips_without_becoming_tuple_or_map() {
        let r = record("r", &[("x", present(Type::integer(3.into())))]);
        assert!(r.meet(&Type::tuple_any()).unwrap().is_none());
        assert!(r.meet(&Type::map_any()).unwrap().is_none());
        let mixed = r.join(&Type::atom("ok")).unwrap();
        assert_eq!(Type::from_value(&mixed.to_value()).unwrap(), mixed);
        let Repr::Families(f) = &mixed.repr else { panic!("family expected") };
        let slots = f.slot_values();
        assert_eq!(slots.len(), 9);
        assert_eq!(slots[8], r.to_value());
        assert_eq!(Type::from_union_record(&slots).unwrap(), mixed);
        let mut raw = vec![Type::none().to_value(); 9];
        raw[8] = r.to_value();
        let raw = Type::from_union_record(&raw).unwrap();
        assert_eq!(raw.to_value()["kind"], "union_raw");
        assert!(!raw.is_native_record());
        assert_eq!(Type::from_value(&raw.to_value()).unwrap(), raw);
        assert!(Type::from_union_record(&vec![Type::none().to_value(); 8]).is_err());
        assert_eq!(r.native_order_key(), r.to_native_value().order_key());
    }

    #[test]
    fn record_presence_and_identity_follow_upstream_algebra() {
        let unconstrained = record("r", &[]);
        let missing = record("r", &[("x", RecordField::Missing)]);
        let a = record("r", &[("x", present(Type::all_integers()))]);
        assert_eq!(unconstrained.record_get("x").unwrap(), Type::any());
        assert_eq!(missing.record_get("x").unwrap(), Type::none());
        assert_eq!(a.record_get("x").unwrap(), Type::all_integers());
        assert_eq!(a.meet(&unconstrained).unwrap(), a);
        assert!(a.meet(&missing).unwrap().is_none());
        assert_eq!(a.join(&missing).unwrap(), unconstrained);
        assert_eq!(a.join(&Type::record_any()).unwrap(), Type::record_any());
        assert_eq!(a.subtract(&a).unwrap(), a);
        assert_eq!(a.subtract(&Type::opaque_sentinel()).unwrap(), a);
        let other = record("s", &[("y", present(Type::atom("ok")))]);
        let set = a.join(&other).unwrap();
        assert!(!set.is_native_record());
        assert_eq!(set.elements().unwrap(), vec![set]);
        assert_eq!(set.meet(&Type::record_any()).unwrap(), set);
        assert_eq!(set.meet(&missing).unwrap(), Type::none());
        assert_eq!(set.subtract(&a).unwrap(), other);
        assert_eq!(set.subtract(&set), Err(LatticeError::NativeNonTerminating));
        assert_eq!(set.join(&a).unwrap(), set);
    }

    #[test]
    fn record_variables_are_hidden_until_an_enclosing_substitution_visits_them() {
        let variable = Type::variable_atom("v");
        let r = record("r", &[("x", present(variable))]);
        let bindings = BTreeMap::from([(VariableId::Atom("v".into()), Type::atom("ok"))]);
        assert!(!r.has_var());
        let mut collected = BTreeSet::new();
        r.append_var_names(&mut collected);
        assert!(collected.is_empty());
        assert_eq!(r.substitute(&bindings).unwrap(), r);
        let enclosing = Type::tuple(vec![r, variable]).unwrap();
        assert_eq!(enclosing.substitute(&bindings).unwrap(), Type::tuple(vec![
            record("r", &[("x", present(Type::atom("ok")))]), Type::atom("ok"),
        ]).unwrap());
        assert_eq!(r.structural().unwrap(), r);
        assert_eq!(r.info.flags & (HAS_VAR | ANY_VAR), 0);
    }

    #[test]
    fn record_depth_and_missing_transform_shortcuts_match_native_results() {
        let missing = record("r", &[("x", RecordField::Missing)]);
        assert_eq!(missing.limit(2).unwrap(), missing);
        assert_eq!(missing.limit(1).unwrap(), Type::record_any());
        assert_eq!(missing.widen_to_number().unwrap(), record("r", &[]));
        assert_eq!(missing.abstract_records(&PresentationContext::default()).unwrap(), record("r", &[]));
        let r = record("r", &[("x", present(Type::tuple(vec![Type::all_integers()]).unwrap()))]);
        assert_eq!(r.limit(2).unwrap(), record("r", &[("x", present(Type::tuple_arity(1)))]));
        for value in [Type::record_any(), missing, r, r.join(&record("s", &[])).unwrap()] {
            for depth in -1..6 { assert_eq!(value.is_limited(depth), value.is_limited_walk(depth)); }
        }
    }

    #[test]
    fn record_predicates_use_raw_names_and_do_not_unwrap_nominals() {
        let r = record("r", &[]);
        assert!(r.predicate(TypePredicate::Record).unwrap());
        assert_eq!(r.is_native_record_named(&NativeValue::Atom("r".into())), RecordTest::True);
        assert_eq!(r.is_native_record_named(&NativeValue::Atom("other".into())), RecordTest::Unknown);
        assert_eq!(r.is_native_record_named(&Type::atom("r").to_native_value()), RecordTest::False);
        assert_eq!(r.is_native_record_qualified(&NativeValue::Atom("m".into()), &NativeValue::Atom("r".into())), RecordTest::True);
        assert_eq!(Type::record_any().is_native_record_qualified(&NativeValue::Atom("m".into()), &NativeValue::Atom("r".into())), RecordTest::False);
        assert!(!Type::nominal(NominalName::Symbol("n".into()), r).predicate(TypePredicate::Record).unwrap());
    }

    #[test]
    fn native_record_printing_and_field_diffs_keep_upstream_order() {
        let ctx = PresentationContext::default();
        let a = record("a", &[("x", present(Type::all_integers()))]);
        let b = record("b", &[("y", present(Type::atom("ok")))]);
        assert_eq!(ctx.type_to_string(&a.join(&b).unwrap()).unwrap(), "#m:a{x=integer()} | #m:b{y='ok'}");
        assert_eq!(ctx.type_to_string(&Type::record_any()).unwrap(), "record()");
        let wildcard = Type::record(NativeRecordName::Any, BTreeMap::from([("x".into(), present(Type::any()))]));
        assert_eq!(ctx.type_to_string(&wildcard).unwrap(), "_#{x=any()}");
        let expected = record("r", &[("a", present(Type::all_integers())), ("b", present(Type::all_atoms()))]);
        let actual = record("r", &[("a", present(Type::atom("bad"))), ("b", present(Type::integer(0.into())))]);
        assert_eq!(expected.native_record_field_diffs(&actual, &ctx).unwrap(), "b::atom(), a::integer()");
        assert_eq!(expected.native_record_field_diffs(&b, &ctx).unwrap(), "#m:r{}");
        assert_eq!(Type::record_any().native_record_field_diffs(&b, &ctx), Err(LatticeError::BadMatchNative(Box::new(NativeValue::Atom("nil".into())))));
    }

    #[test]
    fn record_unification_ignores_names_but_respects_native_field_iteration() {
        let x = Type::variable_atom("x");
        let y = Type::variable_atom("y");
        let a = record("a", &[("a", present(x)), ("b", present(y))]);
        let b = record("b", &[("c", present(Type::all_integers())), ("d", present(Type::all_atoms()))]);
        let bindings = a.unify_bindings(&b).unwrap();
        assert_eq!(bindings[&VariableId::Atom("x".into())], UnificationValue::Type(Type::all_integers()));
        let order = |keys: &[Atom]| Ok(if keys.first().is_some_and(|k| *k == "c") { vec![1, 0] } else { vec![0, 1] });
        let bindings = a.unify_bindings_with_record_order(&b, &order).unwrap();
        assert_eq!(bindings[&VariableId::Atom("x".into())], UnificationValue::Type(Type::all_atoms()));
        let large = Type::record(name("large"), (0..33).map(|i| (format!("f{i:02}").into(), present(Type::any()))).collect());
        assert!(matches!(large.unify_bindings(&large), Err(UnificationError::NativeError(LatticeError::RecordIterationOrderRequired))));
        assert!(large.unify_bindings_with_record_order(&large, &|keys| Ok((0..keys.len()).rev().collect())).is_ok());
    }

    #[test]
    fn native_record_replacement_keeps_raw_set_shape_and_noop_updates() {
        let r = NativeRecord { name: name("r"), fields: BTreeMap::new() };
        let raw_set = Type::record_set(vec![r.clone()]).unwrap();
        assert_eq!(raw_set.to_value()["kind"], "record_set");
        assert_eq!(Type::from_value(&raw_set.to_value()).unwrap(), raw_set);
        let updated = raw_set.record_replace(None, &[("x".into(), Type::all_integers())]).unwrap();
        assert_eq!(updated.to_value()["kind"], "record_set");
        let direct = Type::record(r.name, r.fields);
        assert_eq!(direct.record_replace(Some(&name("different")), &[("x".into(), Type::all_integers())]).unwrap(), direct);
        assert_eq!(Type::any().record_replace(None, &[("x".into(), Type::none())]).unwrap(), Type::any());
        assert_eq!(direct.record_replace(None, &[]).unwrap(), direct);
    }

    #[test]
    fn runtime_record_values_have_the_generic_record_type() {
        let value = RuntimeTerm::record("m", "r", BTreeMap::from([("a".into(), RuntimeTerm::integer(7.into()))]));
        assert_eq!(Type::from_runtime_term(&value).unwrap(), Type::record_any());
    }

    #[test]
    fn exported_constructor_keeps_raw_names_keys_and_last_duplicate_value() {
        let fields = vec![
            (NativeValue::Atom("x".into()), Type::all_integers()),
            (NativeValue::Atom("x".into()), Type::all_atoms()),
        ];
        let NativeTypeOutcome::Canonical(t) = Type::native_record(NativeValue::Nil, fields) else {
            panic!("canonical record expected");
        };
        assert_eq!(t.record_get("x").unwrap(), Type::all_atoms());
        let NativeTypeOutcome::Raw(raw) = Type::native_record(
            NativeValue::Integer(42.into()),
            vec![(NativeValue::Integer(7.into()), Type::all_integers())],
        ) else { panic!("raw constructor result expected"); };
        let NativeValue::Tuple(c) = raw.to_native_value() else { panic!("native c record expected") };
        assert_eq!(c[1], NativeValue::Atom("record".into()));
        let NativeValue::Tuple(elements) = &c[2] else { panic!("record elements expected") };
        assert_eq!(elements[0], NativeValue::Integer(42.into()));
        assert!(matches!(&elements[1], NativeValue::Map(fields) if fields.len() == 1));
    }

    #[test]
    fn missing_fields_can_trigger_opacity_conflicts_without_nominals() {
        let missing = record("r", &[("x", RecordField::Missing)]);
        let present = record("r", &[("x", present(Type::all_integers()))]);
        assert!(!missing.contains_nominal());
        assert!(!present.contains_nominal());
        for (a, b) in [
            (missing, present),
            (Type::tuple(vec![missing]).unwrap(), Type::tuple(vec![present]).unwrap()),
            (record("outer", &[("inner", RecordField::Present(missing))]),
             record("outer", &[("inner", RecordField::Present(present))])),
            (Type::map_pairs(&[(Type::atom("x"), missing)]).unwrap(),
             Type::map_pairs(&[(Type::atom("x"), present)]).unwrap()),
        ] {
            assert_eq!(a.opacity_conflict(&b, "m").unwrap(), OpacityConflict::ExpectedTransparent);
            assert_eq!(b.opacity_conflict(&a, "m").unwrap(), OpacityConflict::ExpectedOpaque);
        }
        let opaque = Type::nominal(NominalName::Defined {
            module: "other".into(), name: "t".into(), arity: 0, opacity: Opacity::Opaque,
        }, Type::all_integers());
        let hidden = record("r", &[("x", RecordField::Present(opaque))]);
        assert!(hidden.contains_nominal());
        assert_eq!(hidden.structural().unwrap(), hidden);
        assert_eq!(present.opacity_conflict(&hidden, "m").unwrap(), OpacityConflict::ExpectedOpaque);
    }
}

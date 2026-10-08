use crate::{
    form_error::ReasonBuilder,
    native_form_environment::RawFormTerm,
    raw_literal::{RawLiteral, RawLiteralArena, normal_list_cmp, normal_union, normal_usort},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default)]
pub struct ModuleDependencies {
    rows: BTreeMap<RawLiteral, RawLiteral>,
    order: crate::native_dict::NativeDictOrder<DependencyKey>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct DependencyKey {
    literal: RawLiteral,
    hash: u32,
}
impl crate::native_dict::PinnedPhashKey for DependencyKey {
    fn pinned_phash_hash(&self) -> u32 {
        self.hash
    }
}
fn dictionary_key(literal: &RawLiteral) -> Result<DependencyKey, String> {
    let key = crate::native_runtime_hash::NativeHashKey::from_raw_with_order(
        literal.raw(),
        literal.order(),
    )
    .map_err(|e| e.to_string())?;
    Ok(DependencyKey {
        literal: literal.clone(),
        hash: crate::native_dict::pinned_hash_key(&key).map_err(|e| e.to_string())?,
    })
}
impl PartialEq for ModuleDependencies {
    fn eq(&self, other: &Self) -> bool {
        self.rows == other.rows
    }
}
impl Eq for ModuleDependencies {}
impl ModuleDependencies {
    pub fn collect_type_rows(rows: Vec<(RawLiteral, Vec<RawLiteral>)>) -> Self {
        // sofs:rel_type normalizes nested sets while prepending each input row.
        let mut rows = rows
            .into_iter()
            .map(|(module, dependencies)| {
                (
                    module,
                    normal_usort(dependencies.into_iter().rev().collect()),
                )
            })
            .rev()
            .collect::<Vec<_>>();
        rows.sort_by(|(a, ad), (b, bd)| a.normal_cmp(b).then_with(|| normal_list_cmp(ad, bd)));
        rows.dedup_by(|(a, ad), (b, bd)| {
            a.normal_cmp(b).is_eq() && normal_list_cmp(ad, bd).is_eq()
        });
        let mut families: Vec<(RawLiteral, Vec<Vec<RawLiteral>>)> = Vec::new();
        for (module, dependencies) in rows {
            if let Some((previous, sets)) = families.last_mut()
                && previous.normal_cmp(&module).is_eq()
            {
                sets.push(dependencies);
            } else {
                families.push((module, vec![dependencies]));
            }
        }
        let mut relation = Vec::new();
        for (module, sets) in families {
            for dependency in family_union(sets) {
                relation.push((module.clone(), dependency));
            }
        }
        // family_to_relation prepends pairs; converse prepends again then sorts.
        let mut converse = relation
            .into_iter()
            .map(|(from, to)| (to, from))
            .collect::<Vec<_>>();
        converse.sort_by(|(a, av), (b, bv)| a.normal_cmp(b).then_with(|| av.normal_cmp(bv)));
        converse.retain(|(a, b)| !a.normal_cmp(b).is_eq());
        let mut result: BTreeMap<RawLiteral, Vec<RawLiteral>> = BTreeMap::new();
        let mut current: Option<RawLiteral> = None;
        for (key, value) in converse {
            let key = match &current {
                Some(old) if old.normal_cmp(&key).is_eq() => old.clone(),
                _ => {
                    current = Some(key.clone());
                    key
                }
            };
            result.entry(key).or_default().push(value);
        }
        Self::from_rows(
            result
                .into_iter()
                .map(|(key, values)| (key, proper_list(&values))),
        )
        .expect("literal dependency dictionary keys")
    }
    pub fn from_named_call_dependencies(rows: &BTreeMap<String, BTreeSet<String>>) -> Self {
        Self::from_rows(rows.iter().map(|(key, values)| {
            (
                RawLiteral::atom(key),
                proper_list(
                    &values
                        .iter()
                        .map(|name| RawLiteral::atom(name))
                        .collect::<Vec<_>>(),
                ),
            )
        }))
        .expect("atom dependency dictionary keys")
    }
    pub fn from_rows(
        rows: impl IntoIterator<Item = (RawLiteral, RawLiteral)>,
    ) -> Result<Self, String> {
        let mut result = Self::default();
        for (key, value) in rows {
            result.order.store(dictionary_key(&key)?);
            result.rows.insert(key, value);
        }
        Ok(result)
    }
    pub fn from_raw_dict(raw: &RawFormTerm) -> Result<Self, DependencyError> {
        use crate::core::ErlangTerm;
        let terms = &raw.arena().terms;
        let ErlangTerm::Tuple(fields) = raw.term() else {
            return Err(DependencyError::BadRecord(RawLiteral::new(raw.clone())?));
        };
        if fields.len() != 9 || !matches!(&terms[fields[0].0],ErlangTerm::Atom(a) if a=="dict") {
            return Err(DependencyError::BadRecord(RawLiteral::new(raw.clone())?));
        }
        let number = |i: usize| match &terms[fields[i].0] {
            ErlangTerm::Integer(v) => v.to_string().parse::<usize>().map_err(|e| e.to_string()),
            _ => Err("dict counter is not an integer".into()),
        };
        let size = number(1)?;
        let n = number(2)?;
        let maxn = number(3)?;
        let bso = number(4)?;
        let ErlangTerm::Tuple(segments) = &terms[fields[8].0] else {
            return Err("dict segments are not a tuple".into());
        };
        let literals = RawLiteralArena::new(raw.arena().clone())?;
        let mut rows = BTreeMap::new();
        let mut buckets = Vec::new();
        for segment in segments {
            let ErlangTerm::Tuple(slots) = &terms[segment.0] else {
                return Err("dict segment is not a tuple".into());
            };
            for slot in slots {
                let pairs = list_values(&literals.at(*slot)?)
                    .map_err(|_| "dict bucket is not a proper list")?;
                let mut bucket = Vec::new();
                for pair in pairs {
                    let mut cursor = ListCursor::new(&pair);
                    let key = cursor.head().ok_or("dict entry has no key")?;
                    cursor.advance();
                    let value = cursor.remainder();
                    bucket.push(dictionary_key(&key)?);
                    rows.insert(key, value);
                }
                buckets.push(bucket);
            }
        }
        let order =
            crate::native_dict::NativeDictOrder::from_record_shape(size, n, maxn, bso, buckets)?;
        Ok(Self { rows, order })
    }

    pub fn from_raw_map(raw: &RawFormTerm) -> Result<Self, String> {
        let crate::core::ErlangTerm::Map(entries) = raw.term() else {
            return Err("dependency header is not a map".into());
        };
        let literals = RawLiteralArena::new(raw.arena().clone())?;
        Self::from_rows(
            entries
                .iter()
                .map(|(key, value)| Ok((literals.at(*key)?, literals.at(*value)?)))
                .collect::<Result<Vec<_>, String>>()?,
        )
    }
    pub fn rows(&self) -> impl Iterator<Item = (&RawLiteral, &RawLiteral)> {
        self.rows.iter()
    }
    pub fn get(&self, module: &RawLiteral) -> Option<&RawLiteral> {
        self.rows.get(module)
    }
    pub fn set_members(
        &self,
        module: &RawLiteral,
    ) -> Result<Option<BTreeSet<RawLiteral>>, DependencyError> {
        self.get(module)
            .map(|value| {
                list_values(value)
                    .map(|values| values.into_iter().collect())
                    .map_err(|_| {
                        DependencyError::Native(crate::native_error::NativeTransferError::Badarg)
                    })
            })
            .transpose()
    }
    pub fn retaining_values(
        &self,
        mut keep: impl FnMut(&RawLiteral) -> bool,
    ) -> Result<Self, DependencyError> {
        let mut rows = BTreeMap::new();
        for key in self.order.map_keys() {
            let key = &key.literal;
            let value = &self.rows[key];
            let mut cursor = ListCursor::new(value);
            let mut values = Vec::new();
            while let Some(head) = cursor.head() {
                if keep(&head) {
                    values.push(head);
                }
                cursor.advance();
            }
            if !cursor.is_nil() {
                return Err(DependencyError::BadGenerator(cursor.remainder()));
            }
            if !values.is_empty() {
                rows.insert(key.clone(), proper_list(&values));
            }
        }
        let mut order = self.order.clone();
        order.retain_keys(|key| rows.contains_key(&key.literal));
        Ok(Self { rows, order })
    }
    pub fn union(&self, other: &Self) -> Result<Self, DependencyError> {
        let (mut result, source, source_is_old) = if self.rows.len() < other.rows.len() {
            (other.clone(), self, true)
        } else {
            (self.clone(), other, false)
        };
        for key in source.order.fold_keys() {
            let values = &source.rows[&key.literal];
            let value = match result.rows.get(&key.literal) {
                Some(existing) => {
                    if source_is_old {
                        ordset_union(values, existing)?
                    } else {
                        ordset_union(existing, values)?
                    }
                }
                None => values.clone(),
            };
            result.order.store(key.clone());
            result.rows.insert(key.literal, value);
        }
        Ok(result)
    }

    pub fn named_projection(&self) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
        self.rows()
            .map(|(key, values)| {
                Ok((
                    key.as_atom().ok_or("dependency key is not an atom")?.into(),
                    list_values(values)
                        .map_err(|_| "dependency value is not a proper list")?
                        .iter()
                        .map(|value| {
                            value
                                .as_atom()
                                .map(str::to_owned)
                                .ok_or_else(|| "dependent module is not an atom".into())
                        })
                        .collect::<Result<_, String>>()?,
                ))
            })
            .collect()
    }
    pub fn to_raw_dict(&self) -> RawFormTerm {
        let mut b = ReasonBuilder::new();
        let root = crate::native_dict::record_term(&self.order, &mut b, |key, b| {
            (
                b.copy(key.literal.raw()),
                b.copy(self.rows[&key.literal].raw()),
            )
        });
        b.finish(root)
    }
    pub fn to_raw(&self) -> RawFormTerm {
        let mut b = ReasonBuilder::new();
        let rows = self
            .rows()
            .map(|(key, values)| (b.copy(key.raw()), b.copy(values.raw())))
            .collect();
        let root = b.push(crate::core::ErlangTerm::Map(rows));
        b.finish(root)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DependencyError {
    Native(crate::native_error::NativeTransferError),
    BadGenerator(RawLiteral),
    BadRecord(RawLiteral),
    Transport(String),
}
impl std::fmt::Display for DependencyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Native(error) => error.fmt(f),
            Self::BadGenerator(_) => f.write_str("OTP bad_generator in module dependencies"),
            Self::BadRecord(_) => f.write_str("OTP badrecord in module dependencies"),
            Self::Transport(message) => f.write_str(message),
        }
    }
}
impl std::error::Error for DependencyError {}
impl From<String> for DependencyError {
    fn from(value: String) -> Self {
        Self::Transport(value)
    }
}
impl From<&str> for DependencyError {
    fn from(value: &str) -> Self {
        Self::Transport(value.into())
    }
}
fn proper_list(values: &[RawLiteral]) -> RawLiteral {
    let mut b = ReasonBuilder::new();
    let values = values.iter().map(|v| b.copy(v.raw())).collect();
    let root = b.list(values);
    RawLiteral::new(b.finish(root)).expect("constructed dependency list")
}
struct ListCursor {
    value: RawLiteral,
    offset: usize,
}
impl ListCursor {
    fn new(value: &RawLiteral) -> Self {
        Self {
            value: value.clone(),
            offset: 0,
        }
    }
    fn normalize(&mut self) {
        while let crate::core::ErlangTerm::List { items, tail } = self.value.raw().term() {
            if self.offset < items.len() {
                break;
            }
            self.value = self.value.at(*tail).expect("validated literal child");
            self.offset = 0;
        }
    }
    fn head(&mut self) -> Option<RawLiteral> {
        self.normalize();
        match self.value.raw().term() {
            crate::core::ErlangTerm::List { items, .. } => Some(
                self.value
                    .at(items[self.offset])
                    .expect("validated literal child"),
            ),
            _ => None,
        }
    }
    fn advance(&mut self) {
        self.offset += 1;
        self.normalize();
    }
    fn remainder(&mut self) -> RawLiteral {
        self.normalize();
        if self.offset == 0 {
            return self.value.clone();
        }
        let crate::core::ErlangTerm::List { items, tail } = self.value.raw().term() else {
            unreachable!()
        };
        let mut b = ReasonBuilder::new();
        let head = items[self.offset..]
            .iter()
            .map(|id| b.copy(self.value.at(*id).expect("validated literal child").raw()))
            .collect();
        let tail = b.copy(self.value.at(*tail).expect("validated literal child").raw());
        let root = b.list_tail(head, tail);
        RawLiteral::new(b.finish(root)).expect("constructed dependency tail")
    }
    fn is_nil(&mut self) -> bool {
        self.normalize();
        matches!(self.value.raw().term(), crate::core::ErlangTerm::Nil)
    }
}
fn list_values(value: &RawLiteral) -> Result<Vec<RawLiteral>, RawLiteral> {
    let mut cursor = ListCursor::new(value);
    let mut values = Vec::new();
    while let Some(head) = cursor.head() {
        values.push(head);
        cursor.advance();
    }
    if cursor.is_nil() {
        Ok(values)
    } else {
        Err(cursor.remainder())
    }
}
fn ordset_union(left: &RawLiteral, right: &RawLiteral) -> Result<RawLiteral, DependencyError> {
    let mut left = ListCursor::new(left);
    let mut right = ListCursor::new(right);
    let mut prefix = Vec::new();
    let tail = loop {
        match (left.head(), right.head()) {
            (Some(a), Some(b)) => match a.normal_cmp(&b) {
                std::cmp::Ordering::Less => {
                    prefix.push(a);
                    left.advance();
                }
                std::cmp::Ordering::Greater => {
                    prefix.push(b);
                    right.advance();
                    std::mem::swap(&mut left, &mut right);
                }
                std::cmp::Ordering::Equal => {
                    prefix.push(a);
                    left.advance();
                    right.advance();
                }
            },
            _ if left.is_nil() => break right.remainder(),
            _ if right.is_nil() => break left.remainder(),
            _ => {
                return Err(DependencyError::Native(
                    crate::native_error::NativeTransferError::FunctionClause,
                ));
            }
        }
    };
    if prefix.is_empty() {
        return Ok(tail);
    }
    let mut b = ReasonBuilder::new();
    let prefix = prefix.iter().map(|v| b.copy(v.raw())).collect();
    let tail = b.copy(tail.raw());
    let root = b.list_tail(prefix, tail);
    Ok(RawLiteral::new(b.finish(root)).expect("constructed ordset union"))
}

fn family_union(sets: Vec<Vec<RawLiteral>>) -> Vec<RawLiteral> {
    let mut sets = sets.into_iter().filter(|set| !set.is_empty());
    let Some(first) = sets.next() else {
        return Vec::new();
    };
    let mut last = first.last().expect("nonempty set").clone();
    let mut block = first;
    let mut blocks = Vec::new();
    for set in sets {
        let next_last = set.last().expect("nonempty set").clone();
        if set[0].normal_cmp(&last).is_gt() {
            block.extend(set);
        } else {
            blocks.push(block);
            block = set;
        }
        last = next_last;
    }
    blocks.push(block);
    // sofs:lunion prepends disjoint blocks, then lists:umerge preserves its first input.
    blocks
        .into_iter()
        .rev()
        .fold(Vec::new(), |merged, block| normal_union(&merged, &block))
}

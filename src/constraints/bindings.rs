#[cfg(test)]
use crate::lattice::NativeValue;
use crate::lattice::{Type, VariableId};
use std::cmp::Ordering;
use std::sync::Arc;
/// Integer/atom names used by substitution and raw records used by the pinned
/// reference solver are distinct Erlang map keys, even for the same variable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingKey {
    Label(VariableId),
    RawVariableRecord(VariableId),
}
impl BindingKey {
    pub fn label(&self) -> Option<&VariableId> {
        match self {
            Self::Label(v) => Some(v),
            Self::RawVariableRecord(_) => None,
        }
    }
    #[cfg(test)]
    fn native(&self) -> NativeValue {
        match self {
            Self::Label(VariableId::Integer(n)) => NativeValue::Integer(n.to_bigint()),
            Self::Label(VariableId::Atom(a)) => NativeValue::Atom(a.clone()),
            Self::RawVariableRecord(VariableId::Integer(n)) => {
                Type::variable_id(VariableId::Integer(n.clone())).to_native_value()
            }
            Self::RawVariableRecord(VariableId::Atom(a)) => {
                Type::variable_atom(a.clone()).to_native_value()
            }
        }
    }
}
impl From<VariableId> for BindingKey {
    fn from(v: VariableId) -> Self {
        Self::Label(v)
    }
}
impl Ord for BindingKey {
    fn cmp(&self, other: &Self) -> Ordering {
        let result = match (self, other) {
            (Self::Label(a), Self::Label(b))
            | (Self::RawVariableRecord(a), Self::RawVariableRecord(b)) => a.cmp(b),
            (Self::Label(_), Self::RawVariableRecord(_)) => Ordering::Less,
            (Self::RawVariableRecord(_), Self::Label(_)) => Ordering::Greater,
        };
        result
    }
}
impl PartialOrd for BindingKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
/// Bindings keyed by `BindingKey`, iterated in `BindingKey` order. Labels that
/// are non-negative machine integers (nearly every key) live in a persistent
/// radix trie, so a first write to a shared map copies a few 16-slot nodes and
/// a lookup is a handful of array indexings; every other key lives in `rest`.
/// No `rest` key sorts between the trie's keys: they are all below
/// `Label(0)` or above every machine integer label.
#[derive(Clone, Default)]
pub struct BindingMap {
    labels: LabelTrie,
    rest: im::OrdMap<BindingKey, Type>,
    /// Under `verify_fast_paths()`, the same contents in the map this replaced.
    shadow: Option<im::OrdMap<BindingKey, Type>>,
}
impl BindingKey {
    fn trie_index(&self) -> Option<u64> {
        match self {
            Self::Label(VariableId::Integer(n)) => n.small().and_then(|n| u64::try_from(n).ok()),
            _ => None,
        }
    }
    fn trie_key(index: u64) -> Self {
        Self::Label(VariableId::Integer((index as i64).into()))
    }
}
impl BindingMap {
    pub fn new() -> Self {
        Self {
            labels: LabelTrie::default(),
            rest: im::OrdMap::new(),
            shadow: crate::lattice::verify_fast_paths().then(im::OrdMap::new),
        }
    }
    pub fn get(&self, key: &BindingKey) -> Option<&Type> {
        let found = match key.trie_index() {
            Some(index) => self.labels.get(index),
            None => self.rest.get(key),
        };
        if let Some(shadow) = &self.shadow {
            assert_eq!(found, shadow.get(key), "binding map lookup");
        }
        found
    }
    pub fn contains_key(&self, key: &BindingKey) -> bool {
        self.get(key).is_some()
    }
    pub fn insert(&mut self, key: BindingKey, value: Type) -> Option<Type> {
        if let Some(shadow) = &mut self.shadow {
            let expected = shadow.insert(key.clone(), value.clone());
            let old = self.insert_unshadowed(key, value);
            assert_eq!(old, expected, "binding map insert");
            return old;
        }
        self.insert_unshadowed(key, value)
    }
    fn insert_unshadowed(&mut self, key: BindingKey, value: Type) -> Option<Type> {
        match key.trie_index() {
            Some(index) => self.labels.insert(index, value),
            None => self.rest.insert(key, value),
        }
    }
    pub fn remove(&mut self, key: &BindingKey) -> Option<Type> {
        let old = match key.trie_index() {
            Some(index) => self.labels.remove(index),
            None => self.rest.remove(key),
        };
        if let Some(shadow) = &mut self.shadow {
            assert_eq!(old, shadow.remove(key), "binding map remove");
        }
        old
    }
    pub fn len(&self) -> usize {
        self.labels.len + self.rest.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn iter(&self) -> impl Iterator<Item = (BindingKey, &Type)> + '_ {
        let zero = BindingKey::trie_key(0);
        fn owned<'a>((k, v): (&BindingKey, &'a Type)) -> (BindingKey, &'a Type) {
            (k.clone(), v)
        }
        self.rest
            .range(..zero.clone())
            .map(owned)
            .chain(
                self.labels
                    .iter()
                    .map(|(index, value)| (BindingKey::trie_key(index), value)),
            )
            .chain(self.rest.range(zero..).map(owned))
    }
    pub fn keys(&self) -> impl Iterator<Item = BindingKey> + '_ {
        self.iter().map(|(key, _)| key)
    }
    pub fn ptr_eq(&self, other: &Self) -> bool {
        self.labels.ptr_eq(&other.labels) && self.rest.ptr_eq(&other.rest)
    }
    /// Under `verify_fast_paths()`, asserts the contents and iteration order
    /// match the shadow `im::OrdMap`.
    pub fn verify_shadow(&self) {
        if let Some(shadow) = &self.shadow {
            assert_eq!(self.len(), shadow.len(), "binding map size");
            assert!(
                self.iter()
                    .zip(shadow.iter())
                    .all(|((k, v), (sk, sv))| k == *sk && v == sv),
                "binding map contents or order"
            );
        }
    }
}
impl PartialEq for BindingMap {
    fn eq(&self, other: &Self) -> bool {
        let equal = self.labels == other.labels && self.rest == other.rest;
        if let (Some(a), Some(b)) = (&self.shadow, &other.shadow) {
            assert_eq!(equal, a == b, "binding map equality");
        }
        equal
    }
}
impl Eq for BindingMap {}
impl std::fmt::Debug for BindingMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}
impl FromIterator<(BindingKey, Type)> for BindingMap {
    fn from_iter<I: IntoIterator<Item = (BindingKey, Type)>>(entries: I) -> Self {
        let mut map = Self::new();
        for (key, value) in entries {
            map.insert(key, value);
        }
        map
    }
}
impl From<Vec<(BindingKey, Type)>> for BindingMap {
    fn from(entries: Vec<(BindingKey, Type)>) -> Self {
        entries.into_iter().collect()
    }
}
impl std::ops::Index<&BindingKey> for BindingMap {
    type Output = Type;
    fn index(&self, key: &BindingKey) -> &Type {
        self.get(key).expect("binding key absent")
    }
}
impl IntoIterator for BindingMap {
    type Item = (BindingKey, Type);
    type IntoIter = std::vec::IntoIter<(BindingKey, Type)>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
            .map(|(k, v)| (k, v.clone()))
            .collect::<Vec<_>>()
            .into_iter()
    }
}

/// A set of binding keys, kept sorted and without duplicates in one vector:
/// the sets the solver passes around are mostly a handful of keys.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeySet(Vec<BindingKey>);
impl KeySet {
    pub fn new() -> Self {
        Self(Vec::new())
    }
    /// The set of the keys in `keys`, in any order and with repeats.
    pub fn from_unsorted(mut keys: Vec<BindingKey>) -> Self {
        keys.sort_unstable();
        keys.dedup();
        Self(keys)
    }
    pub fn insert(&mut self, key: BindingKey) -> bool {
        match self.0.binary_search(&key) {
            Ok(_) => false,
            Err(at) => {
                self.0.insert(at, key);
                true
            }
        }
    }
    pub fn contains(&self, key: &BindingKey) -> bool {
        self.0.binary_search(key).is_ok()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn iter(&self) -> std::slice::Iter<'_, BindingKey> {
        self.0.iter()
    }
    /// Adds every key of `other`.
    pub fn union(&mut self, other: Self) {
        if self.0.is_empty() {
            *self = other;
        } else if !other.0.is_empty() {
            let a = std::mem::take(&mut self.0);
            let mut out = Vec::with_capacity(a.len() + other.0.len());
            let mut a = a.into_iter().peekable();
            let mut b = other.0.into_iter().peekable();
            loop {
                let next = match (a.peek(), b.peek()) {
                    (Some(x), Some(y)) => match x.cmp(y) {
                        Ordering::Less => a.next(),
                        Ordering::Greater => b.next(),
                        Ordering::Equal => {
                            b.next();
                            a.next()
                        }
                    },
                    (Some(_), None) => a.next(),
                    (None, _) => b.next(),
                };
                match next {
                    Some(key) => out.push(key),
                    None => break,
                }
            }
            self.0 = out;
        }
    }
    pub fn into_vec(self) -> Vec<BindingKey> {
        self.0
    }
}
impl IntoIterator for KeySet {
    type Item = BindingKey;
    type IntoIter = std::vec::IntoIter<BindingKey>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
impl<'a> IntoIterator for &'a KeySet {
    type Item = &'a BindingKey;
    type IntoIter = std::slice::Iter<'a, BindingKey>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
impl FromIterator<BindingKey> for KeySet {
    fn from_iter<I: IntoIterator<Item = BindingKey>>(keys: I) -> Self {
        Self::from_unsorted(keys.into_iter().collect())
    }
}
impl<const N: usize> From<[BindingKey; N]> for KeySet {
    fn from(keys: [BindingKey; N]) -> Self {
        keys.into_iter().collect()
    }
}

const BITS: u32 = 4;
const WIDTH: usize = 1 << BITS;
const MASK: u64 = WIDTH as u64 - 1;
/// Leaves sit at level 0; a node at level `l` covers `WIDTH^(l+1)` keys.
/// Empty nodes are pruned, so the shape is a function of the key set and height.
#[derive(Clone)]
enum Node {
    Branch([Option<Arc<Node>>; WIDTH]),
    Leaf([Option<Type>; WIDTH]),
}
impl Node {
    fn empty(level: u32) -> Arc<Self> {
        Arc::new(if level == 0 {
            Self::Leaf([const { None }; WIDTH])
        } else {
            Self::Branch([const { None }; WIDTH])
        })
    }
    fn is_empty(&self) -> bool {
        match self {
            Self::Branch(c) => c.iter().all(Option::is_none),
            Self::Leaf(v) => v.iter().all(Option::is_none),
        }
    }
    fn same(a: &Arc<Self>, b: &Arc<Self>) -> bool {
        Arc::ptr_eq(a, b)
            || match (&**a, &**b) {
                (Self::Branch(x), Self::Branch(y)) => x.iter().zip(y).all(|p| match p {
                    (None, None) => true,
                    (Some(x), Some(y)) => Self::same(x, y),
                    _ => false,
                }),
                (Self::Leaf(x), Self::Leaf(y)) => x == y,
                _ => false,
            }
    }
}
#[derive(Clone, Default)]
struct LabelTrie {
    root: Option<Arc<Node>>,
    /// The root's level; it only grows.
    height: u32,
    len: usize,
}
fn slot(index: u64, level: u32) -> usize {
    ((index >> (BITS * level)) & MASK) as usize
}
impl LabelTrie {
    fn covers(&self, index: u64) -> bool {
        BITS * (self.height + 1) >= u64::BITS || index >> (BITS * (self.height + 1)) == 0
    }
    fn get(&self, index: u64) -> Option<&Type> {
        if !self.covers(index) {
            return None;
        }
        let mut node = self.root.as_deref()?;
        let mut level = self.height;
        loop {
            match node {
                Node::Branch(c) => node = c[slot(index, level)].as_deref()?,
                Node::Leaf(v) => return v[slot(index, 0)].as_ref(),
            }
            level -= 1;
        }
    }
    fn insert(&mut self, index: u64, value: Type) -> Option<Type> {
        while !self.covers(index) {
            if let Some(root) = self.root.take() {
                let mut children = [const { None }; WIDTH];
                children[0] = Some(root);
                self.root = Some(Arc::new(Node::Branch(children)));
            }
            self.height += 1;
        }
        let mut level = self.height;
        let mut node = Arc::make_mut(self.root.get_or_insert_with(|| Node::empty(level)));
        loop {
            match node {
                Node::Branch(c) => {
                    node = Arc::make_mut(
                        c[slot(index, level)].get_or_insert_with(|| Node::empty(level - 1)),
                    );
                    level -= 1;
                }
                Node::Leaf(v) => {
                    let old = v[slot(index, 0)].replace(value);
                    if old.is_none() {
                        self.len += 1;
                    }
                    return old;
                }
            }
        }
    }
    fn remove(&mut self, index: u64) -> Option<Type> {
        self.get(index)?;
        fn remove_in(node: &mut Arc<Node>, index: u64, level: u32) -> Option<Type> {
            match Arc::make_mut(node) {
                Node::Branch(c) => {
                    let child = &mut c[slot(index, level)];
                    let old = remove_in(child.as_mut()?, index, level - 1);
                    if child.as_ref().is_some_and(|n| n.is_empty()) {
                        *child = None;
                    }
                    old
                }
                Node::Leaf(v) => v[slot(index, 0)].take(),
            }
        }
        let root = self.root.as_mut()?;
        let old = remove_in(root, index, self.height);
        if root.is_empty() {
            self.root = None;
        }
        self.len -= 1;
        old
    }
    fn ptr_eq(&self, other: &Self) -> bool {
        self.height == other.height
            && match (&self.root, &other.root) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
    }
    fn iter(&self) -> LabelIter<'_> {
        LabelIter {
            stack: self
                .root
                .as_deref()
                .map(|root| (root, 0, 0, self.height))
                .into_iter()
                .collect(),
        }
    }
}
impl PartialEq for LabelTrie {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len
            && if self.height == other.height {
                match (&self.root, &other.root) {
                    (None, None) => true,
                    (Some(a), Some(b)) => Node::same(a, b),
                    _ => false,
                }
            } else {
                self.iter().eq(other.iter())
            }
    }
}
/// In-order walk: (node, next slot, first index under the node, level).
struct LabelIter<'a> {
    stack: Vec<(&'a Node, usize, u64, u32)>,
}
impl<'a> Iterator for LabelIter<'a> {
    type Item = (u64, &'a Type);
    fn next(&mut self) -> Option<Self::Item> {
        while let Some((node, next, base, level)) = self.stack.last_mut() {
            let (node, i, base, level) = (*node, *next, *base, *level);
            if i == WIDTH {
                self.stack.pop();
                continue;
            }
            self.stack.last_mut().unwrap().1 += 1;
            let index = base | (i as u64) << (BITS * level);
            match node {
                Node::Branch(c) => {
                    if let Some(child) = c[i].as_deref() {
                        self.stack.push((child, 0, index, level - 1));
                    }
                }
                Node::Leaf(v) => {
                    if let Some(value) = v[i].as_ref() {
                        return Some((index, value));
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binding_map_matches_ordered_map() {
        let mut ids: Vec<VariableId> = [
            "-99999999999999999999999",
            "-5",
            "0",
            "1",
            "15",
            "16",
            "4095",
            "4096",
            "1756895",
            "2083339",
            "9223372036854775807",
            "9223372036854775808",
        ]
        .iter()
        .map(|s| VariableId::Integer(s.parse().unwrap()))
        .collect();
        ids.extend((0..200).map(|n| VariableId::Integer((1_900_000 + n * 7).into())));
        ids.push(VariableId::Atom("a".into()));
        let keys: Vec<_> = ids
            .iter()
            .flat_map(|v| {
                [
                    BindingKey::Label(v.clone()),
                    BindingKey::RawVariableRecord(v.clone()),
                ]
            })
            .collect();
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        let values = [Type::atom("x"), Type::atom("y"), Type::any()];
        let mut versions = vec![(BindingMap::new(), im::OrdMap::new())];
        for _ in 0..20_000 {
            let (mut map, mut reference) = versions[next(versions.len())].clone();
            let key = &keys[next(keys.len())];
            if next(3) == 0 {
                assert_eq!(map.remove(key), reference.remove(key));
            } else {
                let value = values[next(values.len())].clone();
                assert_eq!(
                    map.insert(key.clone(), value.clone()),
                    reference.insert(key.clone(), value)
                );
            }
            assert_eq!(map.len(), reference.len());
            assert_eq!(map.get(key), reference.get(key));
            assert!(map.iter().eq(reference.iter().map(|(k, v)| (k.clone(), v))));
            let (other, other_reference) = &versions[next(versions.len())];
            assert_eq!(map == *other, reference == *other_reference);
            versions.push((map, reference));
            if versions.len() > 64 {
                versions.swap_remove(next(versions.len()));
            }
        }
    }
    #[test]
    fn signed_large_unicode_alias_and_raw_keys_keep_native_order() {
        let mut ids = [
            "-99999999999999999999999999999999999",
            "-9223372036854775809",
            "-9223372036854775808",
            "-1",
            "0",
            "1",
            "9223372036854775807",
            "9223372036854775808",
            "18446744073709551615",
            "99999999999999999999999999999999999",
        ]
        .iter()
        .map(|s| VariableId::Integer(s.parse().unwrap()))
        .collect::<Vec<_>>();
        ids.extend(
            ["", "\0", "1", "-1", "é", "😀", "z"]
                .iter()
                .map(|s| VariableId::Atom((*s).into())),
        );
        let keys = ids
            .iter()
            .flat_map(|v| {
                [
                    BindingKey::Label(v.clone()),
                    BindingKey::RawVariableRecord(v.clone()),
                ]
            })
            .collect::<Vec<_>>();
        for a in &keys {
            for b in &keys {
                assert_eq!(a.cmp(b), a.native().cmp(&b.native()));
                assert_eq!(a == b, a.cmp(b).is_eq());
            }
        }
        for id in ids {
            assert_ne!(
                BindingKey::Label(id.clone()),
                BindingKey::RawVariableRecord(id)
            );
        }
    }
}

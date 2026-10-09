//! Persistent maps for dataflow refinement, keyed by Core label.
use std::sync::Arc;

const BITS: u32 = 4;
const WIDTH: usize = 1 << BITS;
const MASK: u64 = WIDTH as u64 - 1;

/// A persistent radix trie from `u64` to `V`, iterated in key order. A first
/// write to a shared trie copies a few 16-slot nodes along one path, and a
/// lookup is a handful of array indexings. Leaves sit at level 0; a node at
/// level `l` covers `WIDTH^(l+1)` keys. The root covers only the keys that
/// share `prefix` above its span, so the height follows the spread of the
/// keys (one module's labels) rather than their magnitude. There is no removal.
#[derive(Clone)]
pub struct LabelTrie<V> {
    root: Option<Arc<Node<V>>>,
    /// The root's level; it only grows.
    height: u32,
    /// The key bits above the root's span (zero once the root spans `u64`).
    prefix: u64,
    len: usize,
}
#[derive(Clone)]
enum Node<V> {
    Branch([Option<Arc<Node<V>>>; WIDTH]),
    Leaf([Option<V>; WIDTH]),
}
impl<V> Node<V> {
    fn empty(level: u32) -> Arc<Self> {
        Arc::new(if level == 0 {
            Self::Leaf([const { None }; WIDTH])
        } else {
            Self::Branch([const { None }; WIDTH])
        })
    }
}
impl<V> Default for LabelTrie<V> {
    fn default() -> Self {
        Self {
            root: None,
            height: 0,
            prefix: 0,
            len: 0,
        }
    }
}
fn slot(index: u64, level: u32) -> usize {
    ((index >> (BITS * level)) & MASK) as usize
}
impl<V: Clone> LabelTrie<V> {
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    fn span(height: u32) -> u32 {
        BITS * (height + 1)
    }
    fn covers(&self, index: u64) -> bool {
        let span = Self::span(self.height);
        span >= u64::BITS || index >> span == self.prefix
    }
    pub fn get(&self, index: u64) -> Option<&V> {
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
    pub fn contains_key(&self, index: u64) -> bool {
        self.get(index).is_some()
    }
    pub fn insert(&mut self, index: u64, value: V) {
        if self.root.is_none() && !self.covers(index) {
            self.prefix = index >> Self::span(self.height);
        }
        while !self.covers(index) {
            let root = self.root.take().expect("an empty trie covers the key");
            let mut children = [const { None }; WIDTH];
            children[(self.prefix & MASK) as usize] = Some(root);
            self.root = Some(Arc::new(Node::Branch(children)));
            self.height += 1;
            self.prefix = if Self::span(self.height) >= u64::BITS {
                0
            } else {
                self.prefix >> BITS
            };
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
                    if v[slot(index, 0)].replace(value).is_none() {
                        self.len += 1;
                    }
                    return;
                }
            }
        }
    }
    pub fn iter(&self) -> LabelIter<'_, V> {
        LabelIter {
            stack: self
                .root
                .as_deref()
                .map(|root| {
                    let span = Self::span(self.height);
                    let base = if span >= u64::BITS { 0 } else { self.prefix << span };
                    (root, 0, base, self.height)
                })
                .into_iter()
                .collect(),
        }
    }
    pub fn keys(&self) -> impl Iterator<Item = u64> + '_ {
        self.iter().map(|(key, _)| key)
    }
}
/// In-order walk: (node, next slot, first index under the node, level).
pub struct LabelIter<'a, V> {
    stack: Vec<(&'a Node<V>, usize, u64, u32)>,
}
impl<'a, V> Iterator for LabelIter<'a, V> {
    type Item = (u64, &'a V);
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

/// A persistent stack: `push_front` and `clone` are O(1). Elements sit in
/// chunks; a push extends the head chunk in place while no clone shares it.
pub struct ConsList<T> {
    head: Option<Arc<Chunk<T>>>,
}
struct Chunk<T> {
    /// Oldest first, so the newest element is last.
    items: Vec<T>,
    next: Option<Arc<Chunk<T>>>,
}
const CHUNK: usize = 32;
impl<T> Default for ConsList<T> {
    fn default() -> Self {
        Self { head: None }
    }
}
impl<T> Clone for ConsList<T> {
    fn clone(&self) -> Self {
        Self {
            head: self.head.clone(),
        }
    }
}
impl<T> Drop for ConsList<T> {
    /// Unlinks iteratively so dropping a long list cannot overflow the stack.
    fn drop(&mut self) {
        let mut next = self.head.take();
        while let Some(chunk) = next {
            next = match Arc::try_unwrap(chunk) {
                Ok(mut chunk) => chunk.next.take(),
                Err(_) => None,
            };
        }
    }
}
impl<T: Copy> ConsList<T> {
    pub fn push_front(&mut self, value: T) {
        if let Some(chunk) = self.head.as_mut().and_then(Arc::get_mut)
            && chunk.items.len() < CHUNK
        {
            chunk.items.push(value);
            return;
        }
        let mut items = Vec::with_capacity(8);
        items.push(value);
        let next = self.head.take();
        self.head = Some(Arc::new(Chunk { items, next }));
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn iter(&self) -> impl Iterator<Item = T> + '_ {
        std::iter::successors(self.head.as_deref(), |chunk| chunk.next.as_deref())
            .flat_map(|chunk| chunk.items.iter().rev().copied())
    }
    /// Puts `self`'s elements, in order, in front of `tail`'s.
    pub fn append(&mut self, tail: Self) {
        if self.head.is_none() {
            *self = tail;
            return;
        }
        let front: Vec<T> = self.iter().collect();
        *self = tail;
        for value in front.into_iter().rev() {
            self.push_front(value);
        }
    }
}
impl<T: Copy + std::fmt::Debug> std::fmt::Debug for ConsList<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn label_trie_matches_ordered_map() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        let mut keys: Vec<u64> = vec![0, 1, 15, 16, 4095, 4096, 1_756_895, u64::MAX];
        keys.extend((0..200).map(|n| 1_900_000 + n * 7));
        let mut versions = vec![(LabelTrie::default(), std::collections::BTreeMap::new())];
        for step in 0..20_000u64 {
            let (mut trie, mut reference) =
                versions[next(versions.len() as u64) as usize].clone();
            let key = keys[next(keys.len() as u64) as usize];
            trie.insert(key, step);
            reference.insert(key, step);
            assert_eq!(trie.len(), reference.len());
            assert_eq!(trie.get(key), reference.get(&key));
            assert!(trie.iter().eq(reference.iter().map(|(k, v)| (*k, v))));
            versions.push((trie, reference));
            if versions.len() > 64 {
                versions.swap_remove(next(versions.len() as u64) as usize);
            }
        }
    }
    #[test]
    fn cons_list_appends_in_order() {
        let mut a = ConsList::default();
        let mut b = ConsList::default();
        for n in 0..3 {
            a.push_front(n);
            b.push_front(10 + n);
        }
        let shared = a.clone();
        a.append(b);
        assert_eq!(a.iter().collect::<Vec<_>>(), [2, 1, 0, 12, 11, 10]);
        assert_eq!(shared.iter().collect::<Vec<_>>(), [2, 1, 0]);
        let mut x = ConsList::default();
        x.push_front(1);
        let mut y = x.clone();
        x.push_front(2);
        y.push_front(3);
        assert_eq!(x.iter().collect::<Vec<_>>(), [2, 1]);
        assert_eq!(y.iter().collect::<Vec<_>>(), [3, 1]);
        let mut long = ConsList::default();
        for n in 0..1_000_000 {
            long.push_front(n);
        }
        assert!(long.iter().eq((0..1_000_000).rev()));
        drop(long);
    }
}

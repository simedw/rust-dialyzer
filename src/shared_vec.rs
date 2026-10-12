//! An append-only vector whose prefix is shared between copies.
//!
//! Module term and node arenas are large (millions of entries) and many
//! setup phases need "the module arena plus a few terms of my own". Copying
//! the arena for each of them kept several gigabytes alive for the whole run.
//! A `SharedVec` keeps the common prefix behind an `Arc`; cloning copies only
//! the entries appended since the last `freeze`.
use std::ops::Index;
use std::sync::Arc;

struct Chunk<T> {
    end: usize,
    items: Arc<Vec<T>>,
}
fn chunk_items<T>(chunk: &Chunk<T>) -> std::slice::Iter<'_, T> {
    chunk.items.iter()
}
type ChunkIter<'a, T> = std::iter::Flatten<
    std::iter::Map<std::slice::Iter<'a, Chunk<T>>, fn(&'a Chunk<T>) -> std::slice::Iter<'a, T>>,
>;
type InnerIter<'a, T> = std::iter::Chain<
    std::iter::Chain<std::slice::Iter<'a, T>, ChunkIter<'a, T>>,
    std::slice::Iter<'a, T>,
>;
pub struct Iter<'a, T> {
    inner: InnerIter<'a, T>,
    remaining: usize,
}
impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        let value = self.inner.next();
        if value.is_some() {
            self.remaining -= 1;
        }
        value
    }
    fn count(self) -> usize {
        self.remaining
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}
impl<T> DoubleEndedIterator for Iter<'_, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let value = self.inner.next_back();
        if value.is_some() {
            self.remaining -= 1;
        }
        value
    }
}
impl<T> ExactSizeIterator for Iter<'_, T> {}
impl<T> std::iter::FusedIterator for Iter<'_, T> {}
impl<T> Clone for Iter<'_, T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            remaining: self.remaining,
        }
    }
}
pub struct SharedVec<T> {
    base: Arc<Vec<T>>,
    chunks: Vec<Chunk<T>>,
    own: Vec<T>,
}

impl<T> SharedVec<T> {
    pub fn new() -> Self {
        Self::from(Vec::new())
    }
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            base: Arc::new(Vec::new()),
            chunks: Vec::new(),
            own: Vec::with_capacity(capacity),
        }
    }
    pub fn len(&self) -> usize {
        self.prefix_len() + self.own.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn prefix_len(&self) -> usize {
        self.chunks
            .last()
            .map_or(self.base.len(), |chunk| chunk.end)
    }
    pub fn get(&self, index: usize) -> Option<&T> {
        if index < self.base.len() {
            return self.base.get(index);
        }
        let prefix_len = self.prefix_len();
        if index >= prefix_len {
            return self.own.get(index - prefix_len);
        }
        let at = self.chunks.partition_point(|chunk| chunk.end <= index);
        let start = if at == 0 {
            self.base.len()
        } else {
            self.chunks[at - 1].end
        };
        self.chunks[at].items.get(index - start)
    }
    pub fn last(&self) -> Option<&T> {
        self.len().checked_sub(1).and_then(|at| self.get(at))
    }
    pub fn push(&mut self, item: T) {
        self.own.push(item);
    }
    /// Mutable access to one entry; copies the shared prefix if another
    /// clone holds it.
    pub fn make_mut(&mut self, index: usize) -> &mut T
    where
        T: Clone,
    {
        if index < self.base.len() {
            return &mut Arc::make_mut(&mut self.base)[index];
        }
        let prefix_len = self.prefix_len();
        if index >= prefix_len {
            return &mut self.own[index - prefix_len];
        }
        let at = self.chunks.partition_point(|chunk| chunk.end <= index);
        let start = if at == 0 {
            self.base.len()
        } else {
            self.chunks[at - 1].end
        };
        &mut Arc::make_mut(&mut self.chunks[at].items)[index - start]
    }
    /// Replaces one entry, copying only its shared block when necessary.
    pub fn set(&mut self, index: usize, item: T)
    where
        T: Clone,
    {
        *self.make_mut(index) = item;
    }
    pub fn iter<'a>(&'a self) -> Iter<'a, T> {
        let project: fn(&'a Chunk<T>) -> std::slice::Iter<'a, T> = chunk_items::<T>;
        let chunks = self.chunks.iter().map(project).flatten();
        Iter {
            inner: self.base.iter().chain(chunks).chain(self.own.iter()),
            remaining: self.len(),
        }
    }
    /// The entries as one slice, when storage consists of one frozen block.
    pub fn as_contiguous(&self) -> Option<&[T]> {
        (self.own.is_empty() && self.chunks.is_empty()).then(|| self.base.as_slice())
    }
    /// Shares appended entries as a new block without copying any prefix.
    /// Use freeze instead when a single contiguous frozen slice is required.
    pub fn freeze_shared(&mut self) {
        if self.own.is_empty() {
            return;
        }
        let end = self.len();
        let own = std::mem::take(&mut self.own);
        if self.base.is_empty() && self.chunks.is_empty() {
            self.base = Arc::new(own);
        } else {
            self.chunks.push(Chunk {
                end,
                items: Arc::new(own),
            });
        }
    }

    /// Shares everything appended so far with later clones.
    pub fn freeze(&mut self)
    where
        T: Clone,
    {
        if !self.chunks.is_empty() {
            let items = self.to_vec();
            self.base = Arc::new(items);
            self.chunks.clear();
            self.own.clear();
            return;
        }
        if self.own.is_empty() {
            return;
        }
        let own = std::mem::take(&mut self.own);
        if self.base.is_empty() {
            self.base = Arc::new(own);
            return;
        }
        match Arc::get_mut(&mut self.base) {
            Some(base) => {
                base.reserve_exact(own.len());
                base.extend(own);
            }
            None => {
                let mut items = Vec::with_capacity(self.base.len() + own.len());
                items.extend(self.base.iter().cloned());
                items.extend(own);
                self.base = Arc::new(items);
            }
        }
    }
    /// Consumes the vector, passing each entry `keep` accepts to `f` in
    /// order. Entries move out of blocks no other clone holds; entries of
    /// shared blocks are cloned.
    pub fn consume_kept<E>(
        self,
        mut keep: impl FnMut(usize) -> bool,
        mut f: impl FnMut(usize, T) -> Result<(), E>,
    ) -> Result<(), E>
    where
        T: Clone,
    {
        let mut index = 0;
        let mut block = |items: Arc<Vec<T>>| -> Result<(), E> {
            match Arc::try_unwrap(items) {
                Ok(items) => {
                    for item in items {
                        if keep(index) {
                            f(index, item)?;
                        }
                        index += 1;
                    }
                }
                Err(items) => {
                    for item in items.iter() {
                        if keep(index) {
                            f(index, item.clone())?;
                        }
                        index += 1;
                    }
                }
            }
            Ok(())
        };
        block(self.base)?;
        for chunk in self.chunks {
            block(chunk.items)?;
        }
        block(Arc::new(self.own))
    }
    /// Copies the entries into one owned vector.
    pub fn to_vec(&self) -> Vec<T>
    where
        T: Clone,
    {
        let mut items = Vec::with_capacity(self.len());
        items.extend(self.iter().cloned());
        items
    }
}

impl<T> Default for SharedVec<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> Clone for SharedVec<T> {
    fn clone(&self) -> Self {
        Self {
            base: self.base.clone(),
            chunks: self
                .chunks
                .iter()
                .map(|chunk| Chunk {
                    end: chunk.end,
                    items: chunk.items.clone(),
                })
                .collect(),
            own: self.own.clone(),
        }
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for SharedVec<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: PartialEq> PartialEq for SharedVec<T> {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter())
    }
}
impl<T: Eq> Eq for SharedVec<T> {}

impl<T> From<Vec<T>> for SharedVec<T> {
    fn from(items: Vec<T>) -> Self {
        Self {
            base: Arc::new(items),
            chunks: Vec::new(),
            own: Vec::new(),
        }
    }
}

impl<T> FromIterator<T> for SharedVec<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self::from(iter.into_iter().collect::<Vec<_>>())
    }
}

impl<T> Index<usize> for SharedVec<T> {
    type Output = T;
    fn index(&self, index: usize) -> &T {
        self.get(index).expect("SharedVec index out of bounds")
    }
}

impl<'a, T> IntoIterator for &'a SharedVec<T> {
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::SharedVec;

    #[test]
    fn block_snapshots_share_prefixes_and_isolate_mutations() {
        let original = SharedVec::from(vec![10, 20]);
        let mut next = original.clone();
        next.push(30);
        next.push(40);
        next.freeze_shared();
        let snapshot = next.clone();
        assert!(std::sync::Arc::ptr_eq(&original.base, &next.base));
        assert!(std::sync::Arc::ptr_eq(
            &snapshot.chunks[0].items,
            &next.chunks[0].items
        ));
        next.push(50);
        next.freeze_shared();
        next.push(60);
        assert_eq!(
            next.iter().copied().collect::<Vec<_>>(),
            [10, 20, 30, 40, 50, 60]
        );
        assert_eq!(
            next.iter().rev().copied().collect::<Vec<_>>(),
            [60, 50, 40, 30, 20, 10]
        );
        for (index, value) in [10, 20, 30, 40, 50, 60].into_iter().enumerate() {
            assert_eq!(next.get(index), Some(&value));
        }
        assert_eq!(next.get(6), None);
        next.set(0, 11);
        next.set(2, 31);
        next.set(4, 51);
        next.set(5, 61);
        assert_eq!(original.iter().copied().collect::<Vec<_>>(), [10, 20]);
        assert_eq!(
            snapshot.iter().copied().collect::<Vec<_>>(),
            [10, 20, 30, 40]
        );
        assert_eq!(next.last(), Some(&61));
        assert!(next.as_contiguous().is_none());
        next.freeze();
        assert_eq!(next.as_contiguous(), Some(&[11, 20, 31, 40, 51, 61][..]));
        let mut iter = next.iter();
        assert_eq!(iter.len(), 6);
        assert_eq!(iter.next(), Some(&11));
        assert_eq!(iter.next_back(), Some(&61));
        assert_eq!(iter.clone().count(), 4);
    }

    #[test]
    fn clones_share_the_frozen_prefix() {
        let mut a = SharedVec::from(vec![1, 2, 3]);
        let mut b = a.clone();
        b.push(4);
        a.push(5);
        assert_eq!(a.iter().copied().collect::<Vec<_>>(), [1, 2, 3, 5]);
        assert_eq!(b.iter().copied().collect::<Vec<_>>(), [1, 2, 3, 4]);
        assert_eq!((b[3], a[3], b.len(), b.get(4)), (4, 5, 4, None));
        assert!(std::sync::Arc::ptr_eq(&a.base, &b.base));
        b.freeze();
        assert_eq!(b.as_contiguous(), Some(&[1, 2, 3, 4][..]));
        assert_eq!(a.last(), Some(&5));
    }

    #[test]
    fn consume_kept_visits_shared_and_owned_blocks_in_order() {
        let mut a = SharedVec::from(vec![String::from("a"), "b".into()]);
        a.push("c".into());
        a.freeze_shared();
        a.push("d".into());
        let other = a.clone();
        let mut kept = Vec::new();
        a.consume_kept(|i| i != 1, |i, item| {
            kept.push((i, item));
            Ok::<_, ()>(())
        })
        .unwrap();
        assert_eq!(kept, [(0, "a".into()), (2, "c".into()), (3, "d".into())]);
        assert_eq!(other.to_vec(), ["a", "b", "c", "d"]);
    }
}

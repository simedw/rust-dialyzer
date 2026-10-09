//! An append-only vector whose prefix is shared between copies.
//!
//! Module term and node arenas are large (millions of entries) and many
//! setup phases need "the module arena plus a few terms of my own". Copying
//! the arena for each of them kept several gigabytes alive for the whole run.
//! A `SharedVec` keeps the common prefix behind an `Arc`; cloning copies only
//! the entries appended since the last `freeze`.
use std::ops::Index;
use std::sync::Arc;

pub struct SharedVec<T> {
    base: Arc<Vec<T>>,
    own: Vec<T>,
}

impl<T> SharedVec<T> {
    pub fn new() -> Self {
        Self::from(Vec::new())
    }
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            base: Arc::new(Vec::new()),
            own: Vec::with_capacity(capacity),
        }
    }
    pub fn len(&self) -> usize {
        self.base.len() + self.own.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn get(&self, index: usize) -> Option<&T> {
        match index.checked_sub(self.base.len()) {
            None => self.base.get(index),
            Some(at) => self.own.get(at),
        }
    }
    pub fn last(&self) -> Option<&T> {
        self.own.last().or_else(|| self.base.last())
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
        match index.checked_sub(self.base.len()) {
            None => &mut Arc::make_mut(&mut self.base)[index],
            Some(at) => &mut self.own[at],
        }
    }
    /// Replaces one entry; copies the shared prefix if another clone holds it.
    pub fn set(&mut self, index: usize, item: T)
    where
        T: Clone,
    {
        match index.checked_sub(self.base.len()) {
            None => Arc::make_mut(&mut self.base)[index] = item,
            Some(at) => self.own[at] = item,
        }
    }
    pub fn iter(&self) -> std::iter::Chain<std::slice::Iter<'_, T>, std::slice::Iter<'_, T>> {
        self.base.iter().chain(self.own.iter())
    }
    /// The entries as one slice, if nothing was appended since the last freeze.
    pub fn as_contiguous(&self) -> Option<&[T]> {
        self.own.is_empty().then(|| self.base.as_slice())
    }
    /// Shares everything appended so far with later clones.
    pub fn freeze(&mut self)
    where
        T: Clone,
    {
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
        match index.checked_sub(self.base.len()) {
            None => &self.base[index],
            Some(at) => &self.own[at],
        }
    }
}

impl<'a, T> IntoIterator for &'a SharedVec<T> {
    type Item = &'a T;
    type IntoIter = std::iter::Chain<std::slice::Iter<'a, T>, std::slice::Iter<'a, T>>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::SharedVec;

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
}

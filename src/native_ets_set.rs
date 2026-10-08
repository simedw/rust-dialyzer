use crate::native_runtime_hash::{NativeAtomLayout, NativeHashKey, RuntimeHashError};

#[derive(Clone, Debug)]
struct Entry<V> {
    key: NativeHashKey,
    hash: u64,
    value: V,
}
#[derive(Clone, Debug)]
pub struct NativeEtsSet<V> {
    buckets: Vec<Vec<Entry<V>>>,
    mask: usize,
    active: usize,
    slots: usize,
    size: usize,
}
impl<V> Default for NativeEtsSet<V> {
    fn default() -> Self {
        Self {
            buckets: (0..256).map(|_| Vec::new()).collect(),
            mask: 255,
            active: 256,
            slots: 256,
            size: 0,
        }
    }
}
impl<V> NativeEtsSet<V> {
    fn index(&self, hash: u64) -> usize {
        let index = hash as usize & self.mask;
        if index >= self.active {
            index & (self.mask >> 1)
        } else {
            index
        }
    }
    pub fn insert(
        &mut self,
        key: NativeHashKey,
        value: V,
        layout: &NativeAtomLayout,
    ) -> Result<bool, RuntimeHashError> {
        let hash = layout.ets_hash(&key)?;
        let index = self.index(hash);
        if let Some(entry) = self.buckets[index]
            .iter_mut()
            .find(|entry| entry.key == key)
        {
            entry.value = value;
            return Ok(false);
        }
        self.buckets[index].push(Entry { key, hash, value });
        self.size += 1;
        for _ in 0..5 {
            if self.size <= self.active {
                break;
            }
            self.grow();
        }
        Ok(true)
    }
    pub fn get(
        &self,
        key: &NativeHashKey,
        layout: &NativeAtomLayout,
    ) -> Result<Option<&V>, RuntimeHashError> {
        let index = self.index(layout.ets_hash(key)?);
        Ok(self.buckets[index]
            .iter()
            .find(|entry| &entry.key == key)
            .map(|entry| &entry.value))
    }
    pub fn remove(
        &mut self,
        key: &NativeHashKey,
        layout: &NativeAtomLayout,
    ) -> Result<Option<V>, RuntimeHashError> {
        let index = self.index(layout.ets_hash(key)?);
        let Some(position) = self.buckets[index]
            .iter()
            .position(|entry| &entry.key == key)
        else {
            return Ok(None);
        };
        let value = self.buckets[index].remove(position).value;
        self.size -= 1;
        for _ in 0..5 {
            let limit = if self.slots >= 256 + 2 * 2048 {
                (self.slots - 2048) / 2
            } else {
                0
            };
            if self.size >= limit {
                break;
            }
            self.shrink();
        }
        Ok(Some(value))
    }
    pub fn select_keys(&self) -> Vec<NativeHashKey> {
        self.buckets[..self.active]
            .iter()
            .rev()
            .flat_map(|bucket| bucket.iter().rev().map(|entry| entry.key.clone()))
            .collect()
    }
    pub fn rows(&self) -> Vec<(&NativeHashKey, &V)> {
        self.buckets[..self.active]
            .iter()
            .rev()
            .flat_map(|bucket| bucket.iter().rev().map(|entry| (&entry.key, &entry.value)))
            .collect()
    }
    fn grow(&mut self) {
        if self.active == self.slots {
            self.slots += 2048;
            self.buckets.resize_with(self.slots, Vec::new);
        }
        let from = if self.active <= self.mask {
            self.active & (self.mask >> 1)
        } else {
            self.mask = (self.mask << 1) | 1;
            0
        };
        let to = self.active;
        self.active += 1;
        let previous = std::mem::take(&mut self.buckets[from]);
        for entry in previous {
            let index = entry.hash as usize & self.mask;
            debug_assert!(index == from || index == to);
            self.buckets[index].push(entry);
        }
    }
    fn shrink(&mut self) {
        let from = self.active - 1;
        let low_mask = self.mask >> 1;
        let to = from & low_mask;
        self.active = from;
        if to == 0 {
            self.mask = low_mask;
        }
        let mut source = std::mem::take(&mut self.buckets[from]);
        source.append(&mut self.buckets[to]);
        self.buckets[to] = source;
        if self.slots - from >= 2048 {
            self.slots -= 2048;
            self.buckets.truncate(self.slots);
        }
    }
}

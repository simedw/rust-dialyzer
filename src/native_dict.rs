use crate::worklist::FunctionKey;
/// Pre-range make_hash value used by pinned OTP phash/2.
/// Integer keys are canonical nonnegative compiler labels.
pub fn pinned_hash_integer(n: u64) -> u32 {
    let mut h = 0u32;
    let digits = if n >> 32 == 0 { 4 } else { 8 };
    for shift in 0..digits {
        h = h
            .wrapping_mul(268439161)
            .wrapping_add(((n >> (8 * shift)) & 255) as u32)
    }
    h.wrapping_mul(268435459)
}
/// atom_hash hashpjw, including OTP's UTF-8 Latin-1 compatibility clutch.
pub fn pinned_hash_atom(atom: &str) -> u32 {
    let bytes = atom.as_bytes();
    let mut index = 0;
    let mut h = 0u32;
    while index < bytes.len() {
        let mut v = bytes[index];
        index += 1;
        if index < bytes.len() && (v & 0xfe) == 0xc2 && (bytes[index] & 0xc0) == 0x80 {
            v = (((v as u16) << 6) | ((bytes[index] & 0x3f) as u16)) as u8;
            index += 1;
        }
        h = (h << 4).wrapping_add(u32::from(v));
        let g = h & 0xf0000000;
        if g != 0 {
            h ^= g >> 24;
            h ^= g;
        }
    }
    h
}
pub trait PinnedPhashKey {
    fn pinned_phash_hash(&self) -> u32;
}
impl PinnedPhashKey for FunctionKey {
    fn pinned_phash_hash(&self) -> u32 {
        match self {
            Self::Top => pinned_hash_atom("top"),
            Self::Label(label) => pinned_hash_integer(label.0),
        }
    }
}
#[derive(Clone, Debug)]
pub struct NativeDictOrder<K = FunctionKey> {
    buckets: Vec<Vec<K>>,
    size: usize,
    n: usize,
    maxn: usize,
    bso: usize,
}
impl<K: PinnedPhashKey + Eq + Clone> Default for NativeDictOrder<K> {
    fn default() -> Self {
        Self {
            buckets: vec![vec![]; 16],
            size: 0,
            n: 16,
            maxn: 16,
            bso: 8,
        }
    }
}
impl<K: PinnedPhashKey + Eq + Clone> NativeDictOrder<K> {
    pub fn from_record_shape(
        size: usize,
        n: usize,
        maxn: usize,
        bso: usize,
        buckets: Vec<Vec<K>>,
    ) -> Result<Self, String> {
        if maxn < 16
            || !maxn.is_power_of_two()
            || bso != maxn / 2
            || n <= bso
            || n > maxn
            || buckets.len() != maxn
            || buckets.iter().map(Vec::len).sum::<usize>() != size
        {
            return Err("invalid imported dict order shape".into());
        }
        Ok(Self {
            size,
            n,
            maxn,
            bso,
            buckets,
        })
    }
    pub fn retain_keys(&mut self, mut keep: impl FnMut(&K) -> bool) {
        for bucket in &mut self.buckets {
            bucket.retain(|key| keep(key));
        }
        self.size = self.buckets.iter().map(Vec::len).sum();
        if self.size < self.n * 3 && self.n > 16 {
            let high = self.n - 1;
            let low = self.n - self.bso - 1;
            let upper = std::mem::take(&mut self.buckets[high]);
            self.buckets[low].extend(upper);
            self.n -= 1;
            if self.n == self.bso {
                self.maxn /= 2;
                self.bso /= 2;
                self.buckets.truncate(self.maxn);
            }
        }
    }
    pub fn map_keys(&self) -> Vec<K> {
        self.buckets
            .iter()
            .flat_map(|bucket| bucket.iter().cloned())
            .collect()
    }
    pub fn record_shape(&self) -> (usize, usize, usize, usize, &[Vec<K>]) {
        (self.size, self.n, self.maxn, self.bso, &self.buckets)
    }

    fn hash_slot(&self, key: &K) -> usize {
        key.pinned_phash_hash() as usize % self.maxn + 1
    }
    pub fn store(&mut self, key: K) {
        let h = self.hash_slot(&key);
        let slot = if h > self.n { h - self.bso } else { h };
        let bucket = &mut self.buckets[slot - 1];
        let increment = if bucket.contains(&key) {
            0
        } else {
            bucket.push(key);
            1
        };
        self.size += increment;
        if self.size > self.n * 5 {
            if self.n == self.maxn {
                self.maxn *= 2;
                self.bso *= 2;
                self.buckets.resize(self.maxn, vec![])
            }
            self.n += 1;
            let low = self.n - self.bso;
            let old = std::mem::take(&mut self.buckets[low - 1]);
            for key in old {
                let h = self.hash_slot(&key);
                self.buckets[h - 1].push(key)
            }
        }
    }
    pub fn keys(&self) -> Vec<K> {
        self.buckets
            .iter()
            .flat_map(|b| b.iter().rev().cloned())
            .collect()
    }
    /// Source dict:fold callback visitation order (opposite fetch_keys accumulation).
    pub fn fold_keys(&self) -> Vec<K> {
        self.buckets
            .iter()
            .rev()
            .flat_map(|bucket| bucket.iter().cloned())
            .collect()
    }
    pub fn slots(&self) -> (usize, usize, usize) {
        (self.n, self.maxn, self.bso)
    }
}

pub fn pinned_hash_key(
    key: &crate::native_runtime_hash::NativeHashKey,
) -> Result<u32, crate::native_runtime_hash::RuntimeHashError> {
    hash_key_in(key, None, 0)
}
pub fn pinned_hash_key_with_layout(
    key: &crate::native_runtime_hash::NativeHashKey,
    layout: &crate::native_runtime_hash::NativeAtomLayout,
) -> Result<u32, crate::native_runtime_hash::RuntimeHashError> {
    hash_key_in(key, Some(layout), 0)
}
fn hash_key_in(
    key: &crate::native_runtime_hash::NativeHashKey,
    layout: Option<&crate::native_runtime_hash::NativeAtomLayout>,
    seed: u32,
) -> Result<u32, crate::native_runtime_hash::RuntimeHashError> {
    use crate::native_runtime_hash::NativeHashKey as K;
    enum Work<'a> {
        Key(&'a K),
        Tuple(usize),
        #[cfg(otp_domain = "29")]
        Record(usize),
        Tail,
        Cdr(&'a K),
    }
    let mut work = vec![Work::Key(key)];
    let mut h = seed;
    while let Some(next) = work.pop() {
        match next {
            Work::Tuple(n) => h = h.wrapping_mul(268439627).wrapping_add(n as u32),
            #[cfg(otp_domain = "29")]
            Work::Record(n) => h = h.wrapping_mul(268440629).wrapping_add(n as u32),
            Work::Tail => h = h.wrapping_mul(268437511),
            Work::Cdr(key) => {
                if !matches!(key, K::Cons(..)) {
                    work.push(Work::Tail);
                }
                work.push(Work::Key(key));
            }
            Work::Key(key) => match key {
                #[cfg(otp_domain = "29")]
                K::Record { module,name,exported,fields } => {
                    let layout = layout.ok_or(crate::native_runtime_hash::RuntimeHashError::UnavailableAtomLayout)?;
                    let mut fields_in_order = fields.iter().enumerate().map(|(index,(key,value))|
                        Ok((layout.atom_identity(key)?,index,key,value)))
                        .collect::<Result<Vec<_>,crate::native_runtime_hash::RuntimeHashError>>()?;
                    fields_in_order.sort_by_key(|(identity,_,_,_)| *identity);
                    let mut order = vec![K::Nil; fields.len()];
                    for (sorted,(_,original,_,_)) in fields_in_order.iter().enumerate() {
                        order[*original] = K::Integer(sorted.into());
                    }
                    let mut definition = vec![K::Tuple(order),K::Integer(
                        crate::portable_term_hash::record_definition_hash(module,name,*exported,fields).into()),
                        K::Atom(module.clone()),K::Atom(name.clone()),K::Atom(exported.to_string())];
                    definition.extend(fields_in_order.iter().map(|(_,_,key,_)| K::Atom((*key).clone())));
                    h = hash_key_in(&K::Tuple(definition),Some(layout),h)?;
                    work.push(Work::Record(fields.len()+1));
                    work.extend(fields_in_order.into_iter().rev().map(|(_,_,_,value)| Work::Key(value)));
                }
                K::ExternalFun {
                    module,
                    function,
                    arity,
                } => {
                    h = h.wrapping_mul(268440577).wrapping_add(u32::from(*arity));
                    h = h
                        .wrapping_mul(268440163)
                        .wrapping_add(pinned_hash_atom(module));
                    h = h
                        .wrapping_mul(268440163)
                        .wrapping_add(pinned_hash_atom(function));
                }
                K::Atom(n) => h = h.wrapping_mul(268440163).wrapping_add(pinned_hash_atom(n)),
                K::Integer(n) => {
                    let (sign, mut bytes) = n.to_bytes_le();
                    bytes.resize(bytes.len().max(1).div_ceil(4) * 4, 0);
                    for byte in bytes {
                        h = h.wrapping_mul(268439161).wrapping_add(byte as u32);
                    }
                    h = h.wrapping_mul(if sign == num_bigint::Sign::Minus {
                        268436141
                    } else {
                        268435459
                    });
                }
                K::Nil => h = h.wrapping_mul(268435459).wrapping_add(1),
                K::Tuple(xs) => {
                    work.push(Work::Tuple(xs.len()));
                    work.extend(xs.iter().rev().map(Work::Key));
                }
                K::Float(bits) => {
                    let bits = if f64::from_bits(*bits) == 0.0 {
                        0
                    } else {
                        *bits
                    };
                    h = h
                        .wrapping_mul(268437017)
                        .wrapping_add((bits as u32) ^ ((bits >> 32) as u32));
                }
                K::Bits { bytes, bits } => {
                    let size = (*bits / 8) as usize;
                    let tail = (*bits % 8) as u32;
                    for byte in &bytes[..size] {
                        h = h.wrapping_mul(268440163).wrapping_add(*byte as u32);
                    }
                    if tail > 0 {
                        h = h
                            .wrapping_mul(268440163)
                            .wrapping_add((bytes[size] >> (8 - tail)) as u32)
                            .wrapping_mul(268440581)
                            .wrapping_add(tail);
                    }
                    h = h.wrapping_mul(268436141).wrapping_add(size as u32);
                }
                K::Cons(head, tail) => {
                    use num_traits::ToPrimitive;
                    if let K::Integer(n) = head.as_ref()
                        && let Some(byte) = n.to_u8()
                    {
                        h = h.wrapping_mul(268439161).wrapping_add(byte as u32);
                        work.push(Work::Cdr(tail));
                        continue;
                    }
                    work.push(Work::Cdr(tail));
                    work.push(Work::Key(head));
                }
                K::Map(_) => {
                    h = h
                        .wrapping_mul(268440593)
                        .wrapping_add(268440611)
                        .wrapping_add(match layout {
                            Some(layout) => crate::portable_term_hash::hash_with_layout(key,layout)?,
                            None => crate::portable_term_hash::hash(key)?,
                        })
                }
            },
        }
    }
    Ok(h)
}

pub(crate) fn record_term<K: PinnedPhashKey + Eq + Clone>(
    order: &NativeDictOrder<K>,
    builder: &mut crate::form_error::ReasonBuilder,
    mut row: impl FnMut(
        &K,
        &mut crate::form_error::ReasonBuilder,
    ) -> (crate::core::TermId, crate::core::TermId),
) -> crate::core::TermId {
    let (size, n, maxn, bso, buckets) = order.record_shape();
    let mut slots = Vec::new();
    for bucket in buckets {
        let mut pairs = Vec::new();
        for key in bucket {
            let (name, value) = row(key, builder);
            pairs.push(builder.list_tail(vec![name], value));
        }
        slots.push(builder.list(pairs));
    }
    let segments = slots
        .chunks(16)
        .map(|chunk| builder.tuple(chunk.to_vec()))
        .collect();
    let segs = builder.tuple(segments);
    let empty = builder.tuple(vec![builder.nil(); 16]);
    let mut fields = vec![builder.atom("dict")];
    for value in [size, n, maxn, bso, n * 5, n * 3] {
        fields.push(builder.integer(value));
    }
    fields.extend([empty, segs]);
    builder.tuple(fields)
}

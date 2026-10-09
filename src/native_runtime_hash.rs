use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct NativeAtomLayout {
    pub scope: String,
    atoms: BTreeMap<String, u64>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeHashError {
    UnavailableAtomLayout,
    MissingAtom(String),
    MissingExternalFunIdentity {
        module: String,
        function: String,
        arity: u8,
    },
    Boundary(String),
}
impl std::fmt::Display for RuntimeHashError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "{self:?}")
    }
}
impl std::error::Error for RuntimeHashError {}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NativeHashKey {
    ExternalFun {
        module: String,
        function: String,
        arity: u8,
    },
    Atom(String),
    Integer(BigInt),
    Float(u64),
    Bits {
        bytes: Vec<u8>,
        bits: u64,
    },
    Map(Vec<(Self, Self)>),
    Tuple(Vec<Self>),
    Cons(Box<Self>, Box<Self>),
    Nil,
}
impl NativeHashKey {
    pub fn from_raw(
        raw: &crate::native_form_environment::RawFormTerm,
    ) -> Result<Self, RuntimeHashError> {
        let order = crate::term_order::TermOrderArena::new(raw.arena())
            .map_err(RuntimeHashError::Boundary)?;
        Self::from_raw_with_order(raw, &order)
    }
    pub(crate) fn from_raw_with_order(
        raw: &crate::native_form_environment::RawFormTerm,
        order: &crate::term_order::TermOrderArena,
    ) -> Result<Self, RuntimeHashError> {
        use crate::core::{ErlangTerm as T, TermId};
        fn read(
            arena: &crate::core::ErlangTerms,
            order: &crate::term_order::TermOrderArena,
            id: TermId,
        ) -> Result<NativeHashKey, RuntimeHashError> {
            use NativeHashKey as K;
            Ok(match &arena.terms[id.0] {
                T::ExternalFun {
                    module,
                    function,
                    arity,
                } => K::ExternalFun {
                    module: module.clone(),
                    function: function.clone(),
                    arity: *arity,
                },
                T::Atom(x) => K::Atom(x.clone()),
                T::Integer(x) => K::Integer(
                    x.parse()
                        .map_err(|_| RuntimeHashError::Boundary("invalid integer".into()))?,
                ),
                T::Float(x) => K::Float(*x),
                T::Binary(bytes) => K::Bits {
                    bytes: bytes.clone(),
                    bits: bytes.len() as u64 * 8,
                },
                T::Bitstring { bytes, bits } => K::Bits {
                    bytes: bytes.clone(),
                    bits: *bits,
                },
                T::Tuple(xs) => K::Tuple(
                    xs.iter()
                        .map(|x| read(arena, order, *x))
                        .collect::<Result<_, _>>()?,
                ),
                T::Nil => K::Nil,
                T::List { items, tail } => {
                    let mut out = read(arena, order, *tail)?;
                    for item in items.iter().rev() {
                        out = K::Cons(Box::new(read(arena, order, *item)?), Box::new(out));
                    }
                    out
                }
                T::Map(pairs) => K::Map(
                    order
                        .canonical_map_pairs(pairs)
                        .map_err(RuntimeHashError::Boundary)?
                        .iter()
                        .map(|(k, v)| Ok((read(arena, order, *k)?, read(arena, order, *v)?)))
                        .collect::<Result<_, RuntimeHashError>>()?,
                ),
            })
        }
        read(raw.arena(), order, raw.id)
    }
    pub(crate) fn literal(&self) -> crate::core::ErlangTerms {
        use crate::core::{ErlangTerm as T, ErlangTerms, TermId};
        fn append(key: &NativeHashKey, nodes: &mut Vec<T>) -> TermId {
            use NativeHashKey as K;
            let node = match key {
                K::ExternalFun {
                    module,
                    function,
                    arity,
                } => T::ExternalFun {
                    module: module.clone(),
                    function: function.clone(),
                    arity: *arity,
                },
                K::Atom(x) => T::Atom(x.clone()),
                K::Integer(x) => T::Integer(x.to_string()),
                K::Float(x) => T::Float(*x),
                K::Bits { bytes, bits } => T::Bitstring {
                    bytes: bytes.clone(),
                    bits: *bits,
                },
                K::Nil => T::Nil,
                K::Tuple(xs) => T::Tuple(xs.iter().map(|x| append(x, nodes)).collect()),
                K::Cons(h, t) => {
                    let h = append(h, nodes);
                    let t = append(t, nodes);
                    T::List {
                        items: vec![h],
                        tail: t,
                    }
                }
                K::Map(xs) => T::Map(
                    xs.iter()
                        .map(|(k, v)| (append(k, nodes), append(v, nodes)))
                        .collect(),
                ),
            };
            let id = TermId(nodes.len());
            nodes.push(node);
            id
        }
        let mut terms = Vec::new();
        let root = append(self, &mut terms);
        ErlangTerms { root, terms: terms.into() }
    }
}
impl NativeAtomLayout {
    pub fn parse(value: &serde_json::Value) -> Result<Self, RuntimeHashError> {
        let fail = |message: &str| RuntimeHashError::Boundary(message.into());
        if value["version"].as_u64() != Some(1) || value["word_size"].as_u64() != Some(8) {
            return Err(fail("unsupported compiler atom primitive profile"));
        }
        let scope = value["scope"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| fail("compiler atom primitive scope missing"))?
            .to_owned();
        let rows = value["atom_hashes"]
            .as_object()
            .ok_or_else(|| fail("compiler atom hashes missing"))?;
        let mut atoms = BTreeMap::new();
        let mut identities = BTreeMap::new();
        for (name, value) in rows {
            let hash = value
                .as_str()
                .and_then(|v| v.parse::<u64>().ok())
                .ok_or_else(|| fail("compiler atom hash is not unsigned decimal"))?;
            let immediate = inverse_mix64(hash);
            if immediate & 63 != 11 || identities.insert(immediate, name).is_some() {
                return Err(fail(
                    "compiler atom hash does not identify a unique tagged atom",
                ));
            }
            atoms.insert(name.clone(), immediate);
        }
        Ok(Self { scope, atoms })
    }
    pub fn atom_identity(&self, name: &str) -> Result<u64, RuntimeHashError> {
        self.atoms
            .get(name)
            .copied()
            .ok_or_else(|| RuntimeHashError::MissingAtom(name.into()))
    }
    pub fn internal_hash(&self, key: &NativeHashKey) -> Result<u64, RuntimeHashError> {
        if let Some(immediate) = self.immediate(key)? {
            return Ok(mix64(immediate));
        }
        crate::runtime_hash_nodes::hash(self, key)
    }
    pub fn ets_hash(&self, key: &NativeHashKey) -> Result<u64, RuntimeHashError> {
        Ok(match key {
            NativeHashKey::Atom(name) => u64::from(crate::native_dict::pinned_hash_atom(name)),
            _ => self.internal_hash(key)?,
        } >> 1)
    }
    pub(crate) fn immediate(&self, key: &NativeHashKey) -> Result<Option<u64>, RuntimeHashError> {
        Ok(match key {
            NativeHashKey::Atom(name) => Some(
                *self
                    .atoms
                    .get(name)
                    .ok_or_else(|| RuntimeHashError::MissingAtom(name.clone()))?,
            ),
            NativeHashKey::Integer(value) => value
                .to_i64()
                .filter(|n| (-(1i64 << 59)..(1i64 << 59)).contains(n))
                .map(|n| (n as u64).wrapping_shl(4) | 15),
            NativeHashKey::Nil => Some(59),
            _ => None,
        })
    }
}
#[derive(Default)]
pub(crate) struct HashState {
    alpha: u64,
    beta: u64,
    ticks: u64,
}
impl HashState {
    pub(crate) fn alpha(&mut self, value: u64) {
        let value = value
            .wrapping_mul(0x87C37B91114253D5)
            .rotate_left(31)
            .wrapping_mul(0x4CF5AD432745937F);
        self.alpha ^= value;
        self.alpha = self
            .alpha
            .rotate_left(27)
            .wrapping_add(self.beta)
            .wrapping_mul(5)
            .wrapping_add(0x52DCE729);
        self.ticks += 1;
    }
    pub(crate) fn beta(&mut self, value: u64) {
        let value = value
            .wrapping_mul(0x4CF5AD432745937F)
            .rotate_left(33)
            .wrapping_mul(0x87C37B91114253D5);
        self.beta ^= value;
        self.beta = self
            .beta
            .rotate_left(31)
            .wrapping_add(self.alpha)
            .wrapping_mul(5)
            .wrapping_add(0x38495AB5);
        self.ticks += 1;
    }
    pub(crate) fn alpha_tail(&mut self, value: u64) {
        self.alpha ^= value
            .wrapping_mul(0x87C37B91114253D5)
            .rotate_left(31)
            .wrapping_mul(0x4CF5AD432745937F);
    }
    pub(crate) fn beta_tail(&mut self, value: u64) {
        self.beta ^= value
            .wrapping_mul(0x4CF5AD432745937F)
            .rotate_left(33)
            .wrapping_mul(0x87C37B91114253D5);
    }
    pub(crate) fn finish(mut self) -> u64 {
        self.alpha ^= self.ticks;
        self.beta ^= self.ticks;
        self.alpha = self.alpha.wrapping_add(self.beta);
        self.beta = self.beta.wrapping_add(self.alpha);
        self.alpha = mix64(self.alpha);
        self.beta = mix64(self.beta);
        self.alpha = self.alpha.wrapping_add(self.beta);
        self.beta = self.beta.wrapping_add(self.alpha);
        self.alpha ^ self.beta
    }
}
fn mix64(mut value: u64) -> u64 {
    value ^= value >> 33;
    value = value.wrapping_mul(0xFF51AFD7ED558CCD);
    value ^= value >> 33;
    value = value.wrapping_mul(0xC4CEB9FE1A85EC53);
    value ^ (value >> 33)
}
fn inverse_mix64(mut value: u64) -> u64 {
    value ^= value >> 33;
    value = value.wrapping_mul(0x9CB4B2F8129337DB);
    value ^= value >> 33;
    value = value.wrapping_mul(0x4F74430C22A54005);
    value ^ (value >> 33)
}

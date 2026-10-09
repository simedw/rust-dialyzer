use crate::{
    core::{ErlangTerm, ErlangTerms, TermId},
    form_error::ReasonBuilder,
    native_form_environment::RawFormTerm,
    term_order::TermOrderArena,
};
use std::{cmp::Ordering, sync::Arc};

#[derive(Clone, Debug)]
pub struct RawLiteral {
    raw: RawFormTerm,
    order: Arc<TermOrderArena>,
}
/// Literals of one arena. Callers take a few literals out of large form
/// arenas, so each literal gets the comparison arena of the terms it reaches
/// only, which orders exactly as the arena of all terms.
pub struct RawLiteralArena {
    arena: Arc<ErlangTerms>,
    /// The arena of all terms, under fast-path verification only.
    full: Option<TermOrderArena>,
}
impl RawLiteralArena {
    pub fn new(arena: Arc<ErlangTerms>) -> Result<Self, String> {
        let full = match crate::lattice::verify_fast_paths() {
            true => Some(TermOrderArena::new(&arena)?),
            false => None,
        };
        Ok(Self { arena, full })
    }
    pub fn at(&self, id: TermId) -> Result<RawLiteral, String> {
        let raw = RawFormTerm::new(self.arena.clone(), id)?;
        let order = TermOrderArena::reachable(&self.arena, &[id])?;
        if let Some(full) = &self.full {
            assert_eq!(full.compare(id, &order, id), Ok(Ordering::Equal), "restricted literal");
            assert_eq!(
                full.compare_map_keys(id, &order, id),
                Ok(Ordering::Equal),
                "restricted literal key"
            );
        }
        Ok(RawLiteral { raw, order: Arc::new(order) })
    }
}
impl RawLiteral {
    pub fn new(raw: RawFormTerm) -> Result<Self, String> {
        RawLiteralArena::new(raw.arena().clone())?.at(raw.id())
    }
    pub fn from_arena(arena: Arc<ErlangTerms>, id: TermId) -> Result<Self, String> {
        Self::new(RawFormTerm::new(arena, id)?)
    }
    pub fn atom(name: &str) -> Self {
        let mut b = ReasonBuilder::new();
        let root = b.atom(name);
        Self::new(b.finish(root)).expect("constructed atom literal")
    }
    pub fn at(&self, id: TermId) -> Result<Self, String> {
        Ok(Self {
            raw: RawFormTerm::new(self.raw.arena().clone(), id)?,
            order: self.order.clone(),
        })
    }
    pub(crate) fn order(&self) -> &TermOrderArena {
        &self.order
    }
    pub fn raw(&self) -> &RawFormTerm {
        &self.raw
    }
    pub fn as_atom(&self) -> Option<&str> {
        match self.raw.term() {
            ErlangTerm::Atom(name) => Some(name),
            _ => None,
        }
    }
    pub fn normal_cmp(&self, other: &Self) -> Ordering {
        self.order
            .compare(self.raw.id(), &other.order, other.raw.id())
            .expect("validated literal roots")
    }
}
impl PartialEq for RawLiteral {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for RawLiteral {}
impl PartialOrd for RawLiteral {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for RawLiteral {
    fn cmp(&self, other: &Self) -> Ordering {
        self.order
            .compare_map_keys(self.raw.id(), &other.order, other.raw.id())
            .expect("validated literal roots")
    }
}

pub fn normal_list_cmp(a: &[RawLiteral], b: &[RawLiteral]) -> Ordering {
    a.iter()
        .zip(b)
        .map(|(a, b)| a.normal_cmp(b))
        .find(|cmp| !cmp.is_eq())
        .unwrap_or_else(|| a.len().cmp(&b.len()))
}
pub fn normal_usort(mut values: Vec<RawLiteral>) -> Vec<RawLiteral> {
    values.sort_by(RawLiteral::normal_cmp);
    values.dedup_by(|a, b| a.normal_cmp(b).is_eq());
    values
}

pub fn normal_union(a: &[RawLiteral], b: &[RawLiteral]) -> Vec<RawLiteral> {
    let (mut ai, mut bi) = (0, 0);
    let mut out = Vec::with_capacity(a.len() + b.len());
    while ai < a.len() && bi < b.len() {
        match a[ai].normal_cmp(&b[bi]) {
            Ordering::Less => {
                out.push(a[ai].clone());
                ai += 1;
            }
            Ordering::Equal => {
                out.push(a[ai].clone());
                ai += 1;
                bi += 1;
            }
            Ordering::Greater => {
                out.push(b[bi].clone());
                bi += 1;
            }
        }
    }
    out.extend_from_slice(&a[ai..]);
    out.extend_from_slice(&b[bi..]);
    out
}

use crate::{
    core::{ErlangTerm, ErlangTerms, TermId},
    lattice::{LatticeError, MapTypeEntry, Presence, Type, TypePredicate},
    term_order::TermOrderArena,
};

pub struct LiteralTypes {
    types: Vec<Option<Type>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn nested(count: usize, shared: bool) -> ErlangTerms {
        let mut terms = vec![ErlangTerm::Atom("value".into())];
        let mut root = TermId(0);
        for _ in 0..count {
            terms.push(ErlangTerm::Tuple(if shared {
                vec![root, root]
            } else {
                vec![root]
            }));
            root = TermId(terms.len() - 1);
        }
        ErlangTerms { root, terms }
    }
    #[test]
    fn deep_and_expanding_literals_fail_before_recursive_type_ownership() {
        for arena in [nested(50_000, false), nested(31, true)] {
            assert!(matches!(
                LiteralTypes::build(&arena),
                Err(LiteralError::ResourceLimit)
            ));
        }
    }
    #[test]
    fn overwritten_map_values_do_not_consume_the_type_materialization_budget() {
        let mut arena = nested(2_000, false);
        let deep = arena.root;
        let key = TermId(arena.terms.len());
        arena.terms.push(ErlangTerm::Atom("key".into()));
        let value = TermId(arena.terms.len());
        arena.terms.push(ErlangTerm::Atom("final".into()));
        arena.root = TermId(arena.terms.len());
        arena
            .terms
            .push(ErlangTerm::Map(vec![(key, deep), (key, value)]));
        let types = LiteralTypes::build(&arena).unwrap();
        assert!(types.get(deep).is_none());
        assert_eq!(
            types
                .get(arena.root)
                .unwrap()
                .map_get(&Type::atom("key"))
                .unwrap(),
            Type::atom("final")
        );
    }
    #[test]
    fn cons_spines_and_compact_lists_materialize_identical_types() {
        for improper in [false, true] {
            let mut arena = ErlangTerms {
                root: TermId(2),
                terms: vec![
                    ErlangTerm::Integer("65".into()),
                    ErlangTerm::Integer("66".into()),
                    if improper {
                        ErlangTerm::Atom("tail".into())
                    } else {
                        ErlangTerm::Nil
                    },
                ],
            };
            let tail = arena.root;
            let items = (0..10_000).map(|i| TermId(i % 2)).collect::<Vec<_>>();
            for head in items.iter().rev() {
                let rest = arena.root;
                arena.root = TermId(arena.terms.len());
                arena.terms.push(ErlangTerm::List {
                    items: vec![*head],
                    tail: rest,
                });
            }
            let spine = arena.root;
            let compact = TermId(arena.terms.len());
            arena.terms.push(ErlangTerm::List { items, tail });
            let types = LiteralTypes::build_roots(&arena, &[spine, compact]).unwrap();
            assert_eq!(types.get(spine), types.get(compact));
        }
    }
    #[test]
    fn nested_list_elements_still_exceed_the_ownership_depth_limit() {
        let mut arena = ErlangTerms {
            root: TermId(0),
            terms: vec![ErlangTerm::Nil],
        };
        for _ in 0..200 {
            let head = arena.root;
            arena.root = TermId(arena.terms.len());
            arena.terms.push(ErlangTerm::List {
                items: vec![head],
                tail: TermId(0),
            });
        }
        assert!(matches!(
            LiteralTypes::build(&arena),
            Err(LiteralError::ResourceLimit)
        ));
    }
    #[test]
    fn compact_lists_have_shallow_type_ownership() {
        let arena = ErlangTerms {
            root: TermId(2),
            terms: vec![
                ErlangTerm::Integer("65".into()),
                ErlangTerm::Nil,
                ErlangTerm::List {
                    items: vec![TermId(0); 10_000],
                    tail: TermId(1),
                },
            ],
        };
        assert!(LiteralTypes::build(&arena).is_ok());
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiteralError {
    Invalid(String),
    Lattice(LatticeError),
    ResourceLimit,
}
impl From<String> for LiteralError {
    fn from(error: String) -> Self {
        Self::Invalid(error)
    }
}
impl From<&str> for LiteralError {
    fn from(error: &str) -> Self {
        Self::Invalid(error.into())
    }
}
impl From<LatticeError> for LiteralError {
    fn from(error: LatticeError) -> Self {
        Self::Lattice(error)
    }
}
impl std::fmt::Display for LiteralError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "literal type: {self:?}")
    }
}
impl std::error::Error for LiteralError {}
impl From<LiteralError> for String {
    fn from(error: LiteralError) -> Self {
        error.to_string()
    }
}
type Result<T> = std::result::Result<T, LiteralError>;
impl LiteralTypes {
    pub fn build(arena: &ErlangTerms) -> Result<Self> {
        Self::build_roots(arena, &[arena.root])
    }
    pub fn build_roots(arena: &ErlangTerms, roots: &[TermId]) -> Result<Self> {
        // Only the terms reachable from the roots are typed or compared.
        let built = TermOrderArena::reachable(arena, roots)
            .map_err(LiteralError::from)
            .and_then(|order| Self::build_with(arena, roots, &order));
        if crate::lattice::verify_fast_paths() {
            let full = TermOrderArena::new(arena)
                .map_err(LiteralError::from)
                .and_then(|order| Self::build_with(arena, roots, &order));
            assert_eq!(
                full.as_ref().map(|x| &x.types),
                built.as_ref().map(|x| &x.types),
                "restricted literal types diverged"
            );
        }
        built
    }
    fn build_with(arena: &ErlangTerms, roots: &[TermId], order: &TermOrderArena) -> Result<Self> {
        let mut needed = vec![false; arena.terms.len()];
        let mut pending = roots.to_vec();
        while let Some(id) = pending.pop() {
            let active = needed
                .get_mut(id.0)
                .ok_or("literal root outside term arena")?;
            if *active {
                continue;
            }
            *active = true;
            match &arena.terms[id.0] {
                ErlangTerm::Tuple(items) => pending.extend(items),
                ErlangTerm::List { items, tail } => {
                    pending.extend(items);
                    pending.push(*tail);
                }
                ErlangTerm::Map(pairs) => pending.extend(
                    order
                        .canonical_map_pairs(pairs)?
                        .iter()
                        .flat_map(|(key, value)| [*key, *value]),
                ),
                _ => {}
            }
        }
        let mut depths: Vec<usize> = vec![0; arena.terms.len()];
        let mut occurrences: Vec<usize> = vec![0; arena.terms.len()];
        let mut list_types = vec![false; arena.terms.len()];
        for (index, term) in arena.terms.iter().enumerate() {
            if !needed[index] {
                continue;
            }
            let children: Vec<_> = match term {
                ErlangTerm::Tuple(items) => items.clone(),
                ErlangTerm::List { items, tail } => items.iter().copied().chain([*tail]).collect(),
                ErlangTerm::Map(pairs) => order
                    .canonical_map_pairs(pairs)?
                    .into_iter()
                    .flat_map(|(key, value)| [key, value])
                    .collect(),
                _ => vec![],
            };
            if let ErlangTerm::List { items, tail } = term {
                if items.is_empty() {
                    depths[index] = depths[tail.0];
                    occurrences[index] = occurrences[tail.0];
                    list_types[index] = list_types[tail.0];
                } else {
                    // Type::cons joins the head into the tail's element domain.
                    // A cons spine therefore owns one list envelope, regardless
                    // of how many input term nodes represent the spine.
                    let tail_depth =
                        depths[tail.0].saturating_add(usize::from(!list_types[tail.0]));
                    depths[index] = items
                        .iter()
                        .map(|id| depths[id.0].saturating_add(1))
                        .chain([tail_depth])
                        .max()
                        .unwrap_or(tail_depth);
                    occurrences[index] = items.iter().fold(
                        occurrences[tail.0].saturating_add(usize::from(!list_types[tail.0])),
                        |sum, id| sum.saturating_add(occurrences[id.0]),
                    );
                    list_types[index] = true;
                }
            } else {
                depths[index] = children
                    .iter()
                    .map(|id| depths[id.0])
                    .max()
                    .unwrap_or(0)
                    .saturating_add(1);
                occurrences[index] = children
                    .iter()
                    .fold(1usize, |sum, id| sum.saturating_add(occurrences[id.0]));
            }
            if depths[index] > 128 || occurrences[index] > 1_000_000 {
                return Err(LiteralError::ResourceLimit);
            }
        }
        let mut types: Vec<Option<Type>> = Vec::with_capacity(arena.terms.len());
        for (index, term) in arena.terms.iter().enumerate() {
            if !needed[index] {
                types.push(None);
                continue;
            }
            let at = |id: TermId| {
                types
                    .get(id.0)
                    .and_then(|typ| typ.clone())
                    .ok_or_else(|| "literal child must precede its parent".to_owned())
            };
            let typ = match term {
                ErlangTerm::ExternalFun { arity, .. } => {
                    Type::function(Some(vec![Type::any(); usize::from(*arity)]), Type::any())
                }
                ErlangTerm::Atom(value) => Type::atom(value),
                ErlangTerm::Integer(value) => {
                    Type::integer(value.parse().map_err(|_| "invalid integer literal")?)
                }
                ErlangTerm::Float(_) => Type::float(),
                ErlangTerm::Binary(bytes) => Type::bitstring(0.into(), (bytes.len() * 8).into())
                    .expect("nonnegative binary length"),
                ErlangTerm::Bitstring { bits, .. } => {
                    Type::bitstring(0.into(), (*bits).into()).expect("nonnegative bitstring length")
                }
                ErlangTerm::Nil => Type::nil(),
                ErlangTerm::Tuple(items) => Type::tuple(
                    items
                        .iter()
                        .map(|id| at(*id))
                        .collect::<std::result::Result<_, _>>()?,
                )?,
                ErlangTerm::List { items, tail } => {
                    let mut typ = at(*tail)?;
                    for head in items.iter().rev() {
                        typ = Type::cons(at(*head)?, typ)?;
                    }
                    typ
                }
                ErlangTerm::Map(pairs) => {
                    let unique = order.canonical_map_pairs(pairs)?;
                    let mut entries = Vec::new();
                    let mut default_key = Type::none();
                    let mut default_value = Type::none();
                    for (key, value) in unique {
                        let key = at(key)?;
                        let value = at(value)?;
                        if key.predicate(TypePredicate::Singleton)? {
                            entries.push(MapTypeEntry {
                                key,
                                presence: Presence::Mandatory,
                                value,
                            });
                        } else {
                            default_key = key.join(&default_key)?;
                            default_value = value.join(&default_value)?;
                        }
                    }
                    entries.sort_by(|a, b| a.key.native_record_order(&b.key));
                    Type::map_types(entries, default_key, default_value)?
                }
            };
            types.push(Some(typ));
        }
        Ok(Self { types })
    }
    pub fn get(&self, id: TermId) -> Option<&Type> {
        self.types.get(id.0).and_then(|typ| typ.as_ref())
    }
}

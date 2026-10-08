use crate::{
    core::{ErlangTerm, TermId},
    form_error::{FormError, ReasonBuilder},
    native_form_environment::RawFormTerm,
    program::FunctionName,
    term_order::{compare_map_keys, compare_terms},
};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

#[derive(Clone, Debug)]
pub struct DeclarationKey {
    raw: RawFormTerm,
    callable: Option<FunctionName>,
}
impl DeclarationKey {
    pub fn parse(module: &str, identifier: &RawFormTerm) -> Result<Self, FormError> {
        let mut b = ReasonBuilder::new();
        let fields = match identifier.term() {
            ErlangTerm::Tuple(fields) if fields.len() == 2 => {
                let m = b.atom(module);
                let f = b.copy(&RawFormTerm::new(identifier.arena().clone(), fields[0])?);
                let a = b.copy(&RawFormTerm::new(identifier.arena().clone(), fields[1])?);
                vec![m, f, a]
            }
            ErlangTerm::Tuple(fields) if fields.len() == 3 => fields
                .iter()
                .map(|id| Ok(b.copy(&RawFormTerm::new(identifier.arena().clone(), *id)?)))
                .collect::<Result<Vec<_>, FormError>>()?,
            _ => {
                let tag = b.atom("case_clause");
                let id = b.copy(identifier);
                let reason = b.tuple(vec![tag, id]);
                return Err(FormError::ErrorReason(b.finish(reason)));
            }
        };
        let root = b.tuple(fields);
        let raw = b.finish(root);
        let ErlangTerm::Tuple(fields) = raw.term() else {
            unreachable!()
        };
        let callable = match (
            &raw.arena().terms[fields[0].0],
            &raw.arena().terms[fields[1].0],
            &raw.arena().terms[fields[2].0],
        ) {
            (ErlangTerm::Atom(module), ErlangTerm::Atom(name), ErlangTerm::Integer(arity)) => arity
                .parse::<BigInt>()
                .ok()
                .and_then(|a| a.to_usize())
                .map(|arity| FunctionName {
                    module: module.clone(),
                    name: name.clone(),
                    arity,
                }),
            _ => None,
        };
        Ok(Self { raw, callable })
    }
    pub fn from_callable(name: &FunctionName) -> Self {
        let mut b = ReasonBuilder::new();
        let m = b.atom(&name.module);
        let f = b.atom(&name.name);
        let a = b.integer(name.arity);
        let root = b.tuple(vec![m, f, a]);
        Self {
            raw: b.finish(root),
            callable: Some(name.clone()),
        }
    }
    pub fn raw(&self) -> &RawFormTerm {
        &self.raw
    }
    pub fn callable(&self) -> Option<&FunctionName> {
        self.callable.as_ref()
    }
    pub fn map_equals(&self, other: &Self) -> Result<bool, String> {
        Ok(compare_map_keys(
            self.raw.arena(),
            self.raw.id(),
            other.raw.arena(),
            other.raw.id(),
        )?
        .is_eq())
    }
    pub fn optional_membership_equals(&self, other: &Self) -> Result<bool, String> {
        Ok(compare_terms(
            self.raw.arena(),
            self.raw.id(),
            other.raw.arena(),
            other.raw.id(),
        )?
        .is_eq())
    }
}

pub fn optional_callbacks(
    module: &str,
    raw: &RawFormTerm,
) -> Result<Vec<DeclarationKey>, FormError> {
    let mut items = Vec::new();
    let mut id = raw.id();
    loop {
        match &raw.arena().terms[id.0] {
            ErlangTerm::Nil => break,
            ErlangTerm::List { items: part, tail } => {
                for id in part {
                    let ErlangTerm::Tuple(fields) = &raw.arena().terms[id.0] else {
                        return Ok(Vec::new());
                    };
                    if fields.len() != 2
                        || !matches!(&raw.arena().terms[fields[0].0], ErlangTerm::Atom(_))
                        || !nonnegative_integer(raw, fields[1])
                    {
                        return Ok(Vec::new());
                    }
                    items.push(*id);
                }
                id = *tail;
            }
            _ => return Ok(Vec::new()),
        }
    }
    items
        .into_iter()
        .map(|id| DeclarationKey::parse(module, &RawFormTerm::new(raw.arena().clone(), id)?))
        .collect()
}
fn nonnegative_integer(raw: &RawFormTerm, id: TermId) -> bool {
    matches!(&raw.arena().terms[id.0],ErlangTerm::Integer(value) if value.parse::<BigInt>().is_ok_and(|n|n>=0.into()))
}

/// get_spec_info recognizes one declaration per attribute and ignores other shapes.
pub fn declarations(
    module: &str,
    raw: &RawFormTerm,
) -> Result<Vec<(DeclarationKey, Vec<TermId>)>, FormError> {
    let Some(items) = proper_list(raw) else {
        return Ok(Vec::new());
    };
    if items.len() != 1 {
        return Ok(Vec::new());
    }
    let ErlangTerm::Tuple(fields) = &raw.arena().terms[items[0].0] else {
        return Ok(Vec::new());
    };
    if fields.len() != 2 {
        return Ok(Vec::new());
    }
    let alternatives = RawFormTerm::new(raw.arena().clone(), fields[1])?;
    let Some(alternatives) = proper_list(&alternatives) else {
        return Ok(Vec::new());
    };
    let identifier = RawFormTerm::new(raw.arena().clone(), fields[0])?;
    Ok(vec![(
        DeclarationKey::parse(module, &identifier)?,
        alternatives,
    )])
}

pub(crate) fn proper_list(raw: &RawFormTerm) -> Option<Vec<TermId>> {
    let mut out = Vec::new();
    let mut id = raw.id();
    loop {
        match &raw.arena().terms[id.0] {
            ErlangTerm::Nil => return Some(out),
            ErlangTerm::List { items, tail } => {
                out.extend(items);
                id = *tail
            }
            _ => return None,
        }
    }
}

impl PartialEq for DeclarationKey {
    fn eq(&self, other: &Self) -> bool {
        self.map_equals(other).expect("validated declaration keys")
    }
}
impl Eq for DeclarationKey {}
impl PartialOrd for DeclarationKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for DeclarationKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        compare_map_keys(
            self.raw.arena(),
            self.raw.id(),
            other.raw.arena(),
            other.raw.id(),
        )
        .expect("validated declaration keys")
    }
}
impl DeclarationKey {
    pub fn callable_mfa(&self) -> Option<crate::bifs::registry::Mfa> {
        self.callable().map(|f| crate::bifs::registry::Mfa {
            module: f.module.clone(),
            function: f.name.clone(),
            arity: f.arity,
        })
    }
}

impl DeclarationKey {
    pub fn fields(&self) -> [RawFormTerm; 3] {
        let ErlangTerm::Tuple(fields) = self.raw.term() else {
            unreachable!("normalized declaration key")
        };
        std::array::from_fn(|i| {
            RawFormTerm::new(self.raw.arena().clone(), fields[i])
                .expect("normalized declaration field")
        })
    }
}

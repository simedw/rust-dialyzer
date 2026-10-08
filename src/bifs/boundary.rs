use crate::lattice::{MapEntry, MapKey, NominalName, Opacity, Presence, Type};
use num_bigint::BigInt;
use serde::Deserialize;
use std::str::FromStr;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NativeTerm {
    Atom { value: String },
    Integer { value: String },
    Tuple { items: Vec<Self> },
    List { items: Vec<Self>, tail: Box<Self> },
    Nil,
}
impl NativeTerm {
    pub(crate) fn append_raw(
        &self,
        builder: &mut crate::form_error::ReasonBuilder,
    ) -> crate::core::TermId {
        enum Frame<'a> {
            Visit(&'a NativeTerm),
            Tuple(usize),
            List(usize),
        }
        let mut frames = vec![Frame::Visit(self)];
        let mut values = Vec::new();
        while let Some(frame) = frames.pop() {
            match frame {
                Frame::Visit(value) => match value {
                    Self::Atom { value } => values.push(builder.atom(value)),
                    Self::Integer { value } => values.push(builder.integer(value)),
                    Self::Nil => values.push(builder.nil()),
                    Self::Tuple { items } => {
                        frames.push(Frame::Tuple(items.len()));
                        frames.extend(items.iter().rev().map(Frame::Visit));
                    }
                    Self::List { items, tail } => {
                        frames.push(Frame::List(items.len()));
                        frames.push(Frame::Visit(tail));
                        frames.extend(items.iter().rev().map(Frame::Visit));
                    }
                },
                Frame::Tuple(count) => {
                    let items = values.split_off(values.len() - count);
                    values.push(builder.tuple(items));
                }
                Frame::List(count) => {
                    let tail = values.pop().expect("list tail visited");
                    let items = values.split_off(values.len() - count);
                    values.push(builder.list_tail(items, tail));
                }
            }
        }
        values.pop().expect("native term emits one root")
    }
    fn atom(&self) -> Result<&str, String> {
        match self {
            Self::Atom { value } => Ok(value),
            _ => Err("expected atom".into()),
        }
    }
    fn integer(&self) -> Result<BigInt, String> {
        match self {
            Self::Integer { value } => BigInt::from_str(value).map_err(|e| e.to_string()),
            _ => Err("expected integer".into()),
        }
    }
    fn tuple(&self) -> Result<&[Self], String> {
        match self {
            Self::Tuple { items } => Ok(items),
            _ => Err("expected tuple".into()),
        }
    }
    fn tuple_len(&self, length: usize) -> Result<&[Self], String> {
        let items = self.tuple()?;
        if items.len() != length {
            return Err(format!(
                "expected tuple of length{length}, found{}",
                items.len()
            ));
        }
        Ok(items)
    }
    fn list(&self) -> Result<&[Self], String> {
        match self {
            Self::List { items, tail } if matches!(tail.as_ref(), Self::Nil) => Ok(items),
            Self::Nil => Ok(&[]),
            _ => Err("expected proper list".into()),
        }
    }
    fn bound(&self) -> Result<String, String> {
        match self {
            Self::Integer { value } | Self::Atom { value } => Ok(value.clone()),
            _ => Err("expected integer endpoint".into()),
        }
    }
    pub fn type_value(&self) -> Result<Type, String> {
        if let Self::Atom { value } = self {
            return match value.as_str() {
                "any" => Ok(Type::any()),
                "none" => Ok(Type::none()),
                "unit" => Ok(Type::unit()),
                "opaque" => Ok(Type::opaque_sentinel()),
                _ => Err(format!("unknown native type atom {value}")),
            };
        }
        let fields = self.tuple_len(4)?;
        if fields.len() != 4 || fields[0].atom()? != "c" {
            return Err("expected native c record".into());
        }
        let elements = &fields[2];
        let qualifier = &fields[3];
        Ok(match fields[1].atom()? {
            "nominal" => {
                let parts = elements.tuple_len(2)?;
                let name = match &parts[0] {
                    Self::Atom { value } => NominalName::Symbol(value.clone()),
                    value => {
                        let n = value.tuple_len(4)?;
                        NominalName::Defined {
                            module: n[0].atom()?.into(),
                            name: n[1].atom()?.into(),
                            arity: n[2]
                                .integer()?
                                .to_string()
                                .parse()
                                .map_err(|_| "invalid nominal arity")?,
                            opacity: match n[3].atom()? {
                                "opaque" => Opacity::Opaque,
                                "transparent" => Opacity::Transparent,
                                _ => return Err("invalid nominal opacity".into()),
                            },
                        }
                    }
                };
                Type::nominal(name, parts[1].type_value()?)
            }
            "nominal_set" => {
                let parts = elements.tuple_len(2)?;
                parts[0]
                    .list()?
                    .iter()
                    .try_fold(parts[1].type_value()?, |acc, n| {
                        acc.join(&n.type_value()?).map_err(|e| e.to_string())
                    })?
            }
            "atom" => {
                if matches!(elements, Self::Atom { value } if value=="any") {
                    Type::all_atoms()
                } else {
                    Type::atoms(
                        elements
                            .list()?
                            .iter()
                            .map(|x| x.atom().map(str::to_owned))
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                }
            }
            "number" => match qualifier.atom()? {
                "float" => Type::float(),
                "unknown" => Type::number(),
                "integer" => {
                    if matches!(elements, Self::Atom { value } if value=="any") {
                        Type::all_integers()
                    } else {
                        let ints = elements.tuple()?;
                        let tag = ints
                            .first()
                            .ok_or("empty native numeric representation")?
                            .atom()?;
                        match tag {
                            "int_set" => {
                                let ints = elements.tuple_len(2)?;
                                Type::integers(
                                    ints[1]
                                        .list()?
                                        .iter()
                                        .map(Self::integer)
                                        .collect::<Result<Vec<_>, _>>()?,
                                )
                            }
                            "int_rng" => {
                                let ints = elements.tuple_len(3)?;
                                Type::range_strings(&ints[1].bound()?, &ints[2].bound()?)?
                            }
                            tag => return Err(format!("unsupported native numeric tag {tag}")),
                        }
                    }
                }
                tag => return Err(format!("unsupported native number qualifier {tag}")),
            },
            "nil" => Type::nil(),
            "list" => {
                let pair = elements.tuple_len(2)?;
                Type::list(
                    pair[0].type_value()?,
                    pair[1].type_value()?,
                    qualifier.atom()? == "nonempty",
                )
            }
            "tuple" => {
                if matches!(elements, Self::Atom { value } if value=="any") {
                    Type::tuple_any()
                } else {
                    Type::tuple(
                        elements
                            .list()?
                            .iter()
                            .map(Self::type_value)
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                    .map_err(|e| e.to_string())?
                }
            }
            "tuple_set" => {
                let mut result = Type::none();
                for group in elements.list()? {
                    for variant in group.tuple_len(2)?[1].list()? {
                        result = result
                            .join(&variant.type_value()?)
                            .map_err(|e| e.to_string())?;
                    }
                }
                result
            }
            "identifier" => {
                if matches!(elements, Self::Atom { value } if value=="any") {
                    Type::identifier(None)?
                } else {
                    Type::identifier(Some(
                        elements
                            .list()?
                            .iter()
                            .map(|x| x.atom().map(str::to_owned))
                            .collect::<Result<Vec<_>, _>>()?,
                    ))?
                }
            }
            "binary" => {
                let pair = elements.tuple_len(2)?;
                Type::bitstring(pair[0].integer()?, pair[1].integer()?)?
            }
            "function" => {
                let pair = elements.tuple_len(2)?;
                let domain = if matches!(&pair[0],Self::Atom{value} if value=="any") {
                    None
                } else {
                    let product = pair[0].tuple_len(4)?;
                    if product[0].atom()? != "c" || product[1].atom()? != "product" {
                        return Err("expected native function product domain".into());
                    }
                    Some(
                        product[2]
                            .list()?
                            .iter()
                            .map(Self::type_value)
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                };
                Type::function(domain, pair[1].type_value()?)
            }
            "map" => {
                let parts = elements.tuple_len(3)?;
                let entries = parts[0]
                    .list()?
                    .iter()
                    .map(|entry| {
                        let parts = entry.tuple_len(3)?;
                        Ok::<_, String>(MapEntry {
                            key: MapKey::from_type(&parts[0].type_value()?)?,
                            presence: match parts[1].atom()? {
                                "mandatory" => Presence::Mandatory,
                                "optional" => Presence::Optional,
                                _ => return Err("unknown map presence".into()),
                            },
                            value: parts[2].type_value()?,
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Type::map(entries, parts[1].type_value()?, parts[2].type_value()?)
                    .map_err(|e| e.to_string())?
            }
            "product" => Type::product(
                elements
                    .list()?
                    .iter()
                    .map(Self::type_value)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            "union" => {
                let values = elements.list()?;
                if values.len() != 8 {
                    return Err("native union requires8slots".into());
                }
                values.iter().try_fold(Type::none(), |a, x| {
                    a.join(&x.type_value()?).map_err(|e| e.to_string())
                })?
            }
            "var" => match elements {
                Self::Atom { value } => Type::variable_atom(value),
                Self::Integer { .. } => Type::variable_integer(elements.integer()?),
                _ => return Err("unsupported type variable id".into()),
            },
            tag => return Err(format!("unsupported native type family {tag}")),
        })
    }
}

impl NativeTerm {
    pub fn from_arena(
        arena: &crate::core::ErlangTerms,
        id: crate::core::TermId,
    ) -> Result<Self, String> {
        if id.0 >= arena.terms.len() {
            return Err("native metadata term reference is outside arena".into());
        }
        decode_arena(arena, id, 0)
    }
}
fn decode_arena(
    arena: &crate::core::ErlangTerms,
    id: crate::core::TermId,
    depth: usize,
) -> Result<NativeTerm, String> {
    if depth > 256 {
        return Err("native metadata type exceeds representation depth".into());
    }
    use crate::core::ErlangTerm;
    Ok(match &arena.terms[id.0] {
        ErlangTerm::Atom(value) => NativeTerm::Atom {
            value: value.clone(),
        },
        ErlangTerm::Integer(value) => NativeTerm::Integer {
            value: value.clone(),
        },
        ErlangTerm::Nil => NativeTerm::Nil,
        ErlangTerm::Tuple(ids) => NativeTerm::Tuple {
            items: ids
                .iter()
                .map(|id| decode_arena(arena, *id, depth + 1))
                .collect::<Result<_, _>>()?,
        },
        ErlangTerm::List { items, tail } => NativeTerm::List {
            items: items
                .iter()
                .map(|id| decode_arena(arena, *id, depth + 1))
                .collect::<Result<_, _>>()?,
            tail: Box::new(decode_arena(arena, *tail, depth + 1)?),
        },
        other => return Err(format!("invalid native type scalar {other:?}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn variants(term: &NativeTerm, out: &mut Vec<NativeTerm>) {
        match term {
            NativeTerm::Tuple { items } => {
                for length in 0..items.len() {
                    out.push(NativeTerm::Tuple {
                        items: items[..length].to_vec(),
                    });
                }
                for (i, item) in items.iter().enumerate() {
                    let mut children = Vec::new();
                    variants(item, &mut children);
                    for child in children {
                        let mut copy = items.clone();
                        copy[i] = child;
                        out.push(NativeTerm::Tuple { items: copy });
                    }
                }
            }
            NativeTerm::List { items, tail } => {
                for (i, item) in items.iter().enumerate() {
                    let mut children = Vec::new();
                    variants(item, &mut children);
                    for child in children {
                        let mut copy = items.clone();
                        copy[i] = child;
                        out.push(NativeTerm::List {
                            items: copy,
                            tail: tail.clone(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    #[test]
    fn malformed_native_record_vectors_never_panic() {
        let catalogue: serde_json::Value =
            serde_json::from_str(include_str!("../../corpus/bifs.json")).unwrap();
        let mut count = 0;
        for entry in catalogue["entries"].as_array().unwrap() {
            for value in entry["admissible_domains"]["arguments"].as_array().unwrap() {
                let term: NativeTerm = serde_json::from_value(value["native"].clone()).unwrap();
                let mut cases = Vec::new();
                variants(&term, &mut cases);
                for malformed in cases {
                    assert!(std::panic::catch_unwind(|| malformed.type_value()).is_ok());
                    count += 1;
                }
            }
        }
        assert!(count > 1000, "actual malformed cases {count}");
    }
}

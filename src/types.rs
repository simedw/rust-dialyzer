use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Type {
    None,
    Any,
    Integer,
    Float,
    Atom(Option<String>),
    Binary,
    List(Box<Type>),
    Tuple(Vec<Type>),
    Union(Vec<Type>),
}

impl Type {
    pub fn number() -> Self {
        Self::Union(vec![Self::Integer, Self::Float])
    }

    pub fn union(self, other: Self) -> Self {
        if self == other || other == Self::None {
            return self;
        }
        if self == Self::None {
            return other;
        }
        if self == Self::Any || other == Self::Any {
            return Self::Any;
        }
        match (&self, &other) {
            (Self::Atom(None), Self::Atom(_)) | (Self::Atom(_), Self::Atom(None)) => {
                return Self::Atom(None);
            }
            (Self::List(a), Self::List(b)) => {
                return Self::List(Box::new(a.clone().union(*b.clone())));
            }
            (Self::Tuple(a), Self::Tuple(b)) if a.len() == b.len() => {
                return Self::Tuple(
                    a.iter()
                        .zip(b)
                        .map(|(a, b)| a.clone().union(b.clone()))
                        .collect(),
                );
            }
            _ => {}
        }
        let mut members = Vec::new();
        for ty in [self, other] {
            match ty {
                Self::Union(xs) => members.extend(xs),
                x => members.push(x),
            }
        }
        members.sort();
        members.dedup();
        if members.contains(&Self::Atom(None)) {
            members.retain(|t| !matches!(t, Self::Atom(Some(_))));
        }
        Self::Union(members)
    }

    pub fn intersect(&self, other: &Self) -> Self {
        if self == other || other == &Self::Any {
            return self.clone();
        }
        if self == &Self::Any {
            return other.clone();
        }
        match (self, other) {
            (Self::Union(xs), y) | (y, Self::Union(xs)) => xs
                .iter()
                .map(|x| x.intersect(y))
                .fold(Self::None, Self::union),
            (Self::Atom(None), Self::Atom(x)) | (Self::Atom(x), Self::Atom(None)) => {
                Self::Atom(x.clone())
            }
            (Self::List(a), Self::List(b)) => Self::List(Box::new(a.intersect(b))),
            (Self::Tuple(a), Self::Tuple(b)) if a.len() == b.len() => {
                let xs: Vec<_> = a.iter().zip(b).map(|(a, b)| a.intersect(b)).collect();
                if xs.contains(&Self::None) {
                    Self::None
                } else {
                    Self::Tuple(xs)
                }
            }
            _ => Self::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Type;
    #[test]
    fn disjoint_and_structural_types() {
        assert_eq!(Type::Integer.intersect(&Type::Binary), Type::None);
        assert_eq!(Type::number().intersect(&Type::Integer), Type::Integer);
        assert_eq!(
            Type::Tuple(vec![Type::Integer]).intersect(&Type::Tuple(vec![Type::Binary])),
            Type::None
        );
        assert_eq!(
            Type::Atom(None).intersect(&Type::Atom(Some("ok".into()))),
            Type::Atom(Some("ok".into()))
        );
    }
}

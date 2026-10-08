#[cfg(test)]
use crate::lattice::NativeValue;
use crate::lattice::{Type, VariableId};
use std::cmp::Ordering;
/// Integer/atom names used by substitution and raw records used by the pinned
/// reference solver are distinct Erlang map keys, even for the same variable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingKey {
    Label(VariableId),
    RawVariableRecord(VariableId),
}
impl BindingKey {
    pub fn label(&self) -> Option<&VariableId> {
        match self {
            Self::Label(v) => Some(v),
            Self::RawVariableRecord(_) => None,
        }
    }
    #[cfg(test)]
    fn native(&self) -> NativeValue {
        match self {
            Self::Label(VariableId::Integer(n)) => NativeValue::Integer(n.to_bigint()),
            Self::Label(VariableId::Atom(a)) => NativeValue::Atom(a.clone()),
            Self::RawVariableRecord(VariableId::Integer(n)) => {
                Type::variable_id(VariableId::Integer(n.clone())).to_native_value()
            }
            Self::RawVariableRecord(VariableId::Atom(a)) => {
                Type::variable_atom(a.clone()).to_native_value()
            }
        }
    }
}
impl From<VariableId> for BindingKey {
    fn from(v: VariableId) -> Self {
        Self::Label(v)
    }
}
impl Ord for BindingKey {
    fn cmp(&self, other: &Self) -> Ordering {
        let result = match (self, other) {
            (Self::Label(a), Self::Label(b))
            | (Self::RawVariableRecord(a), Self::RawVariableRecord(b)) => a.cmp(b),
            (Self::Label(_), Self::RawVariableRecord(_)) => Ordering::Less,
            (Self::RawVariableRecord(_), Self::Label(_)) => Ordering::Greater,
        };
        result
    }
}
impl PartialOrd for BindingKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
pub type BindingMap = im::OrdMap<BindingKey, Type>;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_large_unicode_alias_and_raw_keys_keep_native_order() {
        let mut ids = [
            "-99999999999999999999999999999999999",
            "-9223372036854775809",
            "-9223372036854775808",
            "-1",
            "0",
            "1",
            "9223372036854775807",
            "9223372036854775808",
            "18446744073709551615",
            "99999999999999999999999999999999999",
        ]
        .iter()
        .map(|s| VariableId::Integer(s.parse().unwrap()))
        .collect::<Vec<_>>();
        ids.extend(
            ["", "\0", "1", "-1", "é", "😀", "z"]
                .iter()
                .map(|s| VariableId::Atom((*s).into())),
        );
        let keys = ids
            .iter()
            .flat_map(|v| {
                [
                    BindingKey::Label(v.clone()),
                    BindingKey::RawVariableRecord(v.clone()),
                ]
            })
            .collect::<Vec<_>>();
        for a in &keys {
            for b in &keys {
                assert_eq!(a.cmp(b), a.native().cmp(&b.native()));
                assert_eq!(a == b, a.cmp(b).is_eq());
            }
        }
        for id in ids {
            assert_ne!(
                BindingKey::Label(id.clone()),
                BindingKey::RawVariableRecord(id)
            );
        }
    }
}

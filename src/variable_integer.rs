use num_bigint::{BigInt, Sign};
use num_traits::ToPrimitive;
use std::{cmp::Ordering, fmt, str::FromStr, sync::Arc};
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct VariableInteger(Repr);
#[derive(Clone, PartialEq, Eq, Hash)]
enum Repr {
    Small(i64),
    Large(Arc<BigInt>),
}
impl VariableInteger {
    pub fn to_bigint(&self) -> BigInt {
        match &self.0 {
            Repr::Small(n) => (*n).into(),
            Repr::Large(n) => (**n).clone(),
        }
    }
    pub fn small(&self) -> Option<i64> {
        match &self.0 {
            Repr::Small(n) => Some(*n),
            Repr::Large(_) => None,
        }
    }
    pub fn to_u64(&self) -> Option<u64> {
        match &self.0 {
            Repr::Small(n) => (*n).try_into().ok(),
            Repr::Large(n) => n.to_u64(),
        }
    }
}
impl From<BigInt> for VariableInteger {
    fn from(n: BigInt) -> Self {
        match n.to_i64() {
            Some(n) => Self(Repr::Small(n)),
            None => Self(Repr::Large(Arc::new(n))),
        }
    }
}
macro_rules! primitive {($($kind:ty),*)=>{$(impl From<$kind> for VariableInteger{fn from(n:$kind)->Self{match i64::try_from(n){Ok(n)=>Self(Repr::Small(n)),Err(_)=>Self::from(BigInt::from(n))}}})*};}
primitive!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize
);
impl FromStr for VariableInteger {
    type Err = num_bigint::ParseBigIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<BigInt>().map(Self::from)
    }
}
impl Ord for VariableInteger {
    fn cmp(&self, b: &Self) -> Ordering {
        match (&self.0, &b.0) {
            (Repr::Small(a), Repr::Small(b)) => a.cmp(b),
            (Repr::Large(a), Repr::Large(b)) => a.cmp(b),
            (Repr::Small(_), Repr::Large(b)) => {
                if b.sign() == Sign::Minus {
                    Ordering::Greater
                } else {
                    Ordering::Less
                }
            }
            (Repr::Large(a), Repr::Small(_)) => {
                if a.sign() == Sign::Minus {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            }
        }
    }
}
impl PartialOrd for VariableInteger {
    fn partial_cmp(&self, b: &Self) -> Option<Ordering> {
        Some(self.cmp(b))
    }
}
impl fmt::Display for VariableInteger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Repr::Small(n) => fmt::Display::fmt(n, f),
            Repr::Large(n) => fmt::Display::fmt(n, f),
        }
    }
}
impl fmt::Debug for VariableInteger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_signed_boundary_order_debug_and_shared_large() {
        let values = [
            "-18446744073709551616",
            "-9223372036854775809",
            "-9223372036854775808",
            "-1",
            "-0",
            "0",
            "1",
            "9223372036854775807",
            "9223372036854775808",
            "18446744073709551615",
            "999999999999999999999999999999999999999",
        ];
        let old = values
            .iter()
            .map(|s| s.parse::<BigInt>().unwrap())
            .collect::<Vec<_>>();
        let new = old
            .iter()
            .cloned()
            .map(VariableInteger::from)
            .collect::<Vec<_>>();
        for (a, x) in old.iter().zip(&new) {
            assert_eq!(a.to_string(), x.to_string());
            assert_eq!(format!("{a:?}"), format!("{x:?}"));
            assert_eq!(format!("{a:#?}"), format!("{x:#?}"));
            assert_eq!(*a, x.to_bigint());
            assert_eq!(a.to_u64(), x.to_u64());
            assert_eq!(matches!(x.0, Repr::Small(_)), a.to_i64().is_some());
            for (b, y) in old.iter().zip(&new) {
                assert_eq!(a.cmp(b), x.cmp(y));
                assert_eq!(a == b, x == y);
            }
        }
        let large = new.last().unwrap();
        let cloned = large.clone();
        match (&large.0, &cloned.0) {
            (Repr::Large(a), Repr::Large(b)) => assert!(Arc::ptr_eq(a, b)),
            _ => panic!(),
        };
        assert_eq!(
            VariableInteger::from(u64::MAX).to_bigint(),
            BigInt::from(u64::MAX)
        );
        assert_eq!(
            VariableInteger::from(i64::MIN).to_bigint(),
            BigInt::from(i64::MIN)
        );
    }
}

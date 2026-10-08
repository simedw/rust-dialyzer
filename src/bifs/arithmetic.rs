use crate::lattice::{Endpoint, EndpointView, IntegerView, NumberView, Type, TypeView};
use crate::native_error::NativeTransferError;
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bound {
    Neg,
    Finite(BigInt),
    Pos,
}
impl Bound {
    pub fn finite(value: i32) -> Self {
        Self::Finite(value.into())
    }
    fn inverse(&self) -> Self {
        match self {
            Self::Neg => Self::Pos,
            Self::Pos => Self::Neg,
            Self::Finite(x) => Self::Finite(-x),
        }
    }
    fn abs(&self) -> Self {
        match self {
            Self::Neg | Self::Pos => Self::Pos,
            Self::Finite(x) => Self::Finite(x.abs()),
        }
    }
    fn add(&self, rhs: &Self) -> Self {
        match (self, rhs) {
            (Self::Pos, _) => Self::Pos,
            (Self::Neg, _) => Self::Neg,
            (_, Self::Pos) => Self::Pos,
            (_, Self::Neg) => Self::Neg,
            (Self::Finite(a), Self::Finite(b)) => {
                finite_add(a, b).map(Self::Finite).unwrap_or_else(|| {
                    if a.is_negative() {
                        Self::Neg
                    } else {
                        Self::Pos
                    }
                })
            }
        }
    }
    fn multiply(&self, rhs: &Self) -> Self {
        match (self, rhs) {
            (Self::Neg, x) => {
                if x >= &Self::finite(0) {
                    Self::Neg
                } else {
                    Self::Pos
                }
            }
            (Self::Pos, x) => Self::Neg.multiply(x).inverse(),
            (x, Self::Pos) => Self::Neg.multiply(x).inverse(),
            (x, Self::Neg) => Self::Neg.multiply(x),
            (Self::Finite(a), Self::Finite(b)) => {
                finite_multiply(a, b).map(Self::Finite).unwrap_or_else(|| {
                    if a.is_negative() == b.is_negative() {
                        Self::Pos
                    } else {
                        Self::Neg
                    }
                })
            }
        }
    }
    fn divide(&self, rhs: &Self) -> Vec<Self> {
        use Bound::*;
        match (self, rhs) {
            (Pos, Pos) | (Neg, Neg) => vec![Self::finite(0), Pos],
            (Pos, Neg) | (Neg, Pos) => vec![Neg, Self::finite(0)],
            (Pos, Finite(x)) => vec![if x.is_positive() { Pos } else { Neg }],
            (Neg, Finite(x)) => vec![if x.is_positive() { Neg } else { Pos }],
            (Finite(x), Pos) => vec![if x.is_negative() { Neg } else { Pos }],
            (Finite(x), Neg) => vec![if x.is_negative() { Pos } else { Neg }],
            (Finite(a), Finite(b)) => vec![Finite(a / b)],
        }
    }
    fn shift(&self, rhs: &Self) -> Result<Self, NativeTransferError> {
        use Bound::*;
        Ok(match (self, rhs) {
            (Pos, _) => Pos,
            (Neg, _) => Neg,
            (Finite(x), Pos) => {
                if x.is_zero() {
                    Self::finite(0)
                } else if x.is_positive() {
                    Pos
                } else {
                    Neg
                }
            }
            (Finite(x), Neg) => {
                if x.is_negative() {
                    Self::finite(-1)
                } else {
                    Self::finite(0)
                }
            }
            (Finite(x), Finite(n)) => {
                if n > &BigInt::from(256) {
                    return self.shift(&Pos);
                }
                if n < &BigInt::from(-256) {
                    return self.shift(&Neg);
                }
                Finite(finite_shift(x, n).ok_or(NativeTransferError::SystemLimit)?)
            }
        })
    }
}
fn endpoint(value: EndpointView<'_>) -> Bound {
    match value {
        EndpointView::NegativeInfinity => Bound::Neg,
        EndpointView::PositiveInfinity => Bound::Pos,
        EndpointView::Finite(x) => Bound::Finite(x.clone()),
    }
}
pub fn integer_domain(value: &Type) -> Result<Option<Type>, NativeTransferError> {
    let value = value.shallow_structural()?;
    let integer = matches!(value.view(),TypeView::Families(s) if matches!(s.number,Some(NumberView::Integer(_)))&&s.atoms.is_none()&&s.list.is_none()&&s.tuples.is_none()&&s.identifiers.is_none()&&s.bitstrings.is_none()&&s.functions.is_none()&&s.maps.is_none());
    Ok(if integer { Some(value) } else { None })
}
pub fn values(value: &Type) -> Result<Option<Vec<BigInt>>, NativeTransferError> {
    let Some(value) = integer_domain(value)? else {
        return Ok(None);
    };
    Ok(match value.view() {
        TypeView::Families(s) => match s.number {
            Some(NumberView::Integer(IntegerView::Set(values))) => {
                Some(values.iter().cloned().collect())
            }
            _ => None,
        },
        _ => None,
    })
}
pub fn bounds(value: &Type) -> Result<Option<(Bound, Bound)>, NativeTransferError> {
    let Some(value) = integer_domain(value)? else {
        return Ok(None);
    };
    Ok(match value.view() {
        TypeView::Families(s) => match s.number {
            Some(NumberView::Integer(IntegerView::All)) => Some((Bound::Neg, Bound::Pos)),
            Some(NumberView::Integer(IntegerView::Set(values))) => values
                .first()
                .zip(values.last())
                .map(|(lo, hi)| (Bound::Finite(lo.clone()), Bound::Finite(hi.clone()))),
            Some(NumberView::Integer(IntegerView::Range { from, to })) => {
                Some((endpoint(from), endpoint(to)))
            }
            _ => None,
        },
        _ => None,
    })
}
pub fn range(from: Bound, to: Bound) -> Result<Type, NativeTransferError> {
    let endpoint = |bound| match bound {
        Bound::Neg => Endpoint::NegativeInfinity,
        Bound::Finite(value) => Endpoint::Finite(value),
        Bound::Pos => Endpoint::PositiveInfinity,
    };
    Type::range(endpoint(from), endpoint(to)).map_err(Into::into)
}
fn extremum(values: Vec<Bound>) -> (Bound, Bound) {
    (
        values.iter().min().unwrap().clone(),
        values.into_iter().max().unwrap(),
    )
}
fn width(value: &Bound) -> Bound {
    match value {
        Bound::Neg | Bound::Pos => Bound::Pos,
        Bound::Finite(x) => Bound::Finite(BigInt::from(if x.is_negative() {
            (!x).bits()
        } else {
            x.bits()
        })),
    }
}
fn range_width(lo: &Bound, hi: &Bound) -> Bound {
    width(lo).max(width(hi))
}
fn bit_bounds(
    op: &str,
    lo1: &Bound,
    hi1: &Bound,
    lo2: &Bound,
    hi2: &Bound,
) -> Result<(Bound, Bound), NativeTransferError> {
    let w1 = range_width(lo1, hi1);
    let w2 = range_width(lo2, hi2);
    let zero = Bound::finite(0);
    let (w, positive_lo, positive_hi) = if op == "band" {
        (
            w1.min(w2),
            lo1 >= &zero || lo2 >= &zero,
            hi1 >= &zero || hi2 >= &zero,
        )
    } else {
        (
            w1.max(w2),
            lo1 >= &zero && lo2 >= &zero,
            hi1 >= &zero && hi2 >= &zero,
        )
    };
    Ok((
        if positive_lo {
            zero.clone()
        } else {
            Bound::finite(-1).shift(&w)?
        },
        if positive_hi {
            Bound::finite(1).shift(&w)?.add(&Bound::finite(-1))
        } else if op == "band" {
            zero
        } else {
            Bound::finite(-1)
        },
    ))
}
fn range_operation(op: &str, a: &Type, b: &Type) -> Result<Type, NativeTransferError> {
    let (lo1, hi1) = bounds(a)?.ok_or(NativeTransferError::FunctionClause)?;
    let (lo2, hi2) = bounds(b)?.ok_or(NativeTransferError::FunctionClause)?;
    let zero = Bound::finite(0);
    let result = match op {
        "+" => (lo1.add(&lo2), hi1.add(&hi2)),
        "-" => (lo1.add(&hi2.inverse()), hi1.add(&lo2.inverse())),
        "*" => extremum(vec![
            lo1.multiply(&lo2),
            lo1.multiply(&hi2),
            hi1.multiply(&lo2),
            hi1.multiply(&hi2),
        ]),
        "div" => {
            if lo2 == zero && hi2 == zero {
                (Bound::Pos, Bound::Neg)
            } else {
                let low = if lo2 == zero { Bound::finite(1) } else { lo2 };
                let high = if hi2 == zero { Bound::finite(-1) } else { hi2 };
                extremum(
                    [
                        lo1.divide(&low),
                        lo1.divide(&high),
                        hi1.divide(&low),
                        hi1.divide(&high),
                    ]
                    .concat(),
                )
            }
        }
        "rem" => {
            let maxdiv = lo2.abs().max(hi2.abs());
            (
                if lo1 >= zero || maxdiv == zero {
                    zero.clone()
                } else {
                    maxdiv.inverse().add(&Bound::finite(1))
                },
                if hi1 <= zero || maxdiv == zero {
                    zero
                } else {
                    maxdiv.add(&Bound::finite(-1))
                },
            )
        }
        "bsl" | "bsr" => {
            let (low, high) = if op == "bsr" {
                (hi2.inverse(), lo2.inverse())
            } else {
                (lo2, hi2)
            };
            if lo1 >= zero {
                (lo1.shift(&low)?, hi1.shift(&high)?)
            } else if hi1 >= zero {
                (lo1.shift(&high)?, hi1.shift(&high)?)
            } else {
                (lo1.shift(&high)?, high.shift(&low)?)
            }
        }
        "band" | "bor" | "bxor" => {
            let (mut low, mut high) = bit_bounds(op, &lo1, &hi1, &lo2, &hi2)?;
            if op != "bxor"
                && let Some(set) = values(a)?.or(values(b)?)
            {
                let mask = set
                    .into_iter()
                    .reduce(|x, y| if op == "band" { x | y } else { x & y })
                    .unwrap();
                let apply = |v: Bound| match v {
                    Bound::Finite(x) => Bound::Finite(if op == "band" {
                        x.clone() & mask.clone()
                    } else {
                        x | mask.clone()
                    }),
                    infinite => {
                        if op == "band" {
                            Bound::Finite(mask.clone())
                        } else {
                            infinite
                        }
                    }
                };
                low = apply(low);
                high = apply(high)
            }
            (low, high)
        }
        _ => return Err(format!("unsupported arithmetic operator {op}").into()),
    };
    range(result.0, result.1)
}
// ERTS 16.2 with 64-bit words permits at most 65,535 bignum limbs.
const ERTS_MAX_BIGNUM_BITS: u64 = 65_535 * 64;
fn finite_add(left: &BigInt, right: &BigInt) -> Option<BigInt> {
    let result = left + right;
    (result.bits() <= ERTS_MAX_BIGNUM_BITS).then_some(result)
}
fn finite_multiply(left: &BigInt, right: &BigInt) -> Option<BigInt> {
    if left.is_zero() || right.is_zero() {
        return Some(BigInt::zero());
    }
    let bits = left.bits() + right.bits();
    if bits - 1 > ERTS_MAX_BIGNUM_BITS {
        return None;
    }
    if bits > ERTS_MAX_BIGNUM_BITS {
        let maximum = (BigInt::from(1u8) << ERTS_MAX_BIGNUM_BITS as usize) - 1;
        if right.abs() > maximum / left.abs() {
            return None;
        }
    }
    Some(left * right)
}
fn finite_shift(value: &BigInt, count: &BigInt) -> Option<BigInt> {
    if value.is_zero() {
        return Some(BigInt::zero());
    }
    if count.is_negative() {
        let magnitude = -count;
        if magnitude >= BigInt::from(value.bits()) {
            return Some(BigInt::from(if value.is_negative() { -1 } else { 0 }));
        }
        return Some(
            value
                >> magnitude
                    .to_usize()
                    .expect("shift smaller than native bignum"),
        );
    }
    if count > &BigInt::from(ERTS_MAX_BIGNUM_BITS.saturating_sub(value.bits())) {
        return None;
    }
    Some(value << count.to_usize().expect("shift within native bignum"))
}
pub fn binary(op: &str, a: &Type, b: &Type) -> Result<Type, NativeTransferError> {
    if op == "/" {
        return Ok(Type::float());
    }
    if integer_domain(a)?.is_none() || integer_domain(b)?.is_none() {
        return Ok(
            if a.is_subtype(&Type::float())
                .map_err(NativeTransferError::from)?
                || b.is_subtype(&Type::float())
                    .map_err(NativeTransferError::from)?
            {
                Type::float()
            } else {
                Type::number()
            },
        );
    }
    if let (Some(left), Some(right)) = (values(a)?, values(b)?) {
        let mut output = Vec::new();
        for x in &left {
            for y in &right {
                if matches!(op, "div" | "rem") && y.is_zero() {
                    continue;
                }
                let value = match op {
                    "+" | "-" | "*" => {
                        let value = match op {
                            "+" => finite_add(x, y),
                            "-" => finite_add(x, &-y),
                            "*" => finite_multiply(x, y),
                            _ => unreachable!(),
                        };
                        let Some(value) = value else {
                            return Ok(Type::number());
                        };
                        value
                    }
                    "div" => x / y,
                    "rem" => x % y,
                    "band" => x & y,
                    "bor" => x | y,
                    "bxor" => x ^ y,
                    "bsl" | "bsr" => {
                        let shift = if op == "bsr" { -y } else { y.clone() };
                        let Some(value) = finite_shift(x, &shift) else {
                            return Ok(Type::all_integers());
                        };
                        value
                    }
                    _ => return Err(format!("unsupported arithmetic {op}").into()),
                };
                output.push(value)
            }
        }
        return Ok(Type::integers(output));
    }
    range_operation(op, a, b)
}
pub fn unary(op: &str, a: &Type) -> Result<Type, NativeTransferError> {
    if op == "+" {
        return Ok(a.clone());
    }
    if op == "-" {
        return if integer_domain(a)?.is_some() {
            binary("-", &Type::integer(0.into()), a)
        } else {
            Ok(a.clone())
        };
    }
    if op == "abs" {
        if let Some((lo, hi)) = bounds(a)? {
            let (from, to) = if lo >= Bound::finite(0) {
                (lo, hi)
            } else if hi >= Bound::finite(0) {
                (Bound::finite(0), lo.inverse().max(hi))
            } else {
                (hi.inverse(), lo.inverse())
            };
            return range(from, to);
        }
        return Ok(
            if a.is_subtype(&Type::float())
                .map_err(NativeTransferError::from)?
            {
                Type::float()
            } else {
                Type::number()
            },
        );
    }
    if op == "bnot" {
        let (lo, hi) = bounds(a)?.ok_or(NativeTransferError::FunctionClause)?;
        return range(
            hi.inverse().add(&Bound::finite(-1)),
            lo.inverse().add(&Bound::finite(-1)),
        );
    }
    Err(format!("unsupported unary operator {op}").into())
}

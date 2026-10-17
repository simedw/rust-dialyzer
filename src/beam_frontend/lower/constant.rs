//! OTP erl_eval:partial_eval's constant-expression subset, without executing code.
use super::{ErlangTerm, Lower, Result, TermId};
use num_bigint::{BigInt, Sign};
use num_traits::{ToPrimitive, Zero};
use std::cmp::Ordering;

const MAX_CONSTANT_BITS: u64 = 2_000_000;
impl Lower {
    /// Preserve the original annotation and original AST on nonconstant or
    /// invalid arithmetic, exactly as erl_eval:partial_eval/1 does.
    pub(super) fn partial_eval(&mut self, node: TermId) -> Result<TermId> {
        let fields = self.a.tuple_fields(node)?.to_vec();
        if fields.len() < 2 {
            return Ok(node);
        }
        let Some(value) = self.eval_constant(node, 0)? else {
            return Ok(node);
        };
        let tag = match self.a.get(value) {
            ErlangTerm::Integer(_) => "integer",
            ErlangTerm::Float(_) => "float",
            ErlangTerm::Atom(_) => "atom",
            _ => return Ok(node),
        };
        if self.a.tag(node) == Some(tag) {
            return Ok(node);
        }
        Ok(self.a.tagged(tag, vec![fields[1], value]))
    }
    pub(super) fn constant_pattern(&mut self, node: TermId) -> Result<Option<TermId>> {
        let fields = self.a.tuple_fields(node)?.to_vec();
        if self.a.tag(node) != Some("op") {
            return Ok(None);
        }
        if fields.len() == 5 && self.a.as_atom(fields[2]) == Some("++") {
            let left = self.a.tuple_fields(fields[3])?.to_vec();
            match self.a.tag(fields[3]) {
                Some("nil") => return Ok(Some(fields[4])),
                Some("cons") if left.len() == 4 => {
                    let tail = self
                        .a
                        .tagged("op", vec![left[1], fields[2], left[3], fields[4]]);
                    return Ok(Some(self.a.tagged("cons", vec![left[1], left[2], tail])));
                }
                Some("string") if left.len() == 3 => {
                    let mut tail = fields[4];
                    for value in self.a.list_items(left[2])?.into_iter().rev() {
                        let head = self.a.tagged("char", vec![left[1], value]);
                        tail = self.a.tagged("cons", vec![left[1], head, tail]);
                    }
                    return Ok(Some(tail));
                }
                _ => {}
            }
        }
        let folded = self.partial_eval(node)?;
        Ok((folded != node).then_some(folded))
    }
    fn eval_constant(&mut self, node: TermId, depth: usize) -> Result<Option<TermId>> {
        if depth > 512 {
            return Err("native constant-expression depth limit".into());
        }
        let fields = self.a.tuple_fields(node)?.to_vec();
        let result = match self.a.tag(node) {
            Some("integer" | "char" | "float" | "atom") if fields.len() == 3 => Some(fields[2]),
            Some("nil") if fields.len() == 2 => Some(self.a.nil()),
            Some("tuple") if fields.len() == 3 => {
                let mut items = Vec::new();
                for item in self.a.list_items(fields[2])? {
                    let Some(value) = self.eval_constant(item, depth + 1)? else {
                        return Ok(None);
                    };
                    items.push(value);
                }
                Some(self.a.tuple(items))
            }
            Some("cons") if fields.len() == 4 => {
                let Some(head) = self.eval_constant(fields[2], depth + 1)? else {
                    return Ok(None);
                };
                let Some(tail) = self.eval_constant(fields[3], depth + 1)? else {
                    return Ok(None);
                };
                Some(self.a.push(ErlangTerm::List {
                    items: vec![head],
                    tail,
                }))
            }
            Some("op") if fields.len() == 4 || fields.len() == 5 => {
                let op = self
                    .a
                    .as_atom(fields[2])
                    .ok_or("constant operator name expected")?
                    .to_owned();
                let Some(left) = self.eval_constant(fields[3], depth + 1)? else {
                    return Ok(None);
                };
                if fields.len() == 4 {
                    self.constant_unary(&op, left)?
                } else {
                    let Some(right) = self.eval_constant(fields[4], depth + 1)? else {
                        return Ok(None);
                    };
                    self.constant_operator_binary(&op, left, right)?
                }
            }
            _ => None,
        };
        Ok(result)
    }
    fn constant_integer(&self, value: TermId) -> Option<BigInt> {
        match self.a.get(value) {
            ErlangTerm::Integer(n) => n.parse().ok(),
            _ => None,
        }
    }
    fn constant_float(&self, value: TermId) -> Option<f64> {
        match self.a.get(value) {
            ErlangTerm::Float(bits) => Some(f64::from_bits(*bits)),
            ErlangTerm::Integer(n) => n.parse::<BigInt>().ok()?.to_f64().filter(|n| n.is_finite()),
            _ => None,
        }
    }
    fn constant_bool(&self, value: TermId) -> Option<bool> {
        match self.a.as_atom(value) {
            Some("true") => Some(true),
            Some("false") => Some(false),
            _ => None,
        }
    }
    fn constant_boolean(&mut self, value: bool) -> TermId {
        self.a.atom(if value { "true" } else { "false" })
    }
    fn finite_float(&mut self, value: f64) -> Option<TermId> {
        value
            .is_finite()
            .then(|| self.a.push(ErlangTerm::Float(value.to_bits())))
    }
    fn bounded_integer(&mut self, value: BigInt) -> Result<Option<TermId>> {
        if value.bits() > MAX_CONSTANT_BITS {
            return Err("native constant-expression integer size limit".into());
        }
        Ok(Some(self.a.integer(value)))
    }
    fn constant_unary(&mut self, op: &str, value: TermId) -> Result<Option<TermId>> {
        match op {
            "+" if matches!(
                self.a.get(value),
                ErlangTerm::Integer(_) | ErlangTerm::Float(_)
            ) =>
            {
                Ok(Some(value))
            }
            "-" => {
                if let Some(n) = self.constant_integer(value) {
                    self.bounded_integer(-n)
                } else if let Some(n) = self.constant_float(value) {
                    Ok(self.finite_float(-n))
                } else {
                    Ok(None)
                }
            }
            "bnot" => {
                if let Some(n) = self.constant_integer(value) {
                    self.bounded_integer(!n)
                } else {
                    Ok(None)
                }
            }
            "not" => Ok(self.constant_bool(value).map(|n| self.constant_boolean(!n))),
            _ => Ok(None),
        }
    }
    fn constant_compare(&mut self, a: TermId, b: TermId, exact: bool) -> Result<Ordering> {
        let arena = self.a.snapshot(a);
        let order = crate::term_order::TermOrderArena::reachable(&arena, &[a, b])?;
        if exact {
            Ok(order.compare_map_keys(a, &order, b)?)
        } else {
            Ok(order.compare(a, &order, b)?)
        }
    }
    fn constant_operator_binary(
        &mut self,
        op: &str,
        left: TermId,
        right: TermId,
    ) -> Result<Option<TermId>> {
        if matches!(op, "==" | "/=" | "=:=" | "=/=" | "<" | ">" | "=<" | ">=") {
            let order = self.constant_compare(left, right, matches!(op, "=:=" | "=/="))?;
            let value = match op {
                "==" | "=:=" => order.is_eq(),
                "/=" | "=/=" => !order.is_eq(),
                "<" => order.is_lt(),
                ">" => order.is_gt(),
                "=<" => !order.is_gt(),
                ">=" => !order.is_lt(),
                _ => unreachable!(),
            };
            return Ok(Some(self.constant_boolean(value)));
        }
        if matches!(op, "and" | "or" | "xor") {
            let (Some(a), Some(b)) = (self.constant_bool(left), self.constant_bool(right)) else {
                return Ok(None);
            };
            return Ok(Some(self.constant_boolean(match op {
                "and" => a && b,
                "or" => a || b,
                _ => a ^ b,
            })));
        }
        if matches!(op, "++" | "--") {
            let Ok(mut a) = self.a.list_items(left) else {
                return Ok(None);
            };
            if op == "++" {
                return Ok(Some(if a.is_empty() {
                    right
                } else {
                    self.a.push(ErlangTerm::List {
                        items: a,
                        tail: right,
                    })
                }));
            }
            let Ok(b) = self.a.list_items(right) else {
                return Ok(None);
            };
            for removed in b {
                let mut index = None;
                for (i, &item) in a.iter().enumerate() {
                    if self.constant_compare(item, removed, true)?.is_eq() {
                        index = Some(i);
                        break;
                    }
                }
                if let Some(i) = index {
                    a.remove(i);
                }
            }
            return Ok(Some(self.a.list(a)));
        }
        let integers = self
            .constant_integer(left)
            .zip(self.constant_integer(right));
        if let Some((a, b)) = integers {
            let value = match op {
                "+" => Some(a + b),
                "-" => Some(a - b),
                "*" => Some(a * b),
                "div" if !b.is_zero() => Some(a / b),
                "rem" if !b.is_zero() => Some(a % b),
                "band" => Some(a & b),
                "bor" => Some(a | b),
                "bxor" => Some(a ^ b),
                "bsl" | "bsr" => {
                    let leftward = (op == "bsl") == (b.sign() != Sign::Minus);
                    let shift = if b.sign() == Sign::Minus { -b } else { b };
                    let shift = shift.to_u64().unwrap_or(u64::MAX);
                    if leftward {
                        if shift > MAX_CONSTANT_BITS
                            || a.bits().saturating_add(shift) > MAX_CONSTANT_BITS
                        {
                            if a.is_zero() {
                                Some(a)
                            } else {
                                return Err("native constant-expression shift size limit".into());
                            }
                        } else {
                            Some(a << shift as usize)
                        }
                    } else if shift > a.bits() {
                        Some(if a.sign() == Sign::Minus {
                            BigInt::from(-1)
                        } else {
                            BigInt::zero()
                        })
                    } else {
                        Some(a >> shift as usize)
                    }
                }
                _ => None,
            };
            if let Some(value) = value {
                return self.bounded_integer(value);
            }
            if op != "/" {
                return Ok(None);
            }
        }
        if matches!(op, "+" | "-" | "*" | "/") {
            let (Some(a), Some(b)) = (self.constant_float(left), self.constant_float(right)) else {
                return Ok(None);
            };
            if op == "/" && b == 0.0 {
                return Ok(None);
            }
            return Ok(self.finite_float(match op {
                "+" => a + b,
                "-" => a - b,
                "*" => a * b,
                _ => a / b,
            }));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        beam_frontend,
        external_term::{self, Limits},
    };
    #[test]
    fn numeric_operator_patterns_and_constant_map_keys_match_otp_core() {
        let metadata = beam_frontend::debug::read_beam(
            include_bytes!(
                "../../../tests/fixtures/beam_core_port/constants/native_constants.beam"
            ),
            Limits::default(),
        )
        .unwrap();
        let forms = beam_frontend::forms(&metadata).unwrap();
        let raw = beam_frontend::raw_core(&metadata, &forms).unwrap();
        let clean = beam_frontend::clean::clean(&raw).unwrap();
        let expected_raw = external_term::decode(
            include_bytes!(
                "../../../tests/fixtures/beam_core_port/constants/native_constants.raw_core.etf"
            ),
            Limits::default(),
        )
        .unwrap();
        let expected_clean = external_term::decode(
            include_bytes!(
                "../../../tests/fixtures/beam_core_port/constants/native_constants.clean_core.etf"
            ),
            Limits::default(),
        )
        .unwrap();
        assert_eq!(
            external_term::encode(&raw, Limits::default(), false).unwrap(),
            external_term::encode(&expected_raw, Limits::default(), false).unwrap()
        );
        assert_eq!(
            external_term::encode(&clean, Limits::default(), false).unwrap(),
            external_term::encode(&expected_clean, Limits::default(), false).unwrap()
        );
    }
}

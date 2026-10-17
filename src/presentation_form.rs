use crate::core::{ErlangTerm, TermId};
use crate::lattice::{BuiltinType, LatticeError, PresentationContext, Type};
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};

pub fn form_to_string(terms: &crate::shared_vec::SharedVec<ErlangTerm>, root: TermId) -> Result<String, LatticeError> {
    FormPrinter {
        terms,
        context: PresentationContext::default(),
    }
    .print(root)
}
pub(crate) fn native_float(value: f64) -> String {
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0".into()
        } else {
            "0.0".into()
        };
    }
    let sign = if value.is_sign_negative() { "-" } else { "" };
    let shortest = ryu::Buffer::new().format_finite(value.abs()).to_owned();
    let (mantissa, power) = match shortest.split_once('e') {
        Some((m, e)) => (m, e.parse::<i32>().unwrap()),
        None => (shortest.as_str(), 0),
    };
    let integer_digits = mantissa.find('.').unwrap_or(mantissa.len()) as i32;
    let mut digits = mantissa.replace('.', "");
    let leading = digits.bytes().take_while(|c| *c == b'0').count();
    digits.drain(..leading);
    let exponent = power + integer_digits - leading as i32 - 1;
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
    }
    let fixed = if exponent < 0 {
        format!("0.{}{}", "0".repeat((-exponent - 1) as usize), digits)
    } else if exponent as usize >= digits.len() - 1 {
        format!(
            "{}{}.0",
            digits,
            "0".repeat(exponent as usize + 1 - digits.len())
        )
    } else {
        let split = exponent as usize + 1;
        format!("{}.{}", &digits[..split], &digits[split..])
    };
    let scientific = format!(
        "{}.{}e{}",
        &digits[..1],
        if digits.len() > 1 { &digits[1..] } else { "0" },
        exponent
    );
    format!(
        "{sign}{}",
        if exponent < 16 && fixed.len() <= scientific.len() {
            fixed
        } else {
            scientific
        }
    )
}
struct FormPrinter<'a> {
    terms: &'a crate::shared_vec::SharedVec<ErlangTerm>,
    context: PresentationContext,
}
impl FormPrinter<'_> {
    fn term(&self, id: TermId) -> Result<&ErlangTerm, LatticeError> {
        self.terms.get(id.0).ok_or(LatticeError::BadArg)
    }
    fn tuple(&self, id: TermId) -> Result<&[TermId], LatticeError> {
        match self.term(id)? {
            ErlangTerm::Tuple(t) => Ok(t),
            _ => Err(LatticeError::FunctionClause),
        }
    }
    fn atom(&self, id: TermId) -> Result<&str, LatticeError> {
        match self.term(id)? {
            ErlangTerm::Atom(t) => Ok(t),
            _ => Err(LatticeError::FunctionClause),
        }
    }
    fn list(&self, id: TermId) -> Result<&[TermId], LatticeError> {
        match self.term(id)? {
            ErlangTerm::Nil => Ok(&[]),
            ErlangTerm::List { items, tail } if matches!(self.term(*tail)?, ErlangTerm::Nil) => {
                Ok(items)
            }
            _ => Err(LatticeError::FunctionClause),
        }
    }
    fn atom_form(&self, id: TermId) -> Result<&str, LatticeError> {
        let t = self.tuple(id)?;
        if t.len() != 3 || self.atom(t[0])? != "atom" {
            return Err(LatticeError::FunctionClause);
        }
        self.atom(t[2])
    }
    fn sequence(&self, items: &[TermId], separator: &str) -> Result<String, LatticeError> {
        items
            .iter()
            .map(|id| self.print(*id))
            .collect::<Result<Vec<_>, _>>()
            .map(|v| v.join(separator))
    }
    fn constant(&self, id: TermId) -> Option<BigInt> {
        let t = self.tuple(id).ok()?;
        let tag = self.atom(t.first().copied()?).ok()?;
        match (tag, t.len()) {
            ("integer" | "char", 3) => match self.term(t[2]).ok()? {
                ErlangTerm::Integer(n) => n.parse().ok(),
                _ => None,
            },
            ("op", 4) => {
                let a = self.constant(t[3])?;
                match self.atom(t[2]).ok()? {
                    "+" => Some(a),
                    "-" => Some(-a),
                    "bnot" => Some(!a),
                    _ => None,
                }
            }
            ("op", 5) => {
                let a = self.constant(t[3])?;
                let b = self.constant(t[4])?;
                Some(match self.atom(t[2]).ok()? {
                    "+" => a + b,
                    "-" => a - b,
                    "*" => a * b,
                    "div" if !b.is_zero() => a / b,
                    "rem" if !b.is_zero() => a % b,
                    "band" => a & b,
                    "bor" => a | b,
                    "bxor" => a ^ b,
                    "bsl" | "bsr" => {
                        let shift = b.to_i64()?;
                        let left = (self.atom(t[2]).ok()? == "bsl") == (shift >= 0);
                        let amount: usize = shift.unsigned_abs().try_into().ok()?;
                        if a.is_zero() {
                            a
                        } else if left {
                            a << amount
                        } else {
                            a >> amount
                        }
                    }
                    _ => return None,
                })
            }
            _ => None,
        }
    }
    fn print(&self, id: TermId) -> Result<String, LatticeError> {
        let t = self.tuple(id)?;
        if t.len() < 3 {
            return Err(LatticeError::FunctionClause);
        }
        let tag = self.atom(t[0])?;
        Ok(match (tag, t.len()) {
            ("var", 3) => match self.term(t[2])? {
                ErlangTerm::Atom(name) => name.clone(),
                _ => return Err(LatticeError::BadArg),
            },
            ("atom", 3) => self.context.quoted_atom(self.atom(t[2])?),
            ("integer" | "char", 3) => match self.term(t[2])? {
                ErlangTerm::Integer(n) => n.clone(),
                _ => return Err(LatticeError::FunctionClause),
            },
            ("op", 4 | 5) => match self.constant(id) {
                Some(n) => n.to_string(),
                None => format!("Badly formed type {}", self.term_string(id)?),
            },
            ("ann_type", 3) => {
                let a = self.list(t[2])?;
                if a.len() != 2 {
                    return Err(LatticeError::FunctionClause);
                }
                format!("{}::{}", self.print(a[0])?, self.print(a[1])?)
            }
            ("paren_type", 3) => {
                let a = self.list(t[2])?;
                if a.len() != 1 {
                    return Err(LatticeError::FunctionClause);
                }
                format!("({})", self.print(a[0])?)
            }
            ("remote_type", 3) => {
                let a = self.list(t[2])?;
                if a.len() != 3 {
                    return Err(LatticeError::FunctionClause);
                }
                format!(
                    "{}({})",
                    self.context
                        .qualified_name(self.atom_form(a[0])?, self.atom_form(a[1])?),
                    self.sequence(self.list(a[2])?, ",")?
                )
            }
            ("type", 3) if self.atom(t[2])? == "any" => return Err(LatticeError::FunctionClause),
            ("type" | "user_type", 4) => {
                let name = self.atom(t[2])?;
                if tag == "type"
                    && matches!(name, "map" | "tuple")
                    && matches!(self.term(t[3])?,ErlangTerm::Atom(a) if a=="any")
                {
                    format!("{name}()")
                } else {
                    let a = self.list(t[3])?;
                    if tag == "user_type" {
                        format!(
                            "{}({})",
                            self.context.atom_to_string(name),
                            self.sequence(a, ",")?
                        )
                    } else {
                        self.builtin_form(id, name, a)?
                    }
                }
            }
            _ => return Err(LatticeError::FunctionClause),
        })
    }
    fn builtin_form(&self, id: TermId, name: &str, a: &[TermId]) -> Result<String, LatticeError> {
        Ok(match (name, a.len()) {
            (
                "arity" | "binary" | "bitstring" | "dynamic" | "fun" | "iodata" | "iolist" | "mfa"
                | "module" | "node" | "nonempty_binary" | "nonempty_bitstring" | "nonempty_string"
                | "term" | "timeout",
                0,
            ) => format!("{name}()"),
            ("binary", 2) => match (self.constant(a[0]), self.constant(a[1])) {
                (Some(b), Some(u)) => match (u.to_string().as_str(), b.to_string().as_str()) {
                    ("0", "0") => "<<>>".into(),
                    ("8", "0") => "binary()".into(),
                    ("1", "0") => "bitstring()".into(),
                    ("0", _) => format!("<<_:{b}>>"),
                    (_, "0") => format!("<<_:_*{u}>>"),
                    _ => format!("<<_:{b},_:_*{u}>>"),
                },
                _ => format!("Badly formed bitstr type {}", self.term_string(id)?),
            },
            ("fun", 2) => {
                let domain = self.tuple(a[0])?;
                if domain.len() == 3
                    && self.atom(domain[0])? == "type"
                    && self.atom(domain[2])? == "any"
                {
                    format!("fun(...) -> {}", self.print(a[1])?)
                } else if domain.len() == 4
                    && self.atom(domain[0])? == "type"
                    && self.atom(domain[2])? == "product"
                {
                    format!(
                        "fun(({}) -> {})",
                        self.sequence(self.list(domain[3])?, ",")?,
                        self.print(a[1])?
                    )
                } else {
                    self.named_fallback(name, a)?
                }
            }
            ("list", 1) => format!("[{}]", self.print(a[0])?),
            ("nonempty_list", 1) => format!("[{},...]", self.print(a[0])?),
            ("map", _) => format!("#{{{}}}", self.sequence(a, ",")?),
            ("map_field_assoc" | "map_field_exact", 2) => format!(
                "{}{}{}",
                self.print(a[0])?,
                if name == "map_field_assoc" {
                    "=>"
                } else {
                    ":="
                },
                self.print(a[1])?
            ),
            ("product", _) => format!("<{}>", self.sequence(a, ",")?),
            ("range", 2) => match (self.constant(a[0]), self.constant(a[1])) {
                (Some(lo), Some(hi)) => format!("{lo}..{hi}"),
                _ => format!("Badly formed type {}", self.term_string(id)?),
            },
            ("record", n) if n >= 1 => format!(
                "#{}{{{}}}",
                self.context.atom_to_string(self.atom_form(a[0])?),
                self.sequence(&a[1..], ",")?
            ),
            ("field_type", 2) => format!(
                "{}::{}",
                self.context.atom_to_string(self.atom_form(a[0])?),
                self.print(a[1])?
            ),
            ("tuple", _) => format!("{{{}}}", self.sequence(a, ",")?),
            ("union", _) => a
                .iter()
                .map(|id| {
                    let t = self.tuple(*id)?;
                    let printed = self.print(*id)?;
                    Ok(
                        if t.first()
                            .is_some_and(|tag| self.atom(*tag).ok() == Some("ann_type"))
                        {
                            format!("({printed})")
                        } else {
                            printed
                        },
                    )
                })
                .collect::<Result<Vec<_>, LatticeError>>()?
                .join(" | "),
            (_, 0) => {
                use BuiltinType::*;
                let builtin = match name {
                    "any" => Some(Any),
                    "atom" => Some(Atom),
                    "bool" | "boolean" => Some(Boolean),
                    "byte" => Some(Byte),
                    "char" => Some(Char),
                    "float" => Some(Float),
                    "function" => Some(Function),
                    "identifier" => Some(Identifier),
                    "integer" => Some(Integer),
                    "list" => Some(List),
                    "nil" => Some(Nil),
                    "neg_integer" => Some(NegativeInteger),
                    "non_neg_integer" => Some(NonNegativeInteger),
                    "no_return" => Some(Unit),
                    "none" => Some(None),
                    "nonempty_list" => Some(NonemptyList),
                    "nonempty_maybe_improper_list" => {
                        return PresentationContext::default().type_to_string(&Type::list(
                            Type::any(),
                            Type::any(),
                            true,
                        ));
                    }
                    "number" => Some(Number),
                    "pid" => Some(Pid),
                    "port" => Some(Port),
                    "pos_integer" => Some(PositiveInteger),
                    "maybe_improper_list" => Some(MaybeImproperList),
                    "reference" => Some(Reference),
                    "string" => Some(String),
                    _ => std::option::Option::None,
                };
                if let Some(kind) = builtin {
                    self.context.type_to_string(&Type::builtin(kind)?)?
                } else {
                    return Err(LatticeError::FunctionClause);
                }
            }
            _ => self.named_fallback(name, a)?,
        })
    }
    fn named_fallback(&self, name: &str, a: &[TermId]) -> Result<String, LatticeError> {
        Ok(format!(
            "{}({})",
            self.context.atom_to_string(name),
            self.sequence(a, ",")?
        ))
    }
    fn term_string(&self, id: TermId) -> Result<String, LatticeError> {
        Ok(match self.term(id)? {
            ErlangTerm::ExternalFun(external_fun_payload) => {
                let crate::core::ExternalFun {
                    module,
                    function,
                    arity,
                } = external_fun_payload.as_ref();
                format!(
                    "fun {}:{}/{}",
                    self.context.atom_latin1(module),
                    self.context.atom_latin1(function),
                    arity
                )
            }
            ErlangTerm::Atom(a) => self.context.atom_latin1(a),
            #[cfg(otp_domain = "29")]
            ErlangTerm::Record(record) => format!("#{}:{}{{{}}}", self.context.atom_latin1(&record.module), self.context.atom_latin1(&record.name),
                record.fields.iter().map(|(name, value)| Ok(format!("{} = {}", self.context.atom_latin1(name), self.term_string(*value)?))).collect::<Result<Vec<_>, LatticeError>>()?.join(",")),
            ErlangTerm::Integer(n) => n.clone(),
            ErlangTerm::Float(bits) => {
                let f = f64::from_bits(*bits);
                native_float(f)
            }
            ErlangTerm::Tuple(t) => format!(
                "{{{}}}",
                t.iter()
                    .map(|id| self.term_string(*id))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(",")
            ),
            ErlangTerm::Nil => "[]".into(),
            ErlangTerm::List { items, tail } => {
                let mut s = items
                    .iter()
                    .map(|id| self.term_string(*id))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(",");
                if !matches!(self.term(*tail)?, ErlangTerm::Nil) {
                    s.push('|');
                    s.push_str(&self.term_string(*tail)?);
                }
                format!("[{s}]")
            }
            ErlangTerm::Binary(b) => format!(
                "<<{}>>",
                b.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            ErlangTerm::Bitstring { bytes, bits } => {
                let complete = (*bits / 8) as usize;
                let mut parts = bytes[..complete]
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>();
                let remainder = bits % 8;
                if remainder != 0 {
                    parts.push(format!(
                        "{}:{}",
                        bytes[complete] >> (8 - remainder),
                        remainder
                    ));
                }
                format!("<<{}>>", parts.join(","))
            }
            ErlangTerm::Map(entries) => format!(
                "#{{{}}}",
                entries
                    .iter()
                    .map(|(k, v)| Ok(format!(
                        "{}=>{}",
                        self.term_string(*k)?,
                        self.term_string(*v)?
                    )))
                    .collect::<Result<Vec<_>, LatticeError>>()?
                    .join(",")
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn otp_shortest_float_uses_even_rounding_and_notation_policy() {
        assert_eq!(
            native_float(f64::from_bits(0x4319BC961DC0F995)),
            "1811056837017189.2"
        );
        assert_eq!(native_float(1000.0), "1.0e3");
        assert_eq!(native_float(100.0), "100.0");
        assert_eq!(native_float(-0.0), "-0.0");
    }
}

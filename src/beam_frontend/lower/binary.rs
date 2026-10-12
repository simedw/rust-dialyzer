//! OTP 28.3 erl_bits normalization and v3_core binary expression/pattern lowering.
//! Source: Ericsson/OTP erl_bits.erl and v3_core.erl (Apache-2.0).
use super::*;
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};

#[derive(Clone)]
struct Segment {
    line: TermId,
    value: TermId,
    size: TermId,
    kind: String,
    unit: Option<i64>,
    sign: String,
    endian: String,
    index: usize,
}
impl Lower {
    fn normalize_segment(&mut self, node: TermId, index: usize, matching: bool) -> Result<Segment> {
        let f = self.a.tuple_fields(node)?.to_vec();
        if self.a.tag(node) != Some("bin_element") || f.len() != 5 {
            return Err(self.err("malformed binary segment"));
        }
        let mut kind = None;
        let mut unit = None;
        let mut sign = None;
        let mut endian = None;
        if self.a.as_atom(f[4]) != Some("default") {
            for option in self.a.list_items(f[4])? {
                match self.a.as_atom(option) {
                    Some(k @ ("integer" | "float" | "binary" | "utf8" | "utf16" | "utf32")) => {
                        merge(&mut kind, k.to_owned())?
                    }
                    Some("bytes") => {
                        merge(&mut kind, "binary".into())?;
                        merge(&mut unit, 8)?;
                    }
                    Some("bitstring" | "bits") => {
                        merge(&mut kind, "binary".into())?;
                        merge(&mut unit, 1)?;
                    }
                    Some(k @ ("signed" | "unsigned")) => merge(&mut sign, k.to_owned())?,
                    Some(k @ ("big" | "little" | "native")) => merge(&mut endian, k.to_owned())?,
                    _ if self.a.tag(option) == Some("unit") => {
                        let fields = self.a.tuple_fields(option)?;
                        if self.a.as_atom(fields[1]) == Some("undefined") {
                            continue;
                        }
                        let n = self.a.as_integer(fields[1]).ok_or("lower: binary unit")?;
                        if !(1..=256).contains(&n) {
                            return Err(self.err("binary unit out of range"));
                        }
                        merge(&mut unit, n)?;
                    }
                    _ => return Err(self.err("unknown binary type option")),
                }
            }
        }
        let kind = kind.unwrap_or_else(|| "integer".into());
        let utf = kind.starts_with("utf");
        let default = self.a.as_atom(f[3]) == Some("default");
        if default && matches!(kind.as_str(), "integer" | "float") && unit.is_some() {
            return Err(self.err("binary unit without explicit size"));
        }
        let size = if default {
            if kind == "binary" {
                let generated = self.a.atom("generated");
                let yes = self.a.atom("true");
                let g = self.a.tuple(vec![generated, yes]);
                let mut annotations = vec![g];
                match self.a.get(f[1]) {
                    ErlangTerm::List { .. } => {
                        for annotation in self.a.list_items(f[1])? {
                            if self.a.tag(annotation) != Some("generated") {
                                annotations.push(annotation);
                            }
                        }
                    }
                    _ => {
                        let location = self.a.atom("location");
                        let l = self.a.tuple(vec![location, f[1]]);
                        annotations.push(l);
                    }
                }
                let line = self.a.list(annotations);
                let all = self.a.atom("all");
                self.a.tagged("atom", vec![line, all])
            } else if utf {
                let undefined = self.a.atom("undefined");
                self.a.tagged("atom", vec![f[1], undefined])
            } else {
                let n = self.a.integer(if kind == "float" { 64 } else { 8 });
                self.a.tagged("integer", vec![f[1], n])
            }
        } else {
            let mut size = self.a.tuple_fields(f[3])?.to_vec();
            if self.a.tag(f[3]) == Some("char") {
                return Ok(Segment {
                    line: f[1],
                    value: f[2],
                    size: self.a.tagged("integer", vec![size[1], size[2]]),
                    kind,
                    unit: Some(unit.unwrap_or(1)),
                    sign: sign.unwrap_or_else(|| "unsigned".into()),
                    endian: endian.unwrap_or_else(|| "big".into()),
                    index,
                });
            }
            if self.a.tag(f[3]) == Some("atom") && self.a.as_atom(size[2]) == Some("all") {
                let generated = self
                    .a
                    .keyword(size[1], "generated")
                    .is_some_and(|id| self.a.as_atom(id) == Some("true"));
                if !generated {
                    if matching {
                        return Err(self.err("binary pattern has explicit all size"));
                    }
                    size[2] = self.a.atom("bad_size");
                    self.a.tuple(size)
                } else {
                    f[3]
                }
            } else {
                f[3]
            }
        };
        if utf {
            let sf = self.a.tuple_fields(size)?;
            if unit.is_some()
                || self.a.tag(size) != Some("atom")
                || self.a.as_atom(sf[2]) != Some("undefined")
            {
                return Err(self.err("UTF binary size or unit"));
            }
        } else if unit.is_none() {
            unit = Some(if kind == "binary" { 8 } else { 1 });
        }
        Ok(Segment {
            line: f[1],
            value: f[2],
            size,
            kind,
            unit,
            sign: sign.unwrap_or_else(|| "unsigned".into()),
            endian: endian.unwrap_or_else(|| "big".into()),
            index,
        })
    }
    fn expand_segments(&mut self, input: TermId, matching: bool) -> Result<Vec<Segment>> {
        let mut output = Vec::new();
        for (index, node) in self.a.list_items(input)?.into_iter().enumerate() {
            let s = self.normalize_segment(node, index + 1, matching)?;
            if self.a.tag(s.value) != Some("string") {
                output.push(s);
                continue;
            }
            let vf = self.a.tuple_fields(s.value)?.to_vec();
            let chars = self.a.list_items(vf[2])?;
            if !matching
                && self.a.tag(s.size) == Some("integer")
                && self.a.as_integer(self.a.tuple_fields(s.size)?[2]) == Some(8)
            {
                let mut value = BigInt::zero();
                let mut count = 0;
                for ch in chars {
                    value = (value << 8) | self.term_integer(ch)?;
                    count += 8;
                    if count == 1024 {
                        output.push(self.combined_segment(&s, &value, count));
                        value = BigInt::zero();
                        count = 0;
                    }
                }
                output.push(self.combined_segment(&s, &value, count));
            } else if chars.is_empty() {
                // Empty strings still validate their size. Constant valid sizes
                // disappear; dynamic sizes require the source-level test.
                let sf = self.a.tuple_fields(s.size)?;
                if self.a.tag(s.size) == Some("integer")
                    && self.a.as_integer(sf[2]).is_some_and(|n| n >= 0)
                    || self.a.tag(s.size) == Some("atom")
                        && self.a.as_atom(sf[2]) == Some("undefined")
                {
                    continue;
                }
                return Err(self.err("empty binary string dynamic size validation"));
            } else {
                for ch in chars {
                    let mut s = s.clone();
                    s.index = 0;
                    s.value = self.a.tagged("char", vec![s.line, ch]);
                    output.push(s);
                }
            }
        }
        Ok(output)
    }
    fn combined_segment(&mut self, old: &Segment, value: &BigInt, size: i64) -> Segment {
        let value = self.a.integer(value);
        let value = self.a.tagged("integer", vec![old.line, value]);
        let size = self.a.integer(size);
        let size = self.a.tagged("integer", vec![old.line, size]);
        Segment {
            line: old.line,
            value,
            size,
            kind: "integer".into(),
            unit: Some(1),
            sign: "unsigned".into(),
            endian: "big".into(),
            index: 0,
        }
    }
    fn term_integer(&self, node: TermId) -> Result<BigInt> {
        match self.a.get(node) {
            ErlangTerm::Integer(n) => n.parse().map_err(|_| self.err("invalid binary integer")),
            _ => Err(self.err("noninteger binary value")),
        }
    }
    fn bitstr(&mut self, s: &Segment, matching: bool, bound: &mut HashSet<String>) -> Result<Expr> {
        let (value, mut pre) = if matching {
            (self.raw_pattern(s.value)?, vec![])
        } else {
            let (values, pre) = self.safe_list(vec![s.value])?;
            (values[0], pre)
        };
        let value = if matching && s.kind == "float" && self.a.tag(value) == Some("c_literal") {
            let f = self.a.tuple_fields(value)?.to_vec();
            if let Ok(n) = self.term_integer(f[2]) {
                if let Some(n) = n.to_f64().filter(|x| x.is_finite()) {
                    let float = self.a.push(ErlangTerm::Float(n.to_bits()));
                    self.literal(f[1], float)
                } else {
                    value
                }
            } else {
                value
            }
        } else {
            value
        };
        let size = if matching {
            let size = self.partial_eval(s.size)?;
            self.expr(size)?
        } else {
            let (values, pre) = self.safe_list(vec![s.size])?;
            Expr {
                value: values[0],
                pre,
            }
        };
        let size_value = if matching {
            self.wrap(size.pre, size.value)
        } else {
            pre.extend(size.pre);
            size.value
        };
        let mut ann = self.annotation(s.line)?;
        if !matching && s.index != 0 {
            let key = self.a.atom("segment");
            let n = self.a.integer(s.index);
            let marker = self.a.tuple(vec![key, n]);
            let mut annotations = vec![marker];
            annotations.extend(self.a.list_items(ann)?);
            ann = self.a.list(annotations);
        }
        let empty = self.empty();
        let unit = match s.unit {
            Some(n) => self.a.integer(n),
            None => self.a.atom("undefined"),
        };
        let unit = self.literal(empty, unit);
        let kind = self.atom_literal(&s.kind);
        let sign = self.a.atom(&s.sign);
        let endian = self.a.atom(&s.endian);
        let flags = self.a.list(vec![sign, endian]);
        let flags = self.literal(empty, flags);
        let value = self
            .a
            .tagged("c_bitstr", vec![ann, value, size_value, unit, kind, flags]);
        Ok(Expr { value, pre })
    }
    pub(super) fn binary_expr(&mut self, fields: &[TermId], ann: TermId) -> Result<Expr> {
        let segments = self.expand_segments(fields[2], false)?;
        if let Some(value) = self.constant_binary(&segments)? {
            return Ok(Expr {
                value: self.literal(ann, value),
                pre: vec![],
            });
        }
        let mut values = Vec::new();
        let mut pre = Vec::new();
        let mut bound = HashSet::new();
        let mut invalid = false;
        for s in segments.iter().rev() {
            let e = self.bitstr(s, false, &mut bound)?;
            let bitstr = self.a.tuple_fields(e.value)?;
            invalid |= !self.valid_binary_value(&s.kind, bitstr[2])?
                || !self.valid_binary_size(bitstr[3])?;
            values.push(e.value);
            let mut next = e.pre;
            next.extend(pre);
            pre = next;
        }
        values.reverse();
        if invalid {
            let line = self.annotation(fields[1])?;
            let erlang = self.a.atom("erlang");
            let erlang = self.literal(line, erlang);
            let error = self.a.atom("error");
            let error = self.literal(line, error);
            let reason = self.a.atom("badarg");
            let reason = self.literal(line, reason);
            let args = self.a.list(vec![reason]);
            return Ok(Expr {
                value: self.a.tagged("c_call", vec![line, erlang, error, args]),
                pre,
            });
        }
        let values = self.a.list(values);
        Ok(Expr {
            value: self.a.tagged("c_binary", vec![ann, values]),
            pre,
        })
    }
    fn valid_binary_value(&self, kind: &str, node: TermId) -> Result<bool> {
        if self.a.tag(node) == Some("c_var") {
            return Ok(true);
        }
        if self.a.tag(node) != Some("c_literal") {
            return Ok(false);
        }
        let value = self.a.get(self.a.tuple_fields(node)?[2]);
        Ok(match kind {
            "integer" | "utf8" | "utf16" | "utf32" => matches!(value, ErlangTerm::Integer(_)),
            "float" => matches!(value, ErlangTerm::Integer(_) | ErlangTerm::Float(_)),
            "binary" => matches!(value, ErlangTerm::Binary(_) | ErlangTerm::Bitstring { .. }),
            _ => false,
        })
    }
    fn valid_binary_size(&self, node: TermId) -> Result<bool> {
        if self.a.tag(node) == Some("c_var") {
            return Ok(true);
        }
        if self.a.tag(node) != Some("c_literal") {
            return Ok(false);
        }
        let value = self.a.tuple_fields(node)?[2];
        Ok(match self.a.get(value) {
            ErlangTerm::Integer(_) => !self.term_integer(value)?.is_negative(),
            _ => matches!(self.a.as_atom(value), Some("undefined" | "all")),
        })
    }
    pub(super) fn binary_pattern(
        &mut self,
        fields: &[TermId],
        ann: TermId,
        bound: &mut HashSet<String>,
    ) -> Result<TermId> {
        let segments = self.expand_segments(fields[2], true)?;
        let mut values = Vec::new();
        for s in segments {
            let value = self.bitstr(&s, true, bound)?.value;
            self.collect_raw_pattern_names(value, bound)?;
            let mut fields = self.a.tuple_fields(value)?.to_vec();
            if self.a.tag(fields[3]) == Some("c_var") {
                let mut size = self.a.tuple_fields(fields[3])?.to_vec();
                size[1] = self.empty();
                fields[3] = self.a.tuple(size);
            }
            values.push(self.a.tuple(fields));
        }
        let values = self.a.list(values);
        Ok(self.a.tagged("c_binary", vec![ann, values]))
    }
    fn constant_binary(&mut self, segments: &[Segment]) -> Result<Option<TermId>> {
        let mut bits = Vec::new();
        for s in segments {
            if s.endian == "native" {
                return Ok(None);
            }
            let vf = self.a.tuple_fields(s.value)?.to_vec();
            let sf = self.a.tuple_fields(s.size)?.to_vec();
            let vtag = self.a.tag(s.value).unwrap_or("");
            let size = if self.a.tag(s.size) == Some("integer") {
                self.a.as_integer(sf[2])
            } else {
                None
            };
            if s.kind.starts_with("utf") {
                if !matches!(vtag, "integer" | "char") || self.a.as_atom(sf[2]) != Some("undefined")
                {
                    return Ok(None);
                }
                let Some(n) = self.term_integer(vf[2])?.to_u32().and_then(char::from_u32) else {
                    return Ok(None);
                };
                let bytes = match s.kind.as_str() {
                    "utf8" => {
                        let mut bytes = [0; 4];
                        n.encode_utf8(&mut bytes).as_bytes().to_vec()
                    }
                    "utf16" => {
                        let mut words = [0; 2];
                        n.encode_utf16(&mut words)
                            .iter()
                            .flat_map(|w| {
                                if s.endian == "little" {
                                    w.to_le_bytes()
                                } else {
                                    w.to_be_bytes()
                                }
                            })
                            .collect()
                    }
                    "utf32" => {
                        if s.endian == "little" {
                            (n as u32).to_le_bytes().to_vec()
                        } else {
                            (n as u32).to_be_bytes().to_vec()
                        }
                    }
                    _ => unreachable!(),
                };
                append_bytes(&mut bits, &bytes);
                continue;
            }
            let Some(size) = size
                .and_then(|n| n.checked_mul(s.unit.unwrap_or(1)))
                .filter(|n| *n >= 0)
            else {
                return Ok(None);
            };
            if size > 256 {
                if vtag != "integer" {
                    return Ok(None);
                }
                // OTP checks the source field before string expansion; synthetic
                // 1024-bit chunks must not trigger its sparse-integer heuristic.
                if s.index != 0 {
                    let n = self.term_integer(vf[2])?.abs();
                    let estimate = 64 + ((n.bits() + 63) / 64) * 64;
                    if size as u64 > 2 * estimate {
                        return Ok(None);
                    }
                }
            }
            if size > 8_000_000 {
                return Err(self.err("constant binary resource limit"));
            }
            match s.kind.as_str() {
                "integer" if matches!(vtag, "integer" | "char") => {
                    let n = self.term_integer(vf[2])?;
                    if s.endian == "little" {
                        let mut offset = 0;
                        while offset < size {
                            let width = (size - offset).min(8);
                            append_int(&mut bits, &(&n >> offset as usize), width as usize);
                            offset += width;
                        }
                    } else {
                        append_int(&mut bits, &n, size as usize);
                    }
                }
                "float" if matches!(vtag, "integer" | "char" | "float") => {
                    let n = if vtag == "float" {
                        match self.a.get(vf[2]) {
                            ErlangTerm::Float(b) => f64::from_bits(*b),
                            _ => return Ok(None),
                        }
                    } else {
                        let Some(n) = self.term_integer(vf[2])?.to_f64() else {
                            return Ok(None);
                        };
                        n
                    };
                    if !n.is_finite() {
                        return Ok(None);
                    }
                    let bytes = match size {
                        64 => {
                            if s.endian == "little" {
                                n.to_le_bytes().to_vec()
                            } else {
                                n.to_be_bytes().to_vec()
                            }
                        }
                        32 => {
                            if s.endian == "little" {
                                (n as f32).to_le_bytes().to_vec()
                            } else {
                                (n as f32).to_be_bytes().to_vec()
                            }
                        }
                        16 => {
                            if s.endian == "little" {
                                float16(n).to_le_bytes().to_vec()
                            } else {
                                float16(n).to_be_bytes().to_vec()
                            }
                        }
                        _ => return Ok(None),
                    };
                    append_bytes(&mut bits, &bytes);
                }
                _ => return Ok(None),
            }
        }
        let count = bits.len();
        let mut bytes = vec![0; (count + 7) / 8];
        for (i, bit) in bits.into_iter().enumerate() {
            if bit {
                bytes[i / 8] |= 1 << (7 - i % 8);
            }
        }
        Ok(Some(self.a.push(if count % 8 == 0 {
            ErlangTerm::Binary(bytes)
        } else {
            ErlangTerm::Bitstring {
                bytes,
                bits: count as u64,
            }
        })))
    }
}
fn merge<T: PartialEq>(target: &mut Option<T>, value: T) -> Result<()> {
    if target.as_ref().is_some_and(|old| *old != value) {
        return Err("lower: conflicting binary options".into());
    }
    *target = Some(value);
    Ok(())
}
fn append_bytes(bits: &mut Vec<bool>, bytes: &[u8]) {
    for byte in bytes {
        for shift in (0..8).rev() {
            bits.push(byte & (1 << shift) != 0);
        }
    }
}
fn append_int(bits: &mut Vec<bool>, value: &BigInt, width: usize) {
    for shift in (0..width).rev() {
        bits.push(((value >> shift) & BigInt::from(1)) != BigInt::zero());
    }
}

fn float16(value: f64) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 48) & 0x8000) as u16;
    let exponent = ((bits >> 52) & 0x7ff) as i32 - 1023 + 15;
    let mantissa = (bits & ((1u64 << 52) - 1)) | (1u64 << 52);
    if exponent >= 31 {
        return sign | 0x7c00;
    }
    if exponent < -10 {
        return sign;
    }
    let rounded = round_even(
        mantissa,
        if exponent <= 0 {
            (43 - exponent) as u32
        } else {
            42
        },
    );
    if exponent <= 0 {
        sign | rounded as u16
    } else {
        sign | ((((exponent - 1) as u16) << 10) + rounded as u16)
    }
}
fn round_even(value: u64, shift: u32) -> u64 {
    let base = value >> shift;
    let remainder = value & ((1u64 << shift) - 1);
    let half = 1u64 << (shift - 1);
    base + u64::from(remainder > half || remainder == half && base & 1 == 1)
}

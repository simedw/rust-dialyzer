//! Native-Core cache reader. Structural records remain borrowed until typed node emission.
use super::{Reader, Result};
use crate::core::{CoreModule, CoreNode, ErlangTerm, NodeId, RawNode, TermId, build_node};
use crate::fast_hash::{FxBuildHasher, FxHasher};
use crate::shared_vec::SharedVec;
use num_bigint::{BigInt, Sign};
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

type Table<K, V> = HashMap<K, V, FxBuildHasher>;
#[derive(Clone, Copy, Debug)]
struct Span {
    start: usize,
    end: usize,
}
#[derive(Clone, Copy)]
enum Key {
    Span(Span),
    Byte(u8),
}
struct Layout {
    keys: Vec<Key>,
    order: Vec<usize>,
}

impl<'a> Reader<'a> {
    pub(super) fn native_tuple(&mut self) -> Result<usize> {
        match self.u8()? {
            104 => Ok(self.u8()? as usize),
            105 => self.u32(),
            _ => Err("Native Core: expected tuple".into()),
        }
    }
    pub(super) fn native_atom(&mut self) -> Result<&'a str> {
        let n = match self.u8()? {
            118 | 100 => self.u16()?,
            119 | 115 => self.u8()? as usize,
            _ => return Err("Native Core: expected atom".into()),
        };
        std::str::from_utf8(self.take(n)?).map_err(|_| "Native Core: atom is not UTF-8".into())
    }
    fn native_binary(&mut self) -> Result<&'a [u8]> {
        if self.u8()? != 109 {
            return Err("Native Core: expected binary".into());
        }
        let n = self.u32()?;
        self.take(n)
    }
    fn native_uint(&mut self) -> Result<usize> {
        match self.u8()? {
            97 => Ok(self.u8()? as usize),
            98 => usize::try_from(i32::from_be_bytes(self.take(4)?.try_into().unwrap()))
                .map_err(|_| "Native Core: negative count".into()),
            _ => Err("Native Core: expected nonnegative count".into()),
        }
    }
    fn native_list(&mut self) -> Result<usize> {
        match self.u8()? {
            106 => Ok(0),
            108 => self.u32(),
            _ => Err("Native Core: expected proper list".into()),
        }
    }
    fn native_tail(&mut self, n: usize) -> Result<()> {
        if n > 0 && self.u8()? != 106 {
            return Err("Native Core: improper structural list".into());
        }
        Ok(())
    }
    /// Linear bounds-checked skip, with one scalar pending counter rather than recursion.
    fn native_skip(&mut self) -> Result<()> {
        let mut pending = 1usize;
        while pending != 0 {
            pending -= 1;
            let children = match self.u8()? {
                97 => {
                    self.take(1)?;
                    0
                }
                98 => {
                    self.take(4)?;
                    0
                }
                70 => {
                    self.take(8)?;
                    0
                }
                110 => {
                    let n = self.u8()? as usize;
                    self.take(n.checked_add(1).ok_or("Native Core: size overflow")?)?;
                    0
                }
                111 => {
                    let n = self.u32()?;
                    self.take(n.checked_add(1).ok_or("Native Core: size overflow")?)?;
                    0
                }
                100 | 118 => {
                    let n = self.u16()?;
                    self.take(n)?;
                    0
                }
                115 | 119 => {
                    let n = self.u8()? as usize;
                    self.take(n)?;
                    0
                }
                104 => self.u8()? as usize,
                105 => self.u32()?,
                106 => 0,
                107 => {
                    let n = self.u16()?;
                    self.take(n)?;
                    0
                }
                108 => self
                    .u32()?
                    .checked_add(1)
                    .ok_or("Native Core: size overflow")?,
                109 => {
                    let n = self.u32()?;
                    self.take(n)?;
                    0
                }
                77 => {
                    let n = self.u32()?;
                    self.take(n.checked_add(1).ok_or("Native Core: size overflow")?)?;
                    0
                }
                116 => self
                    .u32()?
                    .checked_mul(2)
                    .ok_or("Native Core: size overflow")?,
                #[cfg(otp_domain = "29")]
                67 => {
                    let count = self.u32()?;
                    if self.u8()? > 1 { return Err("Native Core: invalid record flags".into()); }
                    count.checked_mul(2).and_then(|count| count.checked_add(2)).ok_or("Native Core: size overflow")?
                }
                tag => return Err(format!("Native Core: unsupported ETF tag {tag}")),
            };
            pending = pending
                .checked_add(children)
                .ok_or("Native Core: size overflow")?;
            if pending > self.bytes.len().saturating_sub(self.at) {
                return Err("Native Core: impossible child count".into());
            }
        }
        Ok(())
    }
    fn native_span(&mut self) -> Result<Span> {
        let start = self.at;
        self.native_skip()?;
        Ok(Span {
            start,
            end: self.at,
        })
    }
    fn native_keys(&mut self) -> Result<Vec<Key>> {
        if self.bytes.get(self.at) == Some(&107) {
            self.at += 1;
            let n = self.u16()?;
            return Ok(self.take(n)?.iter().copied().map(Key::Byte).collect());
        }
        let n = self.native_list()?;
        let keys = (0..n)
            .map(|_| self.native_span().map(Key::Span))
            .collect::<Result<_>>()?;
        self.native_tail(n)?;
        Ok(keys)
    }
    fn native_indices(&mut self) -> Result<Vec<usize>> {
        if self.bytes.get(self.at) == Some(&107) {
            self.at += 1;
            let n = self.u16()?;
            return Ok(self.take(n)?.iter().map(|&n| n as usize).collect());
        }
        let n = self.native_list()?;
        let values = (0..n).map(|_| self.native_uint()).collect::<Result<_>>()?;
        self.native_tail(n)?;
        Ok(values)
    }
}

struct Builder<'a> {
    bytes: &'a [u8],
    terms: SharedVec<ErlangTerm>,
    nodes: SharedVec<CoreNode>,
    term_hashes: Table<u64, TermId>,
    collisions: Table<u64, Vec<TermId>>,
    atoms: Table<&'a str, TermId>,
    orders: BTreeMap<TermId, Vec<usize>>,
    layouts: BTreeMap<usize, Vec<Layout>>,
    roots: usize,
    contains_map: Vec<bool>,
    raw_values: Table<&'a [u8], TermId>,
    path: String,
}

struct Group {
    remaining: usize,
    total: usize,
    list_length: usize,
    pairs: bool,
    has_tail: bool,
}
struct Frame<'a> {
    kind: &'a str,
    annotations: TermId,
    schema: &'static [u8],
    position: usize,
    groups: Vec<Vec<u64>>,
    active: Option<Group>,
    argument: Option<Span>,
    argument_id: Option<u64>,
    argument_return: Option<usize>,
    pattern: Option<bool>,
    #[cfg(otp_domain = "29")]
    record_identity: Option<Span>,
    #[cfg(otp_domain = "29")]
    record_identity_id: Option<u64>,
    #[cfg(otp_domain = "29")]
    record_identity_return: Option<usize>,
}

impl<'a> Builder<'a> {
    fn intern(&mut self, term: ErlangTerm, order: Option<Vec<usize>>) -> TermId {
        let mut hasher = FxHasher::default();
        term.hash(&mut hasher);
        order.hash(&mut hasher);
        let hash = hasher.finish();
        let equals =
            |id: TermId| self.terms[id.0] == term && self.orders.get(&id) == order.as_ref();
        if let Some(&id) = self.term_hashes.get(&hash) {
            if equals(id) {
                return id;
            }
            if let Some(ids) = self.collisions.get(&hash) {
                if let Some(&id) = ids.iter().find(|&&id| equals(id)) {
                    return id;
                }
            }
        }
        let id = TermId(self.terms.len());
        let contains_map = match &term {
            ErlangTerm::Map(_) => true,
            #[cfg(otp_domain = "29")]
            ErlangTerm::Record(record) => record.fields.iter().any(|(_, value)| self.contains_map[value.0]),
            ErlangTerm::Tuple(items) => items.iter().any(|id| self.contains_map[id.0]),
            ErlangTerm::List { items, tail } => {
                self.contains_map[tail.0] || items.iter().any(|id| self.contains_map[id.0])
            }
            _ => false,
        };
        self.contains_map.push(contains_map);
        self.terms.push(term);
        if let Some(order) = order {
            self.orders.insert(id, order);
        }
        if self.term_hashes.contains_key(&hash) {
            self.collisions.entry(hash).or_default().push(id);
        } else {
            self.term_hashes.insert(hash, id);
        }
        id
    }
    fn atom(&mut self, atom: &'a str) -> TermId {
        if let Some(&id) = self.atoms.get(atom) {
            return id;
        }
        let id = self.intern(ErlangTerm::Atom(atom.to_owned()), None);
        self.atoms.insert(atom, id);
        id
    }
    fn root(&mut self, reader: &mut Reader<'a>) -> Result<TermId> {
        let mut layouts = self
            .layouts
            .remove(&self.roots)
            .unwrap_or_default()
            .into_iter();
        self.roots += 1;
        let id = self.value(reader, &mut layouts, 0)?;
        if layouts.next().is_some() {
            return Err("Native Core: unused map layouts".into());
        }
        Ok(id)
    }
    fn value(
        &mut self,
        r: &mut Reader<'a>,
        layouts: &mut std::vec::IntoIter<Layout>,
        depth: usize,
    ) -> Result<TermId> {
        // Only composite values benefit; map-containing ancestors never enter this memo.
        let span = if matches!(r.bytes.get(r.at), Some(104 | 105 | 107 | 108))
        {
            let mut probe = Reader {
                bytes: r.bytes,
                at: r.at,
            };
            Some(probe.native_span()?)
        } else {
            None
        };
        if let Some(span) = span {
            if let Some(&id) = self.raw_values.get(&self.bytes[span.start..span.end]) {
                if crate::lattice::verify_fast_paths() {
                    let layouts_before = layouts.len();
                    let fresh = self.value_inner(r, layouts, depth)?;
                    assert_eq!(fresh, id, "native raw memo term ID");
                    assert_eq!(r.at, span.end, "native raw memo value span");
                    assert_eq!(layouts.len(), layouts_before, "native raw memo consumed map provenance");
                } else {
                    r.at = span.end;
                }
                return Ok(id);
            }
        }
        let id = self.value_inner(r, layouts, depth)?;
        if let Some(span) = span {
            if r.at != span.end {
                return Err("Native Core: memo span mismatch".into());
            }
            if !self.contains_map[id.0] {
                self.raw_values
                    .insert(&self.bytes[span.start..span.end], id);
            }
        }
        Ok(id)
    }
    fn value_inner(
        &mut self,
        r: &mut Reader<'a>,
        layouts: &mut std::vec::IntoIter<Layout>,
        depth: usize,
    ) -> Result<TermId> {
        if depth >= 192 {
            return Err("Native Core: value nesting requires flat producer fallback".into());
        }
        let term = match r.u8()? {
            97 => ErlangTerm::Integer(r.u8()?.to_string()),
            98 => {
                ErlangTerm::Integer(i32::from_be_bytes(r.take(4)?.try_into().unwrap()).to_string())
            }
            tag @ (110 | 111) => {
                let n = if tag == 110 {
                    r.u8()? as usize
                } else {
                    r.u32()?
                };
                let sign = match r.u8()? {
                    0 => Sign::Plus,
                    1 => Sign::Minus,
                    _ => return Err("Native Core: invalid bigint sign".into()),
                };
                ErlangTerm::Integer(BigInt::from_bytes_le(sign, r.take(n)?).to_string())
            }
            70 => ErlangTerm::Float(u64::from_be_bytes(r.take(8)?.try_into().unwrap())),
            tag @ (100 | 118 | 115 | 119) => {
                let n = if tag == 100 || tag == 118 {
                    r.u16()?
                } else {
                    r.u8()? as usize
                };
                let atom = std::str::from_utf8(r.take(n)?)
                    .map_err(|_| "Native Core: atom is not UTF-8")?;
                return Ok(self.atom(atom));
            }
            tag @ (104 | 105) => {
                let n = if tag == 104 {
                    r.u8()? as usize
                } else {
                    r.u32()?
                };
                if n > r.bytes.len().saturating_sub(r.at) {
                    return Err("Native Core: impossible tuple length".into());
                }
                ErlangTerm::Tuple(
                    (0..n)
                        .map(|_| self.value(r, layouts, depth + 1))
                        .collect::<Result<_>>()?,
                )
            }
            106 => ErlangTerm::Nil,
            #[cfg(otp_domain = "29")]
            67 => {
                let count = r.u32()?;
                if count > r.bytes.len().saturating_sub(r.at) / 2 { return Err("Native Core: impossible record field count".into()); }
                let exported = match r.u8()? { 0 => false, 1 => true, _ => return Err("Native Core: invalid record flags".into()) };
                let module = r.native_atom()?;
                let name = r.native_atom()?;
                self.atom(module);
                self.atom(name);
                let module = module.to_owned();
                let name = name.to_owned();
                let mut names = Vec::with_capacity(count);
                let mut unique = std::collections::BTreeSet::new();
                for _ in 0..count {
                    let name = r.native_atom()?;
                    if !unique.insert(name) { return Err("Native Core: duplicate record field".into()); }
                    self.atom(name);
                    names.push(name.to_owned());
                }
                let fields = names.into_iter().map(|name| self.value(r, layouts, depth + 1).map(|value| (name, value))).collect::<Result<_>>()?;
                ErlangTerm::Record(Box::new(crate::core::NativeRecord { module, name, exported, fields }))
            }
            tag @ (107 | 108) => {
                let mut tag = tag;
                let mut items = Vec::new();
                let tail;
                loop {
                    if tag == 107 {
                        let n = r.u16()?;
                        for &byte in r.take(n)? {
                            items.push(self.intern(ErlangTerm::Integer(byte.to_string()), None));
                        }
                        tail = self.intern(ErlangTerm::Nil, None);
                        break;
                    }
                    let n = r.u32()?;
                    if n > r.bytes.len().saturating_sub(r.at) {
                        return Err("Native Core: impossible list length".into());
                    }
                    for _ in 0..n {
                        items.push(self.value(r, layouts, depth + 1)?);
                    }
                    match r.bytes.get(r.at) {
                        Some(107 | 108) => {
                            tag = r.u8()?;
                        }
                        _ => {
                            tail = self.value(r, layouts, depth + 1)?;
                            break;
                        }
                    }
                }
                if items.is_empty() {
                    return Ok(tail);
                }
                ErlangTerm::List { items, tail }
            }
            109 => {
                let n = r.u32()?;
                ErlangTerm::Binary(r.take(n)?.to_vec())
            }
            77 => {
                let n = r.u32()?;
                let last = r.u8()?;
                if n == 0 || !(1..=8).contains(&last) {
                    return Err("Native Core: invalid bitstring length".into());
                }
                let bytes = r.take(n)?.to_vec();
                if last == 8 {
                    ErlangTerm::Binary(bytes)
                } else {
                    ErlangTerm::Bitstring {
                        bytes,
                        bits: ((n - 1) as u64) * 8 + last as u64,
                    }
                }
            }
            116 => {
                let n = r.u32()?;
                let layout = layouts.next().ok_or("Native Core: missing map layout")?;
                if layout.keys.len() != n || layout.order.len() != n {
                    return Err("Native Core: map layout arity differs".into());
                }
                let mut positions: Table<&[u8], (Span, Span)> = Table::default();
                for _ in 0..n {
                    let key = r.native_span()?;
                    let value = r.native_span()?;
                    if positions
                        .insert(&self.bytes[key.start..key.end], (key, value))
                        .is_some()
                    {
                        return Err("Native Core: duplicate encoded map key".into());
                    }
                }
                let mut pairs = Vec::with_capacity(n);
                for key in layout.keys {
                    let byte;
                    let bytes = match key {
                        Key::Span(span) => &self.bytes[span.start..span.end],
                        Key::Byte(n) => {
                            byte = [97, n];
                            &byte[..]
                        }
                    };
                    let (key, value) = positions.remove(bytes).ok_or(
                        "Native Core: map key sidecar bytes differ; unsupported writer encoding",
                    )?;
                    let mut kr = Reader {
                        bytes: self.bytes,
                        at: key.start,
                    };
                    let key_id = self.value(&mut kr, layouts, depth + 1)?;
                    if kr.at != key.end {
                        return Err("Native Core: key span mismatch".into());
                    }
                    let mut vr = Reader {
                        bytes: self.bytes,
                        at: value.start,
                    };
                    let value_id = self.value(&mut vr, layouts, depth + 1)?;
                    if vr.at != value.end {
                        return Err("Native Core: value span mismatch".into());
                    }
                    pairs.push((key_id, value_id));
                }
                let mut used = vec![false; n];
                for &position in &layout.order {
                    if position >= n || std::mem::replace(&mut used[position], true) {
                        return Err("Native Core: invalid iterator permutation".into());
                    }
                }
                return Ok(self.intern(ErlangTerm::Map(pairs), Some(layout.order)));
            }
            tag => return Err(format!("Native Core: unsupported value ETF tag {tag}")),
        };
        Ok(self.intern(term, None))
    }
    fn emit_node(
        &mut self,
        kind: &str,
        annotations: TermId,
        groups: Vec<Vec<u64>>,
        value: Option<u64>,
        map_arg: Option<u64>,
        map_pattern: Option<bool>,
        #[cfg(otp_domain = "29")] record_id: Option<u64>,
    ) -> Result<NodeId> {
        let id = self.nodes.len();
        let raw = RawNode {
            kind,
            annotations: annotations.0 as u64,
            groups,
            value,
            map_arg: if kind == "map" { map_arg } else { None },
            #[cfg(otp_domain = "29")]
            record_arg: if kind == "record" { map_arg } else { None },
            #[cfg(otp_domain = "29")]
            record_id,
            map_pattern,
        };
        let node = build_node(
            &raw,
            format_args!("{}.nodes[{id}]", self.path),
            id,
            &self.nodes,
            &self.terms,
        )?;
        self.nodes.push(node);
        Ok(NodeId(id))
    }
    fn core(&mut self, r: &mut Reader<'a>) -> Result<NodeId> {
        let mut frames: Vec<Frame<'a>> = Vec::new();
        let mut entering = true;
        let mut returned = None;
        loop {
            if entering {
                entering = false;
                let arity = r.native_tuple()?;
                let tag = r.native_atom()?;
                let kind = tag
                    .strip_prefix("c_")
                    .ok_or("Native Core: invalid record tag")?;
                let expected = match kind {
                    "literal" | "var" | "opaque" | "binary" | "tuple" | "values" | "catch" => 3,
                    "alias" | "apply" | "case" | "cons" | "fun" | "letrec" | "primop" | "seq" => 4,
                    "call" | "clause" | "let" | "map" | "map_pair" | "receive" => 5,
                    "module" => 6,
                    #[cfg(otp_domain = "29")]
                    "record" => 5,
                    #[cfg(otp_domain = "29")]
                    "record_pair" => 4,
                    "bitstr" | "try" => 7,
                    _ => return Err(format!("Native Core: unsupported record {tag}")),
                };
                if arity != expected {
                    return Err(format!("Native Core: wrong arity for {tag}"));
                }
                let annotations = self.root(r)?;
                if matches!(kind, "literal" | "var" | "opaque") {
                    let value = self.root(r)?;
                    returned = Some(self.emit_node(
                        kind,
                        annotations,
                        Vec::new(),
                        Some(value.0 as u64),
                        None,
                        None,
                        #[cfg(otp_domain = "29")]
                        None,
                    )?);
                } else {
                    let schema: &'static [u8] = match kind {
                        "alias" | "cons" | "seq" => b"SS",
                        "apply" | "primop" | "case" => b"SL",
                        "binary" | "tuple" | "values" | "map" => b"L",
                        "bitstr" => b"SSSSS",
                        "call" => b"SSL",
                        "clause" | "let" | "receive" => b"LSS",
                        "fun" => b"LS",
                        "catch" => b"S",
                        "try" => b"SLSLS",
                        "letrec" => b"PS",
                        "module" => b"SLPP",
                        "map_pair" => b"SSS",
                        #[cfg(otp_domain = "29")]
                        "record" => b"L",
                        #[cfg(otp_domain = "29")]
                        "record_pair" => b"SS",
                        _ => return Err("Native Core: missing group schema".into()),
                    };
                    let argument = if kind == "map" || cfg!(otp_domain = "29") && kind == "record" {
                        Some(r.native_span()?)
                    } else {
                        None
                    };
                    #[cfg(otp_domain = "29")]
                    let record_identity = if kind == "record" { Some(r.native_span()?) } else { None };
                    frames.push(Frame {
                        kind,
                        annotations,
                        schema,
                        position: 0,
                        groups: Vec::new(),
                        active: None,
                        argument,
                        argument_id: None,
                        argument_return: None,
                        pattern: None,
                        #[cfg(otp_domain = "29")]
                        record_identity,
                        #[cfg(otp_domain = "29")]
                        record_identity_id: None,
                        #[cfg(otp_domain = "29")]
                        record_identity_return: None,
                    });
                }
            }
            if let Some(id) = returned.take() {
                let Some(frame) = frames.last_mut() else {
                    return Ok(id);
                };
                if let Some(end) = frame.argument_return.take() {
                    let span = frame.argument.ok_or("Native Core: missing argument span")?;
                    if r.at != span.end {
                        return Err("Native Core: argument span mismatch".into());
                    }
                    frame.argument_id = Some(id.0 as u64);
                    r.at = end;
                } else if cfg!(otp_domain = "29") && frame.kind == "record"
                    && { #[cfg(otp_domain = "29")] { frame.record_identity_return.is_some() } #[cfg(not(otp_domain = "29"))] { false } }
                {
                    #[cfg(otp_domain = "29")]
                    {
                        let end = frame.record_identity_return.take().unwrap();
                        if r.at != frame.record_identity.unwrap().end { return Err("Native Core: record id span mismatch".into()); }
                        frame.record_identity_id = Some(id.0 as u64);
                        r.at = end;
                    }
                } else {
                    let active = frame
                        .active
                        .as_mut()
                        .ok_or("Native Core: missing active group")?;
                    frame
                        .groups
                        .last_mut()
                        .ok_or("Native Core: missing group")?
                        .push(id.0 as u64);
                    active.remaining = active
                        .remaining
                        .checked_sub(1)
                        .ok_or("Native Core: group underflow")?;
                }
            }
            let frame = frames.last_mut().ok_or("Native Core: missing frame")?;
            if let Some(active) = &frame.active {
                if active.remaining != 0 {
                    if active.pairs
                        && (active.total - active.remaining) % 2 == 0
                        && r.native_tuple()? != 2
                    {
                        return Err("Native Core: expected child pair".into());
                    }
                    entering = true;
                    continue;
                }
                if active.has_tail {
                    r.native_tail(active.list_length)?;
                }
                frame.active = None;
            }
            if let Some(&group) = frame.schema.get(frame.position) {
                frame.position += 1;
                let list_length = if group == b'S' { 1 } else { r.native_list()? };
                if list_length > r.bytes.len().saturating_sub(r.at) {
                    return Err("Native Core: impossible group length".into());
                }
                let total = list_length
                    .checked_mul(if group == b'P' { 2 } else { 1 })
                    .ok_or("Native Core: group size overflow")?;
                frame.groups.push(Vec::new());
                frame.active = Some(Group {
                    remaining: total,
                    total,
                    list_length,
                    pairs: group == b'P',
                    has_tail: group != b'S',
                });
                continue;
            }
            if let Some(span) = frame.argument {
                if frame.argument_id.is_none() {
                    if frame.kind == "map" { frame.pattern = Some(match r.native_atom()? {
                        "true" => true,
                        "false" => false,
                        _ => return Err("Native Core: map pattern is not boolean".into()),
                    }); }
                    frame.argument_return = Some(r.at);
                    r.at = span.start;
                    entering = true;
                    continue;
                }
            }
            #[cfg(otp_domain = "29")]
            if let Some(span) = frame.record_identity {
                if frame.record_identity_id.is_none() {
                    frame.record_identity_return = Some(r.at);
                    r.at = span.start;
                    entering = true;
                    continue;
                }
            }
            let frame = frames.pop().unwrap();
            returned = Some(self.emit_node(
                frame.kind,
                frame.annotations,
                frame.groups,
                None,
                frame.argument_id,
                frame.pattern,
                #[cfg(otp_domain = "29")]
                frame.record_identity_id,
            )?);
        }
    }
}

pub(super) fn module(bytes: &[u8]) -> Result<CoreModule> {
    let mut r = Reader { bytes, at: 0 };
    if r.u8()? != 131 {
        return Err("Native Core: missing ETF version".into());
    }
    if r.native_tuple()? != 8 || r.native_atom()? != "native_core_module_v1" {
        return Err("Native Core: expected v1 record".into());
    }
    let name = std::str::from_utf8(r.native_binary()?)
        .map_err(|_| "Native Core: module name is not UTF-8")?
        .to_owned();
    let core = r.native_span()?;
    let source_atoms = r.native_span()?;
    let contracts = r.native_span()?;
    let mut layouts = BTreeMap::new();
    let n = r.native_list()?;
    for _ in 0..n {
        if r.native_tuple()? != 2 {
            return Err("Native Core: invalid root layout".into());
        }
        let root = r.native_uint()?;
        let count = r.native_list()?;
        let mut maps = Vec::new();
        for _ in 0..count {
            if r.native_tuple()? != 2 {
                return Err("Native Core: invalid map layout".into());
            }
            maps.push(Layout {
                keys: r.native_keys()?,
                order: r.native_indices()?,
            });
        }
        r.native_tail(count)?;
        if layouts.insert(root, maps).is_some() {
            return Err("Native Core: duplicate root layout".into());
        }
    }
    r.native_tail(n)?;
    let node_count = r.native_uint()?;
    let root_count = r.native_uint()?;
    if r.at != bytes.len() {
        return Err("Native Core: trailing bytes".into());
    }
    if node_count > bytes.len() || root_count > bytes.len() {
        return Err("Native Core: impossible arena counts".into());
    }
    let mut b = Builder {
        bytes,
        terms: SharedVec::with_capacity(0),
        nodes: SharedVec::with_capacity(node_count),
        term_hashes: Table::default(),
        collisions: Table::default(),
        atoms: Table::default(),
        orders: BTreeMap::new(),
        layouts,
        roots: 0,
        contains_map: Vec::new(),
        raw_values: Table::default(),
        path: format!("$.modules[{name}]"),
    };
    let source_forms = b.intern(ErlangTerm::Nil, None);
    r.at = source_atoms.start;
    let n = r.native_list()?;
    for _ in 0..n {
        b.atom(r.native_atom()?);
    }
    r.native_tail(n)?;
    if r.at != source_atoms.end {
        return Err("Native Core: source atom span mismatch".into());
    }
    r.at = core.start;
    let root = b.core(&mut r)?;
    if r.at != core.end {
        return Err("Native Core: Core span mismatch".into());
    }
    r.at = contracts.start;
    let n = r.native_list()?;
    let mut contract_variable_maps = BTreeMap::new();
    for _ in 0..n {
        if r.native_tuple()? != 2 {
            return Err("Native Core: invalid contract pair".into());
        }
        let form = b.root(&mut r)?;
        let variables = b.root(&mut r)?;
        contract_variable_maps.insert(form, variables);
    }
    r.native_tail(n)?;
    if r.at != contracts.end
        || b.nodes.len() != node_count
        || b.roots != root_count
        || !b.layouts.is_empty()
    {
        return Err("Native Core: final counts differ".into());
    }
    b.nodes.freeze();
    b.terms.freeze();
    Ok(CoreModule {
        name,
        source_forms,
        core: root,
        terms: b.terms,
        nodes: b.nodes,
        map_iterator_orders: b.orders,
        contract_variable_maps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn atom(value: &str) -> Vec<u8> {
        let mut b = vec![119, value.len() as u8];
        b.extend(value.as_bytes());
        b
    }
    fn binary(value: &[u8]) -> Vec<u8> {
        let mut b = vec![109];
        b.extend((value.len() as u32).to_be_bytes());
        b.extend(value);
        b
    }
    fn integer(value: u8) -> Vec<u8> {
        vec![97, value]
    }
    fn tuple(values: Vec<Vec<u8>>) -> Vec<u8> {
        let mut b = vec![104, values.len() as u8];
        for value in values {
            b.extend(value);
        }
        b
    }
    fn list(values: Vec<Vec<u8>>) -> Vec<u8> {
        if values.is_empty() {
            return vec![106];
        }
        let mut b = vec![108];
        b.extend((values.len() as u32).to_be_bytes());
        for value in values {
            b.extend(value);
        }
        b.push(106);
        b
    }
    fn envelope(core: Vec<u8>, layouts: Vec<u8>, nodes: u8, roots: u8) -> Vec<u8> {
        let mut b = vec![131];
        b.extend(tuple(vec![
            atom("native_core_module_v1"),
            binary(b"fixture"),
            core,
            list(vec![]),
            list(vec![]),
            layouts,
            integer(nodes),
            integer(roots),
        ]));
        b
    }
    fn leaf(value: Vec<u8>) -> Vec<u8> {
        tuple(vec![atom("c_literal"), vec![106], value])
    }
    #[test]
    fn canonical_scalar_and_malformed_boundaries() {
        let bytes = envelope(leaf(atom("ok")), list(vec![]), 1, 2);
        let actual = module(&bytes).unwrap();
        assert_eq!(actual.terms.len(), 2);
        assert_eq!(actual.terms[0], ErlangTerm::Nil);
        assert_eq!(actual.terms[1], ErlangTerm::Atom("ok".into()));
        assert_eq!(
            actual.nodes[0].expr,
            crate::core::CoreExpr::Literal(TermId(1))
        );
        for end in 0..bytes.len() {
            assert!(
                module(&bytes[..end]).is_err(),
                "accepted prefix {end}"
            );
        }
        let mut extra = bytes.clone();
        extra.push(106);
        assert!(module(&extra).is_err());
        assert!(module(&envelope(leaf(atom("ok")), list(vec![]), 0, 2)).is_err());
        assert!(
            module(&envelope(leaf(vec![116, 0, 0, 0, 0]), list(vec![]), 1, 2))
            .is_err()
        );
    }
    #[test]
    fn borrowed_memo_never_collapses_map_ancestors() {
        let mut map = vec![116, 0, 0, 0, 2];
        map.extend(atom("a"));
        map.extend(integer(1));
        map.extend(atom("b"));
        map.extend(integer(2));
        let value = tuple(vec![map]);
        let core = tuple(vec![
            atom("c_tuple"),
            vec![106],
            list(vec![leaf(value.clone()), leaf(value)]),
        ]);
        let layout = |root, keys: Vec<Vec<u8>>, order| {
            tuple(vec![
                integer(root),
                list(vec![tuple(vec![list(keys), order])]),
            ])
        };
        let layouts = list(vec![
            layout(2, vec![atom("a"), atom("b")], vec![107, 0, 2, 0, 1]),
            layout(4, vec![atom("b"), atom("a")], vec![107, 0, 2, 1, 0]),
        ]);
        let bytes = envelope(core, layouts, 3, 5);
        let actual = module(&bytes).unwrap();
        assert_eq!(actual.terms.len(), 9);
        assert_eq!(
            actual.terms[5],
            ErlangTerm::Map(vec![(TermId(1), TermId(2)), (TermId(3), TermId(4))])
        );
        assert_eq!(
            actual.terms[7],
            ErlangTerm::Map(vec![(TermId(3), TermId(4)), (TermId(1), TermId(2))])
        );
        assert_eq!(actual.map_iterator_orders[&TermId(5)], vec![0, 1]);
        assert_eq!(actual.map_iterator_orders[&TermId(7)], vec![1, 0]);
        assert_eq!(actual.terms[6], ErlangTerm::Tuple(vec![TermId(5)]));
        assert_eq!(actual.terms[8], ErlangTerm::Tuple(vec![TermId(7)]));
    }
    fn map_layout(root: u8, keys: Vec<Vec<u8>>, order: Vec<Vec<u8>>) -> Vec<u8> {
        tuple(vec![integer(root), list(vec![tuple(vec![list(keys), list(order)])])])
    }
    fn one_map(keys: Vec<Vec<u8>>) -> Vec<u8> {
        let mut bytes = vec![116];
        bytes.extend((keys.len() as u32).to_be_bytes());
        for key in keys { bytes.extend(key); bytes.extend(atom("value")); }
        bytes
    }
    #[test]
    fn map_writer_contract_rejects_alternate_encodings_and_bad_layouts() {
        let map = one_map(vec![integer(1)]);
        let canonical = list(vec![map_layout(1, vec![integer(1)], vec![integer(0)])]);
        assert!(module(&envelope(leaf(map.clone()), canonical, 1, 2)).is_ok());
        // INTEGER_EXT 1 and SMALL_INTEGER_EXT 1 are semantically equal but
        // different bytes: never match the sidecar through semantic guessing.
        let alternate = list(vec![map_layout(1, vec![vec![98, 0, 0, 0, 1]], vec![integer(0)])]);
        let error = module(&envelope(leaf(map.clone()), alternate, 1, 2)).unwrap_err();
        assert!(error.contains("unsupported writer encoding"), "{error}");
        let two = one_map(vec![atom("a"), atom("b")]);
        for order in [vec![integer(0), integer(0)], vec![integer(0), integer(2)]] {
            let layouts = list(vec![map_layout(1, vec![atom("a"), atom("b")], order)]);
            assert!(module(&envelope(leaf(two.clone()), layouts, 1, 2)).is_err());
        }
        let duplicates = one_map(vec![atom("a"), atom("a")]);
        let layouts = list(vec![map_layout(1, vec![atom("a"), atom("a")], vec![integer(0), integer(1)])]);
        assert!(module(&envelope(leaf(duplicates), layouts, 1, 2)).is_err());
        let missing = list(vec![map_layout(99, vec![integer(1)], vec![integer(0)])]);
        assert!(module(&envelope(leaf(map.clone()), missing, 1, 2)).is_err());
        let duplicate_root = list(vec![map_layout(1, vec![integer(1)], vec![integer(0)]), map_layout(1, vec![integer(1)], vec![integer(0)])]);
        assert!(module(&envelope(leaf(map), duplicate_root, 1, 2)).is_err());
    }
    #[test]
    fn native_core_depth_is_iterative_and_deep_values_require_flat_fallback() {
        let depth = 384u32;
        let mut core = leaf(vec![106]);
        for _ in 0..depth { core = tuple(vec![atom("c_cons"), vec![106], leaf(atom("ok")), core]); }
        let mut bytes = envelope(core, list(vec![]), 0, 0);
        bytes.truncate(bytes.len() - 4);
        bytes.push(98); bytes.extend((2 * depth + 1).to_be_bytes());
        bytes.push(98); bytes.extend((3 * depth + 2).to_be_bytes());
        assert_eq!(module(&bytes).unwrap().nodes.len(), (2 * depth + 1) as usize);
        let mut value = atom("ok");
        for _ in 0..depth { value = tuple(vec![value]); }
        let error = module(&envelope(leaf(value), list(vec![]), 1, 2)).unwrap_err();
        assert!(error.contains("flat producer fallback"), "{error}");
    }
    #[test]
    fn oversized_counts_invalid_scalar_bytes_and_noncanonical_structural_lists_reject() {
        for bad in [vec![108,255,255,255,255], vec![105,255,255,255,255],
                    vec![110,1,2,0], vec![77,0,0,0,1,0,0], vec![119,1,255]] {
            assert!(module(&envelope(leaf(bad), list(vec![]), 1, 2)).is_err());
        }
        // The producer writes [] as NIL_EXT. Alternative zero-length LIST_EXT
        // framing is outside the native producer contract and must not misparse.
        assert!(module(&envelope(leaf(atom("ok")), vec![108,0,0,0,0,106], 1, 2)).is_err());
    }

    #[test]
    fn raw_memo_hit_preserves_term_ids_and_signed_zero_bits() {
        let float = |bits: u64| { let mut bytes = vec![70]; bytes.extend(bits.to_be_bytes()); bytes };
        let value = tuple(vec![float(0), float(1u64 << 63)]);
        let core = tuple(vec![atom("c_tuple"), vec![106], list(vec![leaf(value.clone()), leaf(value)])]);
        let actual = module(&envelope(core, list(vec![]), 3, 5)).unwrap();
        assert_eq!(actual.terms.len(), 4);
        assert_eq!(actual.terms[1], ErlangTerm::Float(0));
        assert_eq!(actual.terms[2], ErlangTerm::Float(1u64 << 63));
        assert_eq!(actual.terms[3], ErlangTerm::Tuple(vec![TermId(1), TermId(2)]));
        assert_eq!(actual.nodes[0].expr, crate::core::CoreExpr::Literal(TermId(3)));
        assert_eq!(actual.nodes[1].expr, crate::core::CoreExpr::Literal(TermId(3)));
    }

}

use crate::{
    core::{ErlangTerm, ErlangTerms, TermId},
    term_order::TermOrderArena,
};
use num_bigint::{BigInt, Sign};

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub bytes: usize,
    pub nodes: usize,
    pub expanded: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            bytes: 64 * 1024 * 1024,
            nodes: 2_000_000,
            expanded: 2_000_000,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    UnsupportedTag(u8),
    ResourceLimit,
    Compression,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ETF: {self:?}")
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;

enum Container {
    Tuple,
    List,
    Map,
}
enum Action {
    Read,
    Finish(Container, usize),
}
struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
    terms: Vec<ErlangTerm>,
    values: Vec<TermId>,
    limits: Limits,
}
impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self.position.checked_add(n).ok_or(Error::Invalid)?;
        let bytes = self.bytes.get(self.position..end).ok_or(Error::Invalid)?;
        self.position = end;
        Ok(bytes)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn short(&mut self) -> Result<usize> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().map_err(|_| Error::Invalid)?) as usize)
    }
    fn length(&mut self) -> Result<usize> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().map_err(|_| Error::Invalid)?) as usize)
    }
    fn atom(&mut self) -> Result<String> {
        let tag = self.byte()?;
        let length = match tag {
            100 | 118 => self.short()?,
            115 | 119 => self.byte()? as usize,
            _ => return Err(Error::Invalid),
        };
        let bytes = self.take(length)?;
        let text: String = if tag == 100 || tag == 115 {
            bytes.iter().map(|byte| char::from(*byte)).collect()
        } else {
            std::str::from_utf8(bytes)
                .map_err(|_| Error::Invalid)?
                .to_owned()
        };
        if text.chars().count() > 255 {
            return Err(Error::Invalid);
        }
        Ok(text)
    }
    fn push(&mut self, term: ErlangTerm) -> Result<()> {
        if self.terms.len() >= self.limits.nodes {
            return Err(Error::ResourceLimit);
        }
        self.values.push(TermId(self.terms.len()));
        self.terms.push(term);
        Ok(())
    }
    fn container(&self, actions: &mut Vec<Action>, kind: Container, count: usize) -> Result<()> {
        if count > self.bytes.len() - self.position {
            return Err(Error::Invalid);
        }
        if count > self.limits.nodes || count > self.limits.expanded {
            return Err(Error::ResourceLimit);
        }
        actions.push(Action::Finish(kind, count));
        actions.extend((0..count).map(|_| Action::Read));
        Ok(())
    }
    fn run(mut self) -> Result<ErlangTerms> {
        let mut actions = vec![Action::Read];
        while let Some(action) = actions.pop() {
            match action {
                Action::Finish(kind, count) => {
                    let start = self.values.len().checked_sub(count).ok_or(Error::Invalid)?;
                    let mut items = self.values.split_off(start);
                    let term = match kind {
                        Container::Tuple => ErlangTerm::Tuple(items),
                        Container::List => {
                            let tail = items.pop().ok_or(Error::Invalid)?;
                            ErlangTerm::List { items, tail }
                        }
                        Container::Map => ErlangTerm::Map(
                            items
                                .chunks_exact(2)
                                .map(|pair| (pair[0], pair[1]))
                                .collect(),
                        ),
                    };
                    self.push(term)?;
                }
                Action::Read => match self.byte()? {
                    97 => {
                        let value = self.byte()?;
                        self.push(ErlangTerm::Integer(value.to_string()))?;
                    }
                    98 => {
                        let value = i32::from_be_bytes(
                            self.take(4)?.try_into().map_err(|_| Error::Invalid)?,
                        );
                        self.push(ErlangTerm::Integer(value.to_string()))?;
                    }
                    70 => {
                        let bits = u64::from_be_bytes(
                            self.take(8)?.try_into().map_err(|_| Error::Invalid)?,
                        );
                        if !f64::from_bits(bits).is_finite() {
                            return Err(Error::Invalid);
                        }
                        self.push(ErlangTerm::Float(bits))?;
                    }
                    99 => {
                        let bytes = self.take(31)?;
                        let end = bytes
                            .iter()
                            .position(|byte| *byte == 0)
                            .unwrap_or(bytes.len());
                        let value: f64 = std::str::from_utf8(&bytes[..end])
                            .map_err(|_| Error::Invalid)?
                            .parse()
                            .map_err(|_| Error::Invalid)?;
                        if !value.is_finite() {
                            return Err(Error::Invalid);
                        }
                        self.push(ErlangTerm::Float(value.to_bits()))?;
                    }
                    tag @ (100 | 115 | 118 | 119) => {
                        let length = if tag == 100 || tag == 118 {
                            self.short()?
                        } else {
                            self.byte()? as usize
                        };
                        let bytes = self.take(length)?;
                        let text: String = if tag == 100 || tag == 115 {
                            bytes.iter().map(|byte| char::from(*byte)).collect()
                        } else {
                            std::str::from_utf8(bytes)
                                .map_err(|_| Error::Invalid)?
                                .to_owned()
                        };
                        if text.chars().count() > 255 {
                            return Err(Error::Invalid);
                        }
                        self.push(ErlangTerm::Atom(text))?;
                    }
                    104 => {
                        let count = self.byte()? as usize;
                        self.container(&mut actions, Container::Tuple, count)?;
                    }
                    105 => {
                        let count = self.length()?;
                        self.container(&mut actions, Container::Tuple, count)?;
                    }
                    106 => self.push(ErlangTerm::Nil)?,
                    107 => {
                        let count = self.short()?;
                        let bytes = self.take(count)?.to_vec();
                        let start = self.values.len();
                        for byte in bytes {
                            self.push(ErlangTerm::Integer(byte.to_string()))?;
                        }
                        self.push(ErlangTerm::Nil)?;
                        let mut items = self.values.split_off(start);
                        let tail = items.pop().ok_or(Error::Invalid)?;
                        self.push(ErlangTerm::List { items, tail })?;
                    }
                    108 => {
                        let count = self.length()?.checked_add(1).ok_or(Error::Invalid)?;
                        self.container(&mut actions, Container::List, count)?;
                    }
                    109 => {
                        let count = self.length()?;
                        let bytes = self.take(count)?.to_vec();
                        self.push(ErlangTerm::Binary(bytes))?;
                    }
                    77 => {
                        let count = self.length()?;
                        let last = self.byte()?;
                        if count == 0 || !(1..=8).contains(&last) {
                            return Err(Error::Invalid);
                        }
                        let mut bytes = self.take(count)?.to_vec();
                        let bits = (count as u64).checked_mul(8).ok_or(Error::Invalid)?
                            - u64::from(8 - last);
                        if let Some(byte) = bytes.last_mut() {
                            *byte &= 0xff << (8 - last);
                        }
                        self.push(ErlangTerm::Bitstring { bytes, bits })?;
                    }
                    tag @ (110 | 111) => {
                        let count = if tag == 110 {
                            self.byte()? as usize
                        } else {
                            self.length()?
                        };
                        let sign = if self.byte()? == 0 {
                            Sign::Plus
                        } else {
                            Sign::Minus
                        };
                        let value = BigInt::from_bytes_le(sign, self.take(count)?);
                        self.push(ErlangTerm::Integer(value.to_string()))?;
                    }
                    116 => {
                        let count = self.length()?.checked_mul(2).ok_or(Error::Invalid)?;
                        self.container(&mut actions, Container::Map, count)?;
                    }
                    113 => {
                        let module = self.atom()?;
                        let function = self.atom()?;
                        if self.byte()? != 97 {
                            return Err(Error::Invalid);
                        }
                        let arity = self.byte()?;
                        self.push(ErlangTerm::ExternalFun {
                            module,
                            function,
                            arity,
                        })?;
                    }
                    tag @ (82 | 88 | 89 | 90 | 101 | 102 | 103 | 112 | 114 | 117 | 120 | 121) => {
                        return Err(Error::UnsupportedTag(tag));
                    }
                    _ => return Err(Error::Invalid),
                },
            }
        }
        if self.values.len() != 1 || self.position != self.bytes.len() {
            return Err(Error::Invalid);
        }
        let arena = ErlangTerms {
            root: self.values[0],
            terms: self.terms.into(),
        };
        let order = TermOrderArena::new(&arena).map_err(|_| Error::Invalid)?;
        for term in &arena.terms {
            if let ErlangTerm::Map(pairs) = term
                && order
                    .canonical_map_pairs(pairs)
                    .map_err(|_| Error::Invalid)?
                    .len()
                    != pairs.len()
            {
                return Err(Error::Invalid);
            }
        }
        Ok(arena)
    }
}

pub fn decode(bytes: &[u8], limits: Limits) -> Result<ErlangTerms> {
    if bytes.len() > limits.bytes {
        return Err(Error::ResourceLimit);
    }
    if bytes.first() != Some(&131) {
        return Err(Error::Invalid);
    }
    let expanded;
    let bytes = if bytes.get(1) == Some(&80) {
        let size = u32::from_be_bytes(
            bytes
                .get(2..6)
                .ok_or(Error::Invalid)?
                .try_into()
                .map_err(|_| Error::Invalid)?,
        ) as usize;
        if size > limits.bytes {
            return Err(Error::ResourceLimit);
        }
        expanded = zlib::decompress(&bytes[6..], size)?;
        expanded.as_slice()
    } else {
        &bytes[1..]
    };
    Reader {
        bytes,
        position: 0,
        terms: Vec::new(),
        values: Vec::new(),
        limits,
    }
    .run()
}

fn length(out: &mut Vec<u8>, value: usize) -> Result<()> {
    out.extend(
        u32::try_from(value)
            .map_err(|_| Error::ResourceLimit)?
            .to_be_bytes(),
    );
    Ok(())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapEncoding {
    Canonical,
    PreserveIteration,
}

pub fn encode(arena: &ErlangTerms, limits: Limits, compressed: bool) -> Result<Vec<u8>> {
    encode_with_map_encoding(arena, limits, compressed, MapEncoding::Canonical)
}

pub fn encode_with_map_encoding(
    arena: &ErlangTerms,
    limits: Limits,
    compressed: bool,
    map_encoding: MapEncoding,
) -> Result<Vec<u8>> {
    encode_at(arena, arena.root, limits, compressed, map_encoding)
}
pub fn encode_at(
    arena: &ErlangTerms,
    root: TermId,
    limits: Limits,
    compressed: bool,
    map_encoding: MapEncoding,
) -> Result<Vec<u8>> {
    let order = TermOrderArena::new(arena).map_err(|_| Error::Invalid)?;
    let mut out = vec![131];
    let mut pending = vec![root];
    let mut expanded = 0;
    while let Some(id) = pending.pop() {
        expanded += 1;
        if expanded > limits.expanded || out.len() > limits.bytes {
            return Err(Error::ResourceLimit);
        }
        match arena.terms.get(id.0).ok_or(Error::Invalid)? {
            ErlangTerm::Atom(text) => {
                if text.chars().count() > 255 {
                    return Err(Error::Invalid);
                }
                if text.len() <= 255 {
                    out.extend([119, text.len() as u8]);
                } else {
                    out.push(118);
                    out.extend(
                        u16::try_from(text.len())
                            .map_err(|_| Error::Invalid)?
                            .to_be_bytes(),
                    );
                }
                out.extend(text.as_bytes());
            }
            ErlangTerm::ExternalFun {
                module,
                function,
                arity,
            } => {
                out.push(113);
                for text in [module, function] {
                    if text.chars().count() > 255 {
                        return Err(Error::Invalid);
                    }
                    if text.len() <= 255 {
                        out.extend([119, text.len() as u8]);
                    } else {
                        out.push(118);
                        out.extend(
                            u16::try_from(text.len())
                                .map_err(|_| Error::Invalid)?
                                .to_be_bytes(),
                        );
                    }
                    out.extend(text.as_bytes());
                }
                out.extend([97, *arity]);
            }
            ErlangTerm::Integer(text) => {
                let value: BigInt = text.parse().map_err(|_| Error::Invalid)?;
                if let Ok(n) = u8::try_from(&value) {
                    out.extend([97, n]);
                } else if let Ok(n) = i32::try_from(&value) {
                    out.push(98);
                    out.extend(n.to_be_bytes());
                } else {
                    let (sign, bytes) = value.to_bytes_le();
                    if bytes.len() <= 255 {
                        out.extend([110, bytes.len() as u8]);
                    } else {
                        out.push(111);
                        length(&mut out, bytes.len())?;
                    }
                    out.push(u8::from(sign == Sign::Minus));
                    out.extend(bytes);
                }
            }
            ErlangTerm::Float(bits) => {
                out.push(70);
                out.extend(bits.to_be_bytes());
            }
            ErlangTerm::Binary(bytes) => {
                out.push(109);
                length(&mut out, bytes.len())?;
                out.extend(bytes);
            }
            ErlangTerm::Bitstring { bytes, bits } => {
                let count = usize::try_from(bits.div_ceil(8)).map_err(|_| Error::ResourceLimit)?;
                if bits % 8 == 0 {
                    out.push(109);
                    length(&mut out, count)?;
                } else {
                    out.push(77);
                    length(&mut out, count)?;
                    out.push((bits % 8) as u8);
                }
                out.extend(bytes.get(..count).ok_or(Error::Invalid)?);
            }
            ErlangTerm::Tuple(items) => {
                if items.len() <= 255 {
                    out.extend([104, items.len() as u8]);
                } else {
                    out.push(105);
                    length(&mut out, items.len())?;
                }
                pending.extend(items.iter().rev());
            }
            ErlangTerm::Nil => out.push(106),
            ErlangTerm::List { items, tail } => {
                if items.is_empty() {
                    pending.push(*tail);
                    continue;
                }
                if items.len() <= u16::MAX as usize
                    && matches!(arena.terms[tail.0], ErlangTerm::Nil)
                {
                    let bytes = items
                        .iter()
                        .map(|id| match &arena.terms[id.0] {
                            ErlangTerm::Integer(text) => text.parse::<u8>().ok(),
                            _ => None,
                        })
                        .collect::<Option<Vec<_>>>();
                    if let Some(bytes) = bytes {
                        out.push(107);
                        out.extend((bytes.len() as u16).to_be_bytes());
                        out.extend(bytes);
                        continue;
                    }
                }
                out.push(108);
                length(&mut out, items.len())?;
                pending.push(*tail);
                pending.extend(items.iter().rev());
            }
            ErlangTerm::Map(pairs) => {
                let unique = order
                    .canonical_map_pairs(pairs)
                    .map_err(|_| Error::Invalid)?;
                let entries = match map_encoding {
                    MapEncoding::Canonical => unique,
                    MapEncoding::PreserveIteration if unique.len() == pairs.len() => pairs.clone(),
                    MapEncoding::PreserveIteration => return Err(Error::Invalid),
                };
                out.push(116);
                length(&mut out, entries.len())?;
                for (key, value) in entries.into_iter().rev() {
                    pending.push(value);
                    pending.push(key);
                }
            }
        }
    }
    if out.len() > limits.bytes {
        return Err(Error::ResourceLimit);
    }
    if compressed {
        let data = zlib::compress(&out[1..])?;
        if data.len() + 6 < out.len() {
            let mut packed = vec![131, 80];
            length(&mut packed, out.len() - 1)?;
            packed.extend(data);
            return Ok(packed);
        }
    }
    Ok(out)
}

pub(crate) mod zlib {
    use super::{Error, Result};
    use std::os::raw::{c_char, c_int, c_uint, c_ulong, c_void};
    #[link(name = "z")]
    unsafe extern "C" {
        fn uncompress(
            dest: *mut u8,
            dest_len: *mut c_ulong,
            source: *const u8,
            source_len: c_ulong,
        ) -> c_int;
        fn compress2(
            dest: *mut u8,
            dest_len: *mut c_ulong,
            source: *const u8,
            source_len: c_ulong,
            level: c_int,
        ) -> c_int;
        fn compressBound(source_len: c_ulong) -> c_ulong;
        fn zlibVersion() -> *const c_char;
        fn inflateInit2_(
            stream: *mut Stream,
            window_bits: c_int,
            version: *const c_char,
            size: c_int,
        ) -> c_int;
        fn inflate(stream: *mut Stream, flush: c_int) -> c_int;
        fn inflateReset(stream: *mut Stream) -> c_int;
        fn inflateEnd(stream: *mut Stream) -> c_int;
    }
    #[repr(C)]
    struct Stream {
        next_in: *const u8,
        avail_in: c_uint,
        total_in: c_ulong,
        next_out: *mut u8,
        avail_out: c_uint,
        total_out: c_ulong,
        msg: *const c_char,
        state: *mut c_void,
        zalloc: Option<unsafe extern "C" fn(*mut c_void, c_uint, c_uint) -> *mut c_void>,
        zfree: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
        opaque: *mut c_void,
        data_type: c_int,
        adler: c_ulong,
        reserved: c_ulong,
    }
    struct Inflater(Box<Stream>);
    impl Inflater {
        fn gzip() -> Result<Self> {
            // Zero initializes the nullable C pointers and optional allocation callbacks.
            let mut stream = Box::new(unsafe { std::mem::zeroed::<Stream>() });
            // The boxed stream keeps the address retained by zlib stable until inflateEnd.
            let code = unsafe {
                inflateInit2_(
                    &mut *stream,
                    31,
                    zlibVersion(),
                    std::mem::size_of::<Stream>() as c_int,
                )
            };
            if code != 0 {
                return Err(Error::Compression);
            }
            Ok(Self(stream))
        }
    }
    impl Drop for Inflater {
        fn drop(&mut self) {
            // This instance owns the initialized zlib state; both are released once.
            unsafe {
                inflateEnd(&mut *self.0);
            }
        }
    }
    pub fn gunzip(source: &[u8], max_bytes: usize) -> Result<Vec<u8>> {
        let mut inflater = Inflater::gzip()?;
        let stream = &mut *inflater.0;
        let mut next = 0;
        let mut output = Vec::new();
        let mut block = [0u8; 65536];
        let mut ended = false;
        loop {
            if stream.avail_in == 0 && next < source.len() {
                let length = (source.len() - next).min(c_uint::MAX as usize);
                stream.next_in = source[next..].as_ptr();
                stream.avail_in = length as c_uint;
                next += length;
            }
            if ended {
                if stream.avail_in == 0 {
                    return Ok(output);
                }
                // OTP gunzip resets after each member and processes remaining input.
                if unsafe { inflateReset(stream) } != 0 {
                    return Err(Error::Compression);
                }
            }
            stream.next_out = block.as_mut_ptr();
            stream.avail_out = block.len() as c_uint;
            let before = stream.avail_in;
            // Input and output slices remain live and fixed during this synchronous call.
            let code = unsafe { inflate(stream, 0) };
            let produced = block.len() - stream.avail_out as usize;
            if produced > max_bytes.saturating_sub(output.len()) {
                return Err(Error::ResourceLimit);
            }
            output.extend_from_slice(&block[..produced]);
            ended = code == 1;
            if !matches!(code, 0 | 1) || (!ended && before == stream.avail_in && produced == 0) {
                return Err(Error::Invalid);
            }
        }
    }
    pub fn decompress(source: &[u8], size: usize) -> Result<Vec<u8>> {
        let mut data = vec![0; size];
        let mut length = c_ulong::try_from(size).map_err(|_| Error::ResourceLimit)?;
        let source_length = c_ulong::try_from(source.len()).map_err(|_| Error::ResourceLimit)?;
        // zlib receives the lengths of the owned output buffer and borrowed source slice.
        let code = unsafe {
            uncompress(
                data.as_mut_ptr(),
                &mut length,
                source.as_ptr(),
                source_length,
            )
        };
        if code != 0 || usize::try_from(length).ok() != Some(size) {
            return Err(Error::Invalid);
        }
        Ok(data)
    }
    pub fn compress(source: &[u8]) -> Result<Vec<u8>> {
        let source_length = c_ulong::try_from(source.len()).map_err(|_| Error::ResourceLimit)?;
        let mut length = unsafe { compressBound(source_length) };
        let mut data = vec![0; usize::try_from(length).map_err(|_| Error::ResourceLimit)?];
        // Both buffers remain alive and unmoved for the duration of the synchronous call.
        let code = unsafe {
            compress2(
                data.as_mut_ptr(),
                &mut length,
                source.as_ptr(),
                source_length,
                9,
            )
        };
        if code != 0 {
            return Err(Error::Compression);
        }
        data.truncate(usize::try_from(length).map_err(|_| Error::ResourceLimit)?);
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_function_roundtrips_wire_identity_and_literal_type() {
        let bytes = [
            131, 113, 119, 6, b'e', b'r', b'l', b'a', b'n', b'g', 119, 6, b'l', b'e', b'n', b'g',
            b't', b'h', 97, 1,
        ];
        let arena = decode(&bytes, Limits::default()).unwrap();
        assert_eq!(
            arena.terms[arena.root.0],
            ErlangTerm::ExternalFun {
                module: "erlang".into(),
                function: "length".into(),
                arity: 1,
            }
        );
        assert_eq!(encode(&arena, Limits::default(), false).unwrap(), bytes);
        let wire = crate::type_exports::arena_to_value(&arena.terms, arena.root);
        let restored = ErlangTerms::parse(&wire).unwrap();
        assert_eq!(restored.terms[restored.root.0], arena.terms[arena.root.0]);
        let types = crate::literal_types::LiteralTypes::build(&restored).unwrap();
        assert_eq!(
            types.get(restored.root),
            Some(&crate::lattice::Type::function(
                Some(vec![crate::lattice::Type::any()]),
                crate::lattice::Type::any()
            ))
        );
        let key = crate::native_runtime_hash::NativeHashKey::ExternalFun {
            module: "erlang".into(),
            function: "length".into(),
            arity: 1,
        };
        assert_eq!(
            crate::native_dict::pinned_hash_key(&key)
                .unwrap()
                .wrapping_add(1),
            2635985511
        );
        assert_eq!(crate::portable_term_hash::hash(&key), 3613265408);
    }
    #[test]
    fn external_function_hashes_match_otp_and_require_scoped_internal_identity() {
        use crate::native_runtime_hash::{NativeAtomLayout, NativeHashKey, RuntimeHashError};
        for (module, function, arity, phash, phash2) in [
            ("Elixir.Inspect", "inspect", 2, 746549427u32, 2694485718u32),
            ("Elixir.Enum", "map", 2, 3304570586, 3561230584),
            ("erlang", "length", 1, 2635985511, 3613265408),
            (
                "absent_module",
                "absent_function",
                255,
                4145039141,
                709291959,
            ),
        ] {
            let key = NativeHashKey::ExternalFun {
                module: module.into(),
                function: function.into(),
                arity,
            };
            assert_eq!(
                crate::native_dict::pinned_hash_key(&key)
                    .unwrap()
                    .wrapping_add(1),
                phash
            );
            assert_eq!(crate::portable_term_hash::hash(&key), phash2);
            let layout = NativeAtomLayout::parse(
                &serde_json::json!({"version":1,"word_size":8,"scope":"test","atom_hashes":{}}),
            )
            .unwrap();
            assert_eq!(
                layout.internal_hash(&key).unwrap_err(),
                RuntimeHashError::MissingExternalFunIdentity {
                    module: module.into(),
                    function: function.into(),
                    arity
                }
            );
        }
    }
    #[test]
    fn external_function_rejects_malformed_export_fields() {
        let valid = [131, 113, 119, 1, b'm', 119, 1, b'f', 97, 255];
        for end in 0..valid.len() {
            assert!(decode(&valid[..end], Limits::default()).is_err());
        }
        for index in [2, 5, 8] {
            let mut malformed = valid.to_vec();
            malformed[index] = 98;
            assert_eq!(
                decode(&malformed, Limits::default()).unwrap_err(),
                Error::Invalid
            );
        }
        let mut trailing = valid.to_vec();
        trailing.push(106);
        assert_eq!(
            decode(&trailing, Limits::default()).unwrap_err(),
            Error::Invalid
        );
        let limits = Limits {
            nodes: 0,
            ..Limits::default()
        };
        assert_eq!(decode(&valid, limits).unwrap_err(), Error::ResourceLimit);
        for arity in [-1, 256] {
            let wire = serde_json::json!({"root":0,"terms":[{"kind":"external_fun","module":"m","function":"f","arity":arity}]});
            assert!(ErlangTerms::parse(&wire).is_err());
        }
    }
    #[test]
    fn inspect_opts_real_debug_metadata_roundtrips_without_loss() {
        let bytes =
            include_bytes!("../reports/real-projects/fixes/external-fun/inspect-opts-dbgi.etf");
        let arena = decode(bytes, Limits::default()).unwrap();
        let funs = arena
            .terms
            .iter()
            .filter_map(|term| match term {
                ErlangTerm::ExternalFun {
                    module,
                    function,
                    arity,
                } => Some((module.as_str(), function.as_str(), *arity)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(funs, vec![("Elixir.Inspect", "inspect", 2)]);
        let wire = crate::type_exports::arena_to_value(&arena.terms, arena.root);
        let restored = ErlangTerms::parse(&wire).unwrap();
        let encoded = encode(&restored, Limits::default(), true).unwrap();
        let decoded = decode(&encoded, Limits::default()).unwrap();
        assert_eq!(
            TermOrderArena::new(&arena)
                .unwrap()
                .compare(
                    arena.root,
                    &TermOrderArena::new(&decoded).unwrap(),
                    decoded.root
                )
                .unwrap(),
            std::cmp::Ordering::Equal
        );
        if let Some(path) = std::env::var_os("EXTERNAL_FUN_ROUNDTRIP_PATH") {
            std::fs::write(path, encoded).unwrap();
        }
    }
    #[test]
    fn deep_transport_and_compression_keep_flat_ownership() {
        let mut terms = vec![ErlangTerm::Atom("leaf".into())];
        for index in 0..50_000 {
            terms.push(ErlangTerm::Tuple(vec![TermId(index)]));
        }
        let arena = ErlangTerms {
            root: TermId(50_000),
            terms: terms.into(),
        };
        for compressed in [false, true] {
            let encoded = encode(&arena, Limits::default(), compressed).unwrap();
            let decoded = decode(&encoded, Limits::default()).unwrap();
            assert_eq!(
                TermOrderArena::new(&arena)
                    .unwrap()
                    .compare(
                        arena.root,
                        &TermOrderArena::new(&decoded).unwrap(),
                        decoded.root
                    )
                    .unwrap(),
                std::cmp::Ordering::Equal
            );
        }
    }
    #[test]
    fn decompression_and_shared_expansion_have_explicit_budgets() {
        assert_eq!(
            decode(&[131, 80, 255, 255, 255, 255], Limits::default()).unwrap_err(),
            Error::ResourceLimit
        );
        let mut terms = vec![ErlangTerm::Atom("leaf".into())];
        for index in 0..31 {
            terms.push(ErlangTerm::Tuple(vec![TermId(index), TermId(index)]));
        }
        let arena = ErlangTerms {
            root: TermId(31),
            terms: terms.into(),
        };
        let limits = Limits {
            expanded: 100,
            ..Limits::default()
        };
        assert_eq!(
            encode(&arena, limits, false).unwrap_err(),
            Error::ResourceLimit
        );
    }
}

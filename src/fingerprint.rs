use crate::{
    core::{ErlangTerm, ErlangTerms, TermId},
    external_term,
};

pub fn md5(bytes: &[u8]) -> [u8; 16] {
    let mut state = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];
    let mut chunks = bytes.chunks_exact(64);
    for chunk in &mut chunks {
        block(&mut state, chunk);
    }
    let remaining = chunks.remainder();
    let mut padding = [0u8; 128];
    padding[..remaining.len()].copy_from_slice(remaining);
    padding[remaining.len()] = 0x80;
    let size = if remaining.len() < 56 { 64 } else { 128 };
    padding[size - 8..size].copy_from_slice(&(bytes.len() as u64).wrapping_mul(8).to_le_bytes());
    for chunk in padding[..size].chunks_exact(64) {
        block(&mut state, chunk);
    }
    let mut digest = [0; 16];
    for (word, slot) in state.iter().zip(digest.chunks_exact_mut(4)) {
        slot.copy_from_slice(&word.to_le_bytes());
    }
    digest
}
fn block(state: &mut [u32; 4], bytes: &[u8]) {
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];
    const SHIFT: [[u32; 4]; 4] = [
        [7, 12, 17, 22],
        [5, 9, 14, 20],
        [4, 11, 16, 23],
        [6, 10, 15, 21],
    ];
    let words: Vec<_> = bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("four-byte MD5 word")))
        .collect();
    let [mut a, mut b, mut c, mut d] = *state;
    for (i, constant) in K.into_iter().enumerate() {
        let (f, index) = match i / 16 {
            0 => ((b & c) | (!b & d), i),
            1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
            2 => (b ^ c ^ d, (3 * i + 5) % 16),
            _ => (c ^ (b | !d), (7 * i) % 16),
        };
        let next = b.wrapping_add(
            a.wrapping_add(f)
                .wrapping_add(constant)
                .wrapping_add(words[index])
                .rotate_left(SHIFT[i / 16][i % 4]),
        );
        (a, b, c, d) = (d, next, b, c);
    }
    for (slot, value) in state.iter_mut().zip([a, b, c, d]) {
        *slot = slot.wrapping_add(value);
    }
}

fn proper_list(arena: &ErlangTerms, mut id: TermId) -> Result<Vec<TermId>, String> {
    let mut items = Vec::new();
    loop {
        match arena
            .terms
            .get(id.0)
            .ok_or("form list outside term arena")?
        {
            ErlangTerm::Nil => return Ok(items),
            ErlangTerm::List { items: part, tail } => {
                items.extend(part);
                id = *tail;
            }
            _ => return Err("compiler forms require a proper list".into()),
        }
    }
}
pub fn stabilized_forms(arena: &ErlangTerms) -> Result<ErlangTerms, String> {
    let forms = proper_list(arena, arena.root)?;
    let mut out = arena.clone();
    let mut stable = Vec::with_capacity(forms.len());
    for form in forms {
        let fields = match &arena.terms[form.0] {
            ErlangTerm::Tuple(fields)
                if fields.len() == 4
                    && matches!(&arena.terms[fields[0].0],ErlangTerm::Atom(tag) if tag=="attribute")
                    && matches!(&arena.terms[fields[2].0],ErlangTerm::Atom(tag) if tag=="file") =>
            {
                Some(fields)
            }
            _ => None,
        };
        let Some(fields) = fields else {
            stable.push(form);
            continue;
        };
        let ErlangTerm::Tuple(file) = &arena.terms[fields[3].0] else {
            return Err("file attribute requires a path/line pair".into());
        };
        if file.len() != 2 {
            return Err("file attribute requires a path/line pair".into());
        }
        let path = match &arena.terms[file[0].0] {
            ErlangTerm::Binary(bytes) => {
                let path = bytes.strip_suffix(b"/").unwrap_or(bytes);
                ErlangTerm::Binary(
                    path.rsplit(|byte| *byte == b'/')
                        .next()
                        .unwrap_or_default()
                        .to_vec(),
                )
            }
            _ => {
                let mut chars = proper_list(arena, file[0])?;
                let slash = |id: TermId| matches!(&arena.terms[id.0],ErlangTerm::Integer(value) if value=="47");
                if chars.last().is_some_and(|id| slash(*id)) {
                    chars.pop();
                }
                let start = chars
                    .iter()
                    .rposition(|id| slash(*id))
                    .map_or(0, |index| index + 1);
                let tail = TermId(out.terms.len());
                out.terms.push(ErlangTerm::Nil);
                ErlangTerm::List {
                    items: chars[start..].to_vec(),
                    tail,
                }
            }
        };
        let path_id = TermId(out.terms.len());
        out.terms.push(path);
        let file_id = TermId(out.terms.len());
        out.terms.push(ErlangTerm::Tuple(vec![path_id, file[1]]));
        let form_id = TermId(out.terms.len());
        out.terms.push(ErlangTerm::Tuple(vec![
            fields[0], fields[1], fields[2], file_id,
        ]));
        stable.push(form_id);
    }
    let tail = TermId(out.terms.len());
    out.terms.push(ErlangTerm::Nil);
    out.root = TermId(out.terms.len());
    out.terms.push(ErlangTerm::List {
        items: stable,
        tail,
    });
    Ok(out)
}
pub fn module_fingerprint(
    forms: &ErlangTerms,
    map_order: crate::core::TermMapOrder,
) -> Result<[u8; 16], String> {
    let stable = stabilized_forms(forms)?;
    let map_encoding = match map_order {
        crate::core::TermMapOrder::NativeToListWithIterator => {
            external_term::MapEncoding::PreserveIteration
        }
    };
    let bytes = external_term::encode_with_map_encoding(
        &stable,
        external_term::Limits::default(),
        false,
        map_encoding,
    )
    .map_err(|e| e.to_string())?;
    Ok(md5(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_md5_vectors_cover_padding_and_multiple_blocks() {
        for (input, expected) in [
            ("", "d41d8cd98f00b204e9800998ecf8427e"),
            ("a", "0cc175b9c0f1b6a831c399e269772661"),
            ("abc", "900150983cd24fb0d6963f7d28e17f72"),
            ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
                "d174ab98d277d9f5a5611c2c9f419d9f",
            ),
            (
                "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ] {
            let got = md5(input.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert_eq!(got, expected);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClassicFingerprintError {
    InvalidBeam,
    Transport(external_term::Error),
}
pub fn classic_beam_fingerprint(
    bytes: &[u8],
    max_expanded_bytes: usize,
) -> Result<[u8; 16], ClassicFingerprintError> {
    use ClassicFingerprintError as E;
    let expanded;
    let bytes = if bytes.starts_with(&[0x1f, 0x8b]) {
        expanded = external_term::zlib::gunzip(bytes, max_expanded_bytes).map_err(E::Transport)?;
        expanded.as_slice()
    } else {
        bytes
    };
    if bytes.len() < 12 || &bytes[..4] != b"FOR1" || &bytes[8..12] != b"BEAM" {
        return Err(E::InvalidBeam);
    }
    let mut cursor = 12usize;
    let mut chunks = Vec::new();
    let mut has_atoms = false;
    while cursor < bytes.len() {
        let head = bytes.get(cursor..cursor + 8).ok_or(E::InvalidBeam)?;
        let length = u32::from_be_bytes(head[4..8].try_into().unwrap()) as usize;
        let start = cursor + 8;
        let end = start.checked_add(length).ok_or(E::InvalidBeam)?;
        let data = bytes.get(start..end).ok_or(E::InvalidBeam)?;
        let name: [u8; 4] = head[..4].try_into().unwrap();
        if name == *b"Atom" || name == *b"AtU8" {
            validate_module_atom(data, name == *b"AtU8")?;
            has_atoms = true;
        }
        if name != *b"CInf" && name != *b"Docs" {
            chunks.push((name, data));
        }
        cursor = end
            .checked_add((4 - length % 4) % 4)
            .ok_or(E::InvalidBeam)?;
    }
    if !has_atoms {
        return Err(E::InvalidBeam);
    }
    chunks.sort_unstable();
    let mut input = Vec::new();
    for (name, data) in chunks {
        input.extend(name);
        input.extend(data);
    }
    Ok(md5(&input))
}

fn validate_module_atom(data: &[u8], utf8: bool) -> Result<(), ClassicFingerprintError> {
    use ClassicFingerprintError::InvalidBeam;
    let count = i32::from_be_bytes(data.get(..4).ok_or(InvalidBeam)?.try_into().unwrap());
    let rest = &data[4..];
    let first = *rest.first().ok_or(InvalidBeam)?;
    let (length, offset) = if utf8 && count < 0 {
        if first & 8 == 0 {
            ((first >> 4) as usize, 1)
        } else if first & 16 == 0 {
            (
                (((first >> 5) as usize) << 8) | *rest.get(1).ok_or(InvalidBeam)? as usize,
                2,
            )
        } else {
            return Err(InvalidBeam);
        }
    } else {
        (first as usize, 1)
    };
    let atom = rest.get(offset..offset + length).ok_or(InvalidBeam)?;
    if utf8 {
        let text = std::str::from_utf8(atom).map_err(|_| InvalidBeam)?;
        if text.chars().count() > 255 {
            return Err(InvalidBeam);
        }
    }
    Ok(())
}

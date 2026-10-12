use super::term::Arena;
use crate::{
    core::{ErlangTerms, TermId},
    external_term::{self, Limits},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Elixir,
    Erlang,
}
impl Backend {
    pub fn name(self) -> &'static str {
        match self {
            Self::Elixir => "elixir_erl",
            Self::Erlang => "erl_abstract_code",
        }
    }
}
#[derive(Clone, Debug)]
pub struct DebugModule {
    pub module: String,
    pub backend: Backend,
    pub terms: ErlangTerms,
    pub compiler_options: TermId,
    pub chunk_count: usize,
}

fn u32_at(bytes: &[u8], at: usize) -> Result<usize, String> {
    let raw: [u8; 4] = bytes
        .get(at..at + 4)
        .ok_or("BEAM truncated length")?
        .try_into()
        .unwrap();
    Ok(u32::from_be_bytes(raw) as usize)
}
/// Validated borrowed BEAM chunks shared by debug and runtime-profile readers.
pub fn chunks(bytes: &[u8], limits: Limits) -> Result<Vec<([u8; 4], &[u8])>, String> {
    if bytes.len() > limits.bytes {
        return Err("BEAM resource limit: bytes".into());
    }
    if bytes.get(..4) != Some(b"FOR1") || bytes.get(8..12) != Some(b"BEAM") {
        return Err("BEAM invalid FOR1/BEAM header".into());
    }
    if u32_at(bytes, 4)?.checked_add(8) != Some(bytes.len()) {
        return Err("BEAM container length mismatch".into());
    }
    let mut at = 12;
    let mut chunks = Vec::new();
    while at < bytes.len() {
        let id = bytes.get(at..at + 4).ok_or("BEAM truncated chunk ID")?;
        let size = u32_at(bytes, at + 4)?;
        let start = at.checked_add(8).ok_or("BEAM chunk overflow")?;
        let end = start.checked_add(size).ok_or("BEAM chunk overflow")?;
        let data = bytes.get(start..end).ok_or("BEAM truncated chunk")?;
        let pad = (4 - size % 4) % 4;
        at = end.checked_add(pad).ok_or("BEAM chunk overflow")?;
        if at > bytes.len() {
            return Err("BEAM missing chunk padding".into());
        }
        chunks.push((id.try_into().unwrap(), data));
    }
    Ok(chunks)
}
pub fn read_beam(bytes: &[u8], limits: Limits) -> Result<DebugModule, String> {
    let unpacked;
    let bytes = if bytes.starts_with(&[0x1f, 0x8b]) {
        unpacked = external_term::zlib::gunzip(bytes, limits.bytes)
            .map_err(|e| format!("compressed BEAM {e}"))?;
        unpacked.as_slice()
    } else {
        bytes
    };
    let chunks = chunks(bytes, limits)?;
    let count = chunks.len();
    let mut debug = None;
    let mut abst = None;
    for (id, data) in chunks {
        if &id == b"Dbgi" && debug.replace(data).is_some() {
            return Err("BEAM duplicate Dbgi".into());
        }
        if &id == b"Abst" && abst.replace(data).is_some() {
            return Err("BEAM duplicate Abst".into());
        }
    }
    if let Some(debug) = debug {
        let terms =
            external_term::decode_interned(debug, limits).map_err(|e| format!("Dbgi {e}"))?;
        decode_debug(terms, count)
    } else if let Some(abst) = abst {
        let mut terms =
            external_term::decode_interned(abst, limits).map_err(|e| format!("Abst {e}"))?;
        let mut a = Arena::from_terms(&terms);
        let fields = a.tuple_fields(terms.root)?.to_vec();
        if fields.len() != 2 || a.as_atom(fields[0]) != Some("raw_abstract_v1") {
            return Err("unsupported legacy Abst schema".into());
        }
        let empty = a.nil();
        let metadata = a.tuple(vec![fields[1], empty]);
        let version = a.atom("debug_info_v1");
        let backend = a.atom("erl_abstract_code");
        let root = a.tuple(vec![version, backend, metadata]);
        terms = a.finish(root);
        decode_debug(terms, count)
    } else {
        Err("BEAM missing debug information; bytecode decompilation is not this frontend's input contract".into())
    }
}
fn decode_debug(terms: ErlangTerms, chunk_count: usize) -> Result<DebugModule, String> {
    let a = Arena::from_terms(&terms);
    let root = a.tuple_fields(terms.root)?;
    if root.len() != 3 || a.as_atom(root[0]) != Some("debug_info_v1") {
        return Err("unsupported debug-info version".into());
    }
    let backend = match a.as_atom(root[1]) {
        Some("elixir_erl") => Backend::Elixir,
        Some("erl_abstract_code") => Backend::Erlang,
        other => return Err(format!("unsupported debug backend {other:?}")),
    };
    let data = a.tuple_fields(root[2])?;
    let (module, compiler_options) = match backend {
        Backend::Elixir => {
            if data.len() != 3 || a.as_atom(data[0]) != Some("elixir_v1") {
                return Err("unsupported Elixir metadata version".into());
            }
            let name = a
                .map_get(data[1], "module")
                .and_then(|x| a.as_atom(x))
                .ok_or("Elixir metadata missing module identity")?;
            let options = a
                .map_get(data[1], "compile_opts")
                .ok_or("Elixir metadata missing compile_opts")?;
            (name.to_owned(), options)
        }
        Backend::Erlang => {
            if data.len() != 2 {
                return Err("invalid Erlang debug metadata".into());
            }
            let mut module = None;
            for form in a.list_items(data[0])? {
                if let Ok(f) = a.tuple_fields(form) {
                    if f.len() == 4
                        && a.as_atom(f[0]) == Some("attribute")
                        && a.as_atom(f[2]) == Some("module")
                    {
                        module = a.as_atom(f[3]).map(str::to_owned);
                    }
                }
            }
            (
                module.ok_or("Erlang forms missing module identity")?,
                data[1],
            )
        }
    };
    for option in a.list_items(compiler_options)? {
        if let Ok(fields) = a.tuple_fields(option) {
            if fields.first().and_then(|x| a.as_atom(*x)) == Some("parse_transform") {
                return Err(format!(
                    "module {module}: unsupported parse_transform option"
                ));
            }
        }
    }
    Ok(DebugModule {
        module,
        backend,
        terms,
        compiler_options,
        chunk_count,
    })
}
pub fn erlang_forms(debug: &DebugModule) -> Result<ErlangTerms, String> {
    if debug.backend != Backend::Erlang {
        return Err("expected Erlang debug backend".into());
    }
    let a = Arena::from_terms(&debug.terms);
    let metadata = a.tuple_fields(debug.terms.root)?[2];
    let root = a.tuple_fields(metadata)?[0];
    Ok(ErlangTerms {
        root,
        terms: debug.terms.terms.clone(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn wrap(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut b = b"FOR1\0\0\0\0BEAM".to_vec();
        for (id, data) in chunks {
            b.extend(*id);
            b.extend((data.len() as u32).to_be_bytes());
            b.extend(data);
            b.resize(b.len() + (4 - data.len() % 4) % 4, 0);
        }
        let n = (b.len() - 8) as u32;
        b[4..8].copy_from_slice(&n.to_be_bytes());
        b
    }
    #[test]
    fn checked_chunk_framing() {
        for n in 0..12 {
            assert!(read_beam(&b"FOR1\0\0\0\x04BEAM"[..n], Limits::default()).is_err());
        }
        let b = wrap(&[(b"Dbgi", vec![131, 106]), (b"Dbgi", vec![131, 106])]);
        assert!(
            read_beam(&b, Limits::default())
                .unwrap_err()
                .contains("duplicate")
        );
        let mut b = wrap(&[]);
        b.push(0);
        assert!(
            read_beam(&b, Limits::default())
                .unwrap_err()
                .contains("length")
        );
    }
}

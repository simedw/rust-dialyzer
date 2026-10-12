//! Native runtime context, separate from exact compiler-process identity capture.
//! Production builders read bytes only; oracle context import is explicit.
use crate::{
    constraints::closure_order::ClosureCodeProfile,
    core::{CoreModule, CoreProgram, ErlangTerm},
    external_term::Limits,
    native_runtime_hash::NativeAtomLayout,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityOrigin {
    ImportedContext,
    DeterministicNativeScope,
}

pub struct RuntimeContext {
    pub context: Value,
    pub closure_profile: ClosureCodeProfile,
    pub identity_origin: IdentityOrigin,
}
impl RuntimeContext {
    /// Import a captured compiler context for differential research. This does
    /// not reconstruct its transient atom identities from BEAM metadata.
    pub fn import(context: Value, closure_profile: &Value) -> Result<Self, String> {
        CoreProgram::from_modules(&context, vec![])?;
        NativeAtomLayout::parse(&context["atom_layout"]).map_err(|e| e.to_string())?;
        let closure_profile = ClosureCodeProfile::parse(closure_profile)?;
        if closure_profile.module_name() != "dialyzer_typesig" {
            return Err("expected original dialyzer_typesig closure profile".into());
        }
        Ok(Self {
            context,
            closure_profile,
            identity_origin: IdentityOrigin::ImportedContext,
        })
    }
    /// Build a fresh native scope from the complete workload atom set. All
    /// atom-dependent tables must use this layout; do not mix captured tables.
    pub fn native(
        names: impl IntoIterator<Item = String>,
        typesig_beam: &[u8],
        contracts_beam: &[u8],
        limits: Limits,
    ) -> Result<Self, String> {
        let typesig = fun_profile(typesig_beam, limits)?;
        let contracts = fun_profile(contracts_beam, limits)?;
        if typesig["original_sha256"]
            != "08945a69dd63a44ce877d696e1c27be2753de29ae6fc1f8e183d37ab0add9882"
            || contracts["original_sha256"]
                != "75cf94ef5951b50537e90eab8cb718ec64cbefb61bdca071b35851abbd1c27cb"
        {
            return Err("native context compiler profile mismatch: require pinned OTP28.3 dialyzer5.4 original BEAMs".into());
        }
        if typesig["module"] != "dialyzer_typesig" || contracts["module"] != "dialyzer_contracts" {
            return Err(
                "native context requires original dialyzer_typesig and dialyzer_contracts BEAMs"
                    .into(),
            );
        }
        let mut names: BTreeSet<String> = names.into_iter().collect();
        names.extend(["top", "external", "in", "out", "$e", "$v"].map(str::to_owned));
        for profile in [&typesig, &contracts] {
            names.insert(profile["module"].as_str().unwrap().to_owned());
            for row in profile["fun_table"].as_array().unwrap() {
                names.insert(row["name"].as_str().unwrap().to_owned());
            }
        }
        let mut framed = b"native-atoms-v1\0".to_vec();
        for name in &names {
            framed.extend_from_slice(&(name.len() as u64).to_be_bytes());
            framed.extend_from_slice(name.as_bytes());
        }
        let layout = NativeAtomLayout::deterministic_native(
            format!("native-atoms-v1:{}", sha256_hex(&framed)),
            names,
        )
        .map_err(|e| e.to_string())?;
        let context = json!({"version":6,"profile":pinned_profile(),
            "atom_layout":layout.to_wire(),"contract_closure_profile":contracts});
        CoreProgram::from_modules(&context, vec![])?;
        Ok(Self {
            context,
            closure_profile: ClosureCodeProfile::parse(&typesig)?,
            identity_origin: IdentityOrigin::DeterministicNativeScope,
        })
    }
}

pub fn module_atoms(modules: &[CoreModule]) -> BTreeSet<String> {
    let mut atoms = BTreeSet::new();
    for module in modules {
        atoms.insert(module.name.clone());
        for term in &module.terms {
            match term {
                ErlangTerm::Atom(name) => {
                    atoms.insert(name.clone());
                }
                ErlangTerm::ExternalFun {
                    module, function, ..
                } => {
                    atoms.insert(module.clone());
                    atoms.insert(function.clone());
                }
                _ => {}
            }
        }
    }
    atoms
}

/// This frontend implements this profile, not whichever compiler is installed.
pub fn pinned_profile() -> Value {
    json!({"otp":"28","erts":"16.2","elixir":"1.19.4","dialyzer":"5.4",
        "term_map_order":"native_to_list_with_iterator",
        "core_options":["no_copt","to_core","binary","return_errors","no_inline",
            "strict_record_tests","dialyzer","no_spawn_compiler_process"]})
}

fn word(bytes: &[u8], at: usize) -> Result<u32, String> {
    let raw = bytes
        .get(at..at.checked_add(4).ok_or("BEAM word overflow")?)
        .ok_or("BEAM truncated word")?;
    Ok(u32::from_be_bytes(raw.try_into().unwrap()))
}

/// Parse atom names and FunT without loading code or executing its functions.
/// The digest covers original BEAM bytes, as in CoreCache.fun_profile/1.
pub fn fun_profile(bytes: &[u8], limits: Limits) -> Result<Value, String> {
    let chunks = super::debug::chunks(bytes, limits)?;
    let mut selected: BTreeMap<[u8; 4], &[u8]> = BTreeMap::new();
    for (id, data) in chunks {
        if matches!(&id, b"AtU8" | b"Atom" | b"FunT") && selected.insert(id, data).is_some() {
            return Err(format!("BEAM duplicate {}", String::from_utf8_lossy(&id)));
        }
    }
    if selected.contains_key(b"AtU8") && selected.contains_key(b"Atom") {
        return Err("BEAM ambiguous atom table".into());
    }
    let utf8 = selected.contains_key(b"AtU8");
    let table = selected
        .get(if utf8 { b"AtU8" } else { b"Atom" })
        .ok_or("BEAM missing atom table")?;
    let raw_count = word(table, 0)? as i32;
    let long_atoms = utf8 && raw_count < 0;
    let count = if long_atoms {
        raw_count.checked_neg().ok_or("BEAM atom count overflow")? as usize
    } else {
        usize::try_from(raw_count).map_err(|_| "BEAM negative legacy atom count")?
    };
    if count > limits.nodes {
        return Err("BEAM atom count resource limit".into());
    }
    let mut at = 4usize;
    let mut atoms = Vec::with_capacity(count);
    for _ in 0..count {
        let first = *table.get(at).ok_or("BEAM truncated atom length")?;
        at += 1;
        let length = if !long_atoms {
            usize::from(first)
        } else if first & 8 == 0 {
            usize::from(first >> 4)
        } else if first & 16 == 0 {
            let low = *table.get(at).ok_or("BEAM truncated long atom length")?;
            at += 1;
            (usize::from(first >> 5) << 8) | usize::from(low)
        } else {
            return Err("BEAM unsupported long atom length encoding".into());
        };
        let name = table
            .get(at..at + length)
            .ok_or("BEAM truncated atom name")?;
        atoms.push(if utf8 {
            std::str::from_utf8(name)
                .map_err(|_| "BEAM invalid UTF8 atom")?
                .to_owned()
        } else {
            name.iter().map(|b| char::from(*b)).collect()
        });
        if atoms.last().unwrap().chars().count() > 255 {
            return Err("BEAM atom exceeds 255 Unicode characters".into());
        }
        at += length;
    }
    if at != table.len() {
        return Err("BEAM trailing atom table bytes".into());
    }
    let module = atoms.first().ok_or("BEAM empty atom table")?;
    let funs = selected.get(b"FunT");
    let mut rows = Vec::new();
    if let Some(funs) = funs {
        let count = word(funs, 0)? as usize;
        if count > limits.nodes {
            return Err("BEAM FunT count resource limit".into());
        }
        if count.checked_mul(24).and_then(|n| n.checked_add(4)) != Some(funs.len()) {
            return Err("BEAM FunT length mismatch".into());
        }
        for row in 0..count {
            let at = 4 + row * 24;
            let name = word(funs, at)? as usize;
            let name = name
                .checked_sub(1)
                .and_then(|n| atoms.get(n))
                .ok_or("BEAM FunT atom index out of range")?;
            let arity = word(funs, at + 4)?;
            let free = word(funs, at + 16)?;
            if arity < free {
                return Err("BEAM FunT arity smaller than captures".into());
            }
            rows.push(json!({"name":name,"arity":arity,"label":word(funs,at+8)?,
                "index":word(funs,at+12)?,"env_count":free,"uniq":word(funs,at+20)?,"new_index":row}));
        }
    }
    let profile = json!({"module":module,"original_sha256":sha256_hex(bytes),"fun_table":rows});
    ClosureCodeProfile::parse(&profile)?;
    Ok(profile)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h = [
        0x6a09e667u32,
        0xbb67ae85,
        0x3c6ef372,
        0xa54ff53a,
        0x510e527f,
        0x9b05688c,
        0x1f83d9ab,
        0x5be0cd19,
    ];
    let tail_len = bytes.len() % 64;
    let mut tail = [0u8; 128];
    tail[..tail_len].copy_from_slice(&bytes[bytes.len() - tail_len..]);
    tail[tail_len] = 0x80;
    let padded = if tail_len < 56 { 64 } else { 128 };
    tail[padded - 8..padded].copy_from_slice(&(bytes.len() as u64).wrapping_mul(8).to_be_bytes());
    for block in bytes[..bytes.len() - tail_len]
        .chunks_exact(64)
        .chain(tail[..padded].chunks_exact(64))
    {
        let mut w = [0u32; 64];
        for (i, raw) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes(raw.try_into().unwrap());
        }
        for i in 16..64 {
            let a = w[i - 15];
            let b = w[i - 2];
            let s0 = a.rotate_right(7) ^ a.rotate_right(18) ^ (a >> 3);
            let s1 = b.rotate_right(17) ^ b.rotate_right(19) ^ (b >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut z] = h;
        for i in 0..64 {
            let t1 = z
                .wrapping_add(e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25))
                .wrapping_add((e & f) ^ (!e & g))
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let t2 = (a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22))
                .wrapping_add((a & b) ^ (a & c) ^ (b & c));
            z = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in h.iter_mut().zip([a, b, c, d, e, f, g, z]) {
            *slot = slot.wrapping_add(value);
        }
    }
    h.iter().map(|n| format!("{n:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_runtime_hash::NativeHashKey;
    const TYPESIG: &[u8] =
        include_bytes!("../../tests/fixtures/beam_core_port/runtime_context/dialyzer_typesig.beam");
    const CONTRACTS: &[u8] = include_bytes!(
        "../../tests/fixtures/beam_core_port/runtime_context/dialyzer_contracts.beam"
    );
    fn fixture(bytes: &[u8], oracle: &str) {
        let actual = fun_profile(bytes, Limits::default()).unwrap();
        let mut expected: Value = serde_json::from_str(oracle).unwrap();
        expected["original_sha256"] = json!(
            expected["original_sha256"]
                .as_str()
                .unwrap()
                .to_ascii_lowercase()
        );
        assert_eq!(actual, expected);
    }
    #[test]
    fn original_funt_matches_independent_otp_oracle() {
        fixture(
            TYPESIG,
            include_str!(
                "../../tests/fixtures/beam_core_port/runtime_context/dialyzer_typesig.json"
            ),
        );
        fixture(
            CONTRACTS,
            include_str!(
                "../../tests/fixtures/beam_core_port/runtime_context/dialyzer_contracts.json"
            ),
        );
    }
    #[test]
    fn sha256_standard_vectors_cover_both_padding_blocks() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(
            sha256_hex(&vec![b'a'; 1_000_000]),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }
    #[test]
    fn native_context_is_deterministic_and_scoped() {
        let names = (0..70).map(|n| format!("key{n:02}")).collect::<Vec<_>>();
        let a =
            RuntimeContext::native(names.clone(), TYPESIG, CONTRACTS, Limits::default()).unwrap();
        let b = RuntimeContext::native(
            names.iter().rev().cloned(),
            TYPESIG,
            CONTRACTS,
            Limits::default(),
        )
        .unwrap();
        assert_eq!(a.identity_origin, IdentityOrigin::DeterministicNativeScope);
        assert_eq!(a.context, b.context);
        let layout = NativeAtomLayout::parse(&a.context["atom_layout"]).unwrap();
        let layout_b = NativeAtomLayout::parse(&b.context["atom_layout"]).unwrap();
        assert!(layout.scope.starts_with("native-atoms-v1:"));
        let keys = names
            .iter()
            .cloned()
            .map(NativeHashKey::Atom)
            .collect::<Vec<_>>();
        let ordered = crate::native_map_order::ordered_keys(&layout, keys.clone()).unwrap();
        assert_eq!(
            ordered,
            crate::native_map_order::ordered_keys(&layout_b, keys.iter().rev().cloned()).unwrap()
        );
        let small = keys[..10].to_vec();
        assert_eq!(
            small,
            crate::native_map_order::ordered_keys(&layout, small.iter().rev().cloned()).unwrap()
        );
        let c = RuntimeContext::native(
            names.into_iter().chain(["extra".into()]),
            TYPESIG,
            CONTRACTS,
            Limits::default(),
        )
        .unwrap();
        assert_ne!(
            a.context["atom_layout"]["scope"],
            c.context["atom_layout"]["scope"]
        );
        let profile = fun_profile(TYPESIG, Limits::default()).unwrap();
        let imported = RuntimeContext::import(a.context.clone(), &profile).unwrap();
        assert_eq!(imported.identity_origin, IdentityOrigin::ImportedContext);
        // Importing explicitly preserves the supplied namespace, even when it
        // was originally created natively. Identity origin describes this action.
        assert_eq!(imported.context, a.context);
    }
    #[test]
    fn context_rejects_unpinned_beams_and_funt_rejects_bad_rows() {
        let mut modified = TYPESIG.to_vec();
        let last = modified.len() - 1;
        modified[last] ^= 1;
        assert!(
            RuntimeContext::native([], &modified, CONTRACTS, Limits::default())
                .err()
                .unwrap()
                .contains("profile mismatch")
        );
        let mut bytes = b"FOR1\0\0\0\0BEAM".to_vec();
        fn chunk(bytes: &mut Vec<u8>, name: &[u8], data: &[u8]) {
            bytes.extend_from_slice(name);
            bytes.extend_from_slice(&(data.len() as u32).to_be_bytes());
            bytes.extend_from_slice(data);
            while bytes.len() % 4 != 0 {
                bytes.push(0);
            }
        }
        chunk(&mut bytes, b"AtU8", &[0, 0, 0, 1, 1, b'm']);
        let mut rows = vec![0, 0, 0, 1];
        for value in [2u32, 0, 1, 0, 0, 0] {
            rows.extend_from_slice(&value.to_be_bytes());
        }
        chunk(&mut bytes, b"FunT", &rows);
        let length = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&length.to_be_bytes());
        assert!(
            fun_profile(&bytes, Limits::default())
                .unwrap_err()
                .contains("atom index")
        );
        bytes.pop();
        assert!(fun_profile(&bytes, Limits::default()).is_err());
    }
}

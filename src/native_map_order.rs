use crate::native_runtime_hash::{NativeAtomLayout, NativeHashKey, RuntimeHashError};
use std::cmp::Ordering;

pub fn ordered_keys(
    layout: &NativeAtomLayout,
    keys: impl IntoIterator<Item = NativeHashKey>,
) -> Result<Vec<NativeHashKey>, RuntimeHashError> {
    let mut keys = keys
        .into_iter()
        .map(|key| {
            let literal = key.literal();
            let arena = crate::term_order::TermOrderArena::new(&literal)
                .map_err(RuntimeHashError::Boundary)?;
            Ok((key, literal.root, arena))
        })
        .collect::<Result<Vec<_>, RuntimeHashError>>()?;
    keys.sort_by(|(_, a, aa), (_, b, bb)| {
        aa.compare_map_keys(*a, bb, *b)
            .expect("validated literal keys")
    });
    keys.dedup_by(|(_, a, aa), (_, b, bb)| {
        aa.compare_map_keys(*a, bb, *b)
            .expect("validated literal keys")
            .is_eq()
    });
    let keys = keys.into_iter().map(|(key, _, _)| key).collect::<Vec<_>>();
    // Validate every atom before sorting so the comparator cannot hide missing metadata.
    let hashes = keys
        .iter()
        .map(|key| layout.internal_hash(key))
        .collect::<Result<Vec<_>, _>>()?;
    if keys.len() <= 32 {
        let mut flat = keys
            .into_iter()
            .map(|key| {
                let atom = match &key {
                    NativeHashKey::Atom(name) => Some(layout.atom_identity(name)?),
                    _ => None,
                };
                Ok((key, atom))
            })
            .collect::<Result<Vec<_>, RuntimeHashError>>()?;
        flat.sort_by(|(a, ia), (b, ib)| match (ia, ib) {
            (Some(a), Some(b)) => a.cmp(b),
            _ => term_order(a, b),
        });
        return Ok(flat.into_iter().map(|(key, _)| key).collect());
    }
    let mut pairs = keys.into_iter().zip(hashes).collect::<Vec<_>>();
    pairs.sort_by(|(a, ha), (b, hb)| {
        swizzle(*ha)
            .cmp(&swizzle(*hb))
            .then_with(|| term_order(a, b))
    });
    // maps:to_list prepends each ascending HAMT traversal leaf, including collision leaves.
    Ok(pairs.into_iter().rev().map(|(key, _)| key).collect())
}
fn swizzle(hash: u64) -> u64 {
    let swapped = hash.swap_bytes();
    (swapped & 0xf0f0_f0f0_f0f0_f0f0) >> 4 | (swapped & 0x0f0f_0f0f_0f0f_0f0f) << 4
}
pub(crate) fn term_order(a: &NativeHashKey, b: &NativeHashKey) -> Ordering {
    let a = a.literal();
    let b = b.literal();
    crate::term_order::compare_map_keys(&a, a.root, &b, b.root)
        .expect("validated native hash key literals")
}

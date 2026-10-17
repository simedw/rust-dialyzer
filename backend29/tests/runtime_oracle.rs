use rust_dialyzer_poc::{external_term::{self,Limits},native_form_environment::RawFormTerm,
    native_runtime_hash::{NativeAtomLayout,NativeHashKey},native_dict,
    portable_term_hash,term_order,type_exports};
use std::{cmp::Ordering,sync::Arc};

#[test]
fn otp29_literal_transport_order_and_hashes_match_the_runtime() {
    let data: serde_json::Value = serde_json::from_str(include_str!("fixtures/otp29/runtime_oracle.json")).unwrap();
    let layout = NativeAtomLayout::parse(&data["layout"]).unwrap();
    let mut arenas = Vec::new();
    for (index,row) in data["rows"].as_array().unwrap().iter().enumerate() {
        let bytes: Vec<u8> = serde_json::from_value(row["etf"].clone()).unwrap();
        let arena = Arc::new(external_term::decode(&bytes,Limits::default()).unwrap());
        let key = NativeHashKey::from_raw(&RawFormTerm::new(arena.clone(),arena.root).unwrap()).unwrap();
        assert_eq!(layout.internal_hash(&key).unwrap().to_string(),row["internal_hash"].as_str().unwrap(),"internal hash {index}");
        assert_eq!(native_dict::pinned_hash_key_with_layout(&key,&layout).unwrap().wrapping_add(1),row["phash"].as_u64().unwrap() as u32,"phash {index}");
        assert_eq!(portable_term_hash::hash_with_layout(&key,&layout).unwrap(),row["phash2"].as_u64().unwrap() as u32,"phash2 {index}");
        assert_eq!(external_term::encode_with_map_encoding(&arena,Limits::default(),false,external_term::MapEncoding::PreserveIteration).unwrap(),bytes,"ETF {index}");
        let compact = type_exports::compact_arena(&arena);
        let restored = rust_dialyzer_poc::core::ErlangTerms::parse(&type_exports::arena_to_value(&compact.terms,compact.root)).unwrap();
        assert_eq!(term_order::compare_map_keys(&arena,arena.root,&restored,restored.root).unwrap(),Ordering::Equal,"compact {index}");
        arenas.push(arena);
    }
    let text = |order| match order { Ordering::Less => "less",Ordering::Equal => "equal",Ordering::Greater => "greater" };
    for (index,(a,b)) in arenas.iter().flat_map(|a| arenas.iter().map(move |b| (a,b))).enumerate() {
        assert_eq!(text(term_order::compare_terms(a,a.root,b,b.root).unwrap()),data["comparisons"][index][0],"normal order {index}");
        assert_eq!(text(term_order::compare_map_keys(a,a.root,b,b.root).unwrap()),data["comparisons"][index][1],"exact order {index}");
    }
}

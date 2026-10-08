use rust_dialyzer_poc::{
    core::ErlangTerms, native_dict::pinned_hash_key, native_form_environment::RawFormTerm,
    native_runtime_hash::NativeHashKey, portable_term_hash,
};
use std::{
    io::{self, BufRead},
    sync::Arc,
};
fn main() -> Result<(), String> {
    for line in io::stdin().lock().lines() {
        let row: serde_json::Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let arena = Arc::new(ErlangTerms::parse(&row["key"])?);
        let raw = RawFormTerm::new(arena.clone(), arena.root)?;
        let key = NativeHashKey::from_raw(&raw).map_err(|e| e.to_string())?;
        let hash = pinned_hash_key(&key).map_err(|e| e.to_string())?;
        let hash2 = portable_term_hash::hash(&key);
        println!(
            "{}",
            serde_json::json!({"id":row["id"],"hash":hash,"hash2":hash2,"exact":row["expected"]==hash && row["phash2"]==hash2})
        );
    }
    Ok(())
}

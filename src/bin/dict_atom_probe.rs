use rust_dialyzer_poc::native_dict::{
    NativeDictOrder, PinnedPhashKey, pinned_hash_atom, pinned_hash_integer,
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
#[derive(Clone, PartialEq, Eq)]
enum Caller {
    Label(u64),
    Atom(String),
}
impl PinnedPhashKey for Caller {
    fn pinned_phash_hash(&self) -> u32 {
        match self {
            Self::Label(n) => pinned_hash_integer(*n),
            Self::Atom(s) => pinned_hash_atom(s),
        }
    }
}
fn main() {
    for line in io::stdin().lock().lines() {
        let row: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let mut dict = NativeDictOrder::default();
        for v in row["insert"].as_array().unwrap() {
            dict.store(if let Some(n) = v.as_u64() {
                Caller::Label(n)
            } else {
                Caller::Atom(v.as_str().unwrap().to_owned())
            });
        }
        println!(
            "{}",
            json!({"id":row["id"],"dict":dict.keys().into_iter().map(|k|match k{Caller::Label(n)=>json!(n),Caller::Atom(s)=>json!(s)}).collect::<Vec<_>>(),"fold":dict.fold_keys().into_iter().map(|k|match k{Caller::Label(n)=>json!(n),Caller::Atom(s)=>json!(s)}).collect::<Vec<_>>(),"slots":dict.slots()})
        );
    }
}

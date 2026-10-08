use rust_dialyzer_poc::{
    core::{ErlangTerm, ErlangTerms},
    incremental_warning_cache::{CacheError, CachedWarnings},
    native_form_environment::RawFormTerm,
    native_runtime_hash::NativeAtomLayout,
    raw_literal::RawLiteral,
    term_order::compare_map_keys,
};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead},
    sync::Arc,
};
fn raw(v: &Value) -> Result<RawFormTerm, String> {
    let a = Arc::new(ErlangTerms::parse(v)?);
    RawFormTerm::new(a.clone(), a.root)
}
fn list(r: &RawFormTerm) -> Result<Vec<RawFormTerm>, String> {
    let mut id = r.id();
    let mut out = vec![];
    loop {
        match &r.arena().terms[id.0] {
            ErlangTerm::Nil => return Ok(out),
            ErlangTerm::List { items, tail } => {
                for i in items {
                    out.push(RawFormTerm::new(r.arena().clone(), *i)?)
                }
                id = *tail
            }
            _ => return Err("list".into()),
        }
    }
}
fn exact(a: &RawFormTerm, b: &RawFormTerm) -> Result<bool, String> {
    Ok(compare_map_keys(a.arena(), a.id(), b.arena(), b.id())?.is_eq())
}
fn error(e: CacheError, v: &Value) -> Result<bool, String> {
    if v["error"]["class"] != "error" {
        return Ok(false);
    }
    let expected = raw(&v["error"]["reason"])?;
    let (tag, value) = match e {
        CacheError::FunctionClause => {
            return Ok(matches!(expected.term(),ErlangTerm::Atom(a) if a=="function_clause"));
        }
        CacheError::CaseClause(value) => ("case_clause", value),
        CacheError::BadMap(value) => ("badmap", value),
        CacheError::BadGenerator(value) => ("bad_generator", value),
        e => return Err(format!("{e:?}")),
    };
    let ErlangTerm::Tuple(fields) = expected.term() else {
        return Ok(false);
    };
    if fields.len() != 2 {
        return Ok(false);
    };
    if !matches!(&expected.arena().terms[fields[0].0],ErlangTerm::Atom(a) if a==tag) {
        return Ok(false);
    }
    exact(
        &value,
        &RawFormTerm::new(expected.arena().clone(), fields[1])?,
    )
}
fn main() -> Result<(), String> {
    for line in io::stdin().lock().lines() {
        let r: Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let c = raw(&r["collected"])?;
        let u = raw(&r["unknown"])?;
        let o = raw(&r["old"])?;
        let matched = match CachedWarnings::merge_raw(&c, &u, &o) {
            Err(e) => error(e, &r["expected"])?,
            Ok(cache) => {
                let mut ok = exact(&cache.raw(), &raw(&r["expected"]["value"])?)?;
                if !r["selected"].is_null() {
                    let m = list(&raw(&r["modules"])?)?
                        .into_iter()
                        .map(RawLiteral::new)
                        .collect::<Result<Vec<_>, _>>()?;
                    let layout =
                        NativeAtomLayout::parse(&r["layout"]).map_err(|e| e.to_string())?;
                    ok &= match cache.select(&m, &layout) {
                        Ok(actual) => exact(&actual, &raw(&r["selected"]["value"])?)?,
                        Err(e) => error(e, &r["selected"])?,
                    };
                }
                if !r["purged"].is_null() {
                    let modules = list(&raw(&r["modules"])?)?
                        .into_iter()
                        .map(RawLiteral::new)
                        .collect::<Result<Vec<_>, _>>()?;
                    ok &= match cache.without_modules(&modules) {
                        Ok(actual) => exact(&actual.raw(), &raw(&r["purged"]["value"])?)?,
                        Err(e) => error(e, &r["purged"])?,
                    };
                }
                ok
            }
        };
        println!("{}", json!({"id":r["id"],"matched":matched}));
    }
    Ok(())
}

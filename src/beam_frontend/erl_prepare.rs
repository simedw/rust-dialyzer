//! OTP 28.3 erl_expand_records call qualification (Apache-2.0, Ericsson AB).
//! Record expansion uses the bridge's pinned dialyzer preparation profile.
#[path = "erl_records.rs"]
mod records;
use super::{erl_bifs, term::Arena};
use crate::core::{ErlangTerm, ErlangTerms, TermId};
use std::collections::BTreeMap;

pub fn prepare(forms: &ErlangTerms) -> Result<ErlangTerms, String> {
    let expanded = records::expand(forms)?;
    let mut a = Arena::from_terms(&expanded);
    let input = a.list_items(expanded.root)?;
    let mut calls = BTreeMap::<(String, usize), Option<String>>::new();
    let mut roots = Vec::new();
    for &form in &input {
        let f = a.tuple_fields(form)?;
        if a.tag(form) == Some("function") && f.len() == 5 {
            let name = a.as_atom(f[2]).ok_or("Erlang preparation: function name")?;
            let arity = a
                .as_integer(f[3])
                .and_then(|n| usize::try_from(n).ok())
                .ok_or("Erlang preparation: function arity")?;
            calls.insert((name.into(), arity), None);
            roots.push(form);
        }
    }
    for &form in &input {
        let f = a.tuple_fields(form)?;
        if a.tag(form) == Some("attribute") && f.len() == 4 && a.as_atom(f[2]) == Some("import") {
            let import = a.tuple_fields(f[3])?;
            if import.len() != 2 {
                return Err("Erlang preparation: import shape".into());
            }
            let module = a
                .as_atom(import[0])
                .ok_or("Erlang preparation: import module")?
                .to_owned();
            for fa in a.list_items(import[1])? {
                let pair = a.tuple_fields(fa)?;
                if pair.len() != 2 {
                    return Err("Erlang preparation: imported signature".into());
                }
                let name = a
                    .as_atom(pair[0])
                    .ok_or("Erlang preparation: import name")?;
                let arity = a
                    .as_integer(pair[1])
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or("Erlang preparation: import arity")?;
                calls.insert((name.into(), arity), Some(module.clone()));
            }
        }
    }
    let original = a.terms.len();
    let mut reachable = vec![false; original];
    let mut pending = roots;
    while let Some(id) = pending.pop() {
        if reachable[id.0] {
            continue;
        }
        reachable[id.0] = true;
        match a.get(id) {
            ErlangTerm::Tuple(xs) => pending.extend(xs.iter().copied()),
            ErlangTerm::List { items, tail } => {
                pending.extend(items.iter().copied());
                pending.push(*tail);
            }
            ErlangTerm::Map(xs) => {
                for &(k, v) in xs {
                    pending.extend([k, v]);
                }
            }
            #[cfg(otp_domain = "29")]
            ErlangTerm::Record(record) => pending.extend(record.fields.iter().map(|(_, value)| *value)),
            _ => (),
        }
    }
    let mut remap: Vec<_> = (0..original).map(TermId).collect();
    for index in 0..original {
        if !reachable[index] {
            continue;
        }
        let old = TermId(index);
        let term = a.get(old).clone();
        let mapped = match term {
            ErlangTerm::Tuple(xs) => ErlangTerm::Tuple(xs.iter().map(|id| remap[id.0]).collect()),
            ErlangTerm::List { items, tail } => ErlangTerm::List {
                items: items.iter().map(|id| remap[id.0]).collect(),
                tail: remap[tail.0],
            },
            ErlangTerm::Map(xs) => {
                ErlangTerm::Map(xs.iter().map(|(k, v)| (remap[k.0], remap[v.0])).collect())
            }
            #[cfg(otp_domain = "29")]
            ErlangTerm::Record(mut record) => {
                for (_, value) in &mut record.fields { *value = remap[value.0]; }
                ErlangTerm::Record(record)
            }
            _ => continue,
        };
        let current = if &mapped == a.get(old) {
            old
        } else {
            a.push(mapped)
        };
        let mut fields = a.tuple_fields(current).map(Vec::from).unwrap_or_default();
        if a.tag(current) == Some("call") && fields.len() == 4 {
            let target = a.tuple_fields(fields[2]).map(Vec::from).unwrap_or_default();
            if target.len() == 3 && a.tag(fields[2]) == Some("atom") {
                let name = a
                    .as_atom(target[2])
                    .ok_or("Erlang preparation: call name")?
                    .to_owned();
                let arity = a.list_items(fields[3])?.len();
                if let Some(module) = resolve(&calls, &name, arity) {
                    let module = a.atom(module);
                    let module = a.tagged("atom", vec![fields[1], module]);
                    fields[2] = a.tagged("remote", vec![fields[1], module, fields[2]]);
                    remap[index] = a.tuple(fields);
                    continue;
                }
            }
        }
        if a.tag(current) == Some("fun") && fields.len() == 3 {
            let target = a.tuple_fields(fields[2]).map(Vec::from).unwrap_or_default();
            if target.len() == 3 && a.as_atom(target[0]) == Some("function") {
                let name = a
                    .as_atom(target[1])
                    .ok_or("Erlang preparation: capture name")?
                    .to_owned();
                let arity = a
                    .as_integer(target[2])
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or("Erlang preparation: capture arity")?;
                if let Some(module) = resolve(&calls, &name, arity) {
                    let module = a.atom(module);
                    let module = a.tagged("atom", vec![fields[1], module]);
                    let name = a.tagged("atom", vec![fields[1], target[1]]);
                    let arity = a.tagged("integer", vec![fields[1], target[2]]);
                    fields[2] = a.tagged("function", vec![module, name, arity]);
                    remap[index] = a.tuple(fields);
                    continue;
                } else if !calls.contains_key(&(name, arity)) {
                    return Err(
                        "Erlang preparation: pseudo-function capture wrapper not yet ported".into(),
                    );
                }
            }
        }
        remap[index] = current;
    }
    let output = input.iter().map(|id| remap[id.0]).collect();
    let root = a.list(output);
    Ok(a.finish(root))
}
fn resolve(
    calls: &BTreeMap<(String, usize), Option<String>>,
    name: &str,
    arity: usize,
) -> Option<String> {
    match calls.get(&(name.into(), arity)) {
        Some(module) => module.clone(),
        None if erl_bifs::is_auto_bif(name, arity) => Some("erlang".into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Oracle fixtures are generated by erl_expand_records:module/2 with the
    /// same strict_record_tests,dialyzer options as the production bridge.
    #[test]
    #[ignore = "requires RECORD_PREP_ORACLES fixture directory"]
    fn record_preparation_oracles() {
        let dir = std::path::PathBuf::from(std::env::var("RECORD_PREP_ORACLES").unwrap());
        let mut inputs: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|x| x.unwrap().path())
            .filter(|p| p.to_string_lossy().ends_with(".input.etf"))
            .collect();
        inputs.sort();
        let mut failures = Vec::new();
        for path in inputs {
            let limits = crate::external_term::Limits::default();
            let input =
                crate::external_term::decode(&std::fs::read(&path).unwrap(), limits).unwrap();
            let base = path
                .to_string_lossy()
                .trim_end_matches(".input.etf")
                .to_owned();
            let expected = crate::external_term::decode(
                &std::fs::read(format!("{base}.expected.etf")).unwrap(),
                limits,
            )
            .unwrap();
            match prepare(&input) {
                Ok(actual) => {
                    let actual = crate::external_term::encode(&actual, limits, false).unwrap();
                    let expected = crate::external_term::encode(&expected, limits, false).unwrap();
                    if actual != expected {
                        std::fs::write(format!("{base}.actual.etf"), actual).unwrap();
                        failures.push(base);
                    }
                }
                Err(error) => failures.push(format!("{base}: {error}")),
            }
        }
        assert!(
            failures.is_empty(),
            "record preparation failures ({}): {failures:#?}",
            failures.len()
        );
    }
}

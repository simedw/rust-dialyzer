//! Counts Core nodes with the same literal expansion rules as cerl_trees:size/1.
use super::term::Arena;
use crate::core::{ErlangTerms, TermId};

pub fn core_node_count(core: &ErlangTerms) -> Result<usize, String> {
    let a = Arena::from_terms(core);
    let mut pending = vec![core.root];
    let mut count = 0;
    while let Some(node) = pending.pop() {
        if count == 2_000_000 {
            return Err("Core statistics occurrence limit".into());
        }
        count += 1;
        let f = a.tuple_fields(node)?;
        let (nodes, lists): (&[usize], &[usize]) = match a.tag(node) {
            Some("c_literal") => {
                let mut literals = vec![*f.get(2).ok_or("literal value missing")?];
                let mut literal_nodes = 0;
                while let Some(value) = literals.pop() {
                    literal_nodes += 1;
                    if count + literal_nodes > 2_000_000 {
                        return Err("Core statistics occurrence limit".into());
                    }
                    match a.get(value) {
                        crate::core::ErlangTerm::Tuple(items) => {
                            literals.extend(items.iter().copied())
                        }
                        crate::core::ErlangTerm::List { items, tail } => {
                            // Packed lists stand for one cons per item plus their tail.
                            literal_nodes += items.len().saturating_sub(1);
                            literals.extend(items.iter().copied());
                            literals.push(*tail);
                        }
                        _ => (),
                    }
                }
                count += literal_nodes - 1;
                continue;
            }
            Some("c_var") => (&[], &[]),
            Some("c_module") => {
                pending.push(*f.get(2).ok_or("module name missing")?);
                pending.extend(a.list_items(*f.get(3).ok_or("module exports missing")?)?);
                for field in [4, 5] {
                    for pair in a.list_items(*f.get(field).ok_or("module collection missing")?)? {
                        let pair = a.tuple_fields(pair)?;
                        if pair.len() != 2 {
                            return Err("module pair malformed".into());
                        }
                        pending.extend(pair.iter().copied());
                    }
                }
                continue;
            }
            Some("c_values" | "c_tuple" | "c_binary") => (&[], &[2]),
            Some("c_cons" | "c_alias" | "c_seq") => (&[2, 3], &[]),
            Some("c_bitstr") => (&[2, 3, 4, 5, 6], &[]),
            Some("c_fun") => (&[3], &[2]),
            Some("c_let") => (&[3, 4], &[2]),
            Some("c_letrec") => {
                for pair in a.list_items(*f.get(2).ok_or("letrec definitions missing")?)? {
                    let pair = a.tuple_fields(pair)?;
                    if pair.len() != 2 {
                        return Err("letrec pair malformed".into());
                    }
                    pending.extend(pair.iter().copied());
                }
                pending.push(*f.get(3).ok_or("letrec body missing")?);
                continue;
            }
            Some("c_case") => (&[2], &[3]),
            Some("c_clause") => (&[3, 4], &[2]),
            Some("c_receive") => (&[3, 4], &[2]),
            Some("c_call") => (&[2, 3], &[4]),
            Some("c_apply" | "c_primop") => (&[2], &[3]),
            Some("c_try") => (&[2, 4, 6], &[3, 5]),
            Some("c_catch") => (&[2], &[]),
            Some("c_map") => (&[], &[3]),
            Some("c_map_pair") => (&[2, 3, 4], &[]),
            #[cfg(otp_domain = "29")]
            Some("c_record") => (&[], &[4]),
            #[cfg(otp_domain = "29")]
            Some("c_record_pair") => (&[2, 3], &[]),
            tag => return Err(format!("unsupported Core statistics tag {tag:?}")),
        };
        for &field in nodes {
            pending.push(*f.get(field).ok_or("Core statistics node field missing")?);
        }
        for &field in lists {
            pending
                .extend(a.list_items(*f.get(field).ok_or("Core statistics list field missing")?)?);
        }
    }
    Ok(count)
}

pub fn reachable_term_count(terms: &ErlangTerms) -> usize {
    let mut seen = vec![false; terms.terms.len()];
    let mut pending = vec![terms.root];
    let mut count = 0;
    while let Some(TermId(id)) = pending.pop() {
        if seen[id] {
            continue;
        }
        seen[id] = true;
        count += 1;
        match &terms.terms[id] {
            crate::core::ErlangTerm::Tuple(xs) => pending.extend(xs.iter().copied()),
            crate::core::ErlangTerm::List { items, tail } => {
                pending.extend(items.iter().copied());
                pending.push(*tail);
            }
            crate::core::ErlangTerm::Map(xs) => {
                for &(key, value) in xs {
                    pending.extend([key, value]);
                }
            }
            #[cfg(otp_domain = "29")]
            crate::core::ErlangTerm::Record(record) => pending.extend(record.fields.iter().map(|(_, value)| *value)),
            _ => (),
        }
    }
    count
}

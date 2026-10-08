use rust_dialyzer_poc::{
    core::{ErlangTerm, ErlangTerms, TermId},
    term_order::TermOrderArena,
    warning_render::process_warnings,
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn list(arena: &ErlangTerms, mut id: TermId) -> Vec<TermId> {
    let mut items = vec![];
    loop {
        match &arena.terms[id.0] {
            ErlangTerm::Nil => return items,
            ErlangTerm::List { items: next, tail } => {
                items.extend(next);
                id = *tail;
            }
            _ => panic!("invalid list"),
        }
    }
}
fn main() {
    for line in io::stdin().lock().lines() {
        let row: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let a = ErlangTerms::parse(&row["input"]).unwrap();
        let b = ErlangTerms::parse(&row["expected"]).unwrap();
        let expected = list(&b, b.root);
        let input = list(&a, a.root);
        let actual = process_warnings(&a, &input).unwrap();
        let left = TermOrderArena::new(&a).unwrap();
        let right = TermOrderArena::new(&b).unwrap();
        let matched = actual.len() == expected.len()
            && actual
                .iter()
                .zip(&expected)
                .all(|(x, y)| left.compare_map_keys(*x, &right, *y).unwrap().is_eq());
        println!(
            "{}",
            json!({"id":row["id"],"matched":matched,"input":input.len(),"output":actual.len()})
        );
    }
}

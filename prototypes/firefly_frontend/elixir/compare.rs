use rust_dialyzer_poc::{
    core::{ErlangTerm as T, ErlangTerms, TermId},
    external_term::decode,
};
fn first(a: &ErlangTerms, x: TermId, b: &ErlangTerms, y: TermId, path: String) -> Option<String> {
    match (&a.terms[x.0], &b.terms[y.0]) {
        (T::Tuple(xs), T::Tuple(ys)) if xs.len() == ys.len() => {
            for (i, (x, y)) in xs.iter().zip(ys).enumerate() {
                if let Some(p) = first(a, *x, b, *y, format!("{path}.tuple[{i}]")) {
                    return Some(p);
                }
            }
            None
        }
        (
            T::List {
                items: xs,
                tail: xt,
            },
            T::List {
                items: ys,
                tail: yt,
            },
        ) if xs.len() == ys.len() => {
            for (i, (x, y)) in xs.iter().zip(ys).enumerate() {
                if let Some(p) = first(a, *x, b, *y, format!("{path}.list[{i}]")) {
                    return Some(p);
                }
            }
            first(a, *xt, b, *yt, format!("{path}.tail"))
        }
        (x, y) if x == y => None,
        (x, y) => Some(format!("{path}: native {x:?}; oracle {y:?}")),
    }
}
fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let a = decode(&std::fs::read(&args[1]).unwrap(), Default::default()).unwrap();
    let b = decode(&std::fs::read(&args[2]).unwrap(), Default::default()).unwrap();
    let equal = rust_dialyzer_poc::term_order::compare_map_keys(&a, a.root, &b, b.root)
        .unwrap()
        .is_eq();
    if !equal {
        let p = first(&a, a.root, &b, b.root, "forms".into())
            .unwrap_or_else(|| "semantic term mismatch".into());
        println!("{p}");
        std::process::exit(1)
    }
    println!("equal")
}

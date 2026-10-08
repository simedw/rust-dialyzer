use crate::serialization::etf::Term;
use firefly_intern::Symbol;
use std::collections::BTreeSet;

pub(super) fn normalize(term: Term) -> Term {
    let mut used = BTreeSet::new();
    let mut work = vec![&term];
    while let Some(term) = work.pop() {
        match term {
            Term::Tuple(tuple) => {
                if tuple.elements.len() == 3 && tag(&tuple.elements[0]) == Some("var") {
                    if let Term::Atom(name) = &tuple.elements[2] {
                        used.insert(name.name);
                    }
                }
                work.extend(tuple.elements.iter());
            }
            Term::List(list) => work.extend(list.elements.iter()),
            _ => {}
        }
    }
    Normalizer { used, next: 0 }.forms(term)
}
fn tag(term: &Term) -> Option<&str> {
    match term {
        Term::Atom(atom) => Some(atom.name.as_str().get()),
        _ => None,
    }
}
fn atom(name: &str) -> Term {
    Term::Atom(Symbol::intern(name).into())
}
fn tuple(items: Vec<Term>) -> Term {
    Term::Tuple(items.into())
}
fn list(items: Vec<Term>) -> Term {
    Term::List(items.into())
}
fn lit(loc: &Term, name: &str) -> Term {
    tuple(vec![atom("atom"), loc.clone(), atom(name)])
}
fn variable(loc: &Term, name: Symbol) -> Term {
    tuple(vec![atom("var"), loc.clone(), Term::Atom(name.into())])
}
fn call(loc: &Term, module: &str, function: &str, args: Vec<Term>) -> Term {
    tuple(vec![
        atom("call"),
        loc.clone(),
        tuple(vec![
            atom("remote"),
            loc.clone(),
            lit(loc, module),
            lit(loc, function),
        ]),
        list(args),
    ])
}
fn clause(loc: &Term, pattern: Term, body: Term) -> Term {
    tuple(vec![
        atom("clause"),
        loc.clone(),
        list(vec![pattern]),
        list(vec![]),
        list(vec![body]),
    ])
}
fn generated_clause(loc: &Term, pattern: Term, body: Term) -> Term {
    clause(
        &list(vec![
            tuple(vec![atom("location"), loc.clone()]),
            tuple(vec![atom("generated"), atom("true")]),
        ]),
        pattern,
        body,
    )
}
struct Normalizer {
    used: BTreeSet<Symbol>,
    next: usize,
}
impl Normalizer {
    fn forms(&mut self, term: Term) -> Term {
        match term {
            Term::List(mut forms) => {
                forms.elements = forms
                    .elements
                    .into_iter()
                    .map(|form| {
                        let is_code = match &form {
                            Term::Tuple(t) => {
                                t.elements.first().and_then(tag) == Some("function")
                                    || (t.elements.len() == 4
                                        && tag(&t.elements[0]) == Some("attribute")
                                        && tag(&t.elements[2]) == Some("record"))
                            }
                            _ => false,
                        };
                        if is_code {
                            self.walk(form)
                        } else {
                            form
                        }
                    })
                    .collect();
                Term::List(forms)
            }
            Term::Tuple(mut outer) => {
                if outer.elements.len() == 2 && tag(&outer.elements[0]) == Some("raw_abstract_v1") {
                    outer.elements[1] = self.forms(outer.elements[1].clone());
                } else if outer.elements.len() == 3
                    && tag(&outer.elements[0]) == Some("debug_info_v1")
                {
                    if let Term::Tuple(ref mut data) = outer.elements[2] {
                        if data.elements.len() == 2 {
                            data.elements[0] = self.forms(data.elements[0].clone());
                        }
                    }
                }
                Term::Tuple(outer)
            }
            other => other,
        }
    }
    fn fresh(&mut self) -> Symbol {
        loop {
            let name = Symbol::intern(&format!("_NativeMaybe{}", self.next));
            self.next += 1;
            if self.used.insert(name) {
                return name;
            }
        }
    }
    fn walk(&mut self, term: Term) -> Term {
        let term = match term {
            Term::Tuple(mut tuple) => {
                tuple.elements = tuple.elements.into_iter().map(|x| self.walk(x)).collect();
                Term::Tuple(tuple)
            }
            Term::List(mut list) => {
                list.elements = list.elements.into_iter().map(|x| self.walk(x)).collect();
                Term::List(list)
            }
            other => other,
        };
        let Term::Tuple(ref t) = term else {
            return term;
        };
        let xs = &t.elements;
        match xs.first().and_then(tag) {
            Some("mc") if xs.len() == 4 => {
                let Term::Tuple(pair) = &xs[2] else {
                    return term;
                };
                if pair.elements.len() != 4 || tag(&pair.elements[0]) != Some("map_field_assoc") {
                    return term;
                }
                let pair = tuple(vec![
                    atom("tuple"),
                    pair.elements[1].clone(),
                    list(pair.elements[2..].to_vec()),
                ]);
                let comprehension = tuple(vec![atom("lc"), xs[1].clone(), pair, xs[3].clone()]);
                call(&xs[1], "maps", "from_list", vec![comprehension])
            }
            Some("maybe") if xs.len() == 3 || xs.len() == 4 => {
                let Term::List(body) = &xs[2] else {
                    return term;
                };
                if body.elements.is_empty() {
                    return term;
                }
                let else_clauses = if xs.len() == 4 {
                    let Term::Tuple(other) = &xs[3] else {
                        return term;
                    };
                    if other.elements.len() != 3 || tag(&other.elements[0]) != Some("else") {
                        return term;
                    }
                    let Term::List(clauses) = &other.elements[2] else {
                        return term;
                    };
                    Some(clauses.elements.clone())
                } else {
                    None
                };
                self.maybe_body(&body.elements, &xs[1], &else_clauses)
            }
            _ => term,
        }
    }
    fn maybe_body(&mut self, body: &[Term], loc: &Term, else_clauses: &Option<Vec<Term>>) -> Term {
        let (first, rest) = body.split_first().expect("nonempty maybe body");
        if let Term::Tuple(t) = first {
            let xs = &t.elements;
            if xs.len() == 4 && tag(&xs[0]) == Some("maybe_match") {
                let value = variable(&xs[1], self.fresh());
                let alias = tuple(vec![
                    atom("match"),
                    xs[1].clone(),
                    value.clone(),
                    xs[2].clone(),
                ]);
                let success = if rest.is_empty() {
                    value.clone()
                } else {
                    self.maybe_body(rest, loc, else_clauses)
                };
                let failure = match else_clauses {
                    None => value.clone(),
                    Some(clauses) => {
                        let mut clauses = clauses.clone();
                        let error = tuple(vec![
                            atom("tuple"),
                            loc.clone(),
                            list(vec![lit(loc, "else_clause"), value.clone()]),
                        ]);
                        clauses.push(generated_clause(
                            loc,
                            variable(loc, self.fresh()),
                            call(loc, "erlang", "error", vec![error]),
                        ));
                        tuple(vec![
                            atom("case"),
                            loc.clone(),
                            value.clone(),
                            list(clauses),
                        ])
                    }
                };
                return tuple(vec![
                    atom("case"),
                    xs[1].clone(),
                    xs[3].clone(),
                    list(vec![
                        clause(&xs[1], alias, success),
                        generated_clause(&xs[1], value, failure),
                    ]),
                ]);
            }
        }
        if rest.is_empty() {
            first.clone()
        } else {
            tuple(vec![
                atom("block"),
                loc.clone(),
                list(vec![
                    first.clone(),
                    self.maybe_body(rest, loc, else_clauses),
                ]),
            ])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires MODERN_FORMS_INPUT and MODERN_FORMS_OUTPUT for OTP runtime verification"]
    fn normalize_actual_modern_forms_for_otp() {
        let input = std::env::var("MODERN_FORMS_INPUT").unwrap();
        let output = std::env::var("MODERN_FORMS_OUTPUT").unwrap();
        let bytes = std::fs::read(input).unwrap();
        let term = Term::decode(std::io::Cursor::new(bytes)).unwrap();
        let normalized = normalize(term);
        let mut bytes = vec![];
        normalized.encode(&mut bytes).unwrap();
        super::super::AbstractCode::from_abstract_forms(&bytes).unwrap();
        std::fs::write(output, bytes).unwrap();
    }
}

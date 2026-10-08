//! Source-derived Dialyzer contract presentation over original flat Forms.
use crate::{
    core::{ErlangTerm, ErlangTerms, TermId},
    form_error::{FormError, ReasonBuilder},
    native_error::NativeTransferError,
    native_form_environment::RawFormTerm,
    presentation_form,
};
type Result<T> = std::result::Result<T, FormError>;
fn fc() -> FormError {
    FormError::Native(NativeTransferError::FunctionClause)
}
fn badarg() -> FormError {
    FormError::Native(NativeTransferError::Badarg)
}
fn reason(terms: &ErlangTerms, id: TermId, tag: &str) -> FormError {
    let mut b = ReasonBuilder::new();
    let raw = RawFormTerm::new(std::sync::Arc::new(terms.clone()), id).expect("validated term");
    let value = b.copy(&raw);
    let tag = b.atom(tag);
    let root = b.tuple(vec![tag, value]);
    FormError::ErrorReason(b.finish(root))
}
#[derive(Clone, Copy)]
struct Cursor {
    id: TermId,
    index: usize,
}
impl Cursor {
    fn new(id: TermId) -> Self {
        Self { id, index: 0 }
    }
    fn next(&mut self, terms: &ErlangTerms) -> std::result::Result<Option<TermId>, TermId> {
        loop {
            match &terms.terms[self.id.0] {
                ErlangTerm::Nil => return Ok(None),
                ErlangTerm::List { items, tail } => {
                    if let Some(id) = items.get(self.index) {
                        self.index += 1;
                        return Ok(Some(*id));
                    }
                    self.id = *tail;
                    self.index = 0;
                }
                _ => return Err(self.id),
            }
        }
    }
    fn empty(self, terms: &ErlangTerms) -> bool {
        let mut c = self;
        matches!(c.next(terms), Ok(None))
    }
}
fn tuple(terms: &ErlangTerms, id: TermId, len: usize) -> Result<&[TermId]> {
    match &terms.terms[id.0] {
        ErlangTerm::Tuple(xs) if xs.len() == len => Ok(xs),
        _ => Err(fc()),
    }
}
fn atom_is(terms: &ErlangTerms, id: TermId, name: &str) -> bool {
    matches!(&terms.terms[id.0],ErlangTerm::Atom(s) if s==name)
}
fn atom_list(terms: &ErlangTerms, id: TermId) -> Result<&str> {
    match &terms.terms[id.0] {
        ErlangTerm::Atom(s) => Ok(s),
        _ => Err(badarg()),
    }
}
fn fields(raw: &RawFormTerm) -> Result<&[TermId]> {
    let t = tuple(raw.arena(), raw.id(), 4)?;
    if !atom_is(raw.arena(), t[0], "contract") {
        return Err(fc());
    }
    Ok(t)
}
/// Native behavior: a single alternative returns true; two or more return false.
pub fn is_overloaded(raw: &RawFormTerm) -> Result<bool> {
    let cs = fields(raw)?[1];
    let mut c = Cursor::new(cs);
    match c.next(raw.arena()) {
        Ok(Some(_)) => {
            if c.empty(raw.arena()) {
                Ok(true)
            } else if matches!(c.next(raw.arena()), Ok(Some(_))) {
                Ok(false)
            } else {
                Err(reason(raw.arena(), cs, "case_clause"))
            }
        }
        _ => Err(reason(raw.arena(), cs, "case_clause")),
    }
}
pub fn contract_to_string(raw: &RawFormTerm) -> Result<String> {
    forms_to_string(raw.arena(), fields(raw)?[3])
}
pub fn dependency_to_string(contract: &crate::dependencies::Contract) -> Result<String> {
    forms_to_string(&contract.forms, contract.forms.root)
}
pub fn dependency_is_overloaded(contract: &crate::dependencies::Contract) -> Result<bool> {
    match contract.alternatives.len() {
        1 => Ok(true),
        n if n >= 2 => Ok(false),
        _ => Err(FormError::Native(NativeTransferError::CaseClauseNil)),
    }
}
pub fn forms_to_string(terms: &ErlangTerms, forms: TermId) -> Result<String> {
    let mut c = Cursor::new(forms);
    let mut output = String::new();
    let mut had = false;
    loop {
        let Some(id) = c.next(terms).map_err(|_| fc())? else {
            return if had { Ok(output) } else { Err(fc()) };
        };
        had = true;
        let pair = tuple(terms, id, 2)?;
        let printed = presentation_form::form_to_string(&terms.terms, pair[0])?;
        let Some(printed) = printed.strip_prefix("fun(") else {
            return Err(fc());
        };
        let mut printed = printed.to_owned();
        printed.pop();
        output.push_str(&printed);
        if Cursor::new(pair[1]).empty(terms) {
            if c.empty(terms) {
                return Ok(output);
            }
            output.push_str("\n    ; ");
        } else {
            output.push_str(" when ");
            output.push_str(&constraints_to_string(terms, pair[1])?);
            if c.empty(terms) {
                return Ok(output);
            }
            output.push(';');
        }
    }
}
fn constraints_to_string(terms: &ErlangTerms, root: TermId) -> Result<String> {
    let mut c = Cursor::new(root);
    let mut output = String::new();
    let mut first = true;
    while let Some(id) = c.next(terms).map_err(|_| fc())? {
        let t = tuple(terms, id, 4)?;
        if !atom_is(terms, t[0], "type") || !atom_is(terms, t[2], "constraint") {
            return Err(fc());
        }
        let mut args = Cursor::new(t[3]);
        let what = args.next(terms).map_err(|_| fc())?.ok_or_else(fc)?;
        let types = args.next(terms).map_err(|_| fc())?.ok_or_else(fc)?;
        if !args.empty(terms) {
            return Err(fc());
        }
        let name = tuple(terms, what, 3)?;
        if !atom_is(terms, name[0], "atom") {
            return Err(fc());
        }
        if !first {
            output.push_str(", ");
        }
        first = false;
        let what = name[2];
        let mut tc = Cursor::new(types);
        let lhs = tc.next(terms);
        let rhs = tc.next(terms);
        let subtype = if atom_is(terms, what, "is_subtype") {
            match (lhs, rhs, tc.empty(terms)) {
                (Ok(Some(lhs)), Ok(Some(rhs)), true) => match &terms.terms[lhs.0] {
                    ErlangTerm::Tuple(v) if v.len() == 3 && atom_is(terms, v[0], "var") => {
                        Some((v[2], rhs))
                    }
                    _ => None,
                },
                _ => None,
            }
        } else {
            None
        };
        if let Some((variable, body)) = subtype {
            output.push_str(atom_list(terms, variable)?);
            output.push_str(" :: ");
            output.push_str(&presentation_form::form_to_string(&terms.terms, body)?);
        } else {
            output.push_str(atom_list(terms, what)?);
            output.push('(');
            let mut tc = Cursor::new(types);
            let mut first = true;
            while let Some(t) = tc
                .next(terms)
                .map_err(|id| reason(terms, id, "bad_generator"))?
            {
                if !first {
                    output.push(',');
                }
                first = false;
                output.push_str(&presentation_form::form_to_string(&terms.terms, t)?);
            }
            output.push(')');
        }
    }
    Ok(output)
}

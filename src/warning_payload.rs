use crate::{
    binder::{OpaqueMatch, WarningFunction, WarningInfo, WarningLocation},
    core::ErlangTerms,
    form_error::ReasonBuilder,
    lattice::NativeValue,
    native_form_environment::RawFormTerm,
    term_order::compare_map_keys,
};
use std::cmp::Ordering;
#[derive(Clone, Debug)]
pub struct NativeWarning {
    pub raw: RawFormTerm,
}
impl NativeWarning {
    pub fn from_raw_origin(category: &str, origin: &RawFormTerm, message: &RawFormTerm) -> Self {
        let mut b = ReasonBuilder::new();
        let category = b.atom(category);
        let origin = b.copy(origin);
        let message = b.copy(message);
        let root = b.tuple(vec![category, origin, message]);
        Self {
            raw: b.finish(root),
        }
    }
    pub fn from_parts(category: &str, info: &WarningInfo, message: &RawFormTerm) -> Self {
        let mut b = ReasonBuilder::new();
        let tag = b.atom(category);
        let file = b.string(&info.filename);
        let location = match info.location {
            WarningLocation::Line(n) => b.integer(n),
            WarningLocation::Position { line, column } => {
                let line = b.integer(line);
                let column = b.integer(column);
                b.tuple(vec![line, column])
            }
        };
        let function = match &info.current_fun {
            WarningFunction::Top => b.atom("top"),
            WarningFunction::Undefined => b.atom("undefined"),
            WarningFunction::Label(label) => b.integer(label.0),
            WarningFunction::Named(name) => {
                let module = b.atom(&name.module);
                let function = b.atom(&name.name);
                let arity = b.integer(name.arity);
                b.tuple(vec![module, function, arity])
            }
        };
        let origin = b.tuple(vec![file, location, function]);
        let message = b.copy(message);
        let root = b.tuple(vec![tag, origin, message]);
        Self {
            raw: b.finish(root),
        }
    }
    pub fn from_value(category: &str, info: &WarningInfo, message: &NativeValue) -> Self {
        let mut b = ReasonBuilder::new();
        let root = b.native(message);
        Self::from_parts(category, info, &b.finish(root))
    }
    pub fn list(warnings: &[Self]) -> RawFormTerm {
        let mut b = ReasonBuilder::new();
        let terms = warnings.iter().map(|w| b.copy(&w.raw)).collect();
        let root = b.list(terms);
        b.finish(root)
    }
    pub fn matches(&self, raw: &ErlangTerms) -> Result<bool, String> {
        Ok(compare_map_keys(self.raw.arena(), self.raw.id(), raw, raw.root)? == Ordering::Equal)
    }
}
impl PartialEq for NativeWarning {
    fn eq(&self, other: &Self) -> bool {
        compare_map_keys(
            self.raw.arena(),
            self.raw.id(),
            other.raw.arena(),
            other.raw.id(),
        ) == Ok(Ordering::Equal)
    }
}
impl Eq for NativeWarning {}
pub fn string(value: &str) -> NativeValue {
    NativeValue::List(
        value
            .chars()
            .map(|c| NativeValue::Integer((c as u32).into()))
            .collect(),
    )
}
impl OpaqueMatch {
    pub fn native_warning(&self) -> NativeWarning {
        NativeWarning::from_value(
            "warn_opaque",
            &self.info,
            &NativeValue::Tuple(vec![
                NativeValue::Atom("opaque_match".into()),
                NativeValue::List(self.message.iter().map(|s| string(s)).collect()),
            ]),
        )
    }
}

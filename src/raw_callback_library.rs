use crate::{
    bifs::registry::Mfa,
    core::{ErlangTerm, ErlangTerms},
    declaration_key::DeclarationKey,
    dependencies::Contract,
    native_form_environment::RawFormTerm, term_order::TermOrderArena,
};
use std::sync::{Arc, OnceLock};

pub struct CallbackRow {
    pub key: DeclarationKey,
    pub raw: RawFormTerm,
    pub location: RawFormTerm,
    pub capture: RawFormTerm,
    pub flags: RawFormTerm,
    callable: Option<Mfa>,
    decoded: OnceLock<Result<Contract, String>>,
}
impl CallbackRow {
    pub fn callable(&self) -> Option<&Mfa> {
        self.callable.as_ref()
    }
    pub fn contract(&self) -> Result<&Contract, String> {
        self.decoded
            .get_or_init(|| Contract::from_capture(&self.capture, &self.location, &self.flags))
            .as_ref()
            .map_err(Clone::clone)
    }
}
pub struct CallbackGroup {
    pub module: RawFormTerm,
    pub callbacks: RawFormTerm,
    rows: Vec<usize>,
}
pub struct RawCallbackLibrary {
    raw: RawFormTerm,
    groups: Vec<CallbackGroup>,
    rows: Vec<CallbackRow>,
}
fn list(raw: &RawFormTerm) -> Result<Vec<RawFormTerm>, String> {
    let mut next = raw.id();
    let mut rows = Vec::new();
    loop {
        match &raw.arena().terms[next.0] {
            ErlangTerm::Nil => return Ok(rows),
            ErlangTerm::List { items, tail } => {
                rows.extend(
                    items
                        .iter()
                        .map(|id| RawFormTerm::new(raw.arena().clone(), *id))
                        .collect::<Result<Vec<_>, _>>()?,
                );
                next = *tail;
            }
            _ => return Err("callback provenance requires proper lists".into()),
        }
    }
}
fn tuple(raw: &RawFormTerm, n: usize) -> Result<Vec<RawFormTerm>, String> {
    let ErlangTerm::Tuple(fields) = raw.term() else {
        return Err("callback provenance requires tuples".into());
    };
    if fields.len() != n {
        return Err("callback provenance tuple arity differs".into());
    }
    fields
        .iter()
        .map(|id| RawFormTerm::new(raw.arena().clone(), *id))
        .collect()
}
fn exact(
    a: &RawFormTerm,
    b: &RawFormTerm,
    source: &Arc<ErlangTerms>,
    cached: &OnceLock<Result<TermOrderArena, String>>,
) -> Result<bool, String> {
    let source_order = || {
        cached
            .get_or_init(|| TermOrderArena::new(source))
            .as_ref()
            .map_err(Clone::clone)
    };
    let local_a;
    let a_order = if Arc::ptr_eq(a.arena(), source) {
        source_order()?
    } else {
        local_a = TermOrderArena::new(a.arena())?;
        &local_a
    };
    let local_b;
    let b_order = if Arc::ptr_eq(b.arena(), source) {
        source_order()?
    } else {
        local_b = TermOrderArena::new(b.arena())?;
        &local_b
    };
    Ok(a_order.compare_map_keys(a.id(), b_order, b.id())?.is_eq())
}
impl RawCallbackLibrary {
    pub fn parse(raw: RawFormTerm, purged: impl Fn(&str) -> bool) -> Result<Self, String> {
        let mut groups: Vec<CallbackGroup> = Vec::new();
        let mut rows: Vec<CallbackRow> = Vec::new();
        let order = OnceLock::new();
        for group in list(&raw)? {
            let fields = tuple(&group, 2)?;
            let module = fields[0].clone();
            if matches!(module.term(),ErlangTerm::Atom(name) if purged(name)) {
                continue;
            }
            for group in &groups {
                if exact(&group.module, &module, raw.arena(), &order)? {
                    return Err("duplicate raw callback module group".into());
                }
            }
            let callbacks = fields[1].clone();
            let mut indexes = Vec::new();
            for row in list(&callbacks)? {
                let fields = tuple(&row, 2)?;
                tuple(&fields[0], 3)?;
                let key = DeclarationKey::parse("", &fields[0]).map_err(|e| e.to_string())?;
                let key_fields = tuple(key.raw(), 3)?;
                if !exact(&key_fields[0], &module, raw.arena(), &order)? {
                    return Err("raw callback group differs from MFA module".into());
                }
                for row in &rows {
                    if row.key.map_equals(&key)? {
                        return Err("duplicate raw callback MFA".into());
                    }
                }
                let value = tuple(&fields[1], 3)?;
                let callable = key.callable().map(|name| Mfa {
                    module: name.module.clone(),
                    function: name.name.clone(),
                    arity: name.arity,
                });
                indexes.push(rows.len());
                rows.push(CallbackRow {
                    key,
                    raw: row,
                    location: value[0].clone(),
                    capture: value[1].clone(),
                    flags: value[2].clone(),
                    callable,
                    decoded: OnceLock::new(),
                });
            }
            groups.push(CallbackGroup {
                module,
                callbacks,
                rows: indexes,
            });
        }
        Ok(Self { raw, groups, rows })
    }
    pub fn groups(&self) -> &[CallbackGroup] {
        &self.groups
    }
    pub fn rows(&self) -> impl Iterator<Item = &CallbackRow> {
        self.groups
            .iter()
            .flat_map(|group| group.rows.iter().map(|i| &self.rows[*i]))
    }
    pub fn lookup(&self, module: &str) -> Option<Vec<&CallbackRow>> {
        self.groups
            .iter()
            .find(|group| matches!(group.module.term(),ErlangTerm::Atom(name) if name==module))
            .map(|group| group.rows.iter().map(|i| &self.rows[*i]).collect())
    }
    pub fn raw(&self) -> RawFormTerm {
        let mut b = crate::form_error::ReasonBuilder::new();
        let rows = self
            .groups
            .iter()
            .map(|group| {
                let m = b.copy(&group.module);
                let callbacks = b.copy(&group.callbacks);
                b.tuple(vec![m, callbacks])
            })
            .collect();
        let id = b.list(rows);
        b.finish(id)
    }
    pub fn arena(&self) -> &RawFormTerm {
        &self.raw
    }
}

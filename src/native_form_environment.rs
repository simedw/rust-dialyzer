use crate::{
    core::{ErlangTerm, ErlangTerms, TermId},
    forms::DeclarationKind,
    program::TypeName,
};
use std::{collections::BTreeMap, sync::Arc};
#[derive(Clone, Debug)]
pub struct RawFormTerm {
    pub(crate) arena: Arc<ErlangTerms>,
    pub(crate) id: TermId,
}
impl RawFormTerm {
    pub fn new(arena: Arc<ErlangTerms>, id: TermId) -> Result<Self, String> {
        if arena.terms.get(id.0).is_none() {
            return Err("raw form handle outside arena".into());
        }
        Ok(Self { arena, id })
    }
    pub fn arena(&self) -> &Arc<ErlangTerms> {
        &self.arena
    }
    pub fn id(&self) -> TermId {
        self.id
    }
    pub fn term(&self) -> &ErlangTerm {
        &self.arena.terms[self.id.0]
    }
}
#[derive(Clone)]
pub struct LazyFormEnvironment {
    pub arena: Arc<ErlangTerms>,
    pub tables: BTreeMap<String, TermId>,
}
#[derive(Clone)]
pub struct RawDeclaration {
    pub kind: DeclarationKind,
    pub value: RawFormTerm,
}
impl LazyFormEnvironment {
    // Native lookup_type checks aliases, opaque and nominal in that exact order;
    // unrelated malformed keys/values are never inspected.
    pub fn declaration(&self, name: &TypeName) -> Option<RawDeclaration> {
        let root = *self.tables.get(&name.module)?;
        let ErlangTerm::Map(entries) = &self.arena.terms[root.0] else {
            return None;
        };
        for (tag, kind) in [
            ("type", DeclarationKind::Alias),
            ("opaque", DeclarationKind::Opaque),
            ("nominal", DeclarationKind::Nominal),
        ] {
            for (key, value) in entries {
                let ErlangTerm::Tuple(fields) = &self.arena.terms[key.0] else {
                    continue;
                };
                if fields.len() != 3 {
                    continue;
                }
                let (
                    ErlangTerm::Atom(actual_tag),
                    ErlangTerm::Atom(actual_name),
                    ErlangTerm::Integer(arity),
                ) = (
                    &self.arena.terms[fields[0].0],
                    &self.arena.terms[fields[1].0],
                    &self.arena.terms[fields[2].0],
                )
                else {
                    continue;
                };
                if actual_tag == tag
                    && actual_name == &name.name
                    && arity.parse::<usize>().ok() == Some(name.arity)
                {
                    return Some(RawDeclaration {
                        kind,
                        value: RawFormTerm {
                            arena: self.arena.clone(),
                            id: *value,
                        },
                    });
                }
            }
        }
        None
    }
}
#[derive(Clone)]
pub struct RawDefinition {
    pub module: String,
    pub file: RawFormTerm,
    pub body: RawFormTerm,
    pub parameters: RawFormTerm,
}
fn tuple(a: &ErlangTerms, id: TermId, n: usize) -> Result<&[TermId], String> {
    match &a.terms[id.0] {
        ErlangTerm::Tuple(v) if v.len() == n => Ok(v),
        _ => Err("native declaration tuple shape".into()),
    }
}
fn atom(a: &ErlangTerms, id: TermId) -> Result<&str, String> {
    match &a.terms[id.0] {
        ErlangTerm::Atom(v) => Ok(v),
        _ => Err("native declaration expected atom".into()),
    }
}
fn list(a: &ErlangTerms, mut id: TermId) -> Result<Vec<TermId>, String> {
    let mut out = Vec::new();
    loop {
        match &a.terms[id.0] {
            ErlangTerm::Nil => return Ok(out),
            ErlangTerm::List { items, tail } => {
                out.extend(items);
                id = *tail
            }
            _ => return Err("native declaration parameter list".into()),
        }
    }
}
impl RawDefinition {
    pub fn parameter_names(
        &self,
        arity: usize,
    ) -> Result<Vec<crate::lattice::VariableId>, crate::form_error::FormError> {
        use crate::{form_error::FormError, native_error::NativeTransferError};
        let ids = list(&self.parameters.arena, self.parameters.id)
            .map_err(|_| FormError::Native(NativeTransferError::FunctionClause))?;
        if ids.len() != arity {
            return Err(FormError::Native(NativeTransferError::FunctionClause));
        }
        ids.into_iter()
            .map(|id| match &self.parameters.arena.terms[id.0] {
                ErlangTerm::Atom(s) => Ok(crate::lattice::VariableId::Atom(s.clone())),
                ErlangTerm::Integer(s) => Ok(crate::lattice::VariableId::Integer(
                    s.parse().map_err(|_| "invalid variable integer")?,
                )),
                _ => Err(FormError::Transport(
                    "native declaration parameter outside typed variable domain".into(),
                )),
            })
            .collect()
    }
}
impl RawDeclaration {
    fn lookup_case_clause(&self) -> crate::form_error::FormError {
        let mut b = crate::form_error::ReasonBuilder::new();
        let tag = b.atom(match self.kind {
            DeclarationKind::Alias => "type",
            DeclarationKind::Opaque => "opaque",
            DeclarationKind::Nominal => "nominal",
        });
        let value = b.copy(&self.value);
        let lookup = b.tuple(vec![tag, value]);
        let case = b.atom("case_clause");
        let reason = b.tuple(vec![case, lookup]);
        crate::form_error::FormError::ErrorReason(b.finish(reason))
    }
    pub(crate) fn checked_shape(&self) -> Result<(), crate::form_error::FormError> {
        self.check_shape().map_err(|_| self.lookup_case_clause())
    }
    pub(crate) fn checked_definition(&self) -> Result<RawDefinition, crate::form_error::FormError> {
        let a = &self.value.arena;
        let v = tuple(a, self.value.id, 2).map_err(|_| self.lookup_case_clause())?;
        let m = tuple(a, v[0], 4).map_err(|_| self.lookup_case_clause())?;
        tuple(a, m[1], 2).map_err(|_| self.lookup_case_clause())?;
        self.definition()
            .map_err(crate::form_error::FormError::Transport)
    }
    pub fn check_shape(&self) -> Result<(), String> {
        tuple(&self.value.arena, self.value.id, 2).map(|_| ())
    }
    pub fn definition(&self) -> Result<RawDefinition, String> {
        let a = &self.value.arena;
        let v = tuple(a, self.value.id, 2)?;
        let m = tuple(a, v[0], 4)?;
        let loc = tuple(a, m[1], 2)?;
        Ok(RawDefinition {
            module: atom(a, m[0])?.into(),
            file: RawFormTerm::new(a.clone(), loc[0])?,
            body: RawFormTerm::new(a.clone(), m[2])?,
            parameters: RawFormTerm::new(a.clone(), m[3])?,
        })
    }
}
impl LazyFormEnvironment {
    #[cfg(otp_domain = "29")]
    pub fn native_record_fields(&self, module: &str, name: &str) -> Result<Option<RawFormTerm>, String> {
        let Some(root) = self.tables.get(module) else { return Ok(None) };
        let ErlangTerm::Map(entries) = &self.arena.terms[root.0] else { return Err("native record table not map".into()) };
        for (key, value) in entries {
            let Ok(k) = tuple(&self.arena, *key, 2) else { continue };
            if atom(&self.arena, k[0]).ok() != Some("native_record") { continue }
            let Ok(n) = tuple(&self.arena, k[1], 2) else { continue };
            if atom(&self.arena, n[0]).ok() == Some(module) && atom(&self.arena, n[1]).ok() == Some(name) {
                let Ok(v) = tuple(&self.arena, *value, 2) else { return Ok(None) };
                return Ok(Some(RawFormTerm::new(self.arena.clone(), v[1])?));
            }
        }
        Ok(None)
    }
    #[cfg(otp_domain = "29")]
    pub fn native_record_import(&self, module: &str, name: &str) -> Result<Option<String>, String> {
        let Some(root) = self.tables.get(module) else { return Ok(None) };
        let ErlangTerm::Map(entries) = &self.arena.terms[root.0] else { return Err("native record table not map".into()) };
        for (key, value) in entries {
            let Ok(k) = tuple(&self.arena, *key, 2) else { continue };
            if atom(&self.arena, k[0]).ok() == Some("import_record") && atom(&self.arena, k[1]).ok() == Some(name) {
                return Ok(Some(atom(&self.arena, *value)?.into()));
            }
        }
        Ok(None)
    }
    pub fn record_fields(&self, module: &str, name: &str) -> Result<Option<RawFormTerm>, String> {
        let Some(root) = self.tables.get(module) else {
            return Ok(None);
        };
        let ErlangTerm::Map(entries) = &self.arena.terms[root.0] else {
            return Err("native record table not map".into());
        };
        for (key, value) in entries {
            let ErlangTerm::Tuple(k) = &self.arena.terms[key.0] else {
                continue;
            };
            if k.len() != 2
                || !matches!(&self.arena.terms[k[0].0],ErlangTerm::Atom(tag)if tag=="record")
                || !matches!(&self.arena.terms[k[1].0],ErlangTerm::Atom(tag)if tag==name)
            {
                continue;
            }
            // lookup_record/2 accepts exactly one variant, without inspecting its fields.
            let Ok(v) = tuple(&self.arena, *value, 2) else {
                return Ok(None);
            };
            let Ok(variants) = list(&self.arena, v[1]) else {
                return Ok(None);
            };
            if variants.len() != 1 {
                return Ok(None);
            };
            let Ok(variant) = tuple(&self.arena, variants[0], 2) else {
                return Ok(None);
            };
            return Ok(Some(RawFormTerm::new(self.arena.clone(), variant[1])?));
        }
        Ok(None)
    }
}
pub struct RawRecordField {
    pub name: String,
    pub form: RawFormTerm,
    pub cached_type: RawFormTerm,
}
impl RawFormTerm {
    pub fn record_entries(&self) -> Result<Vec<TermId>, crate::form_error::FormError> {
        list(&self.arena, self.id).map_err(|_| {
            crate::form_error::FormError::Native(crate::native_error::NativeTransferError::Badarg)
        })
    }
    pub fn record_arity(&self) -> Result<usize, crate::form_error::FormError> {
        list(&self.arena, self.id).map(|v| v.len()).map_err(|_| {
            crate::form_error::FormError::Native(crate::native_error::NativeTransferError::Badarg)
        })
    }
    pub fn cached_field_type(
        &self,
        name: &str,
    ) -> Result<RawFormTerm, crate::form_error::FormError> {
        use crate::{
            form_error::{FormError, ReasonBuilder},
            native_error::NativeTransferError,
        };
        let fields = list(&self.arena, self.id)
            .map_err(|_| FormError::Native(NativeTransferError::Badarg))?;
        for id in fields {
            let ErlangTerm::Tuple(values) = &self.arena.terms[id.0] else {
                continue;
            };
            if !values
                .first()
                .is_some_and(|i| matches!(&self.arena.terms[i.0],ErlangTerm::Atom(s) if s==name))
            {
                continue;
            }
            if values.len() != 3 {
                let mut b = ReasonBuilder::new();
                let tag = b.atom("badmatch");
                let v = b.copy(&Self::new(self.arena.clone(), id)?);
                let reason = b.tuple(vec![tag, v]);
                return Err(FormError::ErrorReason(b.finish(reason)));
            }
            return Self::new(self.arena.clone(), values[2]).map_err(FormError::from);
        }
        Err(FormError::Native(NativeTransferError::BadMatchBoolean(
            false,
        )))
    }
    pub fn record_fields(&self) -> Result<Vec<RawRecordField>, String> {
        list(&self.arena, self.id)?
            .into_iter()
            .map(|id| {
                let f = tuple(&self.arena, id, 3)?;
                Ok(RawRecordField {
                    name: atom(&self.arena, f[0])?.into(),
                    form: Self::new(self.arena.clone(), f[1])?,
                    cached_type: Self::new(self.arena.clone(), f[2])?,
                })
            })
            .collect()
    }
}
#[derive(Clone, Debug)]
pub enum NativeSiteKind {
    Spec,
    Type,
    Record,
    #[cfg(otp_domain = "29")]
    NativeRecord,
    Check,
}
#[derive(Clone, Debug)]
pub enum NativeFormScope {
    Named(TypeName),
    Contract(crate::declaration_key::DeclarationKey),
}
impl From<TypeName> for NativeFormScope {
    fn from(v: TypeName) -> Self {
        Self::Named(v)
    }
}
impl NativeFormScope {
    pub(crate) fn module(&self) -> Result<&str, crate::form_error::FormError> {
        match self {
            Self::Named(v) => Ok(&v.module),
            Self::Contract(key) => {
                let ErlangTerm::Tuple(fields) = key.raw().term() else {
                    unreachable!()
                };
                match &key.raw().arena().terms[fields[0].0] {
                    ErlangTerm::Atom(module) => Ok(module),
                    _ => Err(crate::form_error::FormError::Native(
                        crate::native_error::NativeTransferError::FunctionClause,
                    )),
                }
            }
        }
    }
    pub(crate) fn named(&self) -> &TypeName {
        match self {
            Self::Named(v) => v,
            Self::Contract(_) => unreachable!("type resolver site has a named scope"),
        }
    }
}
#[derive(Clone, Debug)]
pub struct NativeFormSite {
    pub kind: NativeSiteKind,
    pub scope: NativeFormScope,
    pub file: RawFormTerm,
}
impl NativeFormSite {
    pub fn resolver_site(&self) -> crate::resolver::Site {
        match self.kind {
            NativeSiteKind::Spec => crate::resolver::Site::Spec,
            NativeSiteKind::Type => crate::resolver::Site::Type(self.scope.named().clone()),
            NativeSiteKind::Record => crate::resolver::Site::Record,
            #[cfg(otp_domain = "29")]
            NativeSiteKind::NativeRecord => crate::resolver::Site::NativeRecord,
            NativeSiteKind::Check => crate::resolver::Site::Check,
        }
    }
}
#[derive(Clone, Debug)]
pub struct ExternalTypeEvent {
    pub target: TypeName,
    pub site: NativeFormSite,
    pub location: RawFormTerm,
}
impl ExternalTypeEvent {
    pub fn to_arena(&self) -> RawFormTerm {
        let mut b = crate::form_error::ReasonBuilder::new();
        let mfa = |t: &TypeName, b: &mut crate::form_error::ReasonBuilder| {
            let m = b.atom(&t.module);
            let f = b.atom(&t.name);
            let a = b.integer(t.arity);
            b.tuple(vec![m, f, a])
        };
        let target = mfa(&self.target, &mut b);
        let file = b.copy(&self.site.file);
        let loc = b.copy(&self.location);
        let origin = match &self.site.scope {
            #[cfg(otp_domain = "29")]
            NativeFormScope::Named(name) if matches!(self.site.kind, NativeSiteKind::NativeRecord) => {
                let module=b.atom(&name.module);let name=b.atom(&name.name);b.tuple(vec![module,name])
            }
            NativeFormScope::Named(name) => mfa(name, &mut b),
            NativeFormScope::Contract(key) => b.copy(key.raw()),
        };
        let attrs = b.tuple(vec![file, loc, origin]);
        let root = b.tuple(vec![target, attrs]);
        b.finish(root)
    }
}
pub(crate) fn annotation_location(
    raw: &RawFormTerm,
) -> Result<RawFormTerm, crate::form_error::FormError> {
    use crate::{
        form_error::{FormError, ReasonBuilder},
        native_error::NativeTransferError,
    };
    let a = &raw.arena;
    let nonnegative = |id: TermId| matches!(&a.terms[id.0],ErlangTerm::Integer(s)if s.parse::<num_bigint::BigInt>().is_ok_and(|n|n>=0.into()));
    match raw.term() {
        ErlangTerm::Integer(_) if nonnegative(raw.id) => return Ok(raw.clone()),
        ErlangTerm::Tuple(v)
            if v.len() == 2
                && nonnegative(v[0])
                && matches!(&a.terms[v[1].0],ErlangTerm::Integer(s)if s.parse::<num_bigint::BigInt>().is_ok_and(|n|n>=1.into())) =>
        {
            return Ok(raw.clone());
        }
        _ => {}
    }
    let mut id = raw.id;
    loop {
        match &a.terms[id.0] {
            ErlangTerm::Nil => {
                let mut b = ReasonBuilder::new();
                let root = b.integer(0);
                return Ok(b.finish(root));
            }
            ErlangTerm::List { items, tail } => {
                for item in items {
                    if let ErlangTerm::Tuple(fields) = &a.terms[item.0]
                        && fields.first().is_some_and(
                            |k| matches!(&a.terms[k.0],ErlangTerm::Atom(s)if s=="location"),
                        )
                    {
                        if fields.len() == 2 {
                            return RawFormTerm::new(a.clone(), fields[1]).map_err(FormError::from);
                        }
                        let mut b = ReasonBuilder::new();
                        let reason = b.atom("try_clause");
                        let payload = b.copy(&RawFormTerm {
                            arena: a.clone(),
                            id: *item,
                        });
                        let root = b.tuple(vec![reason, payload]);
                        return Err(FormError::ErrorReason(b.finish(root)));
                    }
                }
                id = *tail;
            }
            _ => return Err(FormError::Native(NativeTransferError::Badarg)),
        }
    }
}

use crate::{
    core::{ErlangTerm, TermId},
    form_error::ReasonBuilder,
    native_form_environment::RawFormTerm,
    native_map_order,
    native_runtime_hash::{NativeAtomLayout, NativeHashKey, RuntimeHashError},
    raw_literal::{RawLiteral, RawLiteralArena},
    warning_payload::NativeWarning,
};
use std::collections::BTreeMap;

#[derive(Debug)]
pub enum CacheError {
    Boundary(String),
    FunctionClause,
    CaseClause(RawFormTerm),
    BadGenerator(RawFormTerm),
    BadMap(RawFormTerm),
    Runtime(RuntimeHashError),
}
impl From<RuntimeHashError> for CacheError {
    fn from(value: RuntimeHashError) -> Self {
        Self::Runtime(value)
    }
}
type Result<T> = std::result::Result<T, CacheError>;

#[derive(Clone, Debug)]
pub enum CachedWarnings {
    Unavailable,
    Available(WarningCache),
    Retained(RawFormTerm),
}
#[derive(Clone, Debug, Default)]
pub struct WarningCache {
    entries: BTreeMap<RawLiteral, RawFormTerm>,
}
fn child(raw: &RawFormTerm, id: TermId) -> Result<RawFormTerm> {
    RawFormTerm::new(raw.arena().clone(), id).map_err(CacheError::Boundary)
}
fn fields<const N: usize>(raw: &RawFormTerm) -> Result<[TermId; N]> {
    let ErlangTerm::Tuple(fields) = raw.term() else {
        return Err(CacheError::FunctionClause);
    };
    fields
        .as_slice()
        .try_into()
        .map_err(|_| CacheError::FunctionClause)
}
fn warning_module(warning: &NativeWarning) -> Result<RawLiteral> {
    let [_, info, _] = fields::<3>(&warning.raw)?;
    let info = child(&warning.raw, info)?;
    let [_, _, origin] = fields::<3>(&info)?;
    let origin = child(&info, origin)?;
    let module = match origin.term() {
        ErlangTerm::Tuple(fields) if fields.len() == 3 => child(&origin, fields[0])?,
        _ => origin,
    };
    RawLiteral::new(module).map_err(CacheError::Boundary)
}
fn proper_list(raw: &RawFormTerm) -> Result<Vec<RawFormTerm>> {
    let mut current = raw.id();
    let mut values = Vec::new();
    loop {
        match &raw.arena().terms[current.0] {
            ErlangTerm::Nil => return Ok(values),
            ErlangTerm::List { items, tail } => {
                for item in items {
                    values.push(child(raw, *item)?);
                }
                current = *tail;
            }
            _ => return Err(CacheError::BadGenerator(child(raw, current)?)),
        }
    }
}
fn fold_list(raw: &RawFormTerm) -> Result<Vec<RawFormTerm>> {
    proper_list(raw).map_err(|e| match raw.term() {
        ErlangTerm::List { .. } => CacheError::FunctionClause,
        _ => match e {
            CacheError::BadGenerator(tail) => CacheError::CaseClause(tail),
            e => e,
        },
    })
}
impl CachedWarnings {
    pub fn from_raw(raw: &RawFormTerm) -> Result<Self> {
        if matches!(raw.term(), ErlangTerm::Atom(atom) if atom == "none") {
            return Ok(Self::Unavailable);
        }
        let ErlangTerm::Map(entries) = raw.term() else {
            return Ok(Self::Retained(raw.clone()));
        };
        let arena = RawLiteralArena::new(raw.arena().clone()).map_err(CacheError::Boundary)?;
        let entries = entries
            .iter()
            .map(|(key, value)| {
                Ok((
                    arena.at(*key).map_err(CacheError::Boundary)?,
                    child(raw, *value)?,
                ))
            })
            .collect::<Result<_>>()?;
        Ok(Self::Available(WarningCache { entries }))
    }
    pub fn is_available(&self) -> bool {
        match self {
            Self::Unavailable => false,
            Self::Retained(raw) => !matches!(raw.term(), ErlangTerm::Atom(a) if a == "none"),
            Self::Available(_) => true,
        }
    }
    pub fn raw(&self) -> RawFormTerm {
        if let Self::Retained(raw) = self {
            return raw.clone();
        }
        let mut builder = ReasonBuilder::new();
        let root = match self {
            Self::Unavailable => builder.atom("none"),
            Self::Retained(_) => unreachable!(),
            Self::Available(cache) => {
                let rows = cache
                    .entries
                    .iter()
                    .map(|(module, warnings)| (builder.copy(module.raw()), builder.copy(warnings)))
                    .collect();
                builder.push(ErlangTerm::Map(rows))
            }
        };
        builder.finish(root)
    }
    pub fn without_modules(&self, modules: &[RawLiteral]) -> Result<Self> {
        match self {
            Self::Unavailable => Err(CacheError::BadMap(self.raw())),
            Self::Retained(raw) => match Self::from_raw(raw)? {
                Self::Retained(_) => Err(CacheError::BadMap(raw.clone())),
                cache => cache.without_modules(modules),
            },
            Self::Available(cache) => {
                let mut entries = cache.entries.clone();
                for module in modules {
                    entries.remove(module);
                }
                Ok(Self::Available(WarningCache { entries }))
            }
        }
    }
    pub fn merge_raw(
        collected: &RawFormTerm,
        unknown: &RawFormTerm,
        old: &RawFormTerm,
    ) -> Result<Self> {
        if matches!(collected.term(), ErlangTerm::Atom(a) if a == "none") {
            return Ok(Self::Retained(old.clone()));
        }
        let rows = fold_list(collected)?;
        let warnings = rows
            .into_iter()
            .map(|raw| NativeWarning { raw })
            .collect::<Vec<_>>();
        // Reach warning shape errors before processing the unknown list.
        for warning in &warnings {
            warning_module(warning)?;
        }
        let unknown = fold_list(unknown)?
            .into_iter()
            .map(|row| {
                let [module, warning] = fields::<2>(&row)?;
                Ok((
                    child(&row, module)?,
                    NativeWarning {
                        raw: child(&row, warning)?,
                    },
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let old = Self::from_raw(old)?;
        Self::merge(Some(&warnings), &unknown, &old)
    }
    pub fn merge(
        collected: Option<&[NativeWarning]>,
        unknown: &[(RawFormTerm, NativeWarning)],
        old: &Self,
    ) -> Result<Self> {
        let Some(collected) = collected else {
            return Ok(old.clone());
        };
        let mut lists: BTreeMap<RawLiteral, Vec<&RawFormTerm>> = BTreeMap::new();
        for warning in collected {
            lists
                .entry(warning_module(warning)?)
                .or_default()
                .push(&warning.raw);
        }
        for (module, warning) in unknown {
            lists
                .entry(RawLiteral::new(module.clone()).map_err(CacheError::Boundary)?)
                .or_default()
                .push(&warning.raw);
        }
        let mut entries = lists
            .into_iter()
            .map(|(module, warnings)| {
                let mut builder = ReasonBuilder::new();
                let warnings = warnings
                    .into_iter()
                    .rev()
                    .map(|warning| builder.copy(warning))
                    .collect();
                let root = builder.list(warnings);
                (module, builder.finish(root))
            })
            .collect::<BTreeMap<_, _>>();
        let resolved;
        let old = if let Self::Retained(raw) = old {
            resolved = Self::from_raw(raw)?;
            &resolved
        } else {
            old
        };
        if let Self::Retained(raw) = old {
            return Err(CacheError::BadMap(raw.clone()));
        }
        if let Self::Available(old) = old {
            entries.extend(
                old.entries
                    .iter()
                    .map(|(module, values)| (module.clone(), values.clone())),
            );
        }
        Ok(Self::Available(WarningCache { entries }))
    }
    pub fn select(&self, modules: &[RawLiteral], layout: &NativeAtomLayout) -> Result<RawFormTerm> {
        let selected = self.selected_rows(modules, layout)?;
        let mut builder = ReasonBuilder::new();
        let rows = selected
            .iter()
            .map(|warning| builder.copy(warning))
            .collect();
        let root = builder.list(rows);
        Ok(builder.finish(root))
    }
    fn selected_rows(
        &self,
        modules: &[RawLiteral],
        layout: &NativeAtomLayout,
    ) -> Result<Vec<RawFormTerm>> {
        if let Self::Retained(raw) = self {
            return match Self::from_raw(raw)? {
                Self::Retained(_) => Err(CacheError::BadMap(raw.clone())),
                cache => cache.selected_rows(modules, layout),
            };
        }
        let Self::Available(cache) = self else {
            return Err(CacheError::BadMap(self.raw()));
        };
        if !modules.is_empty() {
            let mut selected = Vec::new();
            for module in modules {
                if let Some(warnings) = cache.entries.get(module) {
                    selected.extend(proper_list(warnings)?);
                }
            }
            return Ok(selected);
        }
        let entries = cache
            .entries
            .iter()
            .map(|(module, warnings)| Ok((NativeHashKey::from_raw(module.raw())?, warnings)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        let keys = native_map_order::ordered_keys(layout, entries.keys().cloned())?;
        let mut selected = Vec::new();
        for key in keys {
            selected.extend(proper_list(entries[&key])?);
        }
        Ok(selected)
    }
}

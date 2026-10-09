use crate::{
    core::{ErlangTerm, ErlangTerms},
    environment,
    form_error::FormError,
    form_exports::{FormExports, VariableTable},
    forms::Forms,
    lattice::Type,
    native_form_environment::{ExternalTypeEvent, NativeFormSite},
    program::ModuleId,
    resolver::{FormCache, ModuleTableHandle, OrderingInput, Resolver},
};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone)]
pub struct FormContext {
    tables: BTreeMap<String, ModuleTableHandle>,
    ordering_inputs: BTreeMap<String, OrderingInput>,
    exported_types: Arc<ErlangTerms>,
}
pub enum FormOperation {
    Resolve(VariableTable),
    WithoutRemote,
    CheckRemote,
    CheckRecordFields(VariableTable),
}
pub struct FormOutcome {
    pub value: Option<Type>,
    pub cache: FormCache,
    pub external_types: Vec<ExternalTypeEvent>,
}
#[derive(Debug)]
pub struct FormFailure {
    pub error: FormError,
    pub external_types: Vec<ExternalTypeEvent>,
}
impl From<FormError> for FormFailure {
    fn from(error: FormError) -> Self {
        Self {
            error,
            external_types: Vec::new(),
        }
    }
}
impl From<String> for FormFailure {
    fn from(error: String) -> Self {
        FormError::from(error).into()
    }
}
impl From<&str> for FormFailure {
    fn from(error: &str) -> Self {
        FormError::from(error).into()
    }
}
impl std::fmt::Display for FormFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}
impl std::error::Error for FormFailure {}
impl FormContext {
    pub fn new(
        tables: BTreeMap<String, ModuleTableHandle>,
        exported_types: Arc<ErlangTerms>,
    ) -> Self {
        let ordering_inputs = tables.iter()
            .map(|(name, table)| (name.clone(), OrderingInput::new(table.raw())))
            .collect();
        Self {
            tables,
            ordering_inputs,
            exported_types,
        }
    }
    pub fn from_type_tables(
        tables: Arc<ErlangTerms>,
        exports: Arc<ErlangTerms>,
    ) -> Result<Self, String> {
        let ErlangTerm::Map(entries) = &tables.terms[tables.root.0] else {
            return Err("module type tables root must be a map".into());
        };
        let mut handles = BTreeMap::new();
        for (name, root) in entries {
            let ErlangTerm::Atom(name) = &tables.terms[name.0] else {
                return Err("module type table key must be an atom".into());
            };
            handles.insert(name.clone(), ModuleTableHandle::at(tables.clone(), *root)?);
        }
        Ok(Self::new(handles, exports))
    }
    /// Cached tables override the current dictionary, exactly as native lookup_module_types.
    pub fn execute(
        &self,
        site: &NativeFormSite,
        form: &crate::native_form_environment::RawFormTerm,
        cache: &FormCache,
        operation: FormOperation,
    ) -> Result<FormOutcome, FormFailure> {
        let supplied_cache = matches!(
            &operation,
            FormOperation::Resolve(..) | FormOperation::CheckRecordFields(..)
        );
        if supplied_cache && !cache.interop_complete() {
            return Err("cache lacks original metadata handles".into());
        }
        let mut tables = self.tables.clone();
        if supplied_cache {
            tables.extend(
                cache
                    .cached_tables()
                    .iter()
                    .map(|(name, table)| (name.clone(), table.clone())),
            );
        }
        let module = site.scope.module()?;
        let mut forms = Forms::default();
        let mut names = tables.keys().cloned().collect::<Vec<_>>();
        let mid = if let Some(i) = names.iter().position(|n| n == module) {
            ModuleId(i)
        } else {
            let mid = ModuleId(names.len());
            names.push(module.into());
            mid
        };
        // Parse only the requested source; metadata bodies are parsed on lookup.
        let id = forms.defer_raw(mid, form);
        let mut resolver = Resolver::new(&forms, &names);
        let mut ordering_roots = tables.iter().map(|(name, table)| {
            if supplied_cache && cache.cached_tables().contains_key(name) {
                OrderingInput::new(table.raw())
            } else {
                self.ordering_inputs[name].clone()
            }
        }).collect::<Vec<_>>();
        ordering_roots.push(OrderingInput::new(form.clone()));
        resolver.set_ordering_inputs(ordering_roots);
        resolver.set_native_site(site.clone());
        resolver.set_source_tables(tables);
        resolver.set_exported_types(environment::exported_type_names(&self.exported_types)?);
        let result = (|| -> Result<(Option<Type>, FormCache), FormError> {
            let mut exports = FormExports::new(&mut resolver);
            Ok(match operation {
                FormOperation::Resolve(variables) => {
                    let (value, cache) =
                        exports.t_from_form(id, site.resolver_site(), variables, cache)?;
                    (Some(value), cache)
                }
                FormOperation::CheckRecordFields(variables) => {
                    (None, exports.t_check_record_fields(id, &variables, cache)?)
                }
                FormOperation::WithoutRemote => (
                    Some(exports.t_from_form_without_remote(id, site.resolver_site())?),
                    cache.clone(),
                ),
                FormOperation::CheckRemote => {
                    exports.t_from_form_check_remote_at(id, site.resolver_site())?;
                    (None, cache.clone())
                }
            })
        })();
        let external_types = resolver.external_types;
        match result {
            Ok((value, cache)) => Ok(FormOutcome {
                value,
                cache,
                external_types,
            }),
            Err(error) => Err(FormFailure {
                error,
                external_types,
            }),
        }
    }
}

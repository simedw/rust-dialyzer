use crate::{
    forms::FormId,
    lattice::{Type, VariableId},
    resolver::{FormCache, Resolver, Site},
};
use std::collections::BTreeMap;

pub type VariableTable = BTreeMap<VariableId, Type>;
pub fn var_table_new() -> VariableTable {
    BTreeMap::new()
}
pub fn cache_new() -> FormCache {
    FormCache::new()
}

/// The caller owns cache values explicitly, as in the native exported API.
/// Each call restores the resolver's prior internal cache even on a failed resolution.
pub struct FormExports<'r, 'f> {
    resolver: &'r mut Resolver<'f>,
}
impl<'r, 'f> FormExports<'r, 'f> {
    pub fn new(resolver: &'r mut Resolver<'f>) -> Self {
        Self { resolver }
    }
    fn with_cache<T>(
        &mut self,
        cache: &FormCache,
        operation: impl FnOnce(&mut Resolver<'f>) -> Result<T, crate::form_error::FormError>,
    ) -> Result<(T, FormCache), crate::form_error::FormError> {
        let previous = self.resolver.replace_cache(cache.clone());
        let result = operation(self.resolver);
        let updated = self.resolver.replace_cache(previous);
        result.map(|value| (value, updated))
    }
    pub fn t_from_form(
        &mut self,
        form: FormId,
        site: Site,
        variables: VariableTable,
        cache: &FormCache,
    ) -> Result<(Type, FormCache), crate::form_error::FormError> {
        self.with_cache(cache, |resolver| resolver.resolve(form, site, variables))
    }
    pub fn t_from_form_without_remote(
        &mut self,
        form: FormId,
        site: Site,
    ) -> Result<Type, crate::form_error::FormError> {
        self.with_cache(&cache_new(), |resolver| {
            resolver.resolve_without_remote(form, site)
        })
        .map(|(value, _)| value)
    }
    pub fn t_from_form_check_remote(
        &mut self,
        form: FormId,
    ) -> Result<(), crate::form_error::FormError> {
        self.with_cache(&cache_new(), |resolver| resolver.check_remote(form))
            .map(|_| ())
    }
    pub fn t_from_form_check_remote_at(
        &mut self,
        form: FormId,
        site: Site,
    ) -> Result<(), crate::form_error::FormError> {
        self.with_cache(&cache_new(), |resolver| {
            resolver.check_remote_at(form, site)
        })
        .map(|_| ())
    }
    pub fn t_check_record_fields(
        &mut self,
        form: FormId,
        variables: &VariableTable,
        cache: &FormCache,
    ) -> Result<FormCache, crate::form_error::FormError> {
        self.with_cache(cache, |resolver| {
            resolver.check_record_fields(form, variables)
        })
        .map(|(_, cache)| cache)
    }
}

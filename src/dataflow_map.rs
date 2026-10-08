use crate::{
    dataflow_key::DataflowKey,
    lattice::{LatticeError, Type},
};
use std::{collections::BTreeSet, sync::Arc};

#[derive(Clone, Debug, Default)]
pub struct DataflowMap {
    types: im::OrdMap<DataflowKey, Type>,
    substitutions: im::OrdMap<DataflowKey, DataflowKey>,
    modified: im::Vector<DataflowKey>,
    scope: Option<Arc<ScopeIdentity>>,
}
#[derive(Debug)]
struct ScopeIdentity;

pub struct JoinScope {
    base: DataflowMap,
    outer_modified: im::Vector<DataflowKey>,
    outer_scope: Option<Arc<ScopeIdentity>>,
    identity: Arc<ScopeIdentity>,
}

impl DataflowMap {
    pub fn types(&self) -> &im::OrdMap<DataflowKey, Type> {
        &self.types
    }
    pub fn substitutions(&self) -> &im::OrdMap<DataflowKey, DataflowKey> {
        &self.substitutions
    }
    pub fn modified(&self) -> &im::Vector<DataflowKey> {
        &self.modified
    }
    fn resolve(&self, mut key: DataflowKey) -> DataflowKey {
        while let Some(next) = self.substitutions.get(&key) {
            key = *next;
        }
        key
    }
    pub fn lookup(&self, key: impl Into<DataflowKey>) -> Type {
        let key = key.into();
        self.types
            .get(&self.resolve(key))
            .cloned()
            .unwrap_or_else(Type::none)
    }
    fn store(&mut self, key: DataflowKey, value: Type) {
        self.types.insert(key, value);
        if self.scope.is_some() {
            self.modified.push_front(key);
        }
    }
    pub fn enter_type(&mut self, key: impl Into<DataflowKey>, value: Type) {
        let key = key.into();
        let key = self.resolve(key);
        if self.types.get(&key) != Some(&value) {
            self.store(key, value);
        }
    }
    pub fn enter_literal_substitution(&mut self, key: impl Into<DataflowKey>, value: Type) {
        let key = key.into();
        self.store(key, value);
    }
    pub fn enter_variable_substitution(
        &mut self,
        key: impl Into<DataflowKey>,
        value: impl Into<DataflowKey>,
    ) {
        let key = key.into();
        let value = value.into();
        if self.substitutions.contains_key(&value) {
            return;
        }
        if key != value {
            self.substitutions.insert(key, value);
            if self.scope.is_some() {
                self.modified.push_front(key);
            }
        }
    }
    pub fn join_all(&self, branches: &[Self]) -> Result<Self, LatticeError> {
        let keys: BTreeSet<_> = self
            .types
            .keys()
            .chain(self.substitutions.keys())
            .copied()
            .collect();
        self.join_keys(keys, branches)
    }
    fn join_keys(
        &self,
        keys: BTreeSet<DataflowKey>,
        branches: &[Self],
    ) -> Result<Self, LatticeError> {
        let mut changes = Vec::new();
        for key in keys {
            let mut joined = Type::none();
            for branch in branches {
                if joined.is_any() {
                    break;
                }
                joined = branch.lookup(key).join(&joined)?;
            }
            if self.lookup(key) != joined {
                changes.push((key, joined));
            }
        }
        let mut output = self.clone();
        for (key, value) in changes {
            output.enter_type(key, value);
        }
        Ok(output)
    }
    pub fn begin_join(&self) -> JoinScope {
        let identity = Arc::new(ScopeIdentity);
        let mut base = self.clone();
        base.modified.clear();
        base.scope = Some(identity.clone());
        JoinScope {
            base,
            outer_modified: self.modified.clone(),
            outer_scope: self.scope.clone(),
            identity,
        }
    }
}
impl JoinScope {
    pub fn branch(&self) -> DataflowMap {
        self.base.clone()
    }
    pub fn finish(self, branches: &[DataflowMap]) -> Result<DataflowMap, LatticeError> {
        if branches.iter().any(|branch| {
            branch
                .scope
                .as_ref()
                .is_none_or(|identity| !Arc::ptr_eq(identity, &self.identity))
        }) {
            return Err(LatticeError::BadMatchBoolean(false));
        }
        let keys: BTreeSet<_> = if branches.is_empty() {
            self.base
                .types
                .keys()
                .chain(self.base.substitutions.keys())
                .copied()
                .collect()
        } else {
            branches
                .iter()
                .flat_map(|branch| branch.modified.iter())
                .filter(|key| {
                    self.base.types.contains_key(key) || self.base.substitutions.contains_key(key)
                })
                .copied()
                .collect()
        };
        let mut output = self.base.join_keys(keys, branches)?;
        output.scope = self.outer_scope;
        output.modified.append(self.outer_modified);
        Ok(output)
    }
}

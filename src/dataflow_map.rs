use crate::{
    dataflow_key::DataflowKey,
    dataflow_trie::{ConsList, LabelTrie},
    labels::CoreLabel,
    lattice::{LatticeError, Type},
};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct DataflowMap {
    types: KeyTable<Type>,
    substitutions: KeyTable<DataflowKey>,
    modified: ConsList<DataflowKey>,
    scope: Option<Arc<ScopeIdentity>>,
    /// Under `verify_fast_paths()`, the same map in the representation this
    /// replaced; read points assert both agree.
    shadow: Option<Box<Shadow>>,
}
impl Default for DataflowMap {
    /// Starts the shadow map when verification is on.
    fn default() -> Self {
        Self {
            types: KeyTable::default(),
            substitutions: KeyTable::default(),
            modified: ConsList::default(),
            scope: None,
            shadow: Self::new_shadow(),
        }
    }
}
#[derive(Debug)]
struct ScopeIdentity;

/// Keys in `DataflowKey` order: every synthetic key sorts below every Core
/// label, so synthetic keys (rare) come first, then the Core label trie.
#[derive(Clone)]
struct KeyTable<V> {
    core: LabelTrie<V>,
    synthetic: im::OrdMap<DataflowKey, V>,
}
impl<V: Clone> Default for KeyTable<V> {
    fn default() -> Self {
        Self {
            core: LabelTrie::default(),
            synthetic: im::OrdMap::new(),
        }
    }
}
impl<V: Clone> KeyTable<V> {
    fn get(&self, key: &DataflowKey) -> Option<&V> {
        match key {
            DataflowKey::Core(label) => self.core.get(label.0),
            DataflowKey::Synthetic(_) => self.synthetic.get(key),
        }
    }
    fn contains_key(&self, key: &DataflowKey) -> bool {
        self.get(key).is_some()
    }
    fn insert(&mut self, key: DataflowKey, value: V) {
        match key {
            DataflowKey::Core(label) => self.core.insert(label.0, value),
            DataflowKey::Synthetic(_) => {
                self.synthetic.insert(key, value);
            }
        }
    }
    fn iter(&self) -> impl Iterator<Item = (DataflowKey, &V)> + '_ {
        self.synthetic.iter().map(|(key, value)| (*key, value)).chain(
            self.core
                .iter()
                .map(|(label, value)| (DataflowKey::Core(CoreLabel(label)), value)),
        )
    }
    fn keys(&self) -> impl Iterator<Item = DataflowKey> + '_ {
        self.iter().map(|(key, _)| key)
    }
    fn to_ord_map(&self) -> im::OrdMap<DataflowKey, V> {
        self.iter().map(|(key, value)| (key, value.clone())).collect()
    }
}
impl<V: Clone + std::fmt::Debug> std::fmt::Debug for KeyTable<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

#[derive(Clone, Debug)]
struct Shadow {
    types: im::OrdMap<DataflowKey, Type>,
    substitutions: im::OrdMap<DataflowKey, DataflowKey>,
    modified: im::Vector<DataflowKey>,
}

pub struct JoinScope {
    base: DataflowMap,
    outer_modified: ConsList<DataflowKey>,
    outer_shadow_modified: Option<im::Vector<DataflowKey>>,
    outer_scope: Option<Arc<ScopeIdentity>>,
    identity: Arc<ScopeIdentity>,
}

/// Merges two ascending key sequences into one without duplicates.
fn union_sorted(
    a: impl Iterator<Item = DataflowKey>,
    b: impl Iterator<Item = DataflowKey>,
) -> Vec<DataflowKey> {
    let mut keys: Vec<_> = a.chain(b).collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

impl DataflowMap {
    fn new_shadow() -> Option<Box<Shadow>> {
        crate::lattice::verify_fast_paths().then(|| {
            Box::new(Shadow {
                types: im::OrdMap::new(),
                substitutions: im::OrdMap::new(),
                modified: im::Vector::new(),
            })
        })
    }
    /// The type bindings, materialized for probes and traces.
    pub fn types(&self) -> im::OrdMap<DataflowKey, Type> {
        self.types.to_ord_map()
    }
    /// The variable substitutions, materialized for probes and traces.
    pub fn substitutions(&self) -> im::OrdMap<DataflowKey, DataflowKey> {
        self.substitutions.to_ord_map()
    }
    /// Keys stored in the current join scope, most recent first.
    pub fn modified(&self) -> im::Vector<DataflowKey> {
        self.modified.iter().collect()
    }
    fn resolve(&self, mut key: DataflowKey) -> DataflowKey {
        while let Some(next) = self.substitutions.get(&key) {
            key = *next;
        }
        key
    }
    pub fn lookup(&self, key: impl Into<DataflowKey>) -> Type {
        let key = key.into();
        let found = self
            .types
            .get(&self.resolve(key))
            .cloned()
            .unwrap_or_else(Type::none);
        if let Some(shadow) = self.shadow() {
            assert_eq!(found, shadow.lookup(key), "dataflow map lookup");
        }
        found
    }
    fn shadow(&self) -> Option<&Shadow> {
        self.shadow.as_deref()
    }
    fn store(&mut self, key: DataflowKey, value: Type) {
        self.types.insert(key, value);
        if self.scope.is_some() {
            self.modified.push_front(key);
        }
    }
    pub fn enter_type(&mut self, key: impl Into<DataflowKey>, value: Type) {
        let key = key.into();
        if let Some(shadow) = &mut self.shadow {
            shadow.enter_type(key, value.clone(), self.scope.is_some());
        }
        let key = self.resolve(key);
        if self.types.get(&key) != Some(&value) {
            self.store(key, value);
        }
    }
    pub fn enter_literal_substitution(&mut self, key: impl Into<DataflowKey>, value: Type) {
        let key = key.into();
        if let Some(shadow) = &mut self.shadow {
            shadow.store(key, value.clone(), self.scope.is_some());
        }
        self.store(key, value);
    }
    pub fn enter_variable_substitution(
        &mut self,
        key: impl Into<DataflowKey>,
        value: impl Into<DataflowKey>,
    ) {
        let key = key.into();
        let value = value.into();
        if let Some(shadow) = &mut self.shadow {
            shadow.enter_variable_substitution(key, value, self.scope.is_some());
        }
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
        let keys = union_sorted(self.types.keys(), self.substitutions.keys());
        if let Some(shadow) = self.shadow() {
            assert!(keys.iter().copied().eq(shadow.all_keys()), "dataflow map join keys");
        }
        self.join_keys(keys, branches)
    }
    fn join_keys(&self, keys: Vec<DataflowKey>, branches: &[Self]) -> Result<Self, LatticeError> {
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
        output.check_shadow();
        Ok(output)
    }
    pub fn begin_join(&self) -> JoinScope {
        let identity = Arc::new(ScopeIdentity);
        let mut base = self.clone();
        base.modified.clear();
        base.scope = Some(identity.clone());
        let outer_shadow_modified = base.shadow.as_mut().map(|shadow| {
            let outer = shadow.modified.clone();
            shadow.modified.clear();
            outer
        });
        JoinScope {
            base,
            outer_modified: self.modified.clone(),
            outer_shadow_modified,
            outer_scope: self.scope.clone(),
            identity,
        }
    }
    /// Under `verify_fast_paths()`, asserts the contents, iteration order and
    /// modified sequence match the shadow map.
    fn check_shadow(&self) {
        if let Some(shadow) = self.shadow() {
            assert!(
                self.types.iter().eq(shadow.types.iter().map(|(k, v)| (*k, v))),
                "dataflow map types"
            );
            assert!(
                self.substitutions
                    .iter()
                    .eq(shadow.substitutions.iter().map(|(k, v)| (*k, v))),
                "dataflow map substitutions"
            );
            assert!(
                self.modified.iter().eq(shadow.modified.iter().copied()),
                "dataflow map modified"
            );
        }
    }
}
impl Shadow {
    fn resolve(&self, mut key: DataflowKey) -> DataflowKey {
        while let Some(next) = self.substitutions.get(&key) {
            key = *next;
        }
        key
    }
    fn lookup(&self, key: DataflowKey) -> Type {
        self.types
            .get(&self.resolve(key))
            .cloned()
            .unwrap_or_else(Type::none)
    }
    fn store(&mut self, key: DataflowKey, value: Type, scoped: bool) {
        self.types.insert(key, value);
        if scoped {
            self.modified.push_front(key);
        }
    }
    fn enter_type(&mut self, key: DataflowKey, value: Type, scoped: bool) {
        let key = self.resolve(key);
        if self.types.get(&key) != Some(&value) {
            self.store(key, value, scoped);
        }
    }
    fn enter_variable_substitution(&mut self, key: DataflowKey, value: DataflowKey, scoped: bool) {
        if self.substitutions.contains_key(&value) {
            return;
        }
        if key != value {
            self.substitutions.insert(key, value);
            if scoped {
                self.modified.push_front(key);
            }
        }
    }
    fn all_keys(&self) -> impl Iterator<Item = DataflowKey> {
        self.types
            .keys()
            .chain(self.substitutions.keys())
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
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
        let keys: Vec<DataflowKey> = if branches.is_empty() {
            union_sorted(self.base.types.keys(), self.base.substitutions.keys())
        } else {
            let mut keys: Vec<_> = branches
                .iter()
                .flat_map(|branch| branch.modified.iter())
                .filter(|key| {
                    self.base.types.contains_key(key) || self.base.substitutions.contains_key(key)
                })
                .collect();
            keys.sort_unstable();
            keys.dedup();
            keys
        };
        if let Some(shadow) = self.base.shadow() {
            let expected: Vec<DataflowKey> = if branches.is_empty() {
                shadow.all_keys().collect()
            } else {
                branches
                    .iter()
                    .flat_map(|branch| {
                        let branch = branch.shadow().expect("shadowed branch");
                        branch.modified.iter()
                    })
                    .filter(|key| {
                        shadow.types.contains_key(key) || shadow.substitutions.contains_key(key)
                    })
                    .copied()
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect()
            };
            assert_eq!(keys, expected, "dataflow map join scope keys");
            for branch in branches {
                branch.check_shadow();
            }
        }
        let mut output = self.base.join_keys(keys, branches)?;
        output.scope = self.outer_scope;
        output.modified.append(self.outer_modified);
        if let (Some(shadow), Some(outer)) = (&mut output.shadow, self.outer_shadow_modified) {
            shadow.modified.append(outer);
        }
        output.check_shadow();
        Ok(output)
    }
}

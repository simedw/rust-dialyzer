use crate::{
    call_transfer::Signature,
    dataflow_map::DataflowMap,
    labels::CoreLabel,
    lattice::{LatticeError, Type},
    program::{FunctionName, NodeRef},
    success_phase::FunctionTypes,
};
use crate::{
    native_dict::NativeDictOrder,
    worklist::{FunctionKey, Worklist},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};
#[derive(Clone, Debug)]
pub struct FunctionDefinition {
    pub key: FunctionKey,
    pub node: Option<NodeRef>,
    pub arity: usize,
    pub escaping: bool,
    pub name: Option<FunctionName>,
    pub raw_signature: Option<Signature>,
    pub callers: Vec<FunctionKey>,
}
#[derive(Clone)]
pub struct Entry {
    signature: Signature,
    called: bool,
    function_type: OnceLock<Type>,
}
impl Entry {
    fn new(signature: Signature, called: bool) -> Self {
        Self {
            signature,
            called,
            function_type: OnceLock::new(),
        }
    }
    pub fn signature(&self) -> &Signature {
        &self.signature
    }
    pub fn called(&self) -> bool {
        self.called
    }
    fn function_type(&self) -> Type {
        self.function_type
            .get_or_init(|| {
                Type::function(
                    Some(self.signature.arguments.clone()),
                    self.signature.result.clone(),
                )
            })
            .clone()
    }
}
impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.called == other.called && self.signature == other.signature
    }
}
impl Eq for Entry {}
impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple(if self.called { "Handled" } else { "NotHandled" })
            .field(&self.signature)
            .finish()
    }
}
#[derive(Debug)]
pub enum StateError {
    Missing(FunctionKey),
    NotHandled(FunctionKey),
    Arity {
        key: FunctionKey,
        expected: usize,
        actual: usize,
    },
    Lattice(LatticeError),
}
impl From<LatticeError> for StateError {
    fn from(e: LatticeError) -> Self {
        Self::Lattice(e)
    }
}
pub(crate) struct SignatureGeneration;

#[derive(Clone)]
pub struct FunctionState {
    signature_generation: std::sync::Arc<SignatureGeneration>,
    definitions: BTreeMap<FunctionKey, FunctionDefinition>,
    entries: BTreeMap<FunctionKey, Entry>,
    environments: BTreeMap<FunctionKey, DataflowMap>,
    order: Vec<FunctionKey>,
    homes: BTreeMap<CoreLabel, FunctionName>,
    work: Worklist,
    reachable: BTreeSet<CoreLabel>,
    warning_mode: bool,
}
impl std::fmt::Debug for FunctionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FunctionState")
            .field("definitions", &self.definitions).field("entries", &self.entries)
            .field("environments", &self.environments).field("order", &self.order)
            .field("homes", &self.homes).field("work", &self.work)
            .field("reachable", &self.reachable).field("warning_mode", &self.warning_mode).finish()
    }
}
impl FunctionState {
    /// Definitions are supplied in native cerl_trees:fold postorder, retaining occurrences.
    pub fn new(definitions: Vec<FunctionDefinition>) -> Self {
        let mut dict = NativeDictOrder::default();
        let mut entries = BTreeMap::new();
        let mut envs = BTreeMap::new();
        let mut defs = BTreeMap::new();
        let mut homes = BTreeMap::new();
        let mut pending = Vec::new();
        for d in definitions {
            dict.store(d.key);
            if let FunctionKey::Label(label) = d.key {
                if let Some(name) = &d.name {
                    pending.push(label);
                    for key in pending.drain(..) {
                        homes.insert(key, name.clone());
                    }
                } else {
                    pending.push(label);
                }
            }
            let signature = Signature {
                arguments: vec![
                    if d.escaping {
                        Type::any()
                    } else {
                        Type::none()
                    };
                    d.arity
                ],
                result: if d.key == FunctionKey::Top
                    || d.escaping && d.raw_signature.as_ref().is_some_and(|s| s.result.is_none())
                {
                    Type::none()
                } else {
                    Type::unit()
                },
            };
            let entry = Entry::new(signature, d.key == FunctionKey::Top || d.escaping);
            entries.insert(d.key, entry);
            envs.insert(d.key, DataflowMap::default());
            defs.insert(d.key, d);
        }
        let order = dict.keys();
        let initial = order
            .iter()
            .copied()
            .filter(|k| *k != FunctionKey::Top && defs[k].escaping && defs[k].name.is_some())
            .collect();
        Self {
            signature_generation: std::sync::Arc::new(SignatureGeneration),
            definitions: defs,
            entries,
            environments: envs,
            homes,
            order,
            work: Worklist::new(initial),
            reachable: BTreeSet::new(),
            warning_mode: false,
        }
    }
    pub(crate) fn signature_generation(&self) -> &std::sync::Arc<SignatureGeneration> {
        &self.signature_generation
    }
    pub fn definitions(&self) -> &BTreeMap<FunctionKey, FunctionDefinition> {
        &self.definitions
    }
    pub fn environments(&self) -> &BTreeMap<FunctionKey, DataflowMap> {
        &self.environments
    }
    pub fn entries(&self) -> &BTreeMap<FunctionKey, Entry> {
        &self.entries
    }
    pub fn order(&self) -> &[FunctionKey] {
        &self.order
    }
    pub fn homes(&self) -> &BTreeMap<CoreLabel, FunctionName> {
        &self.homes
    }
    pub fn work(&self) -> &Worklist {
        &self.work
    }
    pub fn reachable(&self) -> &BTreeSet<CoreLabel> {
        &self.reachable
    }
    pub fn warning_mode(&self) -> bool {
        self.warning_mode
    }
    pub fn pop_work(&mut self) -> Option<FunctionKey> {
        self.work.pop()
    }
    pub fn environment(&self, key: FunctionKey) -> Option<&DataflowMap> {
        self.environments.get(&key)
    }
    pub fn function_type(&self, key: FunctionKey) -> Result<Type, StateError> {
        Ok(self
            .entries
            .get(&key)
            .ok_or(StateError::Missing(key))?
            .function_type())
    }
    pub fn capture(&mut self, label: CoreLabel, map: DataflowMap) -> Result<Type, StateError> {
        let key = FunctionKey::Label(label);
        let ty = self.function_type(key)?;
        if !self.warning_mode {
            self.work.add(key);
            self.environments.insert(key, map);
            self.reachable.insert(label);
        }
        Ok(ty)
    }
    pub fn set_warning_mode(&mut self) {
        let list = self
            .order
            .iter()
            .copied()
            .filter(|k| match k {
                FunctionKey::Top => false,
                FunctionKey::Label(n) => {
                    self.definitions[k].name.is_some() || self.reachable.contains(n)
                }
            })
            .collect();
        self.work = Worklist::new(list);
        self.warning_mode = true;
    }
    pub fn forward_arguments(
        &mut self,
        key: FunctionKey,
        actual: &[Type],
    ) -> Result<bool, StateError> {
        let old = self.entries.get(&key).ok_or(StateError::Missing(key))?;
        let sig = old.signature();
        if sig.arguments.len() != actual.len() {
            return Err(StateError::Arity {
                key,
                expected: sig.arguments.len(),
                actual: actual.len(),
            });
        }
        let next = if !old.called() {
            actual.to_vec()
        } else {
            actual
                .iter()
                .zip(&sig.arguments)
                .map(|(x, y)| x.join(y))
                .collect::<Result<Vec<_>, _>>()?
        };
        if old.called() && Type::product(next.clone()) == Type::product(sig.arguments.clone()) {
            return Ok(false);
        }
        let result = sig.result.clone();
        self.signature_generation = std::sync::Arc::new(SignatureGeneration);
        self.entries.insert(
            key,
            Entry::new(
                Signature {
                    arguments: next,
                    result,
                },
                true,
            ),
        );
        self.work.add(key);
        Ok(true)
    }
    pub fn update_return(
        &mut self,
        key: FunctionKey,
        arguments: &[Type],
        result: &Type,
    ) -> Result<bool, StateError> {
        let definition = self.definitions.get(&key).ok_or(StateError::Missing(key))?;
        let entry = self.entries.get(&key).ok_or(StateError::Missing(key))?;
        if !entry.called() {
            return Err(StateError::NotHandled(key));
        }
        let old = entry.signature();
        if old.arguments.len() != arguments.len() {
            return Err(StateError::Arity {
                key,
                expected: old.arguments.len(),
                actual: arguments.len(),
            });
        }
        let constrained = if key == FunctionKey::Top {
            result.clone()
        } else if let Some(raw) = &definition.raw_signature {
            raw.result.meet(result)?
        } else {
            result.clone()
        };
        let result = constrained.limit(3)?;
        if old.arguments == arguments && old.result == result {
            return Ok(false);
        }
        let args = old.arguments.clone();
        let callers = definition.callers.clone();
        self.signature_generation = std::sync::Arc::new(SignatureGeneration);
        self.entries.insert(
            key,
            Entry::new(
                Signature {
                    arguments: args,
                    result,
                },
                true,
            ),
        );
        if !self.warning_mode && key != FunctionKey::Top {
            for caller in callers {
                if caller != FunctionKey::Top && self.definitions.contains_key(&caller) {
                    self.work.add(caller)
                }
            }
        }
        Ok(true)
    }
    pub fn function_types(&self) -> FunctionTypes {
        self.entries
            .iter()
            .filter_map(|(key, e)| match key {
                FunctionKey::Top => None,
                FunctionKey::Label(label) => {
                    let mut s = e.signature().clone();
                    if !e.called() {
                        s.result = Type::none()
                    }
                    Some((*label, s))
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod generation_tests {
    use super::*;
    #[test]
    fn signature_generations_isolate_clone_mutations_and_keep_noop_errors() {
        let key = FunctionKey::Label(CoreLabel(1));
        let mut a = FunctionState::new(vec![FunctionDefinition { key, node: None, arity: 1, escaping: false, name: None, raw_signature: None, callers: vec![] }]);
        let mut b = a.clone();
        let initial = a.signature_generation().clone();
        assert!(std::sync::Arc::ptr_eq(&initial, b.signature_generation()));
        assert!(a.forward_arguments(key, &[Type::atom("left")]).unwrap());
        assert!(b.forward_arguments(key, &[Type::atom("right")]).unwrap());
        assert!(!std::sync::Arc::ptr_eq(a.signature_generation(), b.signature_generation()));
        let before = a.signature_generation().clone();
        assert!(!a.forward_arguments(key, &[Type::atom("left")]).unwrap());
        assert!(std::sync::Arc::ptr_eq(&before, a.signature_generation()));
        assert!(a.forward_arguments(key, &[]).is_err());
        assert!(std::sync::Arc::ptr_eq(&before, a.signature_generation()));
        assert!(a.update_return(key, &[Type::atom("left")], &Type::atom("ok")).unwrap());
        assert!(!std::sync::Arc::ptr_eq(&before, a.signature_generation()));
        assert!(!format!("{a:?}").contains("signature_generation"));
    }
}

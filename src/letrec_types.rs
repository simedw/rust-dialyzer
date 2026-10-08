use crate::{
    dataflow_key::DataflowKey,
    function_state::SignatureGeneration,
    labels::CoreLabel,
    lattice::Type,
};
use std::{collections::BTreeMap, ops::Deref, sync::Arc};

#[derive(Clone, Debug, Default)]
pub struct LetrecLinks(Arc<BTreeMap<CoreLabel, CoreLabel>>);
impl From<BTreeMap<CoreLabel, CoreLabel>> for LetrecLinks {
    fn from(value: BTreeMap<CoreLabel, CoreLabel>) -> Self { Self(Arc::new(value)) }
}
impl Deref for LetrecLinks {
    type Target = BTreeMap<CoreLabel, CoreLabel>;
    fn deref(&self) -> &Self::Target { &self.0 }
}
impl<'a> IntoIterator for &'a LetrecLinks {
    type Item = (&'a CoreLabel, &'a CoreLabel);
    type IntoIter = std::collections::btree_map::Iter<'a, CoreLabel, CoreLabel>;
    fn into_iter(self) -> Self::IntoIter { self.0.iter() }
}

#[derive(Clone)]
struct RefreshStamp {
    links: LetrecLinks,
    signatures: Arc<SignatureGeneration>,
}
#[derive(Clone, Default)]
pub struct LetrecTypes {
    values: Arc<BTreeMap<DataflowKey, Type>>,
    refreshed: Option<RefreshStamp>,
}
impl From<BTreeMap<DataflowKey, Type>> for LetrecTypes {
    fn from(values: BTreeMap<DataflowKey, Type>) -> Self {
        Self { values: Arc::new(values), refreshed: None }
    }
}
impl Deref for LetrecTypes {
    type Target = BTreeMap<DataflowKey, Type>;
    fn deref(&self) -> &Self::Target { &self.values }
}
impl AsRef<BTreeMap<DataflowKey, Type>> for LetrecTypes {
    fn as_ref(&self) -> &BTreeMap<DataflowKey, Type> { &self.values }
}
impl std::fmt::Debug for LetrecTypes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.values.fmt(f) }
}
impl LetrecTypes {
    pub fn make_mut(&mut self) -> &mut BTreeMap<DataflowKey, Type> {
        self.refreshed = None;
        Arc::make_mut(&mut self.values)
    }
    pub(crate) fn is_current(&self, links: &LetrecLinks, signatures: &Arc<SignatureGeneration>) -> bool {
        self.refreshed.as_ref().is_some_and(|stamp|
            Arc::ptr_eq(&stamp.links.0, &links.0) && Arc::ptr_eq(&stamp.signatures, signatures))
    }
    pub(crate) fn mark_current(&mut self, links: &LetrecLinks, signatures: &Arc<SignatureGeneration>) {
        self.refreshed = Some(RefreshStamp { links: links.clone(), signatures: signatures.clone() });
    }
    #[cfg(test)]
    pub(crate) fn shares_storage(&self, other: &Self) -> bool { Arc::ptr_eq(&self.values, &other.values) }
    #[cfg(test)]
    pub(crate) fn storage_address(&self) -> *const BTreeMap<DataflowKey, Type> { Arc::as_ptr(&self.values) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refresh_stamp_requires_both_retained_immutable_owners() {
        let links = LetrecLinks::from(BTreeMap::from([(CoreLabel(1), CoreLabel(2))]));
        let generation = Arc::new(SignatureGeneration);
        let mut types = LetrecTypes::default();
        assert!(!types.is_current(&links, &generation));
        types.mark_current(&links, &generation);
        assert!(types.is_current(&links.clone(), &generation.clone()));
        assert!(!types.is_current(&LetrecLinks::from((*links).clone()), &generation));
        assert!(!types.is_current(&links, &Arc::new(SignatureGeneration)));
    }
    #[test]
    fn mutable_table_access_invalidates_only_its_branch() {
        let links = LetrecLinks::default();
        let generation = Arc::new(SignatureGeneration);
        let mut types = LetrecTypes::from(BTreeMap::from([(CoreLabel(1).into(), Type::atom("old"))]));
        types.mark_current(&links, &generation);
        let mut branch = types.clone();
        assert!(types.shares_storage(&branch));
        branch.make_mut().insert(CoreLabel(1).into(), Type::atom("new"));
        assert!(!types.shares_storage(&branch));
        assert!(types.is_current(&links, &generation));
        assert!(!branch.is_current(&links, &generation));
        assert_eq!(types[&CoreLabel(1).into()], Type::atom("old"));
        assert_eq!(branch[&CoreLabel(1).into()], Type::atom("new"));
    }
}

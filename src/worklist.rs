use std::collections::{BTreeSet, VecDeque};
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FunctionKey {
    Label(crate::labels::CoreLabel),
    Top,
}
#[derive(Clone, Debug, Default)]
pub struct Worklist {
    front: VecDeque<FunctionKey>,
    pending: VecDeque<FunctionKey>,
    membership: BTreeSet<FunctionKey>,
}
impl Worklist {
    pub fn new(initial: Vec<FunctionKey>) -> Self {
        Self {
            membership: initial.iter().copied().collect(),
            front: initial.into(),
            pending: VecDeque::new(),
        }
    }
    pub fn add(&mut self, key: FunctionKey) {
        if self.membership.insert(key) {
            self.pending.push_back(key)
        }
    }
    pub fn pop(&mut self) -> Option<FunctionKey> {
        if self.front.is_empty() {
            std::mem::swap(&mut self.front, &mut self.pending)
        }
        let key = self.front.pop_front()?;
        self.membership.remove(&key);
        Some(key)
    }
    pub fn ordered(&self) -> Vec<FunctionKey> {
        self.front
            .iter()
            .chain(self.pending.iter())
            .copied()
            .collect()
    }
    pub fn members(&self) -> &BTreeSet<FunctionKey> {
        &self.membership
    }
}

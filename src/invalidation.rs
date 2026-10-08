use std::collections::{BTreeMap, BTreeSet};

pub type ReverseDependencies = BTreeMap<String, BTreeSet<String>>;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Invalidation {
    pub analyze: BTreeSet<String>,
    pub discard: BTreeSet<String>,
    pub remaining_dependencies: ReverseDependencies,
}

pub fn expand(
    changed: &BTreeSet<String>,
    removed: &BTreeSet<String>,
    dependencies: &ReverseDependencies,
    current: &BTreeSet<String>,
) -> Invalidation {
    let seeds: BTreeSet<_> = changed.union(removed).cloned().collect();
    let mut discard = seeds.clone();
    let mut pending: Vec<_> = seeds.iter().cloned().collect();
    while let Some(module) = pending.pop() {
        if let Some(dependents) = dependencies.get(&module) {
            for dependent in dependents {
                if discard.insert(dependent.clone()) {
                    pending.push(dependent.clone());
                }
            }
        }
    }
    let analyze = discard
        .intersection(current)
        .filter(|module| !removed.contains(*module))
        .cloned()
        .collect();
    let remaining_dependencies = dependencies
        .iter()
        .filter_map(|(module, dependents)| {
            let remaining: BTreeSet<_> = dependents.difference(&seeds).cloned().collect();
            (!remaining.is_empty()).then(|| (module.clone(), remaining))
        })
        .collect();
    Invalidation {
        analyze,
        discard,
        remaining_dependencies,
    }
}

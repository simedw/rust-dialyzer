use crate::invalidation::{ReverseDependencies, expand};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    Fresh,
    MissingWarnings,
    StaleVersion,
    MissingFile,
    Changed,
    ChangedWithoutWarnings,
    WarningProfileChanged,
}

#[derive(Debug, Deserialize)]
pub struct Inputs {
    pub check: Check,
    pub current: BTreeSet<String>,
    pub previous: BTreeSet<String>,
    pub changed: BTreeSet<String>,
    pub removed: BTreeSet<String>,
    pub dependencies: ReverseDependencies,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Plan {
    pub analyze: BTreeSet<String>,
    pub changed_or_removed: Option<BTreeSet<String>>,
    pub discard: BTreeSet<String>,
    pub remaining_dependencies: ReverseDependencies,
    pub reuse_stored_warnings: bool,
}

pub fn plan(input: &Inputs) -> Plan {
    match input.check {
        Check::Fresh => Plan {
            analyze: BTreeSet::new(),
            changed_or_removed: Some(BTreeSet::new()),
            discard: BTreeSet::new(),
            remaining_dependencies: input.dependencies.clone(),
            reuse_stored_warnings: true,
        },
        Check::Changed => {
            let invalidation = expand(
                &input.changed,
                &input.removed,
                &input.dependencies,
                &input.current,
            );
            Plan {
                analyze: invalidation.analyze,
                changed_or_removed: Some(input.changed.union(&input.removed).cloned().collect()),
                discard: invalidation.discard,
                remaining_dependencies: invalidation.remaining_dependencies,
                reuse_stored_warnings: true,
            }
        }
        Check::MissingWarnings
        | Check::StaleVersion
        | Check::MissingFile
        | Check::ChangedWithoutWarnings
        | Check::WarningProfileChanged => Plan {
            analyze: input.current.clone(),
            changed_or_removed: matches!(input.check, Check::MissingWarnings).then(BTreeSet::new),
            discard: input.previous.union(&input.current).cloned().collect(),
            remaining_dependencies: ReverseDependencies::new(),
            reuse_stored_warnings: false,
        },
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleWarning<W> {
    pub module: String,
    pub warning: W,
}

pub type WarningMap<W> = std::collections::BTreeMap<String, Vec<W>>;

pub fn merge_warnings<W: Clone>(
    collected: Option<&[ModuleWarning<W>]>,
    unknown: &[ModuleWarning<W>],
    stored: Option<&WarningMap<W>>,
) -> Option<WarningMap<W>> {
    let Some(collected) = collected else {
        return stored.cloned();
    };
    let mut warnings: WarningMap<W> = WarningMap::new();
    for item in collected.iter().chain(unknown) {
        warnings
            .entry(item.module.clone())
            .or_default()
            .push(item.warning.clone());
    }
    for values in warnings.values_mut() {
        values.reverse();
    }
    if let Some(stored) = stored {
        warnings.extend(stored.clone());
    }
    Some(warnings)
}

pub fn select_warnings<W: Clone>(stored: &WarningMap<W>, modules: &[String]) -> Vec<W> {
    if modules.is_empty() {
        stored.values().flatten().cloned().collect()
    } else {
        modules
            .iter()
            .filter_map(|module| stored.get(module))
            .flatten()
            .cloned()
            .collect()
    }
}

pub fn merge_dependencies(
    old: &ReverseDependencies,
    new: &ReverseDependencies,
) -> ReverseDependencies {
    let mut combined = old.clone();
    for (module, dependents) in new {
        combined
            .entry(module.clone())
            .or_default()
            .extend(dependents.iter().cloned());
    }
    combined
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotProfile {
    pub analysis_version: String,
    pub implementation: std::collections::BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotHeader {
    pub profile: SnapshotProfile,
    pub modules: std::collections::BTreeMap<String, String>,
    pub legal_warnings: Option<BTreeSet<String>>,
    pub warnings_available: bool,
    pub dependencies: ReverseDependencies,
}

impl Inputs {
    pub fn from_snapshot(
        previous: Option<&SnapshotHeader>,
        current: &std::collections::BTreeMap<String, String>,
        profile: &SnapshotProfile,
        legal_warnings: &BTreeSet<String>,
    ) -> Self {
        let Some(previous) = previous else {
            return Self {
                check: Check::MissingFile,
                current: current.keys().cloned().collect(),
                previous: BTreeSet::new(),
                changed: current.keys().cloned().collect(),
                removed: BTreeSet::new(),
                dependencies: ReverseDependencies::new(),
            };
        };
        let changed: BTreeSet<_> = current
            .iter()
            .filter(|(module, digest)| previous.modules.get(*module) != Some(*digest))
            .map(|(module, _)| module.clone())
            .collect();
        let removed: BTreeSet<_> = previous
            .modules
            .keys()
            .filter(|module| !current.contains_key(*module))
            .cloned()
            .collect();
        let version_matches = previous.profile.analysis_version == profile.analysis_version
            && previous
                .profile
                .implementation
                .iter()
                .all(|(module, digest)| profile.implementation.get(module) == Some(digest));
        let warning_profile_matches = previous.legal_warnings.as_ref() == Some(legal_warnings);
        let check = if !version_matches {
            Check::StaleVersion
        } else if !warning_profile_matches {
            Check::WarningProfileChanged
        } else if changed.is_empty() && removed.is_empty() {
            if previous.warnings_available {
                Check::Fresh
            } else {
                Check::MissingWarnings
            }
        } else if previous.warnings_available {
            Check::Changed
        } else {
            Check::ChangedWithoutWarnings
        };
        Self {
            check,
            current: current.keys().cloned().collect(),
            previous: previous.modules.keys().cloned().collect(),
            changed,
            removed,
            dependencies: previous.dependencies.clone(),
        }
    }
}

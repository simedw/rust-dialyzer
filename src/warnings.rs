use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tag {
    WarnBehaviour,
    WarnBinConstruction,
    WarnCallgraph,
    WarnContractExtraReturn,
    WarnContractMissingReturn,
    WarnContractNotEqual,
    WarnContractOpaque,
    WarnContractRange,
    WarnContractSubtype,
    WarnContractSupertype,
    WarnContractSyntax,
    WarnContractTypes,
    WarnFailingCall,
    WarnFunApp,
    WarnMapConstruction,
    WarnMatching,
    WarnNonProperList,
    WarnNotCalled,
    WarnOpaque,
    WarnOpaqueUnion,
    WarnOverlappingContract,
    WarnReturnNoExit,
    WarnReturnOnlyExit,
    WarnUmatchedReturn,
    WarnUndefinedCallbacks,
    WarnUnknown,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Enable,
    Disable,
}
#[derive(Deserialize)]
struct Rule {
    name: String,
    action: Action,
    tags: BTreeSet<Tag>,
}
#[derive(Deserialize)]
struct Catalogue {
    version: u32,
    tags: BTreeSet<Tag>,
    default: BTreeSet<Tag>,
    options: Vec<Rule>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Profile {
    tags: BTreeSet<Tag>,
}
pub struct Registry {
    default: Profile,
    rules: BTreeMap<String, Rule>,
}
impl Registry {
    pub fn load(raw: &str) -> Result<Self, String> {
        let catalogue: Catalogue = serde_json::from_str(raw).map_err(|e| e.to_string())?;
        if catalogue.version != 1 || !catalogue.default.is_subset(&catalogue.tags) {
            return Err("invalid warning catalogue".into());
        }
        let mut rules = BTreeMap::new();
        for rule in catalogue.options {
            if !rule.tags.is_subset(&catalogue.tags) || rules.contains_key(&rule.name) {
                return Err("invalid warning option rule".into());
            }
            rules.insert(rule.name.clone(), rule);
        }
        Ok(Self {
            default: Profile {
                tags: catalogue.default,
            },
            rules,
        })
    }
    pub fn default_profile(&self) -> Profile {
        self.default.clone()
    }
    pub fn apply(&self, initial: &Profile, options: &[String]) -> Result<Profile, String> {
        let mut profile = initial.clone();
        for option in options {
            let rule = self
                .rules
                .get(option)
                .ok_or_else(|| format!("Unknown dialyzer warning option: {option}"))?;
            match rule.action {
                Action::Enable => profile.tags.extend(rule.tags.iter().copied()),
                Action::Disable => profile.tags.retain(|tag| !rule.tags.contains(tag)),
            }
        }
        Ok(profile)
    }
}
impl Profile {
    pub fn from_tags(tags: impl IntoIterator<Item = Tag>) -> Self {
        Self {
            tags: tags.into_iter().collect(),
        }
    }
    pub fn contains(&self, tag: Tag) -> bool {
        self.tags.contains(&tag)
    }
    pub fn tags(&self) -> &BTreeSet<Tag> {
        &self.tags
    }
}

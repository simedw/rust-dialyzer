pub struct PriorInfoRow<'a> {
    pub mfa: &'a Mfa,
    pub arguments: &'a [NativeTerm],
    pub result: &'a NativeTerm,
}
use crate::bifs::{boundary::NativeTerm, registry::Mfa};
use crate::contract_runtime::ContractSummary;
use crate::core::{CoreProgram, ErlangTerm, ErlangTerms, TermId};
use crate::lattice::Type;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

#[derive(Deserialize)]
struct Artifact {
    version: u32,
    profile: Profile,
    source: Source,
    modules: Vec<String>,
    summaries: Vec<NativeSummary>,
    contracts: Vec<NativeContract>,
    #[serde(default)]
    callbacks: Vec<NativeContract>,
    #[serde(default)]
    raw_callback_groups: Option<serde_json::Value>,
    #[serde(default)]
    callback_lists: Option<Vec<NativeCallbackList>>,
    type_tables: serde_json::Value,
    exported_types: serde_json::Value,
}

#[derive(Deserialize)]
struct Transport {
    #[serde(flatten)]
    artifact: Artifact,
    raw_summaries: Option<serde_json::Value>,
    raw_contracts: Option<serde_json::Value>,
}
impl Artifact {
    fn parse(raw: &str) -> Result<Self, String> {
        let transport: Transport = serde_json::from_str(raw).map_err(|e| e.to_string())?;
        let mut artifact = transport.artifact;
        if artifact.version != 5 {
            return Ok(artifact);
        }
        if !artifact.summaries.is_empty() || !artifact.contracts.is_empty() {
            return Err("schema5 requires one raw summary/contract authority".into());
        }
        fn arena(value: Option<serde_json::Value>) -> Result<std::sync::Arc<ErlangTerms>, String> {
            Ok(std::sync::Arc::new(ErlangTerms::parse(
                &value.ok_or("raw prior table missing")?,
            )?))
        }
        fn list(arena: &ErlangTerms, mut id: TermId) -> Result<Vec<TermId>, String> {
            let mut result = Vec::new();
            loop {
                match &arena.terms[id.0] {
                    ErlangTerm::Nil => return Ok(result),
                    ErlangTerm::List { items, tail } => {
                        result.extend(items);
                        id = *tail;
                    }
                    _ => return Err("prior table proper list expected".into()),
                }
            }
        }
        fn tuple(arena: &ErlangTerms, id: TermId, arity: usize) -> Result<&[TermId], String> {
            match &arena.terms[id.0] {
                ErlangTerm::Tuple(fields) if fields.len() == arity => Ok(fields),
                _ => Err("prior table tuple shape differs".into()),
            }
        }
        fn mfa(arena: &ErlangTerms, id: TermId) -> Result<Mfa, String> {
            let fields = tuple(arena, id, 3)?;
            match (
                &arena.terms[fields[0].0],
                &arena.terms[fields[1].0],
                &arena.terms[fields[2].0],
            ) {
                (
                    ErlangTerm::Atom(module),
                    ErlangTerm::Atom(function),
                    ErlangTerm::Integer(arity),
                ) => Ok(Mfa {
                    module: module.clone().into(),
                    function: function.clone().into(),
                    arity: arity.parse().map_err(|_| "prior arity invalid")?,
                }),
                _ => Err("prior MFA shape differs".into()),
            }
        }
        let summaries = arena(transport.raw_summaries)?;
        for id in list(&summaries, summaries.root)? {
            let fields = tuple(&summaries, id, 3)?;
            artifact.summaries.push(NativeSummary {
                mfa: mfa(&summaries, fields[0])?,
                result: NativeTerm::from_arena(&summaries, fields[1])?,
                arguments: list(&summaries, fields[2])?
                    .into_iter()
                    .map(|id| NativeTerm::from_arena(&summaries, id))
                    .collect::<Result<_, _>>()?,
            });
        }
        let contracts = arena(transport.raw_contracts)?;
        for id in list(&contracts, contracts.root)? {
            let fields = tuple(&contracts, id, 2)?;
            let raw = |id| -> Result<crate::native_form_environment::RawFormTerm, String> {
                let source =
                    crate::native_form_environment::RawFormTerm::new(contracts.clone(), id)?;
                let mut builder = crate::form_error::ReasonBuilder::new();
                let root = builder.copy(&source);
                Ok(builder.finish(root))
            };
            let builder = crate::form_error::ReasonBuilder::new();
            let nil = builder.nil();
            let nil = builder.finish(nil);
            let contract = match &contracts.terms[fields[1].0] {
                ErlangTerm::Tuple(record) if record.len() == 4 => {
                    let mut contract = Contract::from_capture(&raw(fields[1])?, &nil, &nil)?;
                    contract.location = None;
                    contract.flags = None;
                    contract
                }
                ErlangTerm::Tuple(record) if record.len() == 3 => {
                    Contract::from_capture(&raw(record[1])?, &raw(record[0])?, &raw(record[2])?)?
                }
                _ => return Err("prior contract shape differs".into()),
            };
            artifact.contracts.push(NativeContract {
                mfa: mfa(&contracts, fields[0])?,
                contract,
            });
        }
        Ok(artifact)
    }
}

#[derive(Deserialize)]
struct Profile {
    otp: String,
    erts: String,
    dialyzer: String,
}
#[derive(Deserialize)]
struct NativeCallbackList {
    module: String,
    mfas: Vec<Mfa>,
}
pub struct CallbackLibrary {
    raw: crate::raw_callback_library::RawCallbackLibrary,
    original_order: bool,
}
impl CallbackLibrary {
    pub fn original(
        &self,
    ) -> Result<&crate::raw_callback_library::RawCallbackLibrary, CallbackLookupError> {
        if self.original_order {
            Ok(&self.raw)
        } else {
            Err(CallbackLookupError::OrderingUnavailable)
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallbackLookupError {
    OrderingUnavailable,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Source {
    pub kind: String,
    pub path: String,
    pub sha256: String,
    pub files: Vec<Fingerprint>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Fingerprint {
    pub path: String,
    pub sha256: String,
}
#[derive(Deserialize)]
struct NativeSummary {
    mfa: Mfa,
    arguments: Vec<NativeTerm>,
    result: NativeTerm,
}
#[derive(Clone, Debug, Deserialize)]
pub struct NativeConstraint {
    pub left: NativeTerm,
    pub right: NativeTerm,
}
#[derive(Clone, Debug, Deserialize)]
pub struct NativeAlternative {
    pub signature: NativeTerm,
    pub constraints: Vec<NativeConstraint>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Contract {
    #[serde(skip)]
    raw_capture: Option<crate::native_form_environment::RawFormTerm>,
    #[serde(skip)]
    summary: OnceLock<Result<ContractSummary, String>>,
    pub alternatives: Vec<NativeAlternative>,
    pub arguments: Vec<NativeTerm>,
    #[serde(deserialize_with = "term_arena")]
    pub forms: ErlangTerms,
    pub location: Option<NativeTerm>,
    pub flags: Option<NativeTerm>,
}
impl Contract {
    pub fn from_capture(
        raw: &crate::native_form_environment::RawFormTerm,
        location: &crate::native_form_environment::RawFormTerm,
        flags: &crate::native_form_environment::RawFormTerm,
    ) -> Result<Self, String> {
        fn list(
            raw: &crate::native_form_environment::RawFormTerm,
            id: TermId,
        ) -> Result<Vec<TermId>, String> {
            let mut next = id;
            let mut out = Vec::new();
            loop {
                match &raw.arena().terms[next.0] {
                    ErlangTerm::Nil => return Ok(out),
                    ErlangTerm::List { items, tail } => {
                        out.extend(items);
                        next = *tail;
                    }
                    _ => return Err("native contract list expected".into()),
                }
            }
        }
        fn pair(
            raw: &crate::native_form_environment::RawFormTerm,
            id: TermId,
        ) -> Result<&[TermId], String> {
            match &raw.arena().terms[id.0] {
                ErlangTerm::Tuple(fields) if fields.len() == 2 => Ok(fields),
                _ => Err("native contract pair expected".into()),
            }
        }
        let ErlangTerm::Tuple(fields) = raw.term() else {
            return Err("native contract record expected".into());
        };
        if fields.len() != 4
            || !matches!(&raw.arena().terms[fields[0].0],ErlangTerm::Atom(tag) if tag=="contract")
        {
            return Err("native contract record expected".into());
        }
        let mut alternatives = Vec::new();
        for id in list(raw, fields[1])? {
            let fields = pair(raw, id)?;
            let signature = NativeTerm::from_arena(raw.arena(), fields[0])?;
            let mut constraints = Vec::new();
            for id in list(raw, fields[1])? {
                let ErlangTerm::Tuple(fields) = &raw.arena().terms[id.0] else {
                    return Err("native subtype tuple expected".into());
                };
                if fields.len() != 3
                    || !matches!(&raw.arena().terms[fields[0].0],ErlangTerm::Atom(tag) if tag=="subtype")
                {
                    return Err("native subtype tuple expected".into());
                }
                constraints.push(NativeConstraint {
                    left: NativeTerm::from_arena(raw.arena(), fields[1])?,
                    right: NativeTerm::from_arena(raw.arena(), fields[2])?,
                });
            }
            alternatives.push(NativeAlternative {
                signature,
                constraints,
            });
        }
        let arguments = list(raw, fields[2])?
            .into_iter()
            .map(|id| NativeTerm::from_arena(raw.arena(), id))
            .collect::<Result<Vec<_>, _>>()?;
        let forms =
            crate::native_form_environment::RawFormTerm::new(raw.arena().clone(), fields[3])?;
        let mut b = crate::form_error::ReasonBuilder::new();
        let id = b.copy(&forms);
        let forms = std::sync::Arc::unwrap_or_clone(b.finish(id).arena);
        Ok(Self {
            raw_capture: Some(raw.clone()),
            summary: OnceLock::new(),
            alternatives,
            arguments,
            forms,
            location: Some(NativeTerm::from_arena(location.arena(), location.id())?),
            flags: Some(NativeTerm::from_arena(flags.arena(), flags.id())?),
        })
    }
    pub fn captured_record(&self) -> crate::native_form_environment::RawFormTerm {
        if let Some(raw) = &self.raw_capture {
            return raw.clone();
        }
        let mut builder = crate::form_error::ReasonBuilder::new();
        let tag = builder.atom("contract");
        let pairs = self
            .alternatives
            .iter()
            .map(|alternative| {
                let signature = alternative.signature.append_raw(&mut builder);
                let constraints = alternative
                    .constraints
                    .iter()
                    .map(|constraint| {
                        let left = constraint.left.append_raw(&mut builder);
                        let right = constraint.right.append_raw(&mut builder);
                        let subtype = builder.atom("subtype");
                        builder.tuple(vec![subtype, left, right])
                    })
                    .collect();
                let constraints = builder.list(constraints);
                builder.tuple(vec![signature, constraints])
            })
            .collect();
        let pairs = builder.list(pairs);
        let arguments = self
            .arguments
            .iter()
            .map(|argument| argument.append_raw(&mut builder))
            .collect();
        let arguments = builder.list(arguments);
        let forms = builder.copy(
            &crate::native_form_environment::RawFormTerm::new(
                std::sync::Arc::new(self.forms.clone()),
                self.forms.root,
            )
            .expect("imported contract form root"),
        );
        let record = builder.tuple(vec![tag, pairs, arguments, forms]);
        builder.finish(record)
    }
    pub fn summary(&self) -> Result<&ContractSummary, String> {
        self.summary
            .get_or_init(|| {
                let alternatives = self
                    .alternatives
                    .iter()
                    .map(|alternative| {
                        let signature = alternative.signature.type_value()?;
                        let constraints = alternative
                            .constraints
                            .iter()
                            .map(|pair| Ok((pair.left.type_value()?, pair.right.type_value()?)))
                            .collect::<Result<Vec<_>, String>>()?;
                        Ok((signature, constraints))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let summary =
                    ContractSummary::from_parts(alternatives).map_err(|e| e.to_string())?;
                let native_arguments = self
                    .arguments
                    .iter()
                    .map(NativeTerm::type_value)
                    .collect::<Result<Vec<_>, _>>()?;
                if summary.arguments != native_arguments {
                    return Err(
                        "contract general domain differs from imported native domain".into(),
                    );
                }
                Ok(summary)
            })
            .as_ref()
            .map_err(Clone::clone)
    }
}
#[derive(Deserialize)]
struct NativeContract {
    mfa: Mfa,
    contract: Contract,
}
#[derive(Clone, Debug)]
pub struct Summary {
    pub arguments: Vec<Type>,
    pub result: Type,
}
#[derive(Serialize)]
pub struct Inspection {
    pub modules: usize,
    pub summaries: usize,
    pub contracts: usize,
    pub callbacks: usize,
    pub excluded_target_summaries: usize,
    pub source_plt: String,
}
/// Classic PLT presence after whole-module purge and before current-key deletion.
#[derive(Clone, Debug, Default)]
pub struct OriginalPltMembership {
    info: BTreeSet<Mfa>,
    contracts: BTreeSet<Mfa>,
}
impl OriginalPltMembership {
    pub fn contains(&self, name: &Mfa) -> bool {
        self.info.contains(name) || self.contracts.contains(name)
    }
    pub fn modules(&self) -> BTreeSet<&str> {
        self.info
            .iter()
            .chain(&self.contracts)
            .map(|m| m.module.as_str())
            .collect()
    }
}
#[derive(Clone, Debug)]
pub struct ImportPlan {
    purged_modules: BTreeSet<String>,
}
impl ImportPlan {
    pub fn before_cleanup(purged_modules: impl IntoIterator<Item = String>) -> Self {
        Self {
            purged_modules: purged_modules.into_iter().collect(),
        }
    }
    pub fn cold_targets(core: &CoreProgram) -> Self {
        Self::before_cleanup(core.modules.iter().map(|module| module.name.clone()))
    }
    fn project_tables(&self, mut arena: ErlangTerms) -> ErlangTerms {
        if let ErlangTerm::Map(entries) = &arena.terms[arena.root.0] {
            let entries = entries
                .iter()
                .copied()
                .filter(|(key, _)| match &arena.terms[key.0] {
                    ErlangTerm::Atom(module) => self.keeps(module),
                    _ => true,
                })
                .collect();
            arena.terms[arena.root.0] = ErlangTerm::Map(entries);
        }
        arena
    }
    fn project_exports(&self, mut arena: ErlangTerms) -> ErlangTerms {
        let mut next = arena.root;
        let mut items = Vec::new();
        let nil = loop {
            match &arena.terms[next.0] {
                ErlangTerm::Nil => break next,
                ErlangTerm::List { items: block, tail } => {
                    items.extend(block.iter().copied().filter(|id| match &arena.terms[id.0] {
                        ErlangTerm::Tuple(fields) if fields.len() == 3 => {
                            match fields.first().map(|id| &arena.terms[id.0]) {
                                Some(ErlangTerm::Atom(module)) => self.keeps(module),
                                _ => true,
                            }
                        }
                        _ => true,
                    }));
                    next = *tail;
                }
                _ => return arena,
            }
        };
        if items.is_empty() {
            arena.root = nil;
        } else {
            arena.root = TermId(arena.terms.len());
            arena.terms.push(ErlangTerm::List { items, tail: nil });
        }
        arena
    }
    fn keeps(&self, module: &str) -> bool {
        !self.purged_modules.contains(module)
    }
}
pub struct PriorPlt {
    library: Store,
    target_modules: BTreeSet<String>,
    plan: ImportPlan,
}
impl PriorPlt {
    pub fn parse(raw: &str, core: &CoreProgram, plan: &ImportPlan) -> Result<Self, String> {
        Ok(Self {
            library: Store::import(raw, &core.profile, plan)?,
            target_modules: core
                .modules
                .iter()
                .map(|module| module.name.clone())
                .collect(),
            plan: plan.clone(),
        })
    }
    pub fn original_membership(&self) -> &OriginalPltMembership {
        &self.library.original_membership
    }
    pub fn prepare_definitions(
        &self,
        core: &CoreProgram,
        program: &crate::program::Program,
    ) -> Result<
        crate::compiled_definitions::PreparedDefinitions,
        crate::compiled_definitions::PreparationFailure,
    > {
        crate::compiled_definitions::PreparedDefinitions::prepare(core, program, &self.library)
    }
    pub fn after_cleanup(
        mut self,
        graph: &crate::source_callgraph::SourceCallGraph,
    ) -> Result<Store, String> {
        let mut current_named = BTreeSet::new();
        for vertex in graph
            .source_graph()
            .ordered_vertices()
            .map_err(|e| format!("{e:?}"))?
        {
            if let crate::source_callgraph::GraphVertex::Named {
                module,
                function,
                arity,
            } = vertex
            {
                use num_traits::ToPrimitive;
                let Some(arity) = arity.to_usize() else {
                    continue;
                };
                current_named.insert(Mfa {
                    module: module.into(),
                    function: function.into(),
                    arity,
                });
            }
        }
        self.library.summaries.retain(|name, _| {
            let keep = !current_named.contains(name);
            if !keep {
                self.library.excluded += 1;
            }
            keep
        });
        self.library
            .contracts
            .retain(|name, _| !current_named.contains(name));
        Ok(self.library)
    }
    pub fn into_cold_store(self) -> Result<Store, String> {
        if !self.target_modules.is_subset(&self.plan.purged_modules) {
            return Err("cold analysis requires explicit whole-module target purge".into());
        }
        Ok(self.library)
    }
}
pub struct Store {
    pub original_membership: OriginalPltMembership,
    pub source: Source,
    summaries: BTreeMap<Mfa, NativeSummary>,
    pub contracts: BTreeMap<Mfa, Contract>,
    pub callbacks: CallbackLibrary,
    pub type_tables: ErlangTerms,
    pub exported_types: ErlangTerms,
    modules: BTreeSet<String>,
    excluded: usize,
}
impl Store {
    pub fn retain_without_analysis(
        raw: &str,
        profile: &crate::core::CompilerProfile,
        plan: &ImportPlan,
    ) -> Result<Self, String> {
        Self::import(raw, profile, plan)
    }
    pub fn prior_info_rows(&self) -> impl Iterator<Item = PriorInfoRow<'_>> {
        self.summaries.values().map(|row| PriorInfoRow {
            mfa: &row.mfa,
            arguments: &row.arguments,
            result: &row.result,
        })
    }

    fn import(
        raw: &str,
        profile: &crate::core::CompilerProfile,
        plan: &ImportPlan,
    ) -> Result<Self, String> {
        let mut timings = crate::phase_timings::PhaseTimings::new("dependency_import");
        let artifact = Artifact::parse(raw)?;
        timings.mark("artifact_parse");
        if !matches!(artifact.version, 2..=5) {
            return Err(format!(
                "unsupported dependency schema {}",
                artifact.version
            ));
        }
        if artifact.profile.otp != profile.otp
            || artifact.profile.erts != profile.erts
            || artifact.profile.dialyzer != profile.dialyzer
        {
            return Err("dependency runtime differs from compiler input".into());
        }
        if !matches!(
            artifact.source.kind.as_str(),
            "classic_plt" | "incremental_plt"
        ) {
            return Err("unsupported dependency source kind".into());
        }
        let original_membership = OriginalPltMembership {
            info: artifact
                .summaries
                .iter()
                .filter(|s| plan.keeps(&s.mfa.module))
                .map(|s| s.mfa.clone())
                .collect(),
            contracts: artifact
                .contracts
                .iter()
                .filter(|c| plan.keeps(&c.mfa.module))
                .map(|c| c.mfa.clone())
                .collect(),
        };
        let modules: BTreeSet<_> = artifact.modules.into_iter().collect();
        let mut summaries = BTreeMap::new();
        let mut excluded = 0;
        for summary in artifact.summaries {
            if summary.arguments.len() != summary.mfa.arity || summary.mfa.arity > 255 {
                return Err(format!("invalid summary arity for {:?}", summary.mfa));
            }
            if !modules.contains(summary.mfa.module.as_str()) {
                return Err(format!(
                    "summary module absent from dependency inventory: {:?}",
                    summary.mfa
                ));
            }
            if !plan.keeps(&summary.mfa.module) {
                excluded += 1;
                continue;
            }
            if summaries.insert(summary.mfa.clone(), summary).is_some() {
                return Err("duplicate dependency summary".into());
            }
        }
        let mut contracts = BTreeMap::new();
        for entry in artifact.contracts {
            if entry.contract.arguments.len() != entry.mfa.arity || entry.mfa.arity > 255 {
                return Err(format!("invalid contract arity for {:?}", entry.mfa));
            }
            if plan.keeps(&entry.mfa.module)
                && contracts.insert(entry.mfa, entry.contract).is_some()
            {
                return Err("duplicate dependency contract".into());
            }
        }
        let (raw, original_order) = if matches!(artifact.version, 4 | 5) {
            if !artifact.callbacks.is_empty() || artifact.callback_lists.is_some() {
                return Err("schema4 callback authority must be raw groups only".into());
            }
            let value = artifact
                .raw_callback_groups
                .ok_or("schema4 raw callback groups missing")?;
            let arena = std::sync::Arc::new(ErlangTerms::parse(&value)?);
            (
                crate::native_form_environment::RawFormTerm::new(arena.clone(), arena.root)?,
                true,
            )
        } else {
            if artifact.raw_callback_groups.is_some() {
                return Err("raw callback groups require schema4".into());
            }
            let mut entries = BTreeMap::new();
            for entry in artifact.callbacks {
                if entries.insert(entry.mfa, entry.contract).is_some() {
                    return Err("duplicate dependency callback".into());
                }
            }
            let (groups, original_order) = match (artifact.version, artifact.callback_lists) {
                (3, Some(lists)) => {
                    let mut seen = BTreeSet::new();
                    let mut modules = BTreeSet::new();
                    for group in &lists {
                        if !modules.insert(group.module.clone()) {
                            return Err("duplicate callback list module".into());
                        }
                        for name in &group.mfas {
                            if name.module != group.module
                                || !entries.contains_key(name)
                                || !seen.insert(name.clone())
                            {
                                return Err("callback list provenance differs from imported callback records".into());
                            }
                        }
                    }
                    if seen.len() != entries.len() {
                        return Err("callback list provenance is incomplete".into());
                    }
                    (lists, true)
                }
                (2, None) => {
                    let mut groups = BTreeMap::<String, Vec<Mfa>>::new();
                    for name in entries.keys() {
                        groups
                            .entry(name.module.clone().to_string())
                            .or_default()
                            .push(name.clone());
                    }
                    (
                        groups
                            .into_iter()
                            .map(|(module, mfas)| NativeCallbackList { module, mfas })
                            .collect(),
                        false,
                    )
                }
                (3, None) => return Err("version3 callback list provenance missing".into()),
                (2, Some(_)) => {
                    return Err("callback list provenance requires schema version3".into());
                }
                _ => unreachable!(),
            };
            let mut b = crate::form_error::ReasonBuilder::new();
            let mut rawgroups = Vec::new();
            for group in groups {
                let mut rows = Vec::new();
                for name in group.mfas {
                    let c = &entries[&name];
                    let module = b.atom(&name.module);
                    let function = b.atom(&name.function);
                    let arity = b.integer(name.arity);
                    let key = b.tuple(vec![module, function, arity]);
                    let location = c
                        .location
                        .as_ref()
                        .ok_or("callback location unavailable")?
                        .append_raw(&mut b);
                    let capture = b.copy(&c.captured_record());
                    let flags = c
                        .flags
                        .as_ref()
                        .ok_or("callback flags unavailable")?
                        .append_raw(&mut b);
                    let value = b.tuple(vec![location, capture, flags]);
                    rows.push(b.tuple(vec![key, value]));
                }
                let module = b.atom(&group.module);
                let rows = b.list(rows);
                rawgroups.push(b.tuple(vec![module, rows]));
            }
            let id = b.list(rawgroups);
            (b.finish(id), original_order)
        };
        let callbacks = CallbackLibrary {
            raw: crate::raw_callback_library::RawCallbackLibrary::parse(raw, |module| {
                !plan.keeps(module)
            })?,
            original_order,
        };
        let store = Self {
            original_membership,
            source: artifact.source,
            summaries,
            contracts,
            callbacks,
            type_tables: plan.project_tables(ErlangTerms::parse(&artifact.type_tables)?),
            exported_types: plan.project_exports(ErlangTerms::parse(&artifact.exported_types)?),
            modules,
            excluded,
        };
        timings.mark("native_store_construction");
        Ok(store)
    }
    pub fn callback_rows(
        &self,
        module: &str,
    ) -> Result<Option<Vec<&crate::raw_callback_library::CallbackRow>>, CallbackLookupError> {
        Ok(self.callbacks.original()?.lookup(module))
    }
    pub fn summary(&self, mfa: &Mfa) -> Result<Option<Summary>, String> {
        self.summaries
            .get(mfa)
            .map(|entry| {
                Ok(Summary {
                    arguments: entry
                        .arguments
                        .iter()
                        .map(NativeTerm::type_value)
                        .collect::<Result<Vec<_>, _>>()?,
                    result: entry.result.type_value()?,
                })
            })
            .transpose()
            .map_err(|e: String| format!("dependency {mfa:?}: {e}"))
    }
    pub fn summary_names(&self) -> impl Iterator<Item = &Mfa> {
        self.summaries.keys()
    }
    pub fn inspect(&self) -> Inspection {
        Inspection {
            modules: self.modules.len(),
            summaries: self.summaries.len(),
            contracts: self.contracts.len(),
            callbacks: self.callbacks.raw.rows().count(),
            excluded_target_summaries: self.excluded,
            source_plt: self.source.path.clone(),
        }
    }
}

fn term_arena<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<ErlangTerms, D::Error> {
    let raw = serde_json::Value::deserialize(deserializer)?;
    ErlangTerms::parse(&raw).map_err(serde::de::Error::custom)
}

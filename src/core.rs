mod wire;
pub mod etf;
use serde::{Serialize, Deserialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TermId(pub usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeId(pub usize);
#[derive(Clone, Debug)]
pub struct ErlangTerms {
    pub root: TermId,
    pub terms: crate::shared_vec::SharedVec<ErlangTerm>,
}
impl ErlangTerms {
    pub fn parse(raw: &Value) -> Result<Self> {
        let obj = object(raw, "$.arena")?;
        let terms = parse_term_table(field(obj, "terms", "$.arena")?, "$.arena.terms")?;
        let root = TermId(reference(
            field(obj, "root", "$.arena")?,
            terms.len(),
            "$.arena.root",
        )?);
        Ok(Self { root, terms: terms.into() })
    }
}
fn parse_term_table(raw: &Value, path: &str) -> Result<Vec<ErlangTerm>> {
    array(raw, path)?
        .iter()
        .enumerate()
        .map(|(id, raw)| parse_term(raw, format_args!("{path}[{id}]"), id))
        .collect()
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ExternalFun {
    pub module: String,
    pub function: String,
    pub arity: u8,
}
#[cfg(otp_domain = "29")]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NativeRecord {
    pub module: String,
    pub name: String,
    pub exported: bool,
    /// Field declaration order is part of RECORD_EXT transport.
    pub fields: Vec<(String, TermId)>,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ErlangTerm {
    Atom(String),
    ExternalFun(Box<ExternalFun>),
    Integer(String),
    Float(u64),
    Binary(Vec<u8>),
    Bitstring {
        bytes: Vec<u8>,
        bits: u64,
    },
    Tuple(Vec<TermId>),
    Nil,
    List {
        items: Vec<TermId>,
        tail: TermId,
    },
    Map(Vec<(TermId, TermId)>),
    #[cfg(otp_domain = "29")]
    Record(Box<NativeRecord>),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreNode {
    pub id: NodeId,
    pub annotations: TermId,
    pub expr: CoreExpr,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TryExpr {
    pub argument: NodeId,
    pub vars: Vec<NodeId>,
    pub body: NodeId,
    pub exception_vars: Vec<NodeId>,
    pub handler: NodeId,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleExpr {
    pub name: NodeId,
    pub exports: Vec<NodeId>,
    pub attributes: Vec<(NodeId, NodeId)>,
    pub definitions: Vec<(NodeId, NodeId)>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoreExpr {
    Literal(TermId),
    Var(TermId),
    Opaque(TermId),
    Values(Vec<NodeId>),
    Binary(Vec<NodeId>),
    Bitstr {
        value: NodeId,
        size: NodeId,
        unit: NodeId,
        typ: NodeId,
        flags: NodeId,
    },
    Cons {
        head: NodeId,
        tail: NodeId,
    },
    Tuple(Vec<NodeId>),
    Map {
        base: NodeId,
        pairs: Vec<NodeId>,
        pattern: bool,
    },
    MapPair {
        operation: NodeId,
        key: NodeId,
        value: NodeId,
    },
    #[cfg(otp_domain = "29")]
    Record {
        argument: NodeId,
        id: NodeId,
        pairs: Vec<NodeId>,
    },
    #[cfg(otp_domain = "29")]
    RecordPair { key: NodeId, value: NodeId },
    Let {
        vars: Vec<NodeId>,
        argument: NodeId,
        body: NodeId,
    },
    Seq {
        argument: NodeId,
        body: NodeId,
    },
    Apply {
        operator: NodeId,
        args: Vec<NodeId>,
    },
    Call {
        module: NodeId,
        name: NodeId,
        args: Vec<NodeId>,
    },
    Primop {
        name: NodeId,
        args: Vec<NodeId>,
    },
    Case {
        argument: NodeId,
        clauses: Vec<NodeId>,
    },
    Clause {
        patterns: Vec<NodeId>,
        guard: NodeId,
        body: NodeId,
    },
    Alias {
        variable: NodeId,
        pattern: NodeId,
    },
    Fun {
        params: Vec<NodeId>,
        body: NodeId,
    },
    Receive {
        clauses: Vec<NodeId>,
        timeout: NodeId,
        action: NodeId,
    },
    Try(Box<TryExpr>),
    Catch {
        body: NodeId,
    },
    Letrec {
        definitions: Vec<(NodeId, NodeId)>,
        body: NodeId,
    },
    Module(Box<ModuleExpr>),
}

impl CoreExpr {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Literal(_) => "literal",
            Self::Var(_) => "var",
            Self::Opaque(_) => "opaque",
            Self::Values(_) => "values",
            Self::Binary(_) => "binary",
            Self::Bitstr { .. } => "bitstr",
            Self::Cons { .. } => "cons",
            Self::Tuple(_) => "tuple",
            Self::Map { .. } => "map",
            Self::MapPair { .. } => "map_pair",
            #[cfg(otp_domain = "29")]
            Self::Record { .. } => "record",
            #[cfg(otp_domain = "29")]
            Self::RecordPair { .. } => "record_pair",
            Self::Let { .. } => "let",
            Self::Seq { .. } => "seq",
            Self::Apply { .. } => "apply",
            Self::Call { .. } => "call",
            Self::Primop { .. } => "primop",
            Self::Case { .. } => "case",
            Self::Clause { .. } => "clause",
            Self::Alias { .. } => "alias",
            Self::Fun { .. } => "fun",
            Self::Receive { .. } => "receive",
            Self::Try(_) => "try",
            Self::Catch { .. } => "catch",
            Self::Letrec { .. } => "letrec",
            Self::Module(_) => "module",
        }
    }
}

impl CoreExpr {
    pub fn native_children(&self) -> Vec<NodeId> {
        match self {
            Self::Map { pairs, .. } => pairs.clone(),
            #[cfg(otp_domain = "29")]
            Self::Record { pairs, .. } => pairs.clone(),
            _ => self.children(),
        }
    }
    pub fn children(&self) -> Vec<NodeId> {
        match self {
            CoreExpr::Literal(_) | CoreExpr::Var(_) | CoreExpr::Opaque(_) => vec![],
            CoreExpr::Values(ids) | CoreExpr::Binary(ids) | CoreExpr::Tuple(ids) => ids.clone(),
            CoreExpr::Bitstr {
                value,
                size,
                unit,
                typ,
                flags,
            } => vec![*value, *size, *unit, *typ, *flags],
            CoreExpr::Cons { head, tail } => vec![*head, *tail],
            CoreExpr::Map { base, pairs, .. } => std::iter::once(*base)
                .chain(pairs.iter().copied())
                .collect(),
            CoreExpr::MapPair {
                operation,
                key,
                value,
            } => vec![*operation, *key, *value],
            #[cfg(otp_domain = "29")]
            CoreExpr::Record { argument, id, pairs } => [*argument, *id]
                .into_iter().chain(pairs.iter().copied()).collect(),
            #[cfg(otp_domain = "29")]
            CoreExpr::RecordPair { key, value } => vec![*key, *value],
            CoreExpr::Let {
                vars,
                argument,
                body,
            } => vars.iter().copied().chain([*argument, *body]).collect(),
            CoreExpr::Seq { argument, body } => vec![*argument, *body],
            CoreExpr::Apply { operator, args } => std::iter::once(*operator)
                .chain(args.iter().copied())
                .collect(),
            CoreExpr::Call { module, name, args } => [*module, *name]
                .into_iter()
                .chain(args.iter().copied())
                .collect(),
            CoreExpr::Primop { name, args } => {
                std::iter::once(*name).chain(args.iter().copied()).collect()
            }
            CoreExpr::Case { argument, clauses } => std::iter::once(*argument)
                .chain(clauses.iter().copied())
                .collect(),
            CoreExpr::Clause {
                patterns,
                guard,
                body,
            } => patterns.iter().copied().chain([*guard, *body]).collect(),
            CoreExpr::Alias { variable, pattern } => vec![*variable, *pattern],
            CoreExpr::Fun { params, body } => params.iter().copied().chain([*body]).collect(),
            CoreExpr::Receive {
                clauses,
                timeout,
                action,
            } => clauses.iter().copied().chain([*timeout, *action]).collect(),
            CoreExpr::Try(try_payload) => { let crate::core::TryExpr {
                argument,
                vars,
                body,
                exception_vars,
                handler,
            } = try_payload.as_ref(); std::iter::once(*argument)
                .chain(vars.iter().copied())
                .chain([*body])
                .chain(exception_vars.iter().copied())
                .chain([*handler])
                .collect() },
            CoreExpr::Catch { body } => vec![*body],
            CoreExpr::Letrec { definitions, body } => definitions
                .iter()
                .flat_map(|(key, value)| [*key, *value])
                .chain([*body])
                .collect(),
            CoreExpr::Module(module_payload) => { let crate::core::ModuleExpr {
                name,
                exports,
                attributes,
                definitions,
            } = module_payload.as_ref(); std::iter::once(*name)
                .chain(exports.iter().copied())
                .chain(attributes.iter().flat_map(|(key, value)| [*key, *value]))
                .chain(definitions.iter().flat_map(|(key, value)| [*key, *value]))
                .collect() },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TermMapOrder {
    NativeToListWithIterator,
}

#[derive(Clone, Debug)]
pub struct CompilerProfile {
    pub otp: String,
    pub erts: String,
    pub elixir: String,
    pub dialyzer: String,
    pub core_options: Vec<String>,
    pub term_map_order: TermMapOrder,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreModule {
    pub name: String,
    pub source_forms: TermId,
    pub core: NodeId,
    pub terms: crate::shared_vec::SharedVec<ErlangTerm>,
    pub nodes: crate::shared_vec::SharedVec<CoreNode>,
    pub map_iterator_orders: BTreeMap<TermId, Vec<usize>>,
    pub contract_variable_maps: BTreeMap<TermId, TermId>,
}
impl CoreModule {
    pub fn named_mfa(&self, variable: NodeId) -> Result<crate::bifs::registry::Mfa> {
        let CoreExpr::Var(term) = self.nodes[variable.0].expr else {
            return Err("named function binder not variable".into());
        };
        let ErlangTerm::Tuple(items) = &self.terms[term.0] else {
            return Err("named function name not pair".into());
        };
        let [name, arity]: &[_; 2] = items
            .as_slice()
            .try_into()
            .map_err(|_| "named function name pair arity".to_owned())?;
        let (ErlangTerm::Atom(name), ErlangTerm::Integer(arity)) =
            (&self.terms[name.0], &self.terms[arity.0])
        else {
            return Err("named function name/arity types".into());
        };
        Ok(crate::bifs::registry::Mfa {
            module: self.name.clone().into(),
            function: name.clone().into(),
            arity: arity
                .parse()
                .map_err(|_| "named function arity invalid".to_owned())?,
        })
    }
    pub fn contract_variable_map(&self, form: TermId) -> Result<TermId> {
        self.contract_variable_maps
            .get(&form)
            .copied()
            .ok_or("bounded contract lacks compiler variable-map provenance".into())
    }
    pub fn map_iterator_pairs(
        &self,
        id: TermId,
    ) -> Result<impl Iterator<Item = (TermId, TermId)> + '_> {
        let Some(ErlangTerm::Map(pairs)) = self.terms.get(id.0) else {
            return Err("expected compiler map term".into());
        };
        let order = self
            .map_iterator_orders
            .get(&id)
            .ok_or("map has no compiler iterator provenance")?;
        Ok(order.iter().map(|&index| pairs[index]))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreVersion {
    V5,
    V6,
}
impl CoreVersion {
    pub fn number(self) -> u32 {
        match self {
            Self::V5 => 5,
            Self::V6 => 6,
        }
    }
}
#[derive(Clone, Debug)]
pub struct CoreProgram {
    pub version: CoreVersion,
    pub profile: CompilerProfile,
    pub atom_layout: Option<crate::native_runtime_hash::NativeAtomLayout>,
    pub contract_closure_profile: Option<crate::constraints::closure_order::ClosureCodeProfile>,
    pub modules: Vec<CoreModule>,
    pub node_count: usize,
    pub term_count: usize,
    pub kinds: BTreeMap<String, usize>,
}
#[derive(Serialize)]
pub struct Inspection {
    pub version: u32,
    pub module_count: usize,
    pub node_count: usize,
    pub term_count: usize,
    pub kinds: BTreeMap<String, usize>,
    pub attributes: BTreeMap<String, usize>,
    pub modules: Vec<ModuleInspection>,
}
#[derive(Serialize)]
pub struct ModuleInspection {
    pub name: String,
    pub node_count: usize,
    pub term_count: usize,
    pub export_count: usize,
    pub definition_count: usize,
    pub attribute_count: usize,
    pub attributes: BTreeMap<String, usize>,
}
impl CoreProgram {
    pub fn parse(input: &str) -> Result<Self> {
        let raw: Value = serde_json::from_str(input).map_err(|e| format!("$: {e}"))?;
        Self::from_value(&raw)
    }

    pub fn from_value(raw: &Value) -> Result<Self> {
        let context = Self::context(raw)?;
        let root = object(raw, "$")?;
        let version = Some(if matches!(context.version, CoreVersion::V6) { 6 } else { 5 });
        let Self { profile, atom_layout, contract_closure_profile, .. } = context;
        let mut modules = Vec::new();
        let mut kinds = BTreeMap::new();
        let mut node_count = 0;
        let mut term_count = 0;
        for (i, raw) in array(field(root, "modules", "$")?, "$.modules")?
            .iter()
            .enumerate()
        {
            let path = format!("$.modules[{i}]");
            let obj = object(raw, &path)?;
            let name = string(field(obj, "name", &path)?, format!("{path}.name"))?;
            if modules
                .iter()
                .any(|module: &CoreModule| module.name == name)
            {
                return Err(format!("{path}.name: duplicate module {name}"));
            }
            let terms = parse_term_table(field(obj, "terms", &path)?, &format!("{path}.terms"))?;
            let mut map_iterator_orders = BTreeMap::new();
            for (j, term) in terms.iter().enumerate() {
                if let ErlangTerm::Map(pairs) = term {
                    let at = format!("{path}.terms[{j}].iterator_order");
                    let raw_terms = array(field(obj, "terms", &path)?, format!("{path}.terms"))?;
                    let record = object(&raw_terms[j], format!("{path}.terms[{j}]"))?;
                    let order = array(field(record, "iterator_order", &at)?, &at)?;
                    if order.len() != pairs.len() {
                        return Err(format!("{at}: expected a complete pair permutation"));
                    }
                    let mut seen = vec![false; pairs.len()];
                    let mut indices = Vec::with_capacity(pairs.len());
                    for (k, value) in order.iter().enumerate() {
                        let index = reference(value, pairs.len(), format!("{at}[{k}]"))?;
                        if std::mem::replace(&mut seen[index], true) {
                            return Err(format!("{at}[{k}]: repeated pair index"));
                        }
                        indices.push(index);
                    }
                    map_iterator_orders.insert(TermId(j), indices);
                }
            }
            let terms: crate::shared_vec::SharedVec<ErlangTerm> = terms.into();
            let source_forms = TermId(reference(
                field(obj, "source_forms", &path)?,
                terms.len(),
                format!("{path}.source_forms"),
            )?);
            proper_list(source_forms, &terms, &format!("{path}.source_forms"))?;
            let mut contract_variable_maps = BTreeMap::new();
            if let Some(raw) = obj.get("contract_variable_maps") {
                for (index, pair) in array(raw, format!("{path}.contract_variable_maps"))?
                    .iter()
                    .enumerate()
                {
                    let at = format!("{path}.contract_variable_maps[{index}]");
                    let pair = array(pair, &at)?;
                    if pair.len() != 2 {
                        return Err(format!("{at}: expected form/map pair"));
                    }
                    let form = TermId(reference(&pair[0], terms.len(), &at)?);
                    let map = TermId(reference(&pair[1], terms.len(), &at)?);
                    let ErlangTerm::Tuple(fields) = &terms[form.0] else {
                        return Err(format!("{at}: expected bounded_fun AST"));
                    };
                    if fields.len() != 4
                        || !matches!(&terms[fields[0].0], ErlangTerm::Atom(a) if a=="type")
                        || !matches!(&terms[fields[2].0], ErlangTerm::Atom(a) if a=="bounded_fun")
                    {
                        return Err(format!("{at}: expected bounded_fun AST"));
                    }
                    let ErlangTerm::Map(pairs) = &terms[map.0] else {
                        return Err(format!("{at}: expected compiler variable map"));
                    };
                    if pairs.iter().any(|(key, value)| {
                        !matches!(&terms[key.0], ErlangTerm::Atom(a) if a!="_")
                            && !matches!(&terms[key.0], ErlangTerm::Integer(_))
                            || !matches!(&terms[value.0], ErlangTerm::Atom(a) if a=="nil")
                    }) {
                        return Err(format!(
                            "{at}: expected variable keys and nil placeholder values"
                        ));
                    }
                    if !map_iterator_orders.contains_key(&map) {
                        return Err(format!("{at}: variable map lacks iterator provenance"));
                    }
                    if contract_variable_maps.insert(form, map).is_some() {
                        return Err(format!("{at}: repeated bounded_fun reference"));
                    }
                }
            } else if version == Some(6) {
                return Err(format!(
                    "{path}.contract_variable_maps: missing compiler provenance"
                ));
            }
            let mut nodes = crate::shared_vec::SharedVec::new();
            for (j, raw) in array(field(obj, "nodes", &path)?, format!("{path}.nodes"))?
                .iter()
                .enumerate()
            {
                let node = parse_node(raw, format_args!("{path}.nodes[{j}]"), j, &nodes, &terms)?;
                *kinds.entry(node.expr.kind().to_owned()).or_insert(0) += 1;
                nodes.push(node);
            }
            let core = NodeId(reference(
                field(obj, "core", &path)?,
                nodes.len(),
                format!("{path}.core"),
            )?);
            match &nodes[core.0].expr {
                CoreExpr::Module(module_payload) if literal_atom(module_payload.name, &nodes, &terms) == Some(name.as_str()) => {}
                CoreExpr::Module(_) => {
                    return Err(format!("{path}.core: Core module name differs from {name}"));
                }
                _ => return Err(format!("{path}.core: expected module node")),
            }
            node_count += nodes.len();
            term_count += terms.len();
            nodes.freeze();
            modules.push(CoreModule {
                name,
                source_forms,
                core,
                terms,
                nodes,
                map_iterator_orders,
                contract_variable_maps,
            });
        }
        Ok(Self {
            version: if version == Some(6) {
                CoreVersion::V6
            } else {
                CoreVersion::V5
            },
            profile,
            atom_layout,
            contract_closure_profile,
            modules,
            node_count,
            term_count,
            kinds,
        })
    }
    fn context(raw: &Value) -> Result<Self> {
        let root = object(raw, "$")?;
        let version = field(root, "version", "$")?.as_u64();
        if !matches!(version, Some(5 | 6)) {
            return Err("$.version: expected 5 or 6".into());
        }
        let profile = object(field(root, "profile", "$")?, "$.profile")?;
        let pf = |key| {
            string(
                field(profile, key, "$.profile")?,
                format!("$.profile.{key}"),
            )
        };
        let profile = CompilerProfile {
            term_map_order: match pf("term_map_order")?.as_str() {
                "native_to_list_with_iterator" => TermMapOrder::NativeToListWithIterator,
                _ => {
                    return Err(
                        "$.profile.term_map_order: expected native_to_list_with_iterator".into(),
                    );
                }
            },
            otp: pf("otp")?,
            erts: pf("erts")?,
            elixir: pf("elixir")?,
            dialyzer: pf("dialyzer")?,
            core_options: array(
                field(profile, "core_options", "$.profile")?,
                "$.profile.core_options",
            )?
            .iter()
            .enumerate()
            .map(|(i, v)| string(v, format!("$.profile.core_options[{i}]")))
            .collect::<Result<_>>()?,
        };
        let atom_layout = root
            .get("atom_layout")
            .map(crate::native_runtime_hash::NativeAtomLayout::parse)
            .transpose()
            .map_err(|error| format!("$.atom_layout: {error}"))?;
        let contract_closure_profile = root
            .get("contract_closure_profile")
            .map(crate::constraints::closure_order::ClosureCodeProfile::parse)
            .transpose()?;
        if contract_closure_profile
            .as_ref()
            .is_some_and(|p| p.module_name() != "dialyzer_contracts")
        {
            return Err(
                "$.contract_closure_profile: original dialyzer_contracts module expected".into(),
            );
        }
        Ok(Self {
            version: if version == Some(6) {
                CoreVersion::V6
            } else {
                CoreVersion::V5
            },
            profile,
            atom_layout,
            contract_closure_profile,
            modules: vec![],
            node_count: 0,
            term_count: 0,
            kinds: BTreeMap::new(),
        })
    }
    pub fn from_modules(context: &Value, modules: Vec<CoreModule>) -> Result<Self> {
        let mut program = Self::context(context)?;
        // Modules validate independently; check them on all cores, then
        // report the first failure in module order as the sequential loop did.
        let version = program.version;
        let indices = (0..modules.len()).collect::<Vec<_>>();
        let checked = crate::ordered_parallel::parallel_map(&indices, |&index| {
            let module = &modules[index];
            let mut kinds = BTreeMap::<&'static str, usize>::new();
            for node in &module.nodes {
                *kinds.entry(node.expr.kind()).or_insert(0) += 1;
            }
            (kinds, module.validate(&format!("$.modules[{index}]"), version))
        });
        let mut names = std::collections::HashSet::with_capacity(modules.len());
        for (index, (module, (kinds, valid))) in modules.iter().zip(checked).enumerate() {
            if !names.insert(module.name.as_str()) {
                return Err(format!(
                    "$.modules[{index}].name: duplicate module {}",
                    module.name
                ));
            }
            valid?;
            program.node_count += module.nodes.len();
            program.term_count += module.terms.len();
            for (kind, count) in kinds {
                *program.kinds.entry(kind.to_owned()).or_insert(0) += count;
            }
        }
        program.modules = modules;
        Ok(program)
    }
    pub fn inspect(&self) -> Inspection {
        let mut attrs = BTreeMap::new();
        let modules = self
            .modules
            .iter()
            .map(|module| {
                let CoreExpr::Module(module_payload) = &module.nodes[module.core.0].expr
                else {
                    unreachable!()
                };
                let crate::core::ModuleExpr {
                    exports,
                    attributes,
                    definitions,
                    ..
                } = module_payload.as_ref();
                let mut counts = BTreeMap::new();
                for (key, _) in attributes {
                    if let Some(name) = literal_atom(*key, &module.nodes, &module.terms) {
                        *counts.entry(name.to_owned()).or_insert(0) += 1;
                        *attrs.entry(name.to_owned()).or_insert(0) += 1;
                    }
                }
                ModuleInspection {
                    name: module.name.clone(),
                    node_count: module.nodes.len(),
                    term_count: module.terms.len(),
                    export_count: exports.len(),
                    definition_count: definitions.len(),
                    attribute_count: attributes.len(),
                    attributes: counts,
                }
            })
            .collect();
        Inspection {
            version: self.version.number(),
            module_count: self.modules.len(),
            node_count: self.node_count,
            term_count: self.term_count,
            kinds: self.kinds.clone(),
            attributes: attrs,
            modules,
        }
    }
}

fn parse_node(
    raw: &Value,
    path: impl std::fmt::Display + Copy,
    id: usize,
    nodes: &crate::shared_vec::SharedVec<CoreNode>,
    terms: &crate::shared_vec::SharedVec<ErlangTerm>,
) -> Result<CoreNode> {
    let obj = object(raw, path)?;
    let indices = |v: &Value, at: &str| -> Result<Vec<u64>> {
        array(v, at)?
            .iter()
            .enumerate()
            .map(|(j, v)| {
                v.as_u64()
                    .ok_or_else(|| format!("{at}[{j}]: expected nonnegative table index"))
            })
            .collect()
    };
    let optional_index = |key: &str| -> Result<Option<u64>> {
        obj.get(key)
            .map(|v| {
                v.as_u64()
                    .ok_or_else(|| format!("{path}.{key}: expected nonnegative table index"))
            })
            .transpose()
    };
    let raw = RawNode {
        kind: string_slice(field(obj, "kind", path)?, format_args!("{path}.kind"))?,
        annotations: field(obj, "annotations", path)?
            .as_u64()
            .ok_or_else(|| format!("{path}.annotations: expected nonnegative table index"))?,
        groups: array(field(obj, "groups", path)?, format_args!("{path}.groups"))?
            .iter()
            .enumerate()
            .map(|(i, g)| indices(g, &format!("{path}.groups[{i}]")))
            .collect::<Result<_>>()?,
        value: optional_index("value")?,
        map_arg: optional_index("map_arg")?,
        #[cfg(otp_domain = "29")]
        record_arg: optional_index("record_arg")?,
        #[cfg(otp_domain = "29")]
        record_id: optional_index("record_id")?,
        map_pattern: obj
            .get("map_pattern")
            .map(|v| {
                v.as_bool()
                    .ok_or_else(|| format!("{path}.map_pattern: expected boolean"))
            })
            .transpose()?,
    };
    build_node(&raw, path, id, nodes, terms)
}

/// A Core node record independent of its serialization (JSON wire or ETF cache).
pub(crate) struct RawNode<'a> {
    pub kind: &'a str,
    pub annotations: u64,
    pub groups: Vec<Vec<u64>>,
    pub value: Option<u64>,
    pub map_arg: Option<u64>,
    #[cfg(otp_domain = "29")]
    pub record_arg: Option<u64>,
    #[cfg(otp_domain = "29")]
    pub record_id: Option<u64>,
    pub map_pattern: Option<bool>,
}

fn index(n: u64, before: usize, path: impl std::fmt::Display) -> Result<usize> {
    match usize::try_from(n) {
        Ok(id) if id < before => Ok(id),
        _ => Err(format!(
            "{path}: index {n} must refer to earlier entry below {before}"
        )),
    }
}

pub(crate) fn build_node(
    raw: &RawNode<'_>,
    path: impl std::fmt::Display + Copy,
    id: usize,
    nodes: &crate::shared_vec::SharedVec<CoreNode>,
    terms: &crate::shared_vec::SharedVec<ErlangTerm>,
) -> Result<CoreNode> {
    let kind = raw.kind;
    let annotations = TermId(index(
        raw.annotations,
        terms.len(),
        format_args!("{path}.annotations"),
    )?);
    proper_list(annotations, terms, &format_args!("{path}.annotations"))?;
    let shape: &[Option<usize>] = match kind {
        "literal" | "var" | "opaque" => &[],
        "values" | "binary" | "tuple" | "map" => &[None],
        #[cfg(otp_domain = "29")]
        "record" => &[None],
        #[cfg(otp_domain = "29")]
        "record_pair" => &[Some(1), Some(1)],
        "bitstr" => &[Some(1), Some(1), Some(1), Some(1), Some(1)],
        "cons" | "seq" | "alias" => &[Some(1), Some(1)],
        "let" | "clause" | "receive" => &[None, Some(1), Some(1)],
        "apply" | "primop" | "case" => &[Some(1), None],
        "map_pair" => &[Some(1), Some(1), Some(1)],
        "call" => &[Some(1), Some(1), None],
        "fun" => &[None, Some(1)],
        "try" => &[Some(1), None, Some(1), None, Some(1)],
        "catch" => &[Some(1)],
        "letrec" => &[None, Some(1)],
        "module" => &[Some(1), None, None, None],
        _ => return Err(format!("{path}.kind: unsupported Core kind {kind}")),
    };
    let rawgroups = &raw.groups;
    if rawgroups.len() != shape.len() {
        return Err(format!(
            "{path}.groups: {kind} needs {} groups, got {}",
            shape.len(),
            rawgroups.len()
        ));
    }
    let mut groups = Vec::new();
    for (i, (raw, count)) in rawgroups.iter().zip(shape).enumerate() {
        let at = format!("{path}.groups[{i}]");
        if let Some(count) = count
            && raw.len() != *count
        {
            return Err(format!("{at}: expected {count} nodes, got {}", raw.len()));
        }
        if ((kind == "module" && i >= 2) || (kind == "letrec" && i == 0))
            && !raw.len().is_multiple_of(2)
        {
            return Err(format!(
                "{at}: expected flattened key/value pairs, got odd length {}",
                raw.len()
            ));
        }
        groups.push(
            raw.iter()
                .enumerate()
                .map(|(j, v)| index(*v, id, format_args!("{at}[{j}]")).map(NodeId))
                .collect::<Result<Vec<_>>>()?,
        );
    }
    let mut groups = groups.into_iter();
    let mut group = || groups.next().unwrap();
    let one = |mut g: Vec<NodeId>| g.pop().unwrap();
    let expr = match kind {
        "literal" | "var" | "opaque" => {
            let value = TermId(index(
                raw.value.ok_or_else(|| format!("{path}.value: missing field"))?,
                terms.len(),
                format_args!("{path}.value"),
            )?);
            match kind {
                "literal" => CoreExpr::Literal(value),
                "opaque" => CoreExpr::Opaque(value),
                _ => {
                    if !valid_var_name(value, terms) {
                        return Err(format!(
                            "{path}.value: expected atom, integer or atom/arity tuple variable name"
                        ));
                    }
                    CoreExpr::Var(value)
                }
            }
        }
        "values" => CoreExpr::Values(group()),
        "tuple" => CoreExpr::Tuple(group()),
        "binary" => {
            let segments = group();
            require_kinds(nodes, &segments, "bitstr", &format_args!("{path}.groups[0]"))?;
            CoreExpr::Binary(segments)
        }
        "bitstr" => CoreExpr::Bitstr {
            value: one(group()),
            size: one(group()),
            unit: one(group()),
            typ: one(group()),
            flags: one(group()),
        },
        "cons" => CoreExpr::Cons {
            head: one(group()),
            tail: one(group()),
        },
        "map" => {
            let pairs = group();
            require_kinds(nodes, &pairs, "map_pair", &format_args!("{path}.groups[0]"))?;
            let pattern = raw
                .map_pattern
                .ok_or_else(|| format!("{path}.map_pattern: expected boolean"))?;
            let base = NodeId(index(
                raw.map_arg
                    .ok_or_else(|| format!("{path}.map_arg: missing field"))?,
                id,
                format_args!("{path}.map_arg"),
            )?);
            CoreExpr::Map {
                base,
                pairs,
                pattern,
            }
        }
        "map_pair" => CoreExpr::MapPair {
            operation: one(group()),
            key: one(group()),
            value: one(group()),
        },
        #[cfg(otp_domain = "29")]
        "record" => {
            let pairs = group();
            require_kinds(nodes, &pairs, "record_pair", &format_args!("{path}.groups[0]"))?;
            let reference = |field: &str, value: Option<u64>| -> Result<NodeId> {
                Ok(NodeId(index(value.ok_or_else(|| format!("{path}.{field}: missing field"))?, id, format_args!("{path}.{field}"))?))
            };
            CoreExpr::Record { argument: reference("record_arg", raw.record_arg)?, id: reference("record_id", raw.record_id)?, pairs }
        }
        #[cfg(otp_domain = "29")]
        "record_pair" => CoreExpr::RecordPair { key: one(group()), value: one(group()) },
        "let" => {
            let vars = group();
            require_kinds(nodes, &vars, "var", &format_args!("{path}.groups[0]"))?;
            CoreExpr::Let {
                vars,
                argument: one(group()),
                body: one(group()),
            }
        }
        "seq" => CoreExpr::Seq {
            argument: one(group()),
            body: one(group()),
        },
        "apply" => CoreExpr::Apply {
            operator: one(group()),
            args: group(),
        },
        "call" => CoreExpr::Call {
            module: one(group()),
            name: one(group()),
            args: group(),
        },
        "primop" => CoreExpr::Primop {
            name: one(group()),
            args: group(),
        },
        "case" => {
            let argument = one(group());
            let clauses = group();
            require_kinds(nodes, &clauses, "clause", &format_args!("{path}.groups[1]"))?;
            CoreExpr::Case { argument, clauses }
        }
        "clause" => CoreExpr::Clause {
            patterns: group(),
            guard: one(group()),
            body: one(group()),
        },
        "alias" => {
            let variable = one(group());
            require_kind(nodes, variable, "var", &format_args!("{path}.groups[0][0]"))?;
            CoreExpr::Alias {
                variable,
                pattern: one(group()),
            }
        }
        "fun" => {
            let params = group();
            require_kinds(nodes, &params, "var", &format_args!("{path}.groups[0]"))?;
            CoreExpr::Fun {
                params,
                body: one(group()),
            }
        }
        "receive" => {
            let clauses = group();
            require_kinds(nodes, &clauses, "clause", &format_args!("{path}.groups[0]"))?;
            CoreExpr::Receive {
                clauses,
                timeout: one(group()),
                action: one(group()),
            }
        }
        "try" => {
            let argument = one(group());
            let vars = group();
            require_kinds(nodes, &vars, "var", &format_args!("{path}.groups[1]"))?;
            let body = one(group());
            let exception_vars = group();
            require_kinds(nodes, &exception_vars, "var", &format_args!("{path}.groups[3]"))?;
            CoreExpr::Try(Box::new(crate::core::TryExpr {
                argument,
                vars,
                body,
                exception_vars,
                handler: one(group()),
            }))
        }
        "catch" => CoreExpr::Catch { body: one(group()) },
        "letrec" => CoreExpr::Letrec {
            definitions: definition_pairs(nodes, terms, group(), &format_args!("{path}.groups[0]"))?,
            body: one(group()),
        },
        "module" => {
            let name = one(group());
            if literal_atom(name, nodes, terms).is_none() {
                return Err(format!(
                    "{path}.groups[0][0]: expected literal atom module name"
                ));
            }
            let exports = group();
            for (i, node) in exports.iter().enumerate() {
                if function_name(*node, nodes, terms).is_none() {
                    return Err(format!(
                        "{path}.groups[1][{i}]: expected atom/arity function variable"
                    ));
                }
            }
            let mut rawattrs = group().into_iter();
            let mut attributes = Vec::new();
            while let Some(key) = rawattrs.next() {
                let value = rawattrs.next().unwrap();
                let i = attributes.len() * 2;
                if literal_atom(key, nodes, terms).is_none() {
                    return Err(format!(
                        "{path}.groups[2][{i}]: expected literal atom attribute name"
                    ));
                }
                require_kind(
                    nodes,
                    value,
                    "literal",
                    &format_args!("{path}.groups[2][{}]", i + 1),
                )?;
                attributes.push((key, value));
            }
            let definitions =
                definition_pairs(nodes, terms, group(), &format_args!("{path}.groups[3]"))?;
            for (i, node) in exports.iter().enumerate() {
                let function = function_name(*node, nodes, terms).unwrap();
                if !definitions
                    .iter()
                    .any(|(key, _)| function_name(*key, nodes, terms) == Some(function))
                {
                    return Err(format!(
                        "{path}.groups[1][{i}]: exported function has no definition"
                    ));
                }
            }
            CoreExpr::Module(Box::new(crate::core::ModuleExpr {
                name,
                exports,
                attributes,
                definitions,
            }))
        }
        _ => unreachable!(),
    };
    if !matches!(kind, "literal" | "var" | "opaque") && raw.value.is_some() {
        return Err(format!("{path}.value: only leaf nodes carry value"));
    }
    if kind != "map" && (raw.map_arg.is_some() || raw.map_pattern.is_some()) {
        return Err(format!("{path}: map metadata on {kind} node"));
    }
    #[cfg(otp_domain = "29")]
    if kind != "record" && (raw.record_arg.is_some() || raw.record_id.is_some()) {
        return Err(format!("{path}: record metadata on {kind} node"));
    }
    Ok(CoreNode {
        id: NodeId(id),
        annotations,
        expr,
    })
}
fn definition_pairs(
    nodes: &crate::shared_vec::SharedVec<CoreNode>,
    terms: &crate::shared_vec::SharedVec<ErlangTerm>,
    raw: Vec<NodeId>,
    path: impl std::fmt::Display + Copy,
) -> Result<Vec<(NodeId, NodeId)>> {
    let mut definitions = Vec::new();
    let mut it = raw.into_iter();
    while let Some(key) = it.next() {
        let value = it.next().unwrap();
        let i = definitions.len() * 2;
        let (_, arity) = function_name(key, nodes, terms)
            .ok_or_else(|| format!("{path}[{i}]: expected atom/arity function variable"))?;
        let CoreExpr::Fun { params, .. } = &nodes[value.0].expr else {
            return Err(format!("{path}[{}]: expected fun definition", i + 1));
        };
        if params.len() != arity {
            return Err(format!(
                "{path}[{}]: function arity {arity} differs from {} params",
                i + 1,
                params.len()
            ));
        }
        if definitions
            .iter()
            .any(|(old, _)| function_name(*old, nodes, terms) == function_name(key, nodes, terms))
        {
            return Err(format!("{path}[{i}]: duplicate function definition"));
        }
        definitions.push((key, value));
    }
    Ok(definitions)
}
fn function_name<'a>(
    id: NodeId,
    nodes: &crate::shared_vec::SharedVec<CoreNode>,
    terms: &'a crate::shared_vec::SharedVec<ErlangTerm>,
) -> Option<(&'a str, usize)> {
    if let CoreExpr::Var(term) = nodes[id.0].expr
        && let ErlangTerm::Tuple(items) = &terms[term.0]
        && items.len() == 2
        && let (ErlangTerm::Atom(name), ErlangTerm::Integer(arity)) =
            (&terms[items[0].0], &terms[items[1].0])
    {
        return Some((name, arity.parse().ok()?));
    }
    None
}
fn valid_var_name(id: TermId, terms: &crate::shared_vec::SharedVec<ErlangTerm>) -> bool {
    match &terms[id.0] {
        ErlangTerm::Atom(_) | ErlangTerm::Integer(_) => true,
        ErlangTerm::Tuple(items) if items.len() == 2 => {
            matches!((&terms[items[0].0],&terms[items[1].0]),(ErlangTerm::Atom(_),ErlangTerm::Integer(n)) if n.parse::<usize>().is_ok())
        }
        _ => false,
    }
}
fn literal_atom<'a>(id: NodeId, nodes: &crate::shared_vec::SharedVec<CoreNode>, terms: &'a crate::shared_vec::SharedVec<ErlangTerm>) -> Option<&'a str> {
    if let CoreExpr::Literal(term) = nodes[id.0].expr
        && let ErlangTerm::Atom(name) = &terms[term.0]
    {
        Some(name)
    } else {
        None
    }
}
fn require_kind(nodes: &crate::shared_vec::SharedVec<CoreNode>, id: NodeId, kind: &str, path: impl std::fmt::Display + Copy) -> Result<()> {
    if nodes[id.0].expr.kind() == kind {
        Ok(())
    } else {
        Err(format!(
            "{path}: expected {kind}, got {}",
            nodes[id.0].expr.kind()
        ))
    }
}
fn require_kinds(nodes: &crate::shared_vec::SharedVec<CoreNode>, ids: &[NodeId], kind: &str, path: impl std::fmt::Display + Copy) -> Result<()> {
    for (i, id) in ids.iter().enumerate() {
        require_kind(nodes, *id, kind, format_args!("{path}[{i}]"))?;
    }
    Ok(())
}
fn proper_list(mut id: TermId, terms: &crate::shared_vec::SharedVec<ErlangTerm>, path: impl std::fmt::Display + Copy) -> Result<()> {
    loop {
        match terms[id.0] {
            ErlangTerm::Nil => return Ok(()),
            ErlangTerm::List { tail, .. } => id = tail,
            _ => return Err(format!("{path}: expected proper Erlang list")),
        }
    }
}
fn reference(v: &Value, before: usize, path: impl std::fmt::Display) -> Result<usize> {
    let id = v
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| format!("{path}: expected nonnegative table index"))?;
    if id >= before {
        Err(format!(
            "{path}: index {id} must refer to earlier entry below {before}"
        ))
    } else {
        Ok(id)
    }
}
fn object(v: &Value, path: impl std::fmt::Display) -> Result<&Map<String, Value>> {
    v.as_object()
        .ok_or_else(|| format!("{path}: expected object"))
}
fn field<'a>(
    obj: &'a Map<String, Value>,
    name: &str,
    path: impl std::fmt::Display,
) -> Result<&'a Value> {
    obj.get(name)
        .ok_or_else(|| format!("{path}.{name}: missing field"))
}
fn array(v: &Value, path: impl std::fmt::Display) -> Result<&[Value]> {
    v.as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("{path}: expected array"))
}
fn string_slice(v: &Value, path: impl std::fmt::Display) -> Result<&str> {
    v.as_str().ok_or_else(|| format!("{path}: expected string"))
}
fn string(v: &Value, path: impl std::fmt::Display) -> Result<String> {
    string_slice(v, path).map(str::to_owned)
}

fn parse_term(raw: &Value, path: impl std::fmt::Display + Copy, id: usize) -> Result<ErlangTerm> {
    let obj = object(raw, path)?;
    let kind = string_slice(field(obj, "kind", path)?, format_args!("{path}.kind"))?;
    let text = || string_slice(field(obj, "value", path)?, format_args!("{path}.value"));
    let items = || {
        array(field(obj, "items", path)?, format_args!("{path}.items"))?
            .iter()
            .enumerate()
            .map(|(i, v)| reference(v, id, format_args!("{path}.items[{i}]")).map(TermId))
            .collect::<Result<Vec<_>>>()
    };
    match kind {
        "atom" => Ok(ErlangTerm::Atom(text()?.to_owned())),
        "external_fun" => {
            let module = string_slice(field(obj, "module", path)?, path)?.to_owned();
            let function = string_slice(field(obj, "function", path)?, path)?.to_owned();
            if module.chars().count() > 255 || function.chars().count() > 255 {
                return Err(format!(
                    "{path}: external function atom exceeds 255 characters"
                ));
            }
            let arity = field(obj, "arity", path)?
                .as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .ok_or_else(|| format!("{path}.arity: expected integer in 0..255"))?;
            Ok(ErlangTerm::ExternalFun(Box::new(
                crate::core::ExternalFun {
                    module,
                    function,
                    arity,
                },
            )))
        }
        "integer" => {
            let n = text()?.to_owned();
            let digits = n.strip_prefix('-').unwrap_or(&n);
            if digits.is_empty()
                || !digits.bytes().all(|b| b.is_ascii_digit())
                || (digits.len() > 1 && digits.starts_with('0'))
                || n == "-0"
            {
                return Err(format!("{path}.value: expected canonical decimal integer"));
            }
            Ok(ErlangTerm::Integer(n))
        }
        "float" => {
            let value = text()?;
            if value.len() != 16
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
            {
                return Err(format!(
                    "{path}.value: expected uppercase 16-digit hexadecimal float64 bits"
                ));
            }
            Ok(ErlangTerm::Float(
                u64::from_str_radix(value, 16).map_err(|e| format!("{path}.value: {e}"))?,
            ))
        }
        "binary" => Ok(ErlangTerm::Binary(base64(
            text()?,
            format_args!("{path}.value"),
        )?)),
        "bitstring" => {
            let bytes = base64(
                string_slice(field(obj, "bytes", path)?, format_args!("{path}.bytes"))?,
                format_args!("{path}.bytes"),
            )?;
            let bits = field(obj, "bits", path)?
                .as_u64()
                .ok_or_else(|| format!("{path}.bits: expected nonnegative integer"))?;
            if bits.div_ceil(8) != bytes.len() as u64 {
                return Err(format!("{path}.bits: length does not match padded bytes"));
            }
            let pad = (8 - bits % 8) % 8;
            if pad != 0 && bytes.last().is_some_and(|b| b & ((1 << pad) - 1) != 0) {
                return Err(format!("{path}.bytes: nonzero bitstring padding"));
            }
            Ok(ErlangTerm::Bitstring { bytes, bits })
        }
        "tuple" => Ok(ErlangTerm::Tuple(items()?)),
        "nil" => Ok(ErlangTerm::Nil),
        "list" => Ok(ErlangTerm::List {
            items: items()?,
            tail: TermId(reference(
                field(obj, "tail", path)?,
                id,
                format_args!("{path}.tail"),
            )?),
        }),
        "map" => {
            let mut pairs = Vec::new();
            for (i, raw) in array(field(obj, "entries", path)?, format_args!("{path}.entries"))?
                .iter()
                .enumerate()
            {
                let at = format!("{path}.entries[{i}]");
                let raw = array(raw, &at)?;
                if raw.len() != 2 {
                    return Err(format!("{at}: expected key/value pair"));
                }
                pairs.push((
                    TermId(reference(&raw[0], id, format_args!("{at}[0]"))?),
                    TermId(reference(&raw[1], id, format_args!("{at}[1]"))?),
                ));
            }
            Ok(ErlangTerm::Map(pairs))
        }
        #[cfg(otp_domain = "29")]
        "record" => {
            let module = string(field(obj, "module", path)?, path)?;
            let name = string(field(obj, "name", path)?, path)?;
            let exported = field(obj, "exported", path)?.as_bool().ok_or_else(|| format!("{path}.exported: expected boolean"))?;
            let mut fields = Vec::new();
            let mut names = std::collections::BTreeSet::new();
            for (position, value) in array(field(obj, "fields", path)?, path)?.iter().enumerate() {
                let pair = array(value, path)?;
                if pair.len() != 2 { return Err(format!("{path}.fields[{position}]: expected field name/value pair")); }
                let name = string(&pair[0], path)?;
                if name.chars().count() > 255 || !names.insert(name.clone()) { return Err(format!("{path}: invalid or duplicate record field")); }
                fields.push((name, TermId(reference(&pair[1], id, path)?)));
            }
            if module.chars().count() > 255 || name.chars().count() > 255 { return Err(format!("{path}: record atom exceeds 255 characters")); }
            Ok(ErlangTerm::Record(Box::new(NativeRecord { module, name, exported, fields })))
        }
        _ => Err(format!("{path}.kind: unsupported Erlang term kind {kind}")),
    }
}
fn base64(input: &str, path: impl std::fmt::Display) -> Result<Vec<u8>> {
    if !input.len().is_multiple_of(4) {
        return Err(format!("{path}: base64 length must be divisible by 4"));
    }
    let mut bytes = Vec::new();
    let chunks = input.as_bytes().chunks_exact(4);
    let count = chunks.len();
    for (i, chunk) in chunks.enumerate() {
        let pad = if chunk[2] == b'=' {
            2
        } else if chunk[3] == b'=' {
            1
        } else {
            0
        };
        if pad > 0 && i + 1 != count {
            return Err(format!("{path}: base64 padding before final group"));
        }
        if pad == 2 && chunk[3] != b'=' {
            return Err(format!("{path}: invalid base64 padding"));
        }
        let digit = |b| match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        };
        let mut values = [0_u8; 4];
        for j in 0..4 - pad {
            values[j] =
                digit(chunk[j]).ok_or_else(|| format!("{path}: invalid base64 character"))?;
        }
        if (pad == 2 && values[1] & 15 != 0) || (pad == 1 && values[2] & 3 != 0) {
            return Err(format!("{path}: nonzero base64 padding bits"));
        }
        bytes.push(values[0] << 2 | values[1] >> 4);
        if pad < 2 {
            bytes.push(values[1] << 4 | values[2] >> 2);
        }
        if pad == 0 {
            bytes.push(values[2] << 6 | values[3]);
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    struct Probe {
        terms: Vec<Value>,
        nodes: Vec<Value>,
    }
    impl Probe {
        fn new() -> Self {
            Self {
                terms: vec![json!({"kind":"nil"})],
                nodes: Vec::new(),
            }
        }
        fn term(&mut self, mut t: Value) -> usize {
            if t["kind"] == "map" {
                let count = t["entries"].as_array().unwrap().len();
                t["iterator_order"] = json!((0..count).collect::<Vec<_>>());
            }
            let id = self.terms.len();
            self.terms.push(t);
            id
        }
        fn atom(&mut self, s: &str) -> usize {
            self.term(json!({"kind":"atom","value":s}))
        }
        fn add(&mut self, kind: &str, groups: Value) -> usize {
            let id = self.nodes.len();
            self.nodes
                .push(json!({"kind":kind,"annotations":0,"groups":groups}));
            id
        }
        fn leaf(&mut self, kind: &str, t: usize) -> usize {
            let id = self.add(kind, json!([]));
            self.nodes[id]["value"] = json!(t);
            id
        }
        fn lit(&mut self, s: &str) -> usize {
            let t = self.atom(s);
            self.leaf("literal", t)
        }
        fn var(&mut self, s: &str) -> usize {
            let t = self.atom(s);
            self.leaf("var", t)
        }
        fn fvar(&mut self, s: &str, n: usize) -> usize {
            let name = self.atom(s);
            let n = self.term(json!({"kind":"integer","value":n.to_string()}));
            let t = self.term(json!({"kind":"tuple","items":[name,n]}));
            self.leaf("var", t)
        }
        fn module(&mut self, defs: Value) -> usize {
            let name = self.lit("m");
            self.add("module", json!([[name], [], [], defs]))
        }
        fn program(&self, core: usize) -> String {
            json!({"version":5,"profile":{"term_map_order":"native_to_list_with_iterator","otp":"28","erts":"16","elixir":"1.19.4","dialyzer":"5","core_options":[]},"modules":[{"name":"m","source_forms":0,"terms":self.terms,"nodes":self.nodes,"core":core}]}).to_string()
        }
        fn decode(&self) -> Result<Vec<CoreNode>> {
            let terms = self
                .terms
                .iter()
                .enumerate()
                .map(|(i, raw)| parse_term(raw, &format!("$.terms[{i}]"), i))
                .collect::<Result<crate::shared_vec::SharedVec<_>>>()?;
            let mut nodes = crate::shared_vec::SharedVec::new();
            for (i, raw) in self.nodes.iter().enumerate() {
                nodes.push(parse_node(
                    raw,
                    &format!("$.nodes[{i}]"),
                    i,
                    &nodes,
                    &terms,
                )?);
            }
            Ok(nodes.to_vec())
        }
    }
    #[test]
    fn all_twenty_four_variants_have_typed_payloads() {
        let mut p = Probe::new();
        let l = p.lit("ok");
        let v = p.var("x");
        let opaque = p.atom("opaque_probe");
        p.leaf("opaque", opaque);
        p.add("values", json!([[l]]));
        let assoc = p.lit("assoc");
        let pair = p.add("map_pair", json!([[assoc], [l], [l]]));
        let bit = p.add("bitstr", json!([[l], [l], [l], [l], [l]]));
        p.add("binary", json!([[bit]]));
        p.add("cons", json!([[l], [l]]));
        p.add("tuple", json!([[l]]));
        let map = p.add("map", json!([[pair]]));
        p.nodes[map]["map_pattern"] = json!(false);
        p.nodes[map]["map_arg"] = json!(v);
        p.add("let", json!([[v], [l], [l]]));
        p.add("seq", json!([[l], [l]]));
        p.add("apply", json!([[v], [l]]));
        p.add("call", json!([[l], [l], [l]]));
        p.add("primop", json!([[l], [l]]));
        let clause = p.add("clause", json!([[v], [l], [l]]));
        p.add("case", json!([[l], [clause]]));
        p.add("alias", json!([[v], [l]]));
        let fun = p.add("fun", json!([[], [l]]));
        p.add("receive", json!([[clause], [l], [l]]));
        p.add("try", json!([[l], [v], [l], [v], [l]]));
        p.add("catch", json!([[l]]));
        let fvar = p.fvar("f", 0);
        p.add("letrec", json!([[fvar, fun], [l]]));
        let core = p.module(json!([fvar, fun]));
        let parsed = CoreProgram::parse(&p.program(core)).unwrap();
        let wire:Value=serde_json::from_str(&p.program(core)).unwrap();
        let typed=CoreProgram::from_modules(&wire,parsed.modules.clone()).unwrap();
        assert_eq!(typed.to_wire_value(&wire),parsed.to_wire_value(&wire));
        assert_eq!(typed.kinds,parsed.kinds);
        assert_eq!(parsed.kinds.len(), 24);
        assert!(matches!(parsed.modules[0].nodes[map].expr,CoreExpr::Map {base,..} if base.0==v));
    }
    #[test]
    fn compiler_map_iterator_requires_a_complete_distinct_permutation() {
        let mut p = Probe::new();
        let a = p.atom("a");
        let b = p.atom("b");
        let map = p.term(json!({"kind":"map","entries":[[a,b],[b,a]]}));
        p.terms[map]["iterator_order"] = json!([1, 0]);
        let core = p.module(json!([]));
        let parsed = CoreProgram::parse(&p.program(core)).unwrap();
        assert_eq!(
            parsed.modules[0]
                .map_iterator_pairs(TermId(map))
                .unwrap()
                .collect::<Vec<_>>(),
            vec![(TermId(b), TermId(a)), (TermId(a), TermId(b))]
        );
        for invalid in [json!([0, 0]), json!([0]), json!([0, 2]), json!([0, -1])] {
            p.terms[map]["iterator_order"] = invalid;
            assert!(
                CoreProgram::parse(&p.program(core))
                    .unwrap_err()
                    .contains("iterator_order")
            );
        }
        p.terms[map]
            .as_object_mut()
            .unwrap()
            .remove("iterator_order");
        assert!(
            CoreProgram::parse(&p.program(core))
                .unwrap_err()
                .contains("iterator_order")
        );
    }

    #[test]
    fn terms_keep_bigints_float_bits_binary_and_improper_list_distinct() {
        let mut p = Probe::new();
        let a = p.atom("a");
        let b = p.term(json!({"kind":"binary","value":"YQ=="}));
        let i = p.term(json!({"kind":"integer","value":"123456789012345678901234567890"}));
        let f = p.term(json!({"kind":"float","value":"8000000000000000"}));
        let bits = p.term(json!({"kind":"bitstring","bytes":"gA==","bits":1}));
        let list = p.term(json!({"kind":"list","items":[a,i],"tail":a}));
        let tuple = p.term(json!({"kind":"tuple","items":[a,b]}));
        let map = p.term(json!({"kind":"map","entries":[[a,b]]}));
        let core = p.module(json!([]));
        let parsed = CoreProgram::parse(&p.program(core)).unwrap();
        let terms = &parsed.modules[0].terms;
        assert_ne!(terms[a], terms[b]);
        assert_eq!(
            terms[i],
            ErlangTerm::Integer("123456789012345678901234567890".into())
        );
        assert_eq!(terms[f], ErlangTerm::Float((-0.0_f64).to_bits()));
        assert_eq!(
            terms[bits],
            ErlangTerm::Bitstring {
                bytes: vec![128],
                bits: 1
            }
        );
        assert_eq!(
            terms[list],
            ErlangTerm::List {
                items: vec![TermId(a), TermId(i)],
                tail: TermId(a)
            }
        );
        assert_eq!(terms[tuple], ErlangTerm::Tuple(vec![TermId(a), TermId(b)]));
        assert_eq!(terms[map], ErlangTerm::Map(vec![(TermId(a), TermId(b))]));
    }
    #[test]
    fn malformed_terms_and_forward_references_report_paths() {
        for (raw, path) in [
            (json!({"kind":"integer","value":"01"}), "$.probe.value"),
            (
                json!({"kind":"float","value":"800000000000000a"}),
                "$.probe.value",
            ),
            (json!({"kind":"binary","value":"A==="}), "$.probe.value"),
            (json!({"kind":"binary","value":"Zh=="}), "$.probe.value"),
            (
                json!({"kind":"bitstring","bytes":"gQ==","bits":1}),
                "$.probe.bytes",
            ),
            (json!({"kind":"map","entries":[[0]]}), "$.probe.entries[0]"),
            (json!({"kind":"tuple","items":[2]}), "$.probe.items[0]"),
            (json!({"kind":"list","items":[],"tail":2}), "$.probe.tail"),
        ] {
            assert!(parse_term(&raw, "$.probe", 2).unwrap_err().contains(path));
        }
        let mut p = Probe::new();
        let seq = p.add("seq", json!([[0], [0]]));
        assert!(
            p.decode()
                .unwrap_err()
                .contains(&format!("$.nodes[{seq}].groups[0][0]"))
        );
    }
    #[test]
    fn malformed_groups_pairs_and_definition_arity_are_rejected() {
        let mut p = Probe::new();
        let l = p.lit("ok");
        p.add("let", json!([[], []]));
        assert!(p.decode().unwrap_err().contains("$.nodes[1].groups"));
        let mut p = Probe::new();
        let f = p.fvar("f", 1);
        let fun = p.add("fun", json!([[], [l]]));
        let core = p.module(json!([f, fun]));
        assert!(
            CoreProgram::parse(&p.program(core))
                .unwrap_err()
                .contains("function arity")
        );
        let mut p = Probe::new();
        let f = p.fvar("f", 0);
        let core = p.module(json!([f]));
        assert!(
            CoreProgram::parse(&p.program(core))
                .unwrap_err()
                .contains("flattened key/value pairs")
        );
        let mut p = Probe::new();
        let l = p.lit("ok");
        p.add("fun", json!([[l], [l]]));
        assert!(p.decode().unwrap_err().contains("expected var"));
    }
    #[test]
    fn source_forms_and_raw_attributes_are_retained() {
        let mut p = Probe::new();
        let key = p.lit("opaque");
        let secret = p.atom("secret");
        let payload = p.term(json!({"kind":"list","items":[secret],"tail":0}));
        let value = p.leaf("literal", payload);
        let core = p.module(json!([]));
        p.nodes[core]["groups"][2] = json!([key, value]);
        let raw = p.program(core);
        let parsed = CoreProgram::parse(&raw).unwrap();
        assert_eq!(parsed.inspect().attributes["opaque"], 1);
        assert_eq!(parsed.modules[0].source_forms, TermId(0));
        assert!(
            matches!(parsed.modules[0].nodes[value].expr,CoreExpr::Literal(TermId(id)) if id==payload)
        );
    }
    #[test]
    fn value_and_json_entry_points_share_validation() {
        let mut p = Probe::new();
        let core = p.module(json!([]));
        let mut wire: Value = serde_json::from_str(&p.program(core)).unwrap();
        let from_value = CoreProgram::from_value(&wire).unwrap();
        let from_json = CoreProgram::parse(&wire.to_string()).unwrap();
        assert_eq!(from_value.modules[0].name, from_json.modules[0].name);
        assert_eq!(from_value.modules[0].core, from_json.modules[0].core);
        assert_eq!(
            from_value.inspect().node_count,
            from_json.inspect().node_count
        );
        wire["version"] = json!(999);
        assert_eq!(
            CoreProgram::from_value(&wire).unwrap_err(),
            CoreProgram::parse(&wire.to_string()).unwrap_err()
        );
    }
    #[test]
    fn external_function_validation_keeps_unicode_limit_and_error_location() {
        let mut probe = Probe::new();
        let core = probe.module(json!([]));
        let mut parsed = CoreProgram::parse(&probe.program(core)).unwrap();
        let mut module = parsed.modules.remove(0);
        let index = module.terms.len();
        module.terms.push(ErlangTerm::ExternalFun(Box::new(ExternalFun {
            module: "λ".repeat(255),
            function: "f".into(),
            arity: 2,
        })));
        module.validate("$.modules[0]", parsed.version).unwrap();
        let ErlangTerm::ExternalFun(payload) = module.terms.make_mut(index) else {
            unreachable!();
        };
        payload.module.push('λ');
        assert_eq!(
            module.validate("$.modules[0]", parsed.version).unwrap_err(),
            format!("$.modules[0].terms[{index}]: external function atom exceeds 255 characters")
        );
    }
    #[test]
    fn deep_core_and_terms_drop_without_recursive_ownership() {
        let mut p = Probe::new();
        let l = p.lit("ok");
        let mut body = l;
        for _ in 0..10_000 {
            body = p.add("seq", json!([[l], [body]]));
        }
        let fun = p.add("fun", json!([[], [body]]));
        let f = p.fvar("f", 0);
        let core = p.module(json!([f, fun]));
        let mut deep_term = 0;
        for _ in 0..10_000 {
            deep_term = p.term(json!({"kind":"tuple","items":[deep_term]}));
        }
        let source = p.term(json!({"kind":"list","items":[deep_term],"tail":0}));
        let mut raw: Value = serde_json::from_str(&p.program(core)).unwrap();
        raw["modules"][0]["source_forms"] = json!(source);
        let parsed = CoreProgram::parse(&raw.to_string()).unwrap();
        assert_eq!(parsed.modules[0].source_forms, TermId(source));
        assert_eq!(parsed.kinds["seq"], 10_000);
        drop(parsed);
    }
}

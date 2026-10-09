use crate::core::{CoreExpr, CoreProgram, ErlangTerm, NodeId};
use crate::program::{BindingId, FunctionId, FunctionName, ModuleId, NodeRef, Program};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct TargetSet {
    pub local: BTreeSet<FunctionId>,
    pub external: bool,
}
impl TargetSet {
    pub fn singleton(id: FunctionId) -> Self {
        Self {
            local: BTreeSet::from([id]),
            external: false,
        }
    }
    pub fn external() -> Self {
        Self {
            local: BTreeSet::new(),
            external: true,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.local.is_empty() && !self.external
    }
    fn union(&self, other: &Self) -> Self {
        Self {
            local: self.local.union(&other.local).copied().collect(),
            external: self.external || other.external,
        }
    }
    /// In-place union; accumulating with `union` rebuilt the whole set per call.
    fn extend_from(&mut self, other: &Self) {
        self.local.extend(other.local.iter().copied());
        self.external |= other.external;
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct TargetValueId(pub usize);
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TargetValue {
    Single(TargetSet),
    Values(Vec<TargetValueId>),
}
#[derive(Clone, Debug, Default)]
pub struct TargetValues {
    pub values: Vec<TargetValue>,
    interned: BTreeMap<TargetValue, TargetValueId>,
}
impl TargetValues {
    fn intern(&mut self, value: TargetValue) -> TargetValueId {
        if let Some(id) = self.interned.get(&value) {
            return *id;
        }
        let id = TargetValueId(self.values.len());
        self.values.push(value.clone());
        self.interned.insert(value, id);
        id
    }
    fn single(&mut self, s: TargetSet) -> TargetValueId {
        self.intern(TargetValue::Single(s))
    }
    fn none(&mut self) -> TargetValueId {
        self.single(TargetSet::default())
    }
    fn external(&mut self) -> TargetValueId {
        self.single(TargetSet::external())
    }
    fn flatten(&self, root: TargetValueId) -> TargetSet {
        let mut tasks = vec![root];
        let mut out = TargetSet::default();
        while let Some(id) = tasks.pop() {
            match &self.values[id.0] {
                TargetValue::Single(s) => out.extend_from(s),
                TargetValue::Values(xs) => tasks.extend(xs),
            }
        }
        out
    }
    fn is_none(&self, id: TargetValueId) -> bool {
        matches!(&self.values[id.0],TargetValue::Single(s) if s.is_empty())
    }
    fn merge(&mut self, a: TargetValueId, b: TargetValueId) -> Result<TargetValueId> {
        enum Task {
            Pair(TargetValueId, TargetValueId),
            Finish(usize),
        }
        let mut tasks = vec![Task::Pair(a, b)];
        let mut out = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Pair(a, b) => {
                    if self.is_none(a) {
                        out.push(b);
                        continue;
                    }
                    if self.is_none(b) {
                        out.push(a);
                        continue;
                    }
                    match (self.values[a.0].clone(), self.values[b.0].clone()) {
                        (TargetValue::Single(a), TargetValue::Single(b)) => {
                            out.push(self.single(a.union(&b)))
                        }
                        (TargetValue::Values(a), TargetValue::Values(b)) => {
                            if a.len() != b.len() {
                                return Err("native dependency output value arities differ".into());
                            }
                            tasks.push(Task::Finish(a.len()));
                            tasks.extend(a.into_iter().zip(b).rev().map(|(a, b)| Task::Pair(a, b)));
                        }
                        _ => {
                            return Err(
                                "native dependency output mixes single and multi-value shapes"
                                    .into(),
                            );
                        }
                    }
                }
                Task::Finish(n) => {
                    let start = out
                        .len()
                        .checked_sub(n)
                        .ok_or("dependency output merge underflow")?;
                    let children = out.split_off(start);
                    out.push(self.intern(TargetValue::Values(children)));
                }
            }
        }
        out.pop()
            .ok_or("dependency output merge produced no value".into())
    }
    fn merge_many(&mut self, values: &[TargetValueId]) -> Result<TargetValueId> {
        let mut out = self.none();
        for v in values {
            out = self.merge(out, *v)?;
        }
        Ok(out)
    }
}
#[derive(Deserialize)]
struct Mfa {
    module: String,
    name: String,
    arity: usize,
}
impl From<Mfa> for FunctionName {
    fn from(m: Mfa) -> Self {
        Self {
            module: m.module,
            name: m.name,
            arity: m.arity,
        }
    }
}
#[derive(Deserialize)]
struct Prim {
    name: String,
    arity: usize,
}
#[derive(Deserialize)]
struct RawPolicy {
    version: u32,
    otp_tag: String,
    purity_default: bool,
    literal_default: bool,
    escape_default: bool,
    pure: Vec<Mfa>,
    literal_remote: Vec<Mfa>,
    literal_primitive: Vec<Prim>,
    noescape_remote: Vec<Mfa>,
    noescape_primitive: Vec<Prim>,
    source_hashes: BTreeMap<String, String>,
}
#[derive(Clone, Debug)]
pub struct DependencyPolicy {
    pub otp_tag: String,
    pub source_hashes: BTreeMap<String, String>,
    pure: BTreeSet<FunctionName>,
    literal_remote: BTreeSet<FunctionName>,
    literal_primitive: BTreeSet<(String, usize)>,
    noescape_remote: BTreeSet<FunctionName>,
    noescape_primitive: BTreeSet<(String, usize)>,
}
impl DependencyPolicy {
    pub fn parse(json: &str) -> Result<Self> {
        let p: RawPolicy = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if p.version != 1 || p.purity_default || p.literal_default || !p.escape_default {
            return Err("unsupported native dependency policy defaults".into());
        }
        Ok(Self {
            otp_tag: p.otp_tag,
            source_hashes: p.source_hashes,
            pure: p.pure.into_iter().map(Into::into).collect(),
            literal_remote: p.literal_remote.into_iter().map(Into::into).collect(),
            literal_primitive: p
                .literal_primitive
                .into_iter()
                .map(|p| (p.name, p.arity))
                .collect(),
            noescape_remote: p.noescape_remote.into_iter().map(Into::into).collect(),
            noescape_primitive: p
                .noescape_primitive
                .into_iter()
                .map(|p| (p.name, p.arity))
                .collect(),
        })
    }
}
#[derive(Clone, Debug)]
pub struct ClosureDependencies {
    pub call_targets: BTreeMap<NodeRef, TargetSet>,
    pub escaping: TargetSet,
    pub dependencies: Vec<TargetSet>,
    pub root_dependencies: TargetSet,
    pub external_dependencies: TargetSet,
    pub letrec_targets: BTreeMap<BindingId, FunctionId>,
    pub letrec_return_arities: BTreeMap<BindingId, usize>,
    pub outputs: TargetValues,
    pub module_callers: Vec<Vec<Option<FunctionId>>>,
    pub module_roots: Vec<TargetSet>,
    pub module_escaping: Vec<TargetSet>,
}
type Env = im::OrdMap<BindingId, TargetValueId>;
#[derive(Clone, Copy)]
struct Site {
    module: ModuleId,
    node: NodeId,
}
impl Site {
    fn reference(self) -> NodeRef {
        NodeRef {
            module: self.module,
            node: self.node.0,
        }
    }
}
enum Frame {
    Eval(Site, Env, Option<FunctionId>, usize),
    Many {
        module: ModuleId,
        nodes: Vec<NodeId>,
        index: usize,
        env: Env,
        current: Option<FunctionId>,
        num_returns: usize,
        values: Vec<TargetValueId>,
        finish: Finish,
    },
    Fun(FunctionId),
    Let {
        site: Site,
        vars: Vec<NodeId>,
        body: NodeId,
        env: Env,
        current: Option<FunctionId>,
        num_returns: usize,
    },
    Seq {
        site: Site,
        body: NodeId,
        env: Env,
        current: Option<FunctionId>,
        num_returns: usize,
    },
    Definitions {
        site: Site,
        body: Option<NodeId>,
        env: Env,
        current: Option<FunctionId>,
        num_returns: usize,
    },
    Case {
        site: Site,
        clauses: Vec<NodeId>,
        env: Env,
        current: Option<FunctionId>,
        num_returns: usize,
    },
    Clauses {
        site: Site,
        clauses: Vec<NodeId>,
        index: usize,
        argument: TargetValueId,
        env: Env,
        current: Option<FunctionId>,
        num_returns: usize,
        values: Vec<TargetValueId>,
    },
    Clause {
        site: Site,
        body: NodeId,
        env: Env,
        current: Option<FunctionId>,
        num_returns: usize,
    },
    Try {
        site: Site,
        vars: Vec<NodeId>,
        body: NodeId,
        exception_vars: Vec<NodeId>,
        handler: NodeId,
        env: Env,
        current: Option<FunctionId>,
        num_returns: usize,
    },
    Handler {
        site: Site,
        exception_vars: Vec<NodeId>,
        handler: NodeId,
        env: Env,
        current: Option<FunctionId>,
        num_returns: usize,
    },
    TryJoin(TargetValueId),
    Receive {
        site: Site,
        timeout: NodeId,
        action: NodeId,
        env: Env,
        current: Option<FunctionId>,
        num_returns: usize,
    },
    ReceiveTimeout {
        site: Site,
        action: NodeId,
        env: Env,
        current: Option<FunctionId>,
        num_returns: usize,
        clauses: TargetValueId,
    },
}
enum Finish {
    Merged,
    Values,
    Apply {
        site: Site,
        operator: NodeId,
        args: usize,
    },
    Call(Site),
    Primop(Site),
}
struct Analysis<'a> {
    core: &'a CoreProgram,
    program: &'a Program,
    policy: &'a DependencyPolicy,
    result: ModuleDependencies,
    active_module: ModuleId,
    caller_seen: BTreeSet<Option<FunctionId>>,
}
/// What one module's walk contributes to `ClosureDependencies`.
#[derive(Default)]
struct ModuleDependencies {
    call_targets: BTreeMap<NodeRef, TargetSet>,
    escaping: TargetSet,
    dependencies: BTreeMap<FunctionId, TargetSet>,
    root_dependencies: TargetSet,
    letrec_targets: BTreeMap<BindingId, FunctionId>,
    letrec_return_arities: BTreeMap<BindingId, usize>,
    /// Interned values of this module's walk; ids are module-local.
    outputs: TargetValues,
    module_callers: Vec<Option<FunctionId>>,
    module_roots: TargetSet,
    module_escaping: TargetSet,
}
impl ClosureDependencies {
    pub fn analyze(
        core: &CoreProgram,
        program: &Program,
        policy: &DependencyPolicy,
    ) -> Result<Self> {
        // A module's walk reads only its own state and writes only entries of
        // its own functions, sites and bindings (or set unions), so modules
        // are walked independently on all cores and merged in module order.
        // The first failing module in module order is reported, as before.
        let indices = (0..core.modules.len()).collect::<Vec<_>>();
        let partials = crate::ordered_parallel::setup_map(&indices, |&index| -> Result<ModuleDependencies> {
            let mut a = Analysis {
                core,
                program,
                policy,
                active_module: ModuleId(index),
                caller_seen: BTreeSet::new(),
                result: ModuleDependencies::default(),
            };
            a.on_load(ModuleId(index))?;
            a.walk(Site {
                module: ModuleId(index),
                node: core.modules[index].core,
            })?;
            Ok(a.result)
        });
        let mut result = Self {
            call_targets: BTreeMap::new(),
            escaping: TargetSet::default(),
            dependencies: vec![TargetSet::default(); program.functions.len()],
            root_dependencies: TargetSet::default(),
            external_dependencies: TargetSet::default(),
            letrec_targets: BTreeMap::new(),
            letrec_return_arities: BTreeMap::new(),
            outputs: TargetValues::default(),
            module_callers: vec![Vec::new(); core.modules.len()],
            module_roots: vec![TargetSet::default(); core.modules.len()],
            module_escaping: vec![TargetSet::default(); core.modules.len()],
        };
        for f in &program.functions {
            if f.exported {
                result.escaping.local.insert(f.id);
                result.module_escaping[f.module.0].local.insert(f.id);
            }
        }
        for (index, partial) in partials.into_iter().enumerate() {
            let partial = partial?;
            if crate::lattice::verify_fast_paths() {
                // The independent walks rely on each module writing only
                // its own entries.
                assert!(partial.call_targets.keys().all(|site| site.module.0 == index));
                assert!(partial
                    .dependencies
                    .keys()
                    .all(|id| program.functions[id.0].module.0 == index));
                assert!(partial.letrec_targets.keys().all(|binding| result.letrec_targets.get(binding).is_none()));
            }
            result.call_targets.extend(partial.call_targets);
            result.escaping.extend_from(&partial.escaping);
            for (id, targets) in partial.dependencies {
                result.dependencies[id.0].extend_from(&targets);
            }
            result.root_dependencies.extend_from(&partial.root_dependencies);
            result.letrec_targets.extend(partial.letrec_targets);
            result.letrec_return_arities.extend(partial.letrec_return_arities);
            result.module_callers[index] = partial.module_callers;
            result.module_roots[index] = partial.module_roots;
            result.module_escaping[index].extend_from(&partial.module_escaping);
        }
        result.external_dependencies = result.escaping.clone();
        Ok(result)
    }
    pub fn function_edges(&self) -> impl Iterator<Item = (FunctionId, FunctionId)> + '_ {
        self.dependencies
            .iter()
            .enumerate()
            .flat_map(|(i, s)| s.local.iter().map(move |id| (FunctionId(i), *id)))
    }
}
impl Analysis<'_> {
    fn error(&self, site: Site, message: impl AsRef<str>) -> String {
        format!("{:?}: {}", site.reference(), message.as_ref())
    }
    fn binding(&self, site: Site) -> Result<BindingId> {
        self.program
            .binder_nodes
            .get(&site.reference())
            .or_else(|| self.program.variable_uses.get(&site.reference()))
            .copied()
            .ok_or_else(|| self.error(site, "dependency variable has no lexical binding"))
    }
    fn atom(&self, module: ModuleId, node: NodeId) -> Option<String> {
        let m = &self.core.modules[module.0];
        match &m.nodes[node.0].expr {
            CoreExpr::Literal(t) => match &m.terms[t.0] {
                ErlangTerm::Atom(a) => Some(a.clone()),
                _ => None,
            },
            _ => None,
        }
    }
    fn dependencies(&mut self, module: ModuleId, current: Option<FunctionId>, targets: TargetSet) {
        if targets.is_empty() {
            return;
        }
        // `module_callers` keeps first-seen order; the set answers membership.
        debug_assert_eq!(module, self.active_module);
        if self.caller_seen.insert(current) {
            self.result.module_callers.push(current);
        }
        if current.is_none() {
            self.result.module_roots.extend_from(&targets);
        }
        let output = match current {
            Some(id) => self.result.dependencies.entry(id).or_default(),
            None => &mut self.result.root_dependencies,
        };
        output.extend_from(&targets);
    }
    fn escape(&mut self, value: TargetValueId) {
        let targets = self.result.outputs.flatten(value);
        self.result.escaping.local.extend(targets.local.iter().copied());
        self.result.escaping.external |= targets.external;
        self.result.module_escaping.local.extend(targets.local.iter().copied());
        self.result.module_escaping.external |= targets.external;
    }
    fn bind(
        &self,
        site: Site,
        nodes: &[NodeId],
        value: TargetValueId,
        env: &mut Env,
    ) -> Result<()> {
        match &self.result.outputs.values[value.0] {
            TargetValue::Single(_) => {
                for n in nodes {
                    env.insert(self.binding(Site { node: *n, ..site })?, value);
                }
            }
            TargetValue::Values(xs) => {
                if nodes.len() != xs.len() {
                    return Err(self.error(site, "dependency binding multi-value arity mismatch"));
                }
                for (n, v) in nodes.iter().zip(xs) {
                    env.insert(self.binding(Site { node: *n, ..site })?, *v);
                }
            }
        }
        Ok(())
    }
    fn bind_patterns(
        &mut self,
        site: Site,
        nodes: &[NodeId],
        value: TargetValueId,
        env: &mut Env,
    ) -> Result<()> {
        if self.result.outputs.is_none(value) {
            return Ok(());
        }
        let values = match &self.result.outputs.values[value.0] {
            TargetValue::Single(_) => {
                if nodes.len() != 1 {
                    return Err(self.error(site, "dependency single pattern arity mismatch"));
                }
                vec![value]
            }
            TargetValue::Values(xs) => {
                if nodes.len() != xs.len() {
                    return Err(self.error(site, "dependency pattern multi-value arity mismatch"));
                }
                xs.clone()
            }
        };
        let m = &self.core.modules[site.module.0];
        for (node, value) in nodes.iter().zip(values) {
            let targets = self.result.outputs.flatten(value);
            let flattened = self.result.outputs.single(targets);
            let mut tasks = vec![*node];
            while let Some(node) = tasks.pop() {
                match &m.nodes[node.0].expr {
                    CoreExpr::Var(_) => {
                        env.insert(self.binding(Site { node, ..site })?, flattened);
                    }
                    CoreExpr::Alias { variable, pattern } => tasks.extend([*variable, *pattern]),
                    CoreExpr::Cons { head, tail } => tasks.extend([*head, *tail]),
                    CoreExpr::Tuple(xs) | CoreExpr::Values(xs) | CoreExpr::Binary(xs) => {
                        tasks.extend(xs)
                    }
                    CoreExpr::Map { base, pairs, .. } => {
                        tasks.push(*base);
                        tasks.extend(pairs);
                    }
                    CoreExpr::MapPair {
                        operation,
                        key,
                        value,
                    } => tasks.extend([*operation, *key, *value]),
                    CoreExpr::Bitstr {
                        value,
                        size,
                        unit,
                        typ,
                        flags,
                    } => tasks.extend([*value, *size, *unit, *typ, *flags]),
                    CoreExpr::Literal(_) => {}
                    _ => {
                        return Err(self
                            .error(Site { node, ..site }, "unsupported dependency pattern kind"));
                    }
                }
            }
        }
        Ok(())
    }
    fn definitions(
        &mut self,
        site: Site,
        defs: &[(NodeId, NodeId)],
        env: &mut Env,
        returns: Option<usize>,
    ) -> Result<()> {
        for (name, node) in defs {
            let target = *self.program.function_nodes[site.module.0]
                .get(&node.0)
                .ok_or_else(|| self.error(site, "dependency definition has no indexed function"))?;
            let binding = self.binding(Site {
                node: *name,
                ..site
            })?;
            let value = self.result.outputs.single(TargetSet::singleton(target));
            env.insert(binding, value);
            if let Some(n) = returns {
                self.result.letrec_targets.insert(binding, target);
                self.result.letrec_return_arities.insert(binding, n);
            }
        }
        Ok(())
    }
    fn remote(&mut self, site: Site, value: TargetValueId) -> Result<TargetValueId> {
        let CoreExpr::Call { module, name, args } =
            &self.core.modules[site.module.0].nodes[site.node.0].expr
        else {
            return Err(self.error(site, "dependency remote node is not call"));
        };
        let target = match (
            self.atom(site.module, *module),
            self.atom(site.module, *name),
        ) {
            (Some(module), Some(name)) => FunctionName {
                module,
                name,
                arity: args.len(),
            },
            _ => {
                self.escape(value);
                return Ok(self.result.outputs.external());
            }
        };
        let literal = self.policy.literal_remote.contains(&target);
        if self.policy.pure.contains(&target) {
            if literal {
                Ok(self.result.outputs.none())
            } else {
                self.escape(value);
                Ok(self.result.outputs.external())
            }
        } else {
            if !self.policy.noescape_remote.contains(&target) {
                self.escape(value);
            }
            if literal {
                Ok(self.result.outputs.none())
            } else {
                let targets = self
                    .result
                    .outputs
                    .flatten(value)
                    .union(&TargetSet::external());
                Ok(self.result.outputs.single(targets))
            }
        }
    }
    fn primop(&mut self, site: Site, value: TargetValueId) -> Result<TargetValueId> {
        let CoreExpr::Primop { name, args } =
            &self.core.modules[site.module.0].nodes[site.node.0].expr
        else {
            return Err(self.error(site, "dependency primop node mismatch"));
        };
        let name = self
            .atom(site.module, *name)
            .ok_or_else(|| self.error(site, "dependency primitive name must be literal atom"))?;
        let op = (name, args.len());
        if !self.policy.noescape_primitive.contains(&op) {
            self.escape(value);
        }
        Ok(if self.policy.literal_primitive.contains(&op) {
            self.result.outputs.none()
        } else {
            value
        })
    }
    fn on_load(&mut self, module: ModuleId) -> Result<()> {
        let m = &self.core.modules[module.0];
        let CoreExpr::Module { attributes, .. } = &m.nodes[m.core.0].expr else {
            return Err("dependency root is not module".into());
        };
        for (name, value) in attributes {
            if self.atom(module, *name).as_deref() != Some("on_load") {
                continue;
            }
            let CoreExpr::Literal(t) = m.nodes[value.0].expr else {
                return Err("on_load attribute must be literal".into());
            };
            let mut tasks = vec![t];
            while let Some(t) = tasks.pop() {
                match &m.terms[t.0] {
                    ErlangTerm::Nil => {}
                    ErlangTerm::List { items, tail } => {
                        tasks.extend(items);
                        tasks.push(*tail);
                    }
                    ErlangTerm::Tuple(xs) if xs.len() == 2 => {
                        let (ErlangTerm::Atom(name), ErlangTerm::Integer(arity)) =
                            (&m.terms[xs[0].0], &m.terms[xs[1].0])
                        else {
                            return Err("on_load function tuple malformed".into());
                        };
                        let arity = arity.parse().map_err(|_| "on_load arity invalid")?;
                        if let Some(id) = self.program.named.get(&FunctionName {
                            module: m.name.clone(),
                            name: name.clone(),
                            arity,
                        }) {
                            self.result.escaping.local.insert(*id);
                            debug_assert_eq!(module, self.active_module);
                            self.result.module_escaping.local.insert(*id);
                        }
                    }
                    _ => return Err("on_load attribute malformed".into()),
                }
            }
        }
        Ok(())
    }
    fn walk(&mut self, root: Site) -> Result<TargetValueId> {
        let core = self.core;
        let mut frames = vec![Frame::Eval(root, Env::new(), None, 1)];
        let mut last = None;
        while let Some(frame) = frames.pop() {
            match frame {
                Frame::Eval(site, mut env, current, num_returns) => {
                    let m = &core.modules[site.module.0];
                    let many = |nodes: Vec<NodeId>, finish: Finish, returns| Frame::Many {
                        module: site.module,
                        nodes,
                        index: 0,
                        env: env.clone(),
                        current,
                        num_returns: returns,
                        values: Vec::new(),
                        finish,
                    };
                    match &m.nodes[site.node.0].expr {
                        CoreExpr::Literal(_) | CoreExpr::Binary(_) => {
                            last = Some(self.result.outputs.none())
                        }
                        CoreExpr::Var(_) => {
                            let value = env
                                .get(&self.binding(site)?)
                                .copied()
                                .unwrap_or_else(|| self.result.outputs.none());
                            let targets = self.result.outputs.flatten(value);
                            if !targets.local.is_empty() {
                                self.dependencies(site.module, current, targets);
                            }
                            last = Some(value);
                        }
                        CoreExpr::Fun { params, body } => {
                            let target = self.program.function_nodes[site.module.0][&site.node.0];
                            if let Some(parent) = current {
                                self.dependencies(
                                    site.module,
                                    Some(target),
                                    TargetSet::singleton(parent),
                                );
                            }
                            self.dependencies(site.module, current, TargetSet::singleton(target));
                            let value = self.result.outputs.external();
                            self.bind(site, params, value, &mut env)?;
                            frames.push(Frame::Fun(target));
                            frames.push(Frame::Eval(
                                Site {
                                    node: *body,
                                    ..site
                                },
                                env,
                                Some(target),
                                1,
                            ));
                        }
                        CoreExpr::Cons { head, tail } => {
                            frames.push(many(vec![*head, *tail], Finish::Merged, num_returns))
                        }
                        CoreExpr::Tuple(xs) => {
                            frames.push(many(xs.clone(), Finish::Merged, num_returns))
                        }
                        CoreExpr::Values(xs) => frames.push(many(xs.clone(), Finish::Values, 1)),
                        CoreExpr::Map { pairs, .. } => {
                            frames.push(many(pairs.clone(), Finish::Merged, num_returns))
                        }
                        CoreExpr::MapPair { key, value, .. } => {
                            frames.push(many(vec![*key, *value], Finish::Merged, num_returns))
                        }
                        CoreExpr::Apply { operator, args } => {
                            if !matches!(m.nodes[operator.0].expr, CoreExpr::Var(_)) {
                                last = Some(self.result.outputs.none());
                            } else {
                                frames.push(many(
                                    args.clone(),
                                    Finish::Apply {
                                        site,
                                        operator: *operator,
                                        args: args.len(),
                                    },
                                    num_returns,
                                ));
                            }
                        }
                        CoreExpr::Call { args, .. } => {
                            frames.push(many(args.clone(), Finish::Call(site), num_returns))
                        }
                        CoreExpr::Primop { args, .. } => {
                            frames.push(many(args.clone(), Finish::Primop(site), num_returns))
                        }
                        CoreExpr::Let {
                            vars,
                            argument,
                            body,
                        } => {
                            frames.push(Frame::Let {
                                site,
                                vars: vars.clone(),
                                body: *body,
                                env: env.clone(),
                                current,
                                num_returns,
                            });
                            frames.push(Frame::Eval(
                                Site {
                                    node: *argument,
                                    ..site
                                },
                                env,
                                current,
                                vars.len(),
                            ));
                        }
                        CoreExpr::Letrec { definitions, body } => {
                            self.definitions(site, definitions, &mut env, Some(num_returns))?;
                            frames.push(Frame::Definitions {
                                site,
                                body: Some(*body),
                                env: env.clone(),
                                current,
                                num_returns,
                            });
                            frames.push(Frame::Many {
                                module: site.module,
                                nodes: definitions.iter().map(|(_, f)| *f).collect(),
                                index: 0,
                                env,
                                current,
                                num_returns,
                                values: Vec::new(),
                                finish: Finish::Merged,
                            });
                        }
                        CoreExpr::Module { definitions, .. } => {
                            self.definitions(site, definitions, &mut env, None)?;
                            frames.push(Frame::Definitions {
                                site,
                                body: None,
                                env: env.clone(),
                                current,
                                num_returns,
                            });
                            frames.push(Frame::Many {
                                module: site.module,
                                nodes: definitions.iter().map(|(_, f)| *f).collect(),
                                index: 0,
                                env,
                                current,
                                num_returns: 1,
                                values: Vec::new(),
                                finish: Finish::Merged,
                            });
                        }
                        CoreExpr::Seq { argument, body } => {
                            frames.push(Frame::Seq {
                                site,
                                body: *body,
                                env: env.clone(),
                                current,
                                num_returns,
                            });
                            frames.push(Frame::Eval(
                                Site {
                                    node: *argument,
                                    ..site
                                },
                                env,
                                current,
                                1,
                            ));
                        }
                        CoreExpr::Case { argument, clauses } => {
                            frames.push(Frame::Case {
                                site,
                                clauses: clauses.clone(),
                                env: env.clone(),
                                current,
                                num_returns,
                            });
                            frames.push(Frame::Eval(
                                Site {
                                    node: *argument,
                                    ..site
                                },
                                env,
                                current,
                                num_returns,
                            ));
                        }
                        CoreExpr::Try {
                            argument,
                            vars,
                            body,
                            exception_vars,
                            handler,
                        } => {
                            frames.push(Frame::Try {
                                site,
                                vars: vars.clone(),
                                body: *body,
                                exception_vars: exception_vars.clone(),
                                handler: *handler,
                                env: env.clone(),
                                current,
                                num_returns,
                            });
                            frames.push(Frame::Eval(
                                Site {
                                    node: *argument,
                                    ..site
                                },
                                env,
                                current,
                                num_returns,
                            ));
                        }
                        CoreExpr::Catch { body } => frames.push(Frame::Eval(
                            Site {
                                node: *body,
                                ..site
                            },
                            env,
                            current,
                            num_returns,
                        )),
                        CoreExpr::Receive {
                            clauses,
                            timeout,
                            action,
                        } => {
                            let none = self.result.outputs.none();
                            frames.push(Frame::Receive {
                                site,
                                timeout: *timeout,
                                action: *action,
                                env: env.clone(),
                                current,
                                num_returns,
                            });
                            frames.push(Frame::Clauses {
                                site,
                                clauses: clauses.clone(),
                                index: 0,
                                argument: none,
                                env,
                                current,
                                num_returns,
                                values: Vec::new(),
                            });
                        }
                        other => {
                            return Err(self.error(
                                site,
                                format!(
                                    "unsupported native dependency expression {}",
                                    other.kind()
                                ),
                            ));
                        }
                    }
                }
                Frame::Many {
                    module,
                    nodes,
                    index,
                    env,
                    current,
                    num_returns,
                    mut values,
                    finish,
                } => {
                    if index > 0 {
                        values.push(last.take().ok_or("dependency child produced no output")?);
                    }
                    if index < nodes.len() {
                        let node = nodes[index];
                        frames.push(Frame::Many {
                            module,
                            nodes,
                            index: index + 1,
                            env: env.clone(),
                            current,
                            num_returns,
                            values,
                            finish,
                        });
                        frames.push(Frame::Eval(
                            Site { module, node },
                            env,
                            current,
                            num_returns,
                        ));
                    } else {
                        last = Some(match finish {
                            Finish::Values => {
                                self.result.outputs.intern(TargetValue::Values(values))
                            }
                            Finish::Merged => self.result.outputs.merge_many(&values)?,
                            Finish::Call(site) => {
                                let value = self.result.outputs.merge_many(&values)?;
                                self.remote(site, value)?
                            }
                            Finish::Primop(site) => {
                                let value = self.result.outputs.merge_many(&values)?;
                                self.primop(site, value)?
                            }
                            Finish::Apply {
                                site,
                                operator,
                                args,
                            } => {
                                let value = self.result.outputs.merge_many(&values)?;
                                self.escape(value);
                                let binding = self.binding(Site {
                                    node: operator,
                                    ..site
                                })?;
                                let value = env
                                    .get(&binding)
                                    .copied()
                                    .unwrap_or_else(|| self.result.outputs.none());
                                let targets = self.result.outputs.flatten(value);
                                self.dependencies(site.module, current, targets.clone());
                                let targets = TargetSet {
                                    local: targets
                                        .local
                                        .into_iter()
                                        .filter(|id| self.program.functions[id.0].arity == args)
                                        .collect(),
                                    external: targets.external,
                                };
                                if !targets.is_empty() {
                                    self.result.call_targets.insert(site.reference(), targets);
                                }
                                let n = self
                                    .result
                                    .letrec_return_arities
                                    .get(&binding)
                                    .copied()
                                    .unwrap_or(1);
                                if n == 1 {
                                    self.result.outputs.external()
                                } else {
                                    let value = self.result.outputs.external();
                                    self.result
                                        .outputs
                                        .intern(TargetValue::Values(vec![value; n]))
                                }
                            }
                        });
                    }
                }
                Frame::Fun(target) => {
                    let value = last
                        .take()
                        .ok_or("dependency function body missing output")?;
                    self.escape(value);
                    last = Some(self.result.outputs.single(TargetSet::singleton(target)));
                }
                Frame::Let {
                    site,
                    vars,
                    body,
                    mut env,
                    current,
                    num_returns,
                } => {
                    let value = last
                        .take()
                        .ok_or("dependency let argument missing output")?;
                    self.bind(site, &vars, value, &mut env)?;
                    frames.push(Frame::Eval(
                        Site { node: body, ..site },
                        env,
                        current,
                        num_returns,
                    ));
                }
                Frame::Seq {
                    site,
                    body,
                    env,
                    current,
                    num_returns,
                } => frames.push(Frame::Eval(
                    Site { node: body, ..site },
                    env,
                    current,
                    num_returns,
                )),
                Frame::Definitions {
                    site,
                    body,
                    env,
                    current,
                    num_returns,
                } => match body {
                    Some(node) => frames.push(Frame::Eval(
                        Site { node, ..site },
                        env,
                        current,
                        num_returns,
                    )),
                    None => last = Some(self.result.outputs.none()),
                },
                Frame::Case {
                    site,
                    clauses,
                    env,
                    current,
                    num_returns,
                } => {
                    let argument = last
                        .take()
                        .ok_or("dependency case argument missing output")?;
                    frames.push(Frame::Clauses {
                        site,
                        clauses,
                        index: 0,
                        argument,
                        env,
                        current,
                        num_returns,
                        values: Vec::new(),
                    });
                }
                Frame::Clauses {
                    site,
                    clauses,
                    index,
                    argument,
                    env,
                    current,
                    num_returns,
                    mut values,
                } => {
                    if index > 0 {
                        values.push(last.take().ok_or("dependency clause missing output")?);
                    }
                    if index == clauses.len() {
                        last = Some(self.result.outputs.merge_many(&values)?);
                    } else {
                        let node = clauses[index];
                        let CoreExpr::Clause {
                            patterns,
                            guard,
                            body,
                        } = &core.modules[site.module.0].nodes[node.0].expr
                        else {
                            return Err(self.error(site, "dependency case member is not clause"));
                        };
                        let mut branch = env.clone();
                        self.bind_patterns(site, patterns, argument, &mut branch)?;
                        frames.push(Frame::Clauses {
                            site,
                            clauses,
                            index: index + 1,
                            argument,
                            env,
                            current,
                            num_returns,
                            values,
                        });
                        frames.push(Frame::Clause {
                            site,
                            body: *body,
                            env: branch.clone(),
                            current,
                            num_returns,
                        });
                        frames.push(Frame::Eval(
                            Site {
                                node: *guard,
                                ..site
                            },
                            branch,
                            current,
                            num_returns,
                        ));
                    }
                }
                Frame::Clause {
                    site,
                    body,
                    env,
                    current,
                    num_returns,
                } => frames.push(Frame::Eval(
                    Site { node: body, ..site },
                    env,
                    current,
                    num_returns,
                )),
                Frame::Try {
                    site,
                    vars,
                    body,
                    exception_vars,
                    handler,
                    mut env,
                    current,
                    num_returns,
                } => {
                    let value = last
                        .take()
                        .ok_or("dependency try argument missing output")?;
                    let original = env.clone();
                    self.bind(site, &vars, value, &mut env)?;
                    frames.push(Frame::Handler {
                        site,
                        exception_vars,
                        handler,
                        env: original,
                        current,
                        num_returns,
                    });
                    frames.push(Frame::Eval(
                        Site { node: body, ..site },
                        env,
                        current,
                        num_returns,
                    ));
                }
                Frame::Handler {
                    site,
                    exception_vars,
                    handler,
                    mut env,
                    current,
                    num_returns,
                } => {
                    let value = last.take().ok_or("dependency try body missing output")?;
                    let external = self.result.outputs.external();
                    self.bind(site, &exception_vars, external, &mut env)?;
                    frames.push(Frame::TryJoin(value));
                    frames.push(Frame::Eval(
                        Site {
                            node: handler,
                            ..site
                        },
                        env,
                        current,
                        num_returns,
                    ));
                }
                Frame::TryJoin(first) => {
                    let second = last.take().ok_or("dependency try handler missing output")?;
                    last = Some(self.result.outputs.merge(first, second)?);
                }
                Frame::Receive {
                    site,
                    timeout,
                    action,
                    env,
                    current,
                    num_returns,
                } => {
                    let clauses = last
                        .take()
                        .ok_or("dependency receive clauses missing output")?;
                    frames.push(Frame::ReceiveTimeout {
                        site,
                        action,
                        env: env.clone(),
                        current,
                        num_returns,
                        clauses,
                    });
                    frames.push(Frame::Eval(
                        Site {
                            node: timeout,
                            ..site
                        },
                        env,
                        current,
                        num_returns,
                    ));
                }
                Frame::ReceiveTimeout {
                    site,
                    action,
                    env,
                    current,
                    num_returns,
                    clauses,
                } => {
                    frames.push(Frame::TryJoin(clauses));
                    frames.push(Frame::Eval(
                        Site {
                            node: action,
                            ..site
                        },
                        env,
                        current,
                        num_returns,
                    ));
                }
            }
        }
        last.ok_or_else(|| self.error(root, "dependency traversal has no output"))
    }
}

#[cfg(test)]
#[path = "closure_dependency_tests.rs"]
mod tests;

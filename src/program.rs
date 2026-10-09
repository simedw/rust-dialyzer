use crate::core::{CoreExpr, CoreModule, CoreProgram, ErlangTerm, NodeId, TermId};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[path = "program_metadata.rs"]
mod metadata;
pub(crate) use metadata::characters_from_terms;
use metadata::{function_variable, literal_atom, source_origin, variable};

type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ModuleId(pub usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct FunctionId(pub usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct BindingId(pub usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ScopeId(pub usize);
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct FunctionName {
    pub module: String,
    pub name: String,
    pub arity: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct TypeName {
    pub module: String,
    pub name: String,
    pub arity: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VariableName {
    Atom(String),
    Integer(String),
    Function(String, usize),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NodeRef {
    pub module: ModuleId,
    pub node: usize,
}
#[derive(Clone, Debug)]
pub struct SourceOrigin {
    pub raw: TermId,
    pub file: Option<String>,
    pub line: Option<i64>,
    pub column: Option<u64>,
    pub generated: bool,
}
#[derive(Clone, Debug)]
pub enum FunctionIdentity {
    Named(FunctionName),
    Local { name: String, arity: usize },
    Closure,
}
#[derive(Clone, Debug)]
pub struct Function {
    pub id: FunctionId,
    pub module: ModuleId,
    pub node: NodeId,
    pub identity: FunctionIdentity,
    pub parent: Option<FunctionId>,
    pub scope: Option<ScopeId>,
    pub exported: bool,
    pub arity: usize,
    pub captures: BTreeSet<BindingId>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingKind {
    Function,
    Parameter,
    Let,
    Pattern,
    TryResult,
    Exception,
}
#[derive(Clone, Debug)]
pub struct Binding {
    pub id: BindingId,
    pub module: ModuleId,
    pub node: NodeId,
    pub name: VariableName,
    pub scope: ScopeId,
    pub owner: Option<FunctionId>,
    pub kind: BindingKind,
    pub declared_function: Option<FunctionId>,
}
#[derive(Clone, Debug)]
pub struct Scope {
    pub id: ScopeId,
    pub parent: Option<ScopeId>,
    pub owner: Option<FunctionId>,
    pub bindings: BTreeMap<VariableName, BindingId>,
}
#[derive(Clone, Debug)]
pub enum CallTarget {
    Named(FunctionName),
    Apply,
    DynamicRemote,
}
#[derive(Clone, Debug)]
pub struct CallEdge {
    pub caller: FunctionId,
    pub module: ModuleId,
    pub node: NodeId,
    pub target: CallTarget,
}
#[derive(Clone, Debug)]
pub enum MetadataKind {
    Type,
    Opaque,
    Nominal,
    Spec,
    Callback,
    Record,
    Behaviour,
    ExportType,
    OptionalCallbacks,
    Suppression,
    Other(String),
}
#[derive(Clone, Debug)]
pub struct Metadata {
    pub module: ModuleId,
    pub kind: MetadataKind,
    pub raw: TermId,
    pub origin: SourceOrigin,
    pub types: Vec<TypeName>,
    pub functions: Vec<FunctionName>,
    pub behaviour: Option<crate::behaviour_metadata::BehaviourAttribute>,
    pub records: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TypeDependency {
    pub module: ModuleId,
    pub metadata: usize,
    pub target: TypeName,
}
#[derive(Clone, Debug)]
pub struct Module {
    pub id: ModuleId,
    pub name: String,
    pub scope: ScopeId,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TargetContracts {
    #[default]
    Use,
    Ignore,
}

#[derive(Clone, Debug)]
pub struct Program {
    target_contracts: TargetContracts,
    pub modules: Vec<Module>,
    pub functions: Vec<Function>,
    pub bindings: Vec<Binding>,
    pub scopes: Vec<Scope>,
    pub calls: Vec<CallEdge>,
    pub metadata: Vec<Metadata>,
    pub type_dependencies: BTreeSet<TypeDependency>,
    pub origins: Vec<Vec<SourceOrigin>>,
    pub variable_uses: BTreeMap<NodeRef, BindingId>,
    pub binder_nodes: BTreeMap<NodeRef, BindingId>,
    pub named: BTreeMap<FunctionName, FunctionId>,
    pub function_nodes: Vec<BTreeMap<usize, FunctionId>>,
}
#[derive(Serialize)]
pub struct Inspection {
    pub modules: usize,
    pub functions: usize,
    pub named_functions: usize,
    pub local_functions: usize,
    pub closures: usize,
    pub bindings: usize,
    pub variable_uses: usize,
    pub scopes: usize,
    pub captures: usize,
    pub static_calls: usize,
    pub local_calls: usize,
    pub dynamic_calls: usize,
    pub metadata: usize,
    pub type_dependencies: usize,
    pub located_nodes: usize,
    pub generated_nodes: usize,
}
#[derive(Clone, Copy)]
struct Task {
    node: NodeId,
    scope: ScopeId,
    owner: Option<FunctionId>,
}
impl Program {
    pub fn build(core: &CoreProgram) -> Result<Self> {
        Self::build_with_contracts(core, TargetContracts::Use)
    }
    pub fn target_contracts(&self) -> TargetContracts {
        self.target_contracts
    }
    pub fn build_with_contracts(
        core: &CoreProgram,
        target_contracts: TargetContracts,
    ) -> Result<Self> {
        let mut index = Self {
            target_contracts,
            modules: Vec::new(),
            functions: Vec::new(),
            bindings: Vec::new(),
            scopes: Vec::new(),
            calls: Vec::new(),
            metadata: Vec::new(),
            type_dependencies: BTreeSet::new(),
            origins: Vec::new(),
            variable_uses: BTreeMap::new(),
            binder_nodes: BTreeMap::new(),
            named: BTreeMap::new(),
            function_nodes: Vec::new(),
        };
        let mut origins = crate::ordered_parallel::setup_map(&core.modules, |module| {
            module
                .nodes
                .iter()
                .map(|node| source_origin(module, node.annotations))
                .collect::<Result<Vec<_>>>()
        })
        .into_iter();
        for (i, module) in core.modules.iter().enumerate() {
            let mid = ModuleId(i);
            let mut identities = BTreeMap::new();
            let CoreExpr::Module {
                definitions,
                exports,
                ..
            } = &module.nodes[module.core.0].expr
            else {
                unreachable!()
            };
            let exported = exports
                .iter()
                .map(|v| function_variable(module, *v))
                .collect::<Result<BTreeSet<_>>>()?;
            for (key, node) in definitions {
                let (name, arity) = function_variable(module, *key)?;
                identities.insert(
                    node.0,
                    (
                        FunctionIdentity::Named(FunctionName {
                            module: module.name.clone(),
                            name: name.clone(),
                            arity,
                        }),
                        exported.contains(&(name, arity)),
                    ),
                );
            }
            for node in &module.nodes {
                if let CoreExpr::Letrec { definitions, .. } = &node.expr {
                    for (key, fun) in definitions {
                        let (name, arity) = function_variable(module, *key)?;
                        identities.insert(fun.0, (FunctionIdentity::Local { name, arity }, false));
                    }
                }
            }
            let mut function_nodes = BTreeMap::new();
            for node in &module.nodes {
                if let CoreExpr::Fun { params, .. } = &node.expr {
                    let id = FunctionId(index.functions.len());
                    let (identity, exported) = identities
                        .remove(&node.id.0)
                        .unwrap_or((FunctionIdentity::Closure, false));
                    if let FunctionIdentity::Named(name) = &identity
                        && index.named.insert(name.clone(), id).is_some()
                    {
                        return Err(format!(
                            "module {}: duplicate named function {name:?}",
                            module.name
                        ));
                    }
                    function_nodes.insert(node.id.0, id);
                    index.functions.push(Function {
                        id,
                        module: mid,
                        node: node.id,
                        identity,
                        parent: None,
                        scope: None,
                        exported,
                        arity: params.len(),
                        captures: BTreeSet::new(),
                    });
                }
            }
            index.function_nodes.push(function_nodes);
            index
                .origins
                .push(origins.next().expect("origins for every module")?);
            index.index_metadata(mid, module)?;
        }
        for (i, module) in core.modules.iter().enumerate() {
            index.walk_module(ModuleId(i), module)?;
        }
        Ok(index)
    }
    pub fn inspect(&self) -> Inspection {
        Inspection {
            modules: self.origins.len(),
            functions: self.functions.len(),
            named_functions: self.named.len(),
            local_functions: self
                .functions
                .iter()
                .filter(|f| matches!(f.identity, FunctionIdentity::Local { .. }))
                .count(),
            closures: self
                .functions
                .iter()
                .filter(|f| matches!(f.identity, FunctionIdentity::Closure))
                .count(),
            bindings: self.bindings.len(),
            variable_uses: self.variable_uses.len(),
            scopes: self.scopes.len(),
            captures: self.functions.iter().map(|f| f.captures.len()).sum(),
            static_calls: self
                .calls
                .iter()
                .filter(|e| matches!(e.target, CallTarget::Named(_)))
                .count(),
            local_calls: self
                .calls
                .iter()
                .filter(|e| matches!(e.target, CallTarget::Apply))
                .count(),
            dynamic_calls: self
                .calls
                .iter()
                .filter(|e| matches!(e.target, CallTarget::DynamicRemote))
                .count(),
            metadata: self.metadata.len(),
            type_dependencies: self.type_dependencies.len(),
            located_nodes: self
                .origins
                .iter()
                .flatten()
                .filter(|o| o.line.is_some())
                .count(),
            generated_nodes: self
                .origins
                .iter()
                .flatten()
                .filter(|o| o.generated)
                .count(),
        }
    }
    fn scope(&mut self, parent: Option<ScopeId>, owner: Option<FunctionId>) -> ScopeId {
        let id = ScopeId(self.scopes.len());
        self.scopes.push(Scope {
            id,
            parent,
            owner,
            bindings: BTreeMap::new(),
        });
        id
    }
    fn snapshot_scope(&mut self, scope: ScopeId) -> ScopeId {
        let bindings = self.scopes[scope.0].bindings.clone();
        let snapshot = self.scope(self.scopes[scope.0].parent, self.scopes[scope.0].owner);
        self.scopes[snapshot.0].bindings = bindings;
        snapshot
    }
    fn lookup(&self, mut scope: ScopeId, name: &VariableName) -> Option<BindingId> {
        loop {
            let s = &self.scopes[scope.0];
            if let Some(id) = s.bindings.get(name) {
                return Some(*id);
            }
            scope = s.parent?;
        }
    }
    fn bind(
        &mut self,
        mid: ModuleId,
        module: &CoreModule,
        node: NodeId,
        scope: ScopeId,
        kind: BindingKind,
        target: Option<FunctionId>,
    ) -> Result<BindingId> {
        let repeat = kind == BindingKind::Pattern;
        let name = variable(module, node)?;
        if let Some(old) = self.scopes[scope.0].bindings.get(&name) {
            if repeat {
                self.binder_nodes.insert(
                    NodeRef {
                        module: mid,
                        node: node.0,
                    },
                    *old,
                );
                return Ok(*old);
            }
            return Err(format!(
                "module {} node {}: duplicate binder {name:?} in scope {}",
                module.name, node.0, scope.0
            ));
        }
        let id = BindingId(self.bindings.len());
        let owner = self.scopes[scope.0].owner;
        self.bindings.push(Binding {
            id,
            module: mid,
            node,
            name: name.clone(),
            scope,
            owner,
            kind,
            declared_function: target,
        });
        self.binder_nodes.insert(
            NodeRef {
                module: mid,
                node: node.0,
            },
            id,
        );
        self.scopes[scope.0].bindings.insert(name, id);
        Ok(id)
    }
    fn use_variable(
        &mut self,
        mid: ModuleId,
        module: &CoreModule,
        task: Task,
    ) -> Result<BindingId> {
        let name = variable(module, task.node)?;
        let binding = self.lookup(task.scope, &name).ok_or_else(|| {
            format!(
                "module {} node {}: unbound variable {name:?} in scope {}",
                module.name, task.node.0, task.scope.0
            )
        })?;
        let at = NodeRef {
            module: mid,
            node: task.node.0,
        };
        if let Some(old) = self.variable_uses.insert(at, binding)
            && old != binding
        {
            return Err(format!(
                "module {} node {}: DAG variable reused across distinct lexical bindings {} and {}",
                module.name, task.node.0, old.0, binding.0
            ));
        }
        if let Some(owner) = task.owner
            && self.bindings[binding.0].owner.is_some()
            && self.bindings[binding.0].owner != Some(owner)
        {
            self.functions[owner.0].captures.insert(binding);
        }
        Ok(binding)
    }
    fn walk_module(&mut self, mid: ModuleId, module: &CoreModule) -> Result<()> {
        let CoreExpr::Module { definitions, .. } = &module.nodes[module.core.0].expr else {
            unreachable!()
        };
        let global = self.scope(None, None);
        self.modules.push(Module {
            id: mid,
            name: module.name.clone(),
            scope: global,
        });
        for (key, node) in definitions {
            self.bind(
                mid,
                module,
                *key,
                global,
                BindingKind::Function,
                Some(self.function_nodes[mid.0][&node.0]),
            )?;
        }
        let mut tasks: Vec<_> = definitions
            .iter()
            .rev()
            .map(|(_, node)| Task {
                node: *node,
                scope: global,
                owner: None,
            })
            .collect();
        while let Some(task) = tasks.pop() {
            let same = |node| Task { node, ..task };
            match &module.nodes[task.node.0].expr {
                CoreExpr::Literal(_) | CoreExpr::Opaque(_) => {}
                CoreExpr::Var(_) => {
                    self.use_variable(mid, module, task)?;
                }
                CoreExpr::Fun { params, body } => {
                    let owner = self.function_nodes[mid.0][&task.node.0];
                    if let Some(old) = self.functions[owner.0].scope {
                        if self.scopes[old.0].parent != Some(task.scope)
                            || self.functions[owner.0].parent != task.owner
                        {
                            return Err(format!(
                                "module {} node{}: DAG function reused across lexical declaration scopes",
                                module.name, task.node.0
                            ));
                        }
                        continue;
                    }
                    self.functions[owner.0].parent = task.owner;
                    let scope = self.scope(Some(task.scope), Some(owner));
                    self.functions[owner.0].scope = Some(scope);
                    for var in params {
                        self.bind(mid, module, *var, scope, BindingKind::Parameter, None)?;
                    }
                    tasks.push(Task {
                        node: *body,
                        scope,
                        owner: Some(owner),
                    });
                }
                CoreExpr::Let {
                    vars,
                    argument,
                    body,
                } => {
                    let scope = self.scope(Some(task.scope), task.owner);
                    for var in vars {
                        self.bind(mid, module, *var, scope, BindingKind::Let, None)?;
                    }
                    tasks.push(Task {
                        node: *body,
                        scope,
                        owner: task.owner,
                    });
                    tasks.push(same(*argument));
                }
                CoreExpr::Letrec { definitions, body } => {
                    let scope = self.scope(Some(task.scope), task.owner);
                    for (var, fun) in definitions {
                        self.bind(
                            mid,
                            module,
                            *var,
                            scope,
                            BindingKind::Function,
                            Some(self.function_nodes[mid.0][&fun.0]),
                        )?;
                    }
                    tasks.push(Task {
                        node: *body,
                        scope,
                        owner: task.owner,
                    });
                    tasks.extend(definitions.iter().rev().map(|(_, node)| Task {
                        node: *node,
                        scope,
                        owner: task.owner,
                    }));
                }
                CoreExpr::Clause {
                    patterns,
                    guard,
                    body,
                } => {
                    let scope = self.scope(Some(task.scope), task.owner);
                    let uses = self.bind_patterns(mid, module, patterns, scope, task.owner)?;
                    tasks.push(Task {
                        node: *body,
                        scope,
                        owner: task.owner,
                    });
                    tasks.push(Task {
                        node: *guard,
                        scope,
                        owner: task.owner,
                    });
                    tasks.extend(uses.into_iter().rev());
                }
                CoreExpr::Try {
                    argument,
                    vars,
                    body,
                    exception_vars,
                    handler,
                } => {
                    let success = self.scope(Some(task.scope), task.owner);
                    for var in vars {
                        self.bind(mid, module, *var, success, BindingKind::TryResult, None)?;
                    }
                    let failure = self.scope(Some(task.scope), task.owner);
                    for var in exception_vars {
                        self.bind(mid, module, *var, failure, BindingKind::Exception, None)?;
                    }
                    tasks.push(Task {
                        node: *handler,
                        scope: failure,
                        owner: task.owner,
                    });
                    tasks.push(Task {
                        node: *body,
                        scope: success,
                        owner: task.owner,
                    });
                    tasks.push(same(*argument));
                }
                CoreExpr::Call {
                    module: called_module,
                    name,
                    args,
                } => {
                    let target = match (
                        literal_atom(module, *called_module),
                        literal_atom(module, *name),
                    ) {
                        (Some(m), Some(f)) => CallTarget::Named(FunctionName {
                            module: m.into(),
                            name: f.into(),
                            arity: args.len(),
                        }),
                        _ => CallTarget::DynamicRemote,
                    };
                    let caller = task.owner.ok_or_else(|| {
                        format!(
                            "module {} node {}: call outside function",
                            module.name, task.node.0
                        )
                    })?;
                    self.calls.push(CallEdge {
                        caller,
                        module: mid,
                        node: task.node,
                        target,
                    });
                    tasks.extend(args.iter().rev().map(|n| same(*n)));
                    tasks.push(same(*name));
                    tasks.push(same(*called_module));
                }
                CoreExpr::Apply { operator, args } => {
                    let target = CallTarget::Apply;
                    let caller = task.owner.ok_or_else(|| {
                        format!(
                            "module {} node {}: apply outside function",
                            module.name, task.node.0
                        )
                    })?;
                    self.calls.push(CallEdge {
                        caller,
                        module: mid,
                        node: task.node,
                        target,
                    });
                    tasks.extend(args.iter().rev().map(|n| same(*n)));
                    tasks.push(same(*operator));
                }
                other => tasks.extend(other.children().into_iter().rev().map(same)),
            }
        }
        Ok(())
    }
    fn bind_patterns(
        &mut self,
        mid: ModuleId,
        module: &CoreModule,
        patterns: &[NodeId],
        scope: ScopeId,
        owner: Option<FunctionId>,
    ) -> Result<Vec<Task>> {
        let mut stack: Vec<_> = patterns.iter().rev().copied().collect();
        let mut uses = Vec::new();
        while let Some(node) = stack.pop() {
            match &module.nodes[node.0].expr {
                CoreExpr::Var(_) => {
                    self.bind(mid, module, node, scope, BindingKind::Pattern, None)?;
                }
                CoreExpr::Literal(_) => {}
                CoreExpr::Alias { variable, pattern } => {
                    self.bind(mid, module, *variable, scope, BindingKind::Pattern, None)?;
                    stack.push(*pattern);
                }
                CoreExpr::Bitstr {
                    value,
                    size,
                    unit,
                    typ,
                    flags,
                } => {
                    let snapshot = self.snapshot_scope(scope);
                    for id in [*size, *unit, *typ, *flags] {
                        uses.push(Task {
                            node: id,
                            scope: snapshot,
                            owner,
                        });
                    }
                    stack.push(*value);
                }
                CoreExpr::Map { base, pairs, .. } => {
                    let snapshot = self.snapshot_scope(scope);
                    uses.push(Task {
                        node: *base,
                        scope: snapshot,
                        owner,
                    });
                    stack.extend(pairs.iter().rev().copied());
                }
                CoreExpr::MapPair {
                    operation,
                    key,
                    value,
                } => {
                    let snapshot = self.snapshot_scope(scope);
                    uses.push(Task {
                        node: *operation,
                        scope: snapshot,
                        owner,
                    });
                    uses.push(Task {
                        node: *key,
                        scope: snapshot,
                        owner,
                    });
                    stack.push(*value);
                }
                CoreExpr::Binary(children)
                | CoreExpr::Tuple(children)
                | CoreExpr::Values(children) => stack.extend(children.iter().rev().copied()),
                CoreExpr::Cons { head, tail } => {
                    stack.push(*tail);
                    stack.push(*head);
                }
                other => {
                    return Err(format!(
                        "module {} node {}: unsupported Core pattern {}",
                        module.name,
                        node.0,
                        other.kind()
                    ));
                }
            }
        }
        Ok(uses)
    }
}

#[cfg(test)]
#[path = "program_tests.rs"]
mod tests;

impl Program {
    /// Compiler-owned export declarations, including exports without a function definition.
    pub fn compiler_exports(core: &CoreProgram) -> Result<BTreeSet<FunctionName>> {
        let mut exports = BTreeSet::new();
        for module in &core.modules {
            let CoreExpr::Module { exports: nodes, .. } = &module.nodes[module.core.0].expr else {
                return Err("Core module root is not a module".into());
            };
            for node in nodes {
                let (name, arity) = function_variable(module, *node)?;
                exports.insert(FunctionName {
                    module: module.name.clone(),
                    name,
                    arity,
                });
            }
        }
        Ok(exports)
    }
}

use crate::{
    bifs::registry::{Mfa, Registry},
    closure_dependencies::ClosureDependencies,
    core::{CoreExpr, CoreProgram, ErlangTerm, NodeId},
    dataflow_nodes::DataflowNodes,
    labels::{CoreLabel, Labels},
    native_dict::{NativeDictOrder, PinnedPhashKey, pinned_hash_atom, pinned_hash_integer},
    program::{FunctionId, FunctionIdentity, ModuleId, NodeRef, Program},
};
use num_bigint::BigInt;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
pub enum CallGraphError {
    Boundary(String),
    Native(crate::native_error::NativeTransferError),
    RuntimeOrdering(crate::native_runtime_hash::RuntimeHashError),
}
impl From<String> for CallGraphError {
    fn from(value: String) -> Self {
        Self::Boundary(value)
    }
}
impl From<&str> for CallGraphError {
    fn from(value: &str) -> Self {
        Self::Boundary(value.into())
    }
}
impl std::fmt::Display for CallGraphError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for CallGraphError {}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GraphVertex {
    Label(CoreLabel),
    Top,
    Named {
        module: String,
        function: String,
        arity: BigInt,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum DependencyCaller {
    Label(CoreLabel),
    Top,
    External,
}
impl PinnedPhashKey for DependencyCaller {
    fn pinned_phash_hash(&self) -> u32 {
        match self {
            Self::Label(label) => pinned_hash_integer(label.0),
            Self::Top => pinned_hash_atom("top"),
            Self::External => pinned_hash_atom("external"),
        }
    }
}
#[derive(Clone, Debug)]
pub struct ModuleEdges {
    pub confirmed: Vec<GraphVertex>,
    pub edges: Vec<(GraphVertex, GraphVertex)>,
    pub escaping: BTreeSet<CoreLabel>,
    pub self_recursive: BTreeSet<GraphVertex>,
}
#[derive(Clone, Debug)]
pub struct SourceCallGraph {
    pub modules: Vec<ModuleEdges>,
    pub named_labels: BTreeMap<GraphVertex, CoreLabel>,
    /// Reverse of `named_labels`, built on first use by `function_name`.
    label_names: std::sync::OnceLock<std::collections::HashMap<CoreLabel, Mfa>>,
    /// Function ids per module, in `program.functions` order.
    module_functions: Vec<Vec<FunctionId>>,
    graph: crate::source_digraph::SourceDigraph<GraphVertex>,
    pub module_names: Vec<String>,
}
fn site(module: ModuleId, node: NodeId) -> NodeRef {
    NodeRef {
        module,
        node: node.0,
    }
}
fn atom(core: &CoreProgram, module: ModuleId, node: NodeId) -> Option<&str> {
    let m = &core.modules[module.0];
    match &m.nodes[node.0].expr {
        CoreExpr::Literal(id) => match &m.terms[id.0] {
            ErlangTerm::Atom(a) => Some(a),
            _ => None,
        },
        _ => None,
    }
}
fn integer(core: &CoreProgram, module: ModuleId, node: NodeId) -> Option<BigInt> {
    let m = &core.modules[module.0];
    match &m.nodes[node.0].expr {
        CoreExpr::Literal(id) => match &m.terms[id.0] {
            ErlangTerm::Integer(a) => a.parse().ok(),
            _ => None,
        },
        _ => None,
    }
}
fn known(
    registry: &Registry,
    module: &str,
    function: &str,
    arity: &BigInt,
) -> Result<bool, CallGraphError> {
    if arity < &BigInt::from(0) || arity > &BigInt::from(255) {
        return Err(CallGraphError::Native(
            crate::native_error::NativeTransferError::FunctionClause,
        ));
    }
    let arity = arity.to_string().parse::<usize>().expect("bounded arity");
    Ok(registry.is_known(&Mfa {
        module: module.into(),
        function: function.into(),
        arity,
    }))
}

impl SourceCallGraph {
    pub fn build(
        nodes: &DataflowNodes<'_>,
        program: &Program,
        deps: &ClosureDependencies,
        registry: &Registry,
    ) -> Result<Self, CallGraphError> {
        let core = nodes.core();
        let labels = nodes.labels();
        let mut vertices = Vec::with_capacity(program.functions.len());
        let mut named_labels = BTreeMap::new();
        for f in &program.functions {
            let label = labels
                .label(site(f.module, f.node))
                .ok_or("function has no Core label")?;
            let vertex = match &f.identity {
                FunctionIdentity::Named(name) => GraphVertex::Named {
                    module: name.module.clone(),
                    function: name.name.clone(),
                    arity: name.arity.into(),
                },
                _ => GraphVertex::Label(label),
            };
            if matches!(vertex, GraphVertex::Named { .. }) {
                named_labels.insert(vertex.clone(), label);
            }
            vertices.push(vertex);
        }
        let module_functions = module_function_index(program);
        let mut modules = Vec::new();
        for (index, m) in core.modules.iter().enumerate() {
            let module = ModuleId(index);
            let mut order = NativeDictOrder::<DependencyCaller>::default();
            let mut by_label = BTreeMap::new();
            for id in module_functions.get(index).into_iter().flatten() {
                by_label.insert(
                    labels
                        .label(site(module, program.functions[id.0].node))
                        .ok_or("function label")?,
                    *id,
                );
            }
            for caller in &deps.module_callers[index] {
                order.store(match caller {
                    Some(id) => DependencyCaller::Label(
                        labels
                            .label(site(module, program.functions[id.0].node))
                            .ok_or("caller label")?,
                    ),
                    None => DependencyCaller::Top,
                });
            }
            let escaping: BTreeSet<_> = deps
                .escaping
                .local
                .iter()
                .filter(|id| program.functions[id.0].module == module)
                .map(|id| {
                    labels
                        .label(site(module, program.functions[id.0].node))
                        .ok_or("escaping label")
                })
                .collect::<Result<_, _>>()?;
            if !escaping.is_empty() || deps.module_escaping[index].external {
                order.store(DependencyCaller::External);
            }
            let mut label_edges = Vec::new();
            for caller in order.fold_keys().into_iter().rev() {
                let (from, targets) = match caller {
                    DependencyCaller::External => continue,
                    DependencyCaller::Top => (GraphVertex::Top, &deps.module_roots[index]),
                    DependencyCaller::Label(label) => {
                        let id = by_label[&label];
                        (vertices[id.0].clone(), &deps.dependencies[id.0])
                    }
                };
                let mut to = targets
                    .local
                    .iter()
                    .map(|id| {
                        (
                            labels
                                .label(site(
                                    program.functions[id.0].module,
                                    program.functions[id.0].node,
                                ))
                                .ok_or("target label"),
                            *id,
                        )
                    })
                    .map(|(l, id)| l.map(|l| (l, id)))
                    .collect::<Result<Vec<_>, _>>()?;
                to.sort_by_key(|(label, _)| *label);
                label_edges.extend(
                    to.into_iter()
                        .map(|(_, id)| (from.clone(), vertices[id.0].clone())),
                );
            }
            let mut self_recursive = BTreeSet::new();
            for (from, to) in &label_edges {
                if from == to {
                    self_recursive.insert(from.clone());
                    if let Some(label) = named_labels.get(from) {
                        self_recursive.insert(GraphVertex::Label(*label));
                    }
                }
            }
            label_edges.reverse();
            let confirmed = label_edges
                .iter()
                .flat_map(|(a, b)| [a.clone(), b.clone()])
                .filter(|v| *v != GraphVertex::Top)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let mut edges = label_edges
                .into_iter()
                .filter(|(a, b)| *a != GraphVertex::Top && *b != GraphVertex::Top)
                .collect::<Vec<_>>();
            let CoreExpr::Module { definitions, .. } = &m.nodes[m.core.0].expr else {
                return Err("call graph expects module".into());
            };
            for (_, fun) in definitions.iter().rev() {
                let id = program.function_nodes[index][&fun.0];
                let caller = vertices[id.0].clone();
                let mut stack = vec![(site(module, *fun), false)];
                let mut calls = Vec::new();
                while let Some((node, ready)) = stack.pop() {
                    if !ready {
                        stack.push((node, true));
                        stack.extend(
                            nodes
                                .native_subtrees(node)?
                                .into_iter()
                                .rev()
                                .map(|n| (n, false)),
                        );
                        continue;
                    }
                    let CoreExpr::Call {
                        module: callee_module,
                        name,
                        args,
                    } = nodes.expression(node)?
                    else {
                        continue;
                    };
                    let (Some(cm), Some(cf)) = (
                        atom(core, module, *callee_module),
                        atom(core, module, *name),
                    ) else {
                        continue;
                    };
                    let arity = BigInt::from(args.len());
                    let target = if known(registry, cm, cf, &arity)? {
                        if cm != "erlang" || cf != "make_fun" || args.len() != 3 {
                            continue;
                        }
                        let (Some(tm), Some(tf), Some(ta)) = (
                            atom(core, module, args[0]),
                            atom(core, module, args[1]),
                            integer(core, module, args[2]),
                        ) else {
                            continue;
                        };
                        if known(registry, tm, tf, &ta)? {
                            continue;
                        }
                        GraphVertex::Named {
                            module: tm.into(),
                            function: tf.into(),
                            arity: ta,
                        }
                    } else {
                        GraphVertex::Named {
                            module: cm.into(),
                            function: cf.into(),
                            arity,
                        }
                    };
                    calls.push((caller.clone(), target));
                }
                calls.reverse();
                edges.extend(calls);
            }
            modules.push(ModuleEdges {
                confirmed,
                edges,
                escaping,
                self_recursive,
            });
        }
        let graph = graph_from_modules(&modules, None)?;
        Ok(Self {
            modules,
            named_labels,
            label_names: Default::default(),
            module_functions,
            module_names: core.modules.iter().map(|m| m.name.clone()).collect(),
            graph,
        })
    }
    pub fn in_neighbours(&self, vertex: &GraphVertex) -> Option<&[GraphVertex]> {
        self.graph.incoming(vertex)
    }
    pub fn attach_layout(
        &mut self,
        layout: crate::native_runtime_hash::NativeAtomLayout,
    ) -> Result<(), CallGraphError> {
        self.graph = graph_from_modules(&self.modules, Some(layout))?;
        Ok(())
    }
    pub fn remove_external(&mut self) -> Result<Vec<(GraphVertex, GraphVertex)>, CallGraphError> {
        let unconfirmed = self
            .graph
            .ordered_vertices()
            .map_err(CallGraphError::RuntimeOrdering)?
            .into_iter()
            .filter(|v| !self.graph.is_confirmed(v))
            .rev()
            .collect::<Vec<_>>();
        let mut bad_calls = Vec::new();
        for vertex in &unconfirmed {
            for edge in self.graph.in_edges(vertex) {
                if let Some(endpoints) = self.graph.edge(edge) {
                    bad_calls.push(endpoints.clone());
                }
            }
        }
        bad_calls.sort_by(|(left, _), (right, _)| left.cmp(right));
        for vertex in unconfirmed {
            self.graph
                .delete_vertex(&vertex)
                .map_err(CallGraphError::RuntimeOrdering)?;
        }
        Ok(bad_calls)
    }
    pub fn source_graph(&self) -> &crate::source_digraph::SourceDigraph<GraphVertex> {
        &self.graph
    }
    pub fn function_name(&self, label: CoreLabel) -> Option<Mfa> {
        // Called for every label of every SCC: index once instead of scanning
        // all named vertices. Keeps the first vertex in map order per label.
        self.label_names
            .get_or_init(|| {
                let mut index = std::collections::HashMap::new();
                for (vertex, value) in &self.named_labels {
                    if let GraphVertex::Named {
                        module,
                        function,
                        arity,
                    } = vertex
                        && let Some(arity) = num_traits::ToPrimitive::to_usize(arity)
                    {
                        index.entry(*value).or_insert_with(|| Mfa {
                            module: module.clone().into(),
                            function: function.clone().into(),
                            arity,
                        });
                    }
                }
                index
            })
            .get(&label)
            .cloned()
    }
    pub fn schedule(&self) -> Result<crate::schedule::DependencyGraph<GraphVertex>, String> {
        let vertices = self
            .modules
            .iter()
            .flat_map(|module| module.confirmed.iter().cloned())
            .collect::<BTreeSet<_>>();
        let edges = self
            .modules
            .iter()
            .flat_map(|module| module.edges.iter().cloned())
            .filter(|(from, to)| vertices.contains(from) && vertices.contains(to));
        crate::schedule::DependencyGraph::build(vertices.iter().cloned(), edges)
    }
    pub fn function_inputs(
        &self,
        module: ModuleId,
        program: &Program,
        labels: &Labels,
        raw_signatures: BTreeMap<CoreLabel, crate::call_transfer::Signature>,
    ) -> Result<crate::function_tree::GraphInputs, String> {
        let facts = self
            .modules
            .get(module.0)
            .ok_or("module outside call graph")?;
        Ok(crate::function_tree::GraphInputs {
            escaping: facts.escaping.clone(),
            raw_signatures,
            ordered_local_callers: self.ordered_local_callers(module, program, labels)?,
        })
    }
    pub fn ordered_local_callers(
        &self,
        module: ModuleId,
        program: &Program,
        labels: &Labels,
    ) -> Result<BTreeMap<CoreLabel, Vec<CoreLabel>>, String> {
        let functions = self
            .module_functions
            .get(module.0)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let local = self.local_callers(
            module,
            program,
            labels,
            functions.iter().map(|id| &program.functions[id.0]),
            false,
        )?;
        if crate::lattice::verify_fast_paths() {
            let slow = self.local_callers(
                module,
                program,
                labels,
                program.functions.iter().filter(|f| f.module == module),
                true,
            )?;
            assert_eq!(local, slow, "indexed ordered_local_callers diverged");
        }
        Ok(local)
    }
    fn local_callers<'a>(
        &self,
        module: ModuleId,
        program: &'a Program,
        labels: &Labels,
        functions: impl Iterator<Item = &'a crate::program::Function> + Clone,
        scan: bool,
    ) -> Result<BTreeMap<CoreLabel, Vec<CoreLabel>>, String> {
        let local_labels = functions
            .clone()
            .map(|f| {
                labels
                    .label(site(module, f.node))
                    .ok_or("local function label")
            })
            .collect::<Result<std::collections::HashSet<_>, _>>()?;
        let mut local = BTreeMap::new();
        for f in functions {
            let label = labels
                .label(site(module, f.node))
                .ok_or("local function label")?;
            let vertex = match &f.identity {
                FunctionIdentity::Named(n) => GraphVertex::Named {
                    module: n.module.clone(),
                    function: n.name.clone(),
                    arity: n.arity.into(),
                },
                _ => GraphVertex::Label(label),
            };
            let callers = self
                .graph
                .incoming(&vertex)
                .into_iter()
                .flatten()
                .filter_map(|caller| match caller {
                    GraphVertex::Label(label) => Some(*label),
                    GraphVertex::Named { .. } => self.named_labels.get(caller).copied(),
                    GraphVertex::Top => None,
                })
                .filter(|label| {
                    if scan {
                        program.functions.iter().any(|f| {
                            f.module == module
                                && labels.label(site(module, f.node)) == Some(*label)
                        })
                    } else {
                        local_labels.contains(label)
                    }
                })
                .collect();
            local.insert(label, callers);
        }
        Ok(local)
    }
}

fn module_function_index(program: &Program) -> Vec<Vec<FunctionId>> {
    let mut index = vec![Vec::new(); program.modules.len()];
    for f in &program.functions {
        if index.len() <= f.module.0 {
            index.resize_with(f.module.0 + 1, Vec::new);
        }
        index[f.module.0].push(f.id);
    }
    index
}

impl crate::source_digraph::RuntimeGraphKey for GraphVertex {
    fn runtime_key(
        &self,
    ) -> Result<
        crate::native_runtime_hash::NativeHashKey,
        crate::native_runtime_hash::RuntimeHashError,
    > {
        use crate::native_runtime_hash::NativeHashKey as K;
        Ok(match self {
            Self::Label(label) => K::Integer(label.0.into()),
            Self::Top => K::Atom("top".into()),
            Self::Named {
                module,
                function,
                arity,
            } => K::Tuple(vec![
                K::Atom(module.clone()),
                K::Atom(function.clone()),
                K::Integer(arity.clone()),
            ]),
        })
    }
}
fn graph_from_modules(
    modules: &[ModuleEdges],
    layout: Option<crate::native_runtime_hash::NativeAtomLayout>,
) -> Result<crate::source_digraph::SourceDigraph<GraphVertex>, CallGraphError> {
    use crate::source_digraph::RuntimeGraphKey;
    let mut graph = crate::source_digraph::SourceDigraph::new(layout, false);
    let fail = |e: crate::native_runtime_hash::RuntimeHashError| CallGraphError::RuntimeOrdering(e);
    for module in modules {
        for vertex in &module.confirmed {
            graph.add_vertex(vertex.clone(), true).map_err(fail)?;
        }
        for (from, to) in &module.edges {
            if !graph.contains(from) {
                graph.add_vertex(from.clone(), false).map_err(fail)?;
            }
            if !graph.contains(to) {
                graph.add_vertex(to.clone(), false).map_err(fail)?;
            }
            let id = crate::native_runtime_hash::NativeHashKey::Tuple(vec![
                from.runtime_key().map_err(fail)?,
                to.runtime_key().map_err(fail)?,
            ]);
            graph.add_edge(id, from.clone(), to.clone()).map_err(fail)?;
        }
    }
    Ok(graph)
}

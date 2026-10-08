use crate::{
    constraints::generation_capture_headers::{
        ActiveDigraphCapture, DigraphCapture, GenerationRuntimeCaptures, SourceReference,
    },
    labels::CoreLabel,
    native_map_order,
    native_runtime_hash::{NativeHashKey, RuntimeHashError},
    program::ModuleId,
    source_callgraph::{GraphVertex, SourceCallGraph},
    source_digraph::SourceDigraph,
    source_graph_algorithms as algorithms,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderedScc {
    pub identity: u64,
    pub members: Vec<GraphVertex>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceRole {
    Outgoing,
    ModuleVertices,
    ModuleEdges,
    ModuleNeighbours,
}
pub trait RuntimeReferenceAllocator {
    type Error;
    fn allocate(&mut self, role: ReferenceRole, epoch: u64)
    -> Result<SourceReference, Self::Error>;
}
#[derive(Debug)]
pub enum ScheduleError<E> {
    Ordering(RuntimeHashError),
    MissingCallGraphCapture,
    UnsupportedInitialActiveGraph,
    Native(crate::native_error::NativeTransferError),
    ReferenceAllocation(E),
    InvalidCondensation,
    ModuleOutsideCompilerInput(String),
    EpochExhausted,
}
impl<E> From<RuntimeHashError> for ScheduleError<E> {
    fn from(value: RuntimeHashError) -> Self {
        Self::Ordering(value)
    }
}
#[derive(Clone, Debug)]
pub enum ActiveGraph {
    Undefined,
    Outgoing(BTreeMap<u64, Vec<u64>>),
    Modules(SourceDigraph<NativeHashKey>),
}
/// Lifecycle owns active phase graphs and captures; original adjacency stays borrowed.
pub struct SourceSchedule<A: RuntimeReferenceAllocator> {
    runtime: GenerationRuntimeCaptures,
    allocator: A,
    epoch: u64,
    active: ActiveGraph,
}
impl<A: RuntimeReferenceAllocator> SourceSchedule<A> {
    pub fn new(
        runtime: GenerationRuntimeCaptures,
        allocator: A,
    ) -> Result<Self, ScheduleError<A::Error>> {
        let capture = runtime
            .callgraph
            .as_ref()
            .ok_or(ScheduleError::MissingCallGraphCapture)?;
        if !matches!(capture.active, ActiveDigraphCapture::Undefined) {
            return Err(ScheduleError::UnsupportedInitialActiveGraph);
        }
        Ok(Self {
            runtime,
            allocator,
            epoch: 0,
            active: ActiveGraph::Undefined,
        })
    }
    pub fn runtime(&self) -> &GenerationRuntimeCaptures {
        &self.runtime
    }
    pub fn active(&self) -> &ActiveGraph {
        &self.active
    }
    fn allocate(
        &mut self,
        role: ReferenceRole,
    ) -> Result<SourceReference, ScheduleError<A::Error>> {
        self.allocator
            .allocate(role, self.epoch)
            .map_err(ScheduleError::ReferenceAllocation)
    }
    fn next_epoch(&mut self) -> Result<(), ScheduleError<A::Error>> {
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or(ScheduleError::EpochExhausted)?;
        Ok(())
    }
    pub fn finalize(
        &mut self,
        graph: &SourceCallGraph,
    ) -> Result<Vec<OrderedScc>, ScheduleError<A::Error>> {
        self.condense(graph.source_graph())
    }
    pub fn reset_from_functions(
        &mut self,
        graph: &SourceCallGraph,
        changed: &[CoreLabel],
    ) -> Result<Vec<OrderedScc>, ScheduleError<A::Error>> {
        self.delete_active()?;
        let roots = function_vertices(graph, changed);
        let sub = algorithms::reaching_subgraph(graph.source_graph(), &roots)?;
        self.condense(&sub)
    }
    pub fn modules_from_functions(
        &mut self,
        graph: &SourceCallGraph,
        changed: &[CoreLabel],
    ) -> Result<Vec<ModuleId>, ScheduleError<A::Error>> {
        self.delete_active()?;
        let roots = function_vertices(graph, changed);
        let sub = algorithms::reaching_subgraph(graph.source_graph(), &roots)?;
        let layout = sub.layout()?.clone();
        let mut modules = BTreeSet::new();
        let mut edges = BTreeSet::new();
        for vertex in sub.ordered_vertices()? {
            if let GraphVertex::Named { module, .. } = vertex {
                modules.insert(module);
            }
        }
        for edge in sub.ordered_edges()? {
            if let Some((
                GraphVertex::Named { module: from, .. },
                GraphVertex::Named { module: to, .. },
            )) = sub.edge(&edge)
                && from != to
            {
                edges.insert(NativeHashKey::Tuple(vec![
                    NativeHashKey::Atom(from.clone()),
                    NativeHashKey::Atom(to.clone()),
                ]));
            }
        }
        let mut active = SourceDigraph::new(Some(layout.clone()), true);
        for module in modules {
            active.add_vertex(NativeHashKey::Atom(module), true)?;
        }
        for edge in native_map_order::ordered_keys(&layout, edges)? {
            if let NativeHashKey::Tuple(pair) = edge
                && let [from, to] = pair.as_slice()
            {
                active.auto_edge(from.clone(), to.clone())?;
            }
        }
        let order = algorithms::topsort(&active)?.ok_or(ScheduleError::InvalidCondensation)?;
        let module_ids = order
            .into_iter()
            .rev()
            .map(|key| match key {
                NativeHashKey::Atom(name) => graph
                    .module_names
                    .iter()
                    .position(|m| m == &name)
                    .map(ModuleId)
                    .ok_or(ScheduleError::ModuleOutsideCompilerInput(name)),
                _ => Err(ScheduleError::InvalidCondensation),
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.next_epoch()?;
        let vertices = self.allocate(ReferenceRole::ModuleVertices)?;
        let edges = self.allocate(ReferenceRole::ModuleEdges)?;
        let neighbours = self.allocate(ReferenceRole::ModuleNeighbours)?;
        self.runtime
            .callgraph
            .as_mut()
            .ok_or(ScheduleError::MissingCallGraphCapture)?
            .active = ActiveDigraphCapture::Digraph(DigraphCapture {
            vertices,
            edges,
            neighbours,
            cyclic: false,
        });
        self.active = ActiveGraph::Modules(active);
        Ok(module_ids)
    }
    fn delete_active(&mut self) -> Result<(), ScheduleError<A::Error>> {
        if matches!(self.active, ActiveGraph::Undefined) {
            return Err(ScheduleError::Native(
                crate::native_error::NativeTransferError::FunctionClause,
            ));
        }
        self.active = ActiveGraph::Undefined;
        self.runtime
            .callgraph
            .as_mut()
            .ok_or(ScheduleError::MissingCallGraphCapture)?
            .active = ActiveDigraphCapture::Undefined;
        Ok(())
    }
    fn condense(
        &mut self,
        graph: &SourceDigraph<GraphVertex>,
    ) -> Result<Vec<OrderedScc>, ScheduleError<A::Error>> {
        let components = algorithms::strong_components(graph)?;
        let mut ids = BTreeMap::new();
        for (index, component) in components.iter().enumerate() {
            for v in component {
                ids.insert(v.clone(), index as u64 + 1);
            }
        }
        let mut outgoing = BTreeMap::<u64, BTreeSet<u64>>::new();
        for edge in graph.ordered_edges()? {
            if let Some((from, to)) = graph.edge(&edge) {
                let from = ids[from];
                let to = ids[to];
                if from != to {
                    outgoing.entry(from).or_default().insert(to);
                }
            }
        }
        let mut condensed = SourceDigraph::new(Some(graph.layout()?.clone()), false);
        for (from, tos) in &outgoing {
            let from = NativeHashKey::Integer((*from).into());
            condensed.add_vertex(from.clone(), false)?;
            for to in tos {
                let to = NativeHashKey::Integer((*to).into());
                condensed.add_vertex(to.clone(), false)?;
                condensed.auto_edge(from.clone(), to)?;
            }
        }
        for id in 1..=components.len() {
            condensed.add_vertex(NativeHashKey::Integer(id.into()), false)?;
        }
        let order = algorithms::topsort(&condensed)?.ok_or(ScheduleError::InvalidCondensation)?;
        let ordered = order
            .into_iter()
            .rev()
            .map(|key| match key {
                NativeHashKey::Integer(id) => Ok(OrderedScc {
                    identity: num_traits::ToPrimitive::to_u64(&id)
                        .ok_or(ScheduleError::InvalidCondensation)?,
                    members: components[num_traits::ToPrimitive::to_usize(&id)
                        .ok_or(ScheduleError::InvalidCondensation)?
                        - 1]
                    .clone(),
                }),
                _ => Err(ScheduleError::InvalidCondensation),
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.next_epoch()?;
        let reference = self.allocate(ReferenceRole::Outgoing)?;
        self.runtime
            .callgraph
            .as_mut()
            .ok_or(ScheduleError::MissingCallGraphCapture)?
            .active = ActiveDigraphCapture::Outgoing(reference);
        self.active = ActiveGraph::Outgoing(
            outgoing
                .into_iter()
                .map(|(id, values)| (id, values.into_iter().collect()))
                .collect(),
        );
        Ok(ordered)
    }
}
fn function_vertices(graph: &SourceCallGraph, changed: &[CoreLabel]) -> Vec<GraphVertex> {
    changed
        .iter()
        .map(|label| {
            graph
                .named_labels
                .iter()
                .find_map(|(v, l)| (*l == *label).then(|| v.clone()))
                .unwrap_or(GraphVertex::Label(*label))
        })
        .collect()
}

/// Identity equality only: these names never stand in for BEAM reference ordering.
/// The caller supplies a fresh allocation scope reserved for this analysis owner.
pub struct ScopedReferenceAllocator {
    scope: String,
}
impl ScopedReferenceAllocator {
    pub fn new(scope: String) -> Result<Self, RuntimeHashError> {
        if scope.is_empty() {
            return Err(RuntimeHashError::Boundary(
                "runtime reference allocation scope missing".into(),
            ));
        }
        Ok(Self { scope })
    }
}
impl RuntimeReferenceAllocator for ScopedReferenceAllocator {
    type Error = std::convert::Infallible;
    fn allocate(
        &mut self,
        role: ReferenceRole,
        epoch: u64,
    ) -> Result<SourceReference, Self::Error> {
        Ok(SourceReference(crate::term_order::RuntimeIdentity {
            scope: self.scope.clone(),
            object: format!("source_active/{epoch}/{role:?}"),
        }))
    }
}

impl<A: RuntimeReferenceAllocator> crate::core_analysis::PhaseSchedule for SourceSchedule<A> {
    type Error = ScheduleError<A::Error>;
    fn runtime(&self) -> &GenerationRuntimeCaptures {
        SourceSchedule::runtime(self)
    }
    fn finalize(&mut self, graph: &SourceCallGraph) -> Result<Vec<OrderedScc>, Self::Error> {
        SourceSchedule::finalize(self, graph)
    }
    fn modules_from_functions(
        &mut self,
        graph: &SourceCallGraph,
        changed: &[CoreLabel],
    ) -> Result<Vec<ModuleId>, Self::Error> {
        SourceSchedule::modules_from_functions(self, graph, changed)
    }
    fn reset_from_functions(
        &mut self,
        graph: &SourceCallGraph,
        changed: &[CoreLabel],
    ) -> Result<Vec<OrderedScc>, Self::Error> {
        SourceSchedule::reset_from_functions(self, graph, changed)
    }
}

use crate::closure_dependencies::ClosureDependencies;
use crate::program::{CallTarget, FunctionId, MetadataKind, ModuleId, NodeRef, Program, TypeName};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct VertexId(pub usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ComponentId(pub usize);
#[derive(Clone, Debug)]
pub struct Component {
    pub id: ComponentId,
    pub members: Vec<VertexId>,
    pub dependencies: Vec<ComponentId>,
    pub reverse_users: Vec<ComponentId>,
    pub recursive: bool,
}
#[derive(Clone, Debug)]
pub struct DependencyGraph<K> {
    pub vertices: Vec<K>,
    pub dependencies: Vec<Vec<VertexId>>,
    pub reverse_users: Vec<Vec<VertexId>>,
    pub components: Vec<Component>,
    pub vertex_component: Vec<ComponentId>,
    pub callee_first: Vec<ComponentId>,
}
impl<K: Clone + Ord> DependencyGraph<K> {
    pub fn build(
        vertices: impl IntoIterator<Item = K>,
        edges: impl IntoIterator<Item = (K, K)>,
    ) -> Result<Self> {
        let vertices: Vec<_> = vertices
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let lookup: BTreeMap<_, _> = vertices
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, key)| (key, i))
            .collect();
        let n = vertices.len();
        let mut dependencies = vec![BTreeSet::new(); n];
        let mut reverse = vec![BTreeSet::new(); n];
        for (user, dependency) in edges {
            let user = *lookup
                .get(&user)
                .ok_or("dependency graph: edge source is not a registered vertex")?;
            let dependency = *lookup
                .get(&dependency)
                .ok_or("dependency graph: edge target is not a registered vertex")?;
            dependencies[user].insert(VertexId(dependency));
            reverse[dependency].insert(VertexId(user));
        }
        let dependencies: Vec<Vec<_>> = dependencies
            .into_iter()
            .map(|set| set.into_iter().collect())
            .collect();
        let reverse_users: Vec<Vec<_>> = reverse
            .into_iter()
            .map(|set| set.into_iter().collect())
            .collect();
        let mut visited = vec![false; n];
        let mut finish = Vec::with_capacity(n);
        for start in 0..n {
            if visited[start] {
                continue;
            }
            visited[start] = true;
            let mut stack = vec![(start, 0)];
            while let Some((node, next)) = stack.last_mut() {
                if *next == dependencies[*node].len() {
                    finish.push(*node);
                    stack.pop();
                    continue;
                }
                let child = dependencies[*node][*next].0;
                *next += 1;
                if !visited[child] {
                    visited[child] = true;
                    stack.push((child, 0));
                }
            }
        }
        let mut assigned = vec![false; n];
        let mut components = Vec::new();
        let mut vertex_component = vec![ComponentId(0); n];
        for start in finish.into_iter().rev() {
            if assigned[start] {
                continue;
            }
            assigned[start] = true;
            let mut stack = vec![start];
            let mut members = Vec::new();
            while let Some(node) = stack.pop() {
                members.push(VertexId(node));
                for user in &reverse_users[node] {
                    if !assigned[user.0] {
                        assigned[user.0] = true;
                        stack.push(user.0);
                    }
                }
            }
            members.sort();
            let id = ComponentId(members[0].0);
            for member in &members {
                vertex_component[member.0] = id;
            }
            let recursive = members.len() > 1 || dependencies[members[0].0].contains(&members[0]);
            components.push(Component {
                id,
                members,
                dependencies: Vec::new(),
                reverse_users: Vec::new(),
                recursive,
            });
        }
        components.sort_by_key(|c| c.id);
        let component_lookup: BTreeMap<_, _> = components
            .iter()
            .enumerate()
            .map(|(i, c)| (c.id, i))
            .collect();
        for component in &mut components {
            let mut deps = BTreeSet::new();
            let mut users = BTreeSet::new();
            for member in &component.members {
                deps.extend(
                    dependencies[member.0]
                        .iter()
                        .map(|v| vertex_component[v.0])
                        .filter(|id| *id != component.id),
                );
                users.extend(
                    reverse_users[member.0]
                        .iter()
                        .map(|v| vertex_component[v.0])
                        .filter(|id| *id != component.id),
                );
            }
            component.dependencies = deps.into_iter().collect();
            component.reverse_users = users.into_iter().collect();
        }
        let mut remaining: Vec<_> = components.iter().map(|c| c.dependencies.len()).collect();
        let mut ready: BTreeSet<_> = components
            .iter()
            .filter(|c| c.dependencies.is_empty())
            .map(|c| c.id)
            .collect();
        let mut callee_first = Vec::new();
        while let Some(id) = ready.pop_first() {
            callee_first.push(id);
            let component = &components[component_lookup[&id]];
            for user in &component.reverse_users {
                let i = component_lookup[user];
                remaining[i] -= 1;
                if remaining[i] == 0 {
                    ready.insert(*user);
                }
            }
        }
        if callee_first.len() != components.len() {
            return Err("dependency graph: cyclic SCC condensation".into());
        }
        let graph = Self {
            vertices,
            dependencies,
            reverse_users,
            components,
            vertex_component,
            callee_first,
        };
        graph.validate()?;
        Ok(graph)
    }
    pub fn validate(&self) -> Result<()> {
        let mut seen = vec![false; self.vertices.len()];
        let mut ranks = BTreeMap::new();
        for (rank, id) in self.callee_first.iter().enumerate() {
            if ranks.insert(*id, rank).is_some() {
                return Err("dependency graph: duplicate scheduled component".into());
            }
        }
        for component in &self.components {
            if component.members.is_empty() || component.id.0 != component.members[0].0 {
                return Err("dependency graph: unstable component identity".into());
            }
            for member in &component.members {
                if seen[member.0] || self.vertex_component[member.0] != component.id {
                    return Err("dependency graph: invalid component membership".into());
                }
                seen[member.0] = true;
            }
            for dependency in &component.dependencies {
                if ranks[dependency] >= ranks[&component.id] {
                    return Err("dependency graph: caller precedes dependency".into());
                }
            }
        }
        if seen.contains(&false) || ranks.len() != self.components.len() {
            return Err("dependency graph: incomplete vertex coverage".into());
        }
        for (user, deps) in self.dependencies.iter().enumerate() {
            for dependency in deps {
                if !self.reverse_users[dependency.0].contains(&VertexId(user)) {
                    return Err("dependency graph: missing reverse user".into());
                }
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MetadataId(pub usize);
#[derive(Clone, Debug)]
pub enum FunctionDependencyCause {
    Call(NodeRef),
    ClosureValue,
    CapturedBinding,
}
#[derive(Clone, Debug)]
pub struct FunctionDependency {
    pub user: FunctionId,
    pub dependency: FunctionId,
    pub cause: FunctionDependencyCause,
}
#[derive(Clone, Debug)]
pub struct ExternalCall {
    pub caller: FunctionId,
    pub callsite: NodeRef,
    pub target: crate::program::FunctionName,
}
#[derive(Clone, Debug)]
pub struct ExternalType {
    pub user: MetadataId,
    pub target: TypeName,
}
#[derive(Clone, Debug)]
pub struct BehaviourDependency {
    pub user: MetadataId,
    pub target: crate::raw_literal::RawLiteral,
    pub internal_module: Option<ModuleId>,
}
#[derive(Clone, Debug)]
pub struct Plan {
    pub closures: ClosureDependencies,
    pub functions: DependencyGraph<FunctionId>,
    pub declarations: DependencyGraph<MetadataId>,
    pub function_dependencies: Vec<FunctionDependency>,
    pub external_calls: Vec<ExternalCall>,
    pub dynamic_calls: Vec<(FunctionId, NodeRef)>,
    pub type_declarations: BTreeMap<TypeName, MetadataId>,
    pub external_types: Vec<ExternalType>,
    pub behaviours: Vec<BehaviourDependency>,
}
#[derive(Serialize)]
pub struct Inspection {
    pub function_vertices: usize,
    pub function_edges: usize,
    pub function_components: usize,
    pub recursive_function_components: usize,
    pub internal_call_sites: usize,
    pub closure_dependencies: usize,
    pub external_call_sites: usize,
    pub dynamic_call_sites: usize,
    pub declaration_vertices: usize,
    pub declaration_edges: usize,
    pub declaration_components: usize,
    pub recursive_declaration_components: usize,
    pub type_declarations: usize,
    pub external_type_dependencies: usize,
    pub behaviour_dependencies: usize,
}
impl Plan {
    pub fn build(program: &Program, closures: ClosureDependencies) -> Result<Self> {
        let mut function_dependencies = Vec::new();
        let mut external_calls = Vec::new();
        let mut dynamic_calls = Vec::new();
        for call in &program.calls {
            let callsite = NodeRef {
                module: call.module,
                node: call.node.0,
            };
            let target = match &call.target {
                CallTarget::Named(name) => program.named.get(name).copied(),
                CallTarget::Apply | CallTarget::DynamicRemote => {
                    dynamic_calls.push((call.caller, callsite));
                    None
                }
            };
            if let Some(dependency) = target {
                function_dependencies.push(FunctionDependency {
                    user: call.caller,
                    dependency,
                    cause: FunctionDependencyCause::Call(callsite),
                });
            } else if let CallTarget::Named(target) = &call.target {
                external_calls.push(ExternalCall {
                    caller: call.caller,
                    callsite,
                    target: target.clone(),
                });
            }
        }
        for (user, dependency) in closures.function_edges() {
            function_dependencies.push(FunctionDependency {
                user,
                dependency,
                cause: FunctionDependencyCause::ClosureValue,
            });
        }
        let functions = DependencyGraph::build(
            program.functions.iter().map(|f| f.id),
            function_dependencies.iter().map(|e| (e.user, e.dependency)),
        )?;
        let mut type_declarations = BTreeMap::new();
        for (i, metadata) in program.metadata.iter().enumerate() {
            if matches!(
                metadata.kind,
                MetadataKind::Type | MetadataKind::Opaque | MetadataKind::Nominal
            ) {
                for name in &metadata.types {
                    if type_declarations
                        .insert(name.clone(), MetadataId(i))
                        .is_some()
                    {
                        return Err(format!("dependency plan: duplicate declaration {name:?}"));
                    }
                }
            }
        }
        let mut declaration_edges = Vec::new();
        let mut external_types = Vec::new();
        for edge in &program.type_dependencies {
            let user = MetadataId(edge.metadata);
            if let Some(dependency) = type_declarations.get(&edge.target) {
                declaration_edges.push((user, *dependency));
            } else {
                external_types.push(ExternalType {
                    user,
                    target: edge.target.clone(),
                });
            }
        }
        let declarations = DependencyGraph::build(
            (0..program.metadata.len()).map(MetadataId),
            declaration_edges,
        )?;
        let module_lookup: BTreeMap<_, _> = program
            .modules
            .iter()
            .map(|m| (m.name.as_str(), m.id))
            .collect();
        let mut behaviours = Vec::new();
        for (i, metadata) in program.metadata.iter().enumerate() {
            let Some(attribute) = &metadata.behaviour else {
                continue;
            };
            for occurrence in
                crate::behaviour_metadata::collect([attribute]).map_err(|e| e.to_string())?
            {
                let target = occurrence.name;
                behaviours.push(BehaviourDependency {
                    user: MetadataId(i),
                    target: target.clone(),
                    internal_module: target
                        .as_atom()
                        .and_then(|name| module_lookup.get(name))
                        .copied(),
                });
            }
        }
        let plan = Self {
            closures,
            functions,
            declarations,
            function_dependencies,
            external_calls,
            dynamic_calls,
            type_declarations,
            external_types,
            behaviours,
        };
        let internal = plan
            .function_dependencies
            .iter()
            .filter(|e| matches!(e.cause, FunctionDependencyCause::Call(_)))
            .count();
        if internal + plan.external_calls.len() + plan.dynamic_calls.len() != program.calls.len() {
            return Err("dependency plan: incomplete callsite coverage".into());
        }
        let internal_types = program
            .type_dependencies
            .iter()
            .filter(|e| plan.type_declarations.contains_key(&e.target))
            .count();
        if internal_types + plan.external_types.len() != program.type_dependencies.len() {
            return Err("dependency plan: incomplete type dependency coverage".into());
        }
        Ok(plan)
    }
    pub fn inspect(&self) -> Inspection {
        Inspection {
            function_vertices: self.functions.vertices.len(),
            function_edges: self.functions.dependencies.iter().map(Vec::len).sum(),
            function_components: self.functions.components.len(),
            recursive_function_components: self
                .functions
                .components
                .iter()
                .filter(|c| c.recursive)
                .count(),
            internal_call_sites: self
                .function_dependencies
                .iter()
                .filter(|e| matches!(e.cause, FunctionDependencyCause::Call(_)))
                .count(),
            closure_dependencies: self
                .function_dependencies
                .iter()
                .filter(|e| !matches!(e.cause, FunctionDependencyCause::Call(_)))
                .count(),
            external_call_sites: self.external_calls.len(),
            dynamic_call_sites: self.dynamic_calls.len(),
            declaration_vertices: self.declarations.vertices.len(),
            declaration_edges: self.declarations.dependencies.iter().map(Vec::len).sum(),
            declaration_components: self.declarations.components.len(),
            recursive_declaration_components: self
                .declarations
                .components
                .iter()
                .filter(|c| c.recursive)
                .count(),
            type_declarations: self.type_declarations.len(),
            external_type_dependencies: self.external_types.len(),
            behaviour_dependencies: self.behaviours.len(),
        }
    }
}

#[cfg(test)]
#[path = "schedule_tests.rs"]
mod tests;

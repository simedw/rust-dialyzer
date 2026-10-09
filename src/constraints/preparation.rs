use crate::constraints::normalize::{
    GroupKind, NativeConstraint, NativeConstraintArena, NativeConstraintId, NativeOperand,
};
use crate::constraints::tree::FunctionVariable;
use crate::lattice::VariableId;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedNodeId(pub usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeListId(pub usize);
#[derive(Clone, Debug)]
pub struct PreparedNode {
    pub source: NativeConstraintId,
    pub children: Vec<PreparedNodeId>,
    pub dependencies: BTreeSet<VariableId>,
    /// Child positions per variable, zero-based as the solver uses them.
    pub masks: Option<BTreeMap<VariableId, BTreeSet<usize>>>,
    pub list_id: Option<NativeListId>,
}
#[derive(Clone, Debug)]
pub struct FunctionBody {
    pub function: FunctionVariable,
    pub body: NativeConstraintId,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DependencyRetention {
    #[default]
    Clear,
    Retain,
}
#[derive(Clone, Debug)]
pub struct PreparedConstruction {
    dependency_retention: DependencyRetention,
    pub nodes: Vec<PreparedNode>,
    pub bodies: BTreeMap<FunctionVariable, PreparedNodeId>,
    pub next_list_id: usize,
}
#[derive(Clone, Debug)]
pub struct FunctionParameters {
    pub arity: usize,
    pub recursive_variable: Option<VariableId>,
    pub propagated_domain: Option<Vec<crate::lattice::Type>>,
    pub self_recursive: bool,
}
#[derive(Clone, Copy)]
enum Phase {
    Enumerate,
    Order,
}
impl PreparedConstruction {
    /// Function references model lexical closure containment, not recursive calls.
    /// The construction boundary requires an acyclic containment graph.
    pub fn build(
        arena: &mut NativeConstraintArena,
        functions: &[FunctionBody],
        dependency_retention: DependencyRetention,
    ) -> Result<Self, String> {
        let out = Self::build_with(arena, functions, dependency_retention, false)?;
        if crate::lattice::verify_fast_paths() {
            let copied = Self::build_with(arena, functions, dependency_retention, true)?;
            assert_eq!(
                out.to_value(arena)?,
                copied.to_value(arena)?,
                "in-place preparation diverged from copying preparation"
            );
        }
        Ok(out)
    }
    /// `copying` keeps every rewritten node as a fresh copy and groups flat
    /// prefixes through the arena, the reference the in-place rewrite is
    /// checked against.
    fn build_with(
        arena: &mut NativeConstraintArena,
        functions: &[FunctionBody],
        dependency_retention: DependencyRetention,
        copying: bool,
    ) -> Result<Self, String> {
        let mut out = Self {
            dependency_retention,
            nodes: Vec::new(),
            bodies: BTreeMap::new(),
            next_list_id: 0,
        };
        let mut roots = BTreeMap::new();
        for f in functions {
            if roots.insert(f.function.clone(), f.body).is_some() {
                return Err("duplicate function body".into());
            }
        }
        for f in functions {
            let body = out.copy_tree(arena, f.body)?;
            out.bodies.insert(f.function.clone(), body);
        }
        for phase in [Phase::Enumerate, Phase::Order] {
            for f in functions {
                let root = out.bodies[&f.function];
                let old_deps = out.nodes[root.0].dependencies.clone();
                let new = out.rewrite(arena, root, phase, copying)?;
                if matches!(phase, Phase::Order) {
                    out.nodes[new.0].dependencies = old_deps;
                }
                out.bodies.insert(f.function.clone(), new);
                if matches!(phase, Phase::Enumerate) {
                    out.next_list_id += 1;
                }
            }
        }
        Ok(out)
    }
    pub fn into_tree(
        mut self,
        arena: NativeConstraintArena,
        parameters: BTreeMap<FunctionVariable, FunctionParameters>,
        entry_functions: Vec<FunctionVariable>,
    ) -> Result<crate::constraints::tree::PreparedTree, String> {
        use crate::constraints::atomic::AtomicConstraint;
        use crate::constraints::tree::{
            Constraint, ConstraintId, ConstraintNode, ConstraintTree, FunctionDefinition,
        };
        if parameters.len() != self.bodies.len()
            || parameters.keys().any(|f| !self.bodies.contains_key(f))
        {
            return Err("function parameter catalogue differs from prepared body catalogue".into());
        }
        if entry_functions.is_empty()
            || entry_functions.iter().any(|f| !self.bodies.contains_key(f))
        {
            return Err("entry function catalogue is empty or undefined".into());
        }
        let entry_deps: Vec<_> = entry_functions
            .iter()
            .map(|function| self.nodes[self.bodies[function].0].dependencies.clone())
            .collect();
        // Every prepared node is reachable once, so its sets move into the tree.
        let mut taken = vec![false; self.nodes.len()];
        let mut nodes = Vec::new();
        let mut definitions = BTreeMap::new();
        for (function, root) in &self.bodies {
            enum Work {
                Enter(PreparedNodeId),
                Exit(PreparedNodeId),
            }
            let mut work = vec![Work::Enter(*root)];
            let mut built = Vec::new();
            while let Some(task) = work.pop() {
                match task {
                    Work::Enter(id) => {
                        work.push(Work::Exit(id));
                        for c in self.nodes[id.0].children.iter().rev() {
                            work.push(Work::Enter(*c));
                        }
                    }
                    Work::Exit(id) => {
                        if std::mem::replace(&mut taken[id.0], true) {
                            return Err("prepared node reached twice".into());
                        }
                        let n = &mut self.nodes[id.0];
                        let children = built.split_off(built.len() - n.children.len());
                        let dependencies = std::mem::take(&mut n.dependencies);
                        let constraint = match arena
                            .node(n.source)
                            .ok_or("foreign prepared source")?
                        {
                            NativeConstraint::Atomic {
                                left,
                                relation,
                                right,
                                ..
                            } => Constraint::Atomic(AtomicConstraint {
                                left: left.clone(),
                                relation: *relation,
                                right: arena.operand(right).map_err(|e| format!("{e:?}"))?,
                            }),
                            NativeConstraint::Reference { function, .. } => Constraint::Reference {
                                function: function.clone(),
                                dependencies: dependencies.clone(),
                            },
                            NativeConstraint::Group { kind, .. } => {
                                if *kind == GroupKind::Conjunction {
                                    Constraint::Conjunction(children)
                                } else {
                                    Constraint::Disjunction(children)
                                }
                            }
                        };
                        let masks = n.masks.take().unwrap_or_default();
                        built.push(ConstraintId(nodes.len()));
                        nodes.push(ConstraintNode {
                            constraint,
                            dependencies,
                            masks,
                        });
                    }
                }
            }
            let p = &parameters[function];
            definitions.insert(
                function.clone(),
                FunctionDefinition {
                    body: built[0],
                    arity: p.arity,
                    recursive_variable: p.recursive_variable.clone(),
                    propagated_domain: p.propagated_domain.clone(),
                    self_recursive: p.self_recursive,
                },
            );
        }
        let mut entries = Vec::new();
        let mut root_deps = BTreeSet::new();
        let mut root_masks: BTreeMap<VariableId, BTreeSet<usize>> = BTreeMap::new();
        for ((index, function), mut deps) in entry_functions.iter().enumerate().zip(entry_deps) {
            deps.remove(function.variable());
            root_deps.extend(deps.clone());
            for v in &deps {
                root_masks.entry(v.clone()).or_default().insert(index);
            }
            entries.push(ConstraintId(nodes.len()));
            nodes.push(ConstraintNode {
                constraint: Constraint::Reference {
                    function: function.clone(),
                    dependencies: deps.clone(),
                },
                dependencies: deps,
                masks: BTreeMap::new(),
            });
        }
        let root = if entries.len() == 1 {
            entries[0]
        } else {
            let id = ConstraintId(nodes.len());
            nodes.push(ConstraintNode {
                constraint: Constraint::Conjunction(entries),
                dependencies: root_deps,
                masks: root_masks,
            });
            id
        };
        ConstraintTree::admit_prepared(nodes, definitions, arena.semantics, root)
    }
    pub fn to_value(&self, arena: &NativeConstraintArena) -> Result<serde_json::Value, String> {
        use serde_json::json;
        fn var(v: &VariableId) -> serde_json::Value {
            match v {
                VariableId::Integer(n) => json!({"integer":n.to_string()}),
                VariableId::Atom(a) => json!({"atom":a}),
            }
        }
        let mut functions = Vec::new();
        for (function, root) in &self.bodies {
            enum Work {
                Enter(PreparedNodeId),
                Exit(PreparedNodeId),
            }
            let mut work = vec![Work::Enter(*root)];
            let mut nodes = Vec::new();
            let mut values = Vec::new();
            while let Some(task) = work.pop() {
                match task {
                    Work::Enter(id) => {
                        work.push(Work::Exit(id));
                        for c in self.nodes[id.0].children.iter().rev() {
                            work.push(Work::Enter(*c));
                        }
                    }
                    Work::Exit(id) => {
                        let n = &self.nodes[id.0];
                        let children = values.split_off(values.len() - n.children.len());
                        let mut value = match arena
                            .node(n.source)
                            .ok_or("foreign prepared source")?
                        {
                            NativeConstraint::Group { kind, .. } => {
                                json!({"kind":if *kind==GroupKind::Conjunction{"conj"}else{"disj"},"children":children,"list_id":n.list_id.map(|id|id.0),"masks":n.masks.as_ref().map(|m|m.iter().map(|(v,indices)|json!({"variable":var(v),"indices":indices.iter().map(|i|i+1).collect::<Vec<_>>()})).collect::<Vec<_>>())})
                            }
                            _ => arena.to_value(n.source).map_err(|e| format!("{e:?}"))?["nodes"]
                                [0]
                            .clone(),
                        };
                        value["deps"] = json!(n.dependencies.iter().map(var).collect::<Vec<_>>());
                        values.push(nodes.len());
                        nodes.push(value);
                    }
                }
            }
            functions.push(json!({"function":var(function.variable()),"graph":{"nodes":nodes,"root":values[0]}}));
        }
        Ok(json!({"functions":functions,"next_list_id":self.next_list_id}))
    }
    fn copy_tree(
        &mut self,
        arena: &NativeConstraintArena,
        root: NativeConstraintId,
    ) -> Result<PreparedNodeId, String> {
        enum Work {
            Enter(NativeConstraintId),
            Exit(NativeConstraintId, usize),
        }
        let mut work = vec![Work::Enter(root)];
        let mut values = Vec::new();
        while let Some(task) = work.pop() {
            match task {
                Work::Enter(source) => {
                    let node = arena.node(source).ok_or("foreign construction identity")?;
                    let children = match node {
                        NativeConstraint::Group { children, .. } => children.as_slice(),
                        _ => &[],
                    };
                    work.push(Work::Exit(source, children.len()));
                    for child in children.iter().rev() {
                        work.push(Work::Enter(*child));
                    }
                }
                Work::Exit(source, count) => {
                    let children = values.split_off(values.len() - count);
                    let id = PreparedNodeId(self.nodes.len());
                    self.nodes.push(PreparedNode {
                        source,
                        children,
                        dependencies: arena
                            .dependencies(source)
                            .map_err(|e| format!("{e:?}"))?
                            .iter()
                            .cloned()
                            .collect(),
                        masks: None,
                        list_id: None,
                    });
                    values.push(id);
                }
            }
        }
        Ok(values.pop().unwrap())
    }
    fn rewrite(
        &mut self,
        arena: &mut NativeConstraintArena,
        root: PreparedNodeId,
        phase: Phase,
        copying: bool,
    ) -> Result<PreparedNodeId, String> {
        enum Work {
            Enter(PreparedNodeId),
            Group(PreparedNodeId, usize, usize),
            Reference(PreparedNodeId, FunctionVariable, BTreeSet<VariableId>),
        }
        let mut work = vec![Work::Enter(root)];
        let mut values = Vec::new();
        let mut active = BTreeSet::new();
        while let Some(task) = work.pop() {
            match task {
                Work::Enter(id) => {
                    let node = &self.nodes[id.0];
                    if matches!(phase, Phase::Order) && node.masks.is_some() {
                        values.push(id);
                        continue;
                    }
                    match arena.node(node.source).ok_or("foreign source identity")? {
                        NativeConstraint::Atomic { .. } => values.push(id),
                        NativeConstraint::Reference { function, .. } => {
                            if !active.insert(function.clone()) {
                                return Err("cyclic lexical function references".into());
                            }
                            let body = *self
                                .bodies
                                .get(function)
                                .ok_or("undefined function reference")?;
                            work.push(Work::Reference(
                                id,
                                function.clone(),
                                self.nodes[body.0].dependencies.clone(),
                            ));
                            work.push(Work::Enter(body));
                        }
                        NativeConstraint::Group { kind, .. } => {
                            let mut children = node.children.clone();
                            let mut flat_count = 0;
                            if *kind == GroupKind::Conjunction {
                                let (mut first, mut last): (Vec<_>, Vec<_>) =
                                    children.into_iter().partition(|id| match phase {
                                        Phase::Enumerate => matches!(
                                            arena.node(self.nodes[id.0].source),
                                            Some(NativeConstraint::Atomic { .. })
                                        ),
                                        Phase::Order => !matches!(
                                            arena.node(self.nodes[id.0].source),
                                            Some(NativeConstraint::Reference { .. })
                                        ),
                                    });
                                flat_count = first.len();
                                if matches!(phase, Phase::Order) {
                                    last.reverse();
                                }
                                first.extend(last);
                                children = first;
                            }
                            work.push(Work::Group(id, children.len(), flat_count));
                            for child in children.iter().rev() {
                                work.push(Work::Enter(*child));
                            }
                        }
                    }
                }
                Work::Reference(id, function, old_deps) => {
                    let body = values.pop().unwrap();
                    if matches!(phase, Phase::Order) {
                        self.nodes[body.0].dependencies = old_deps;
                    }
                    self.bodies.insert(function.clone(), body);
                    active.remove(&function);
                    if matches!(phase, Phase::Enumerate) {
                        self.next_list_id += 1;
                    }
                    values.push(id);
                }
                Work::Group(id, count, flat_count) => {
                    let mut children = values.split_off(values.len() - count);
                    // A rewritten group replaces its source node, which nothing
                    // reaches afterwards, so it is updated in place.
                    let target = if copying {
                        let fresh = PreparedNodeId(self.nodes.len());
                        self.nodes.push(self.nodes[id.0].clone());
                        fresh
                    } else {
                        id
                    };
                    if matches!(phase, Phase::Enumerate) {
                        if flat_count > 0 && flat_count < count {
                            let flat: Vec<_> = children.drain(..flat_count).collect();
                            let parent = self.nodes[id.0].source;
                            let (source, dependencies) =
                                self.flat_source(arena, &flat, parent, copying)?;
                            let tmp = PreparedNodeId(self.nodes.len());
                            self.nodes.push(PreparedNode {
                                source,
                                children: flat,
                                dependencies,
                                masks: None,
                                list_id: Some(NativeListId(self.next_list_id)),
                            });
                            self.next_list_id += 1;
                            children.insert(0, tmp);
                        }
                        self.nodes[target.0].list_id = Some(NativeListId(self.next_list_id));
                        self.next_list_id += 1;
                    } else {
                        let mut masks: BTreeMap<VariableId, BTreeSet<usize>> = BTreeMap::new();
                        for (index, child) in children.iter_mut().enumerate() {
                            for key in &self.nodes[child.0].dependencies {
                                masks.entry(key.clone()).or_default().insert(index);
                            }
                            let reset = if copying {
                                let fresh = PreparedNodeId(self.nodes.len());
                                self.nodes.push(self.nodes[child.0].clone());
                                fresh
                            } else {
                                *child
                            };
                            if self.dependency_retention == DependencyRetention::Clear {
                                self.nodes[reset.0].dependencies.clear();
                            }
                            *child = reset;
                        }
                        self.nodes[target.0].masks = Some(masks);
                    }
                    self.nodes[target.0].children = children;
                    values.push(target);
                }
            }
        }
        Ok(values.pop().unwrap())
    }
    /// The source and dependencies of the conjunction over a group's flat
    /// atom prefix. Only the kind of the source is read later, so the parent
    /// conjunction stands in for it unless the arena's grouping could fail,
    /// which only comparing closure environments that are not native can.
    fn flat_source(
        &self,
        arena: &mut NativeConstraintArena,
        flat: &[PreparedNodeId],
        parent: NativeConstraintId,
        copying: bool,
    ) -> Result<(NativeConstraintId, BTreeSet<VariableId>), String> {
        let sources: Vec<_> = flat.iter().map(|n| self.nodes[n.0].source).collect();
        let fallible = sources
            .iter()
            .filter(|id| match arena.node(**id) {
                Some(NativeConstraint::Atomic {
                    right: NativeOperand::Derived(derived),
                    ..
                }) => arena
                    .derived_order_key(*derived)
                    .is_none_or(|key| !key.compares_infallibly()),
                _ => false,
            })
            .count();
        if copying || fallible > 0 {
            let source = arena
                .group(GroupKind::Conjunction, sources)
                .map_err(|e| format!("{e:?}"))?;
            let dependencies = arena
                .dependencies(source)
                .map_err(|e| format!("{e:?}"))?
                .iter()
                .cloned()
                .collect();
            return Ok((source, dependencies));
        }
        let mut all = Vec::new();
        for id in &sources {
            all.extend(arena.dependencies(*id).map_err(|e| format!("{e:?}"))?.iter());
        }
        all.sort();
        all.dedup();
        Ok((parent, all.into_iter().cloned().collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraints::atomic::{AtomicOutcome, BindingKey, BindingMap, Relation};
    use crate::constraints::derived::NoBifProvider;
    use crate::constraints::normalize::NativeOperand;
    use crate::lattice::Type;
    fn f(n: i64) -> FunctionVariable {
        FunctionVariable::new(VariableId::Integer(n.into())).unwrap()
    }
    #[test]
    fn prepared_intake_preserves_native_masks_and_exports_function_only() {
        let mut a = NativeConstraintArena::default();
        let expected = Type::function(Some(vec![]), Type::atom("ok"));
        let local = a
            .atomic(
                Type::variable_integer(1.into()),
                Relation::Equal,
                NativeOperand::Type(Type::atom("local")),
                &NoBifProvider,
            )
            .unwrap();
        let sig = a
            .atomic(
                Type::variable_integer(100.into()),
                Relation::Equal,
                NativeOperand::Type(expected.clone()),
                &NoBifProvider,
            )
            .unwrap();
        let body = a.group(GroupKind::Conjunction, vec![local, sig]).unwrap();
        let p = PreparedConstruction::build(
            &mut a,
            &[FunctionBody {
                function: f(100),
                body,
            }],
            DependencyRetention::Clear,
        )
        .unwrap();
        let root = &p.nodes[p.bodies[&f(100)].0];
        assert_eq!(root.masks.as_ref().unwrap().len(), 2);
        assert!(
            root.children
                .iter()
                .all(|n| p.nodes[n.0].dependencies.is_empty())
        );
        let tree = p
            .into_tree(
                a,
                BTreeMap::from([(
                    f(100),
                    FunctionParameters {
                        arity: 0,
                        recursive_variable: None,
                        propagated_domain: None,
                        self_recursive: false,
                    },
                )]),
                vec![f(100)],
            )
            .unwrap();
        let result = tree.solve(BindingMap::new(), &NoBifProvider).unwrap();
        let AtomicOutcome::Solved { bindings, .. } = result.outcome else {
            panic!()
        };
        assert_eq!(bindings.len(), 1);
        assert_eq!(
            bindings[&BindingKey::Label(VariableId::Integer(100.into()))],
            expected
        );
    }
    #[test]
    fn lexical_reference_cycle_is_rejected_before_execution() {
        let mut a = NativeConstraintArena::default();
        let r = a.reference(f(100), [VariableId::Integer(1.into())].into());
        let body = a.group(GroupKind::Conjunction, vec![r]).unwrap();
        assert!(
            PreparedConstruction::build(
                &mut a,
                &[FunctionBody {
                    function: f(100),
                    body
                }],
                DependencyRetention::Clear,
            )
            .unwrap_err()
            .contains("cyclic")
        );
    }
}

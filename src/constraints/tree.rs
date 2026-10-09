use crate::constraints::atomic::{
    AtomicConstraint, AtomicOutcome, BindingKey, BindingMap, ConstraintValue, SolveError, WriteLog,
};
use crate::constraints::derived::{BifResultProvider, DerivedArena, DerivedId, DerivedOp};
use crate::lattice::{Type, TypePredicate, UnificationValue, VariableId};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConstraintId(pub usize);
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FunctionVariable(VariableId);
impl FunctionVariable {
    pub fn new(id: VariableId) -> Result<Self, String> {
        match id {
            VariableId::Integer(_) => Ok(Self(id)),
            VariableId::Atom(_) => Err("function identity must be an integer Core label".into()),
        }
    }
    pub fn variable(&self) -> &VariableId {
        &self.0
    }
}
#[derive(Clone, Debug)]
pub struct FunctionDefinition {
    pub body: ConstraintId,
    pub arity: usize,
    pub recursive_variable: Option<VariableId>,
    pub propagated_domain: Option<Vec<Type>>,
    pub self_recursive: bool,
}

#[derive(Clone, Debug)]
pub enum Constraint {
    Atomic(AtomicConstraint),
    Reference {
        function: FunctionVariable,
        dependencies: BTreeSet<VariableId>,
    },
    Conjunction(Vec<ConstraintId>),
    Disjunction(Vec<ConstraintId>),
}
#[derive(Clone, Debug)]
pub struct ConstraintNode {
    pub constraint: Constraint,
    pub dependencies: BTreeSet<VariableId>,
    pub masks: BTreeMap<VariableId, BTreeSet<usize>>,
}
#[derive(Clone, Debug, Default)]
pub struct ConstraintTree {
    nodes: Vec<ConstraintNode>,
    functions: BTreeMap<FunctionVariable, FunctionDefinition>,
    derived: DerivedArena,
}
impl ConstraintTree {
    pub fn push_derived(&mut self, op: DerivedOp) -> DerivedId {
        self.derived.push(op)
    }
    pub fn define_function(
        &mut self,
        function: FunctionVariable,
        definition: FunctionDefinition,
    ) -> Result<(), String> {
        if self.functions.contains_key(&function) {
            return Err("function identity already defined".into());
        }
        if definition.body.0 >= self.nodes.len() {
            return Err("function body outside arena".into());
        }
        if definition
            .propagated_domain
            .as_ref()
            .is_some_and(|xs| xs.len() != definition.arity)
        {
            return Err("propagated domain arity mismatch".into());
        }
        if definition
            .recursive_variable
            .as_ref()
            .is_some_and(|v| matches!(v, VariableId::Atom(_)))
        {
            return Err("recursive variable must be an integer Core label".into());
        }
        if definition.self_recursive && definition.recursive_variable.is_none() {
            return Err("self-recursive function has no recursive variable".into());
        }
        self.functions.insert(function, definition);
        Ok(())
    }

    pub fn finish(self, root: ConstraintId) -> Result<PreparedTree, String> {
        if root.0 >= self.nodes.len() {
            return Err("constraint root outside arena".into());
        }
        for node in &self.nodes {
            if let Constraint::Reference {
                function,
                dependencies,
            } = &node.constraint
            {
                let definition = self
                    .functions
                    .get(function)
                    .ok_or("reference function is undefined")?;
                if *dependencies != self.nodes[definition.body.0].dependencies {
                    return Err("reference dependencies differ from function body".into());
                }
            }
        }
        self.finish_structure(root)
    }
    pub(crate) fn admit_prepared(
        nodes: Vec<ConstraintNode>,
        functions: BTreeMap<FunctionVariable, FunctionDefinition>,
        derived: DerivedArena,
        root: ConstraintId,
    ) -> Result<PreparedTree, String> {
        let mut tree = Self {
            nodes,
            functions: BTreeMap::new(),
            derived,
        };
        for (function, definition) in functions {
            tree.define_function(function, definition)?;
        }
        tree.finish_structure(root)
    }
    fn finish_structure(self, root: ConstraintId) -> Result<PreparedTree, String> {
        if root.0 >= self.nodes.len() {
            return Err("constraint root outside arena".into());
        }
        for node in &self.nodes {
            if let Constraint::Reference { function, .. } = &node.constraint
                && !self.functions.contains_key(function)
            {
                return Err("reference function is undefined".into());
            }
        }
        let mut visited = BTreeSet::new();
        let mut tasks = vec![root];
        tasks.extend(self.functions.values().map(|f| f.body));
        while let Some(id) = tasks.pop() {
            if matches!(
                self.nodes[id.0].constraint,
                Constraint::Conjunction(_) | Constraint::Disjunction(_)
            ) {
                if !visited.insert(id) {
                    return Err("constraint scope has multiple owners; native enumeration requires distinct scope identities".into());
                }
                tasks.extend(children(&self.nodes[id.0].constraint));
            }
        }
        // Native reference nesting is lexical, not recursive call dependency.
        let mut graph = BTreeMap::new();
        for (function, definition) in &self.functions {
            let mut refs = BTreeSet::new();
            let mut tasks = vec![definition.body];
            while let Some(id) = tasks.pop() {
                match &self.nodes[id.0].constraint {
                    Constraint::Reference { function, .. } => {
                        refs.insert(function.clone());
                    }
                    Constraint::Conjunction(xs) | Constraint::Disjunction(xs) => tasks.extend(xs),
                    Constraint::Atomic(_) => (),
                }
            }
            graph.insert(function.clone(), refs);
        }
        let mut users: BTreeMap<FunctionVariable, BTreeSet<FunctionVariable>> = BTreeMap::new();
        for (function, refs) in &graph {
            for dependency in refs {
                users
                    .entry(dependency.clone())
                    .or_default()
                    .insert(function.clone());
            }
        }
        let mut ready: BTreeSet<_> = graph
            .iter()
            .filter(|(_, refs)| refs.is_empty())
            .map(|(f, _)| f.clone())
            .collect();
        let mut removed = 0;
        while let Some(function) = ready.pop_first() {
            removed += 1;
            if let Some(callers) = users.get(&function) {
                for caller in callers {
                    let refs = graph.get_mut(caller).expect("validated reference target");
                    refs.remove(&function);
                    if refs.is_empty() {
                        ready.insert(caller.clone());
                    }
                }
            }
        }
        if removed != graph.len() {
            return Err("lexical function reference cycle".into());
        }
        Ok(PreparedTree { tree: self, root })
    }
    pub fn push(&mut self, constraint: Constraint) -> Result<ConstraintId, String> {
        let mut dependencies = BTreeSet::new();
        let mut masks: BTreeMap<VariableId, BTreeSet<usize>> = BTreeMap::new();
        match &constraint {
            Constraint::Reference {
                dependencies: deps, ..
            } => dependencies.extend(deps.iter().cloned()),
            Constraint::Atomic(a) => {
                dependencies.extend(a.left.collect_var_names());
                match &a.right {
                    ConstraintValue::Type(ty) => dependencies.extend(ty.collect_var_names()),
                    ConstraintValue::Derived(id) => dependencies.extend(
                        self.derived
                            .dependencies(*id)
                            .ok_or("derived rhs must reference an earlier derived expression")?
                            .iter()
                            .cloned(),
                    ),
                }
            }
            Constraint::Conjunction(children) | Constraint::Disjunction(children) => {
                let mut atomic = None;
                for (index, id) in children.iter().enumerate() {
                    let child = self
                        .nodes
                        .get(id.0)
                        .ok_or("constraint child must reference earlier node")?;
                    let is_atomic = matches!(child.constraint, Constraint::Atomic(_));
                    if matches!(constraint, Constraint::Disjunction(_))
                        && !matches!(
                            child.constraint,
                            Constraint::Conjunction(_)
                                | Constraint::Disjunction(_)
                                | Constraint::Reference { .. }
                        )
                    {
                        return Err("native disjunction children must be scoped lists or function references".into());
                    }
                    if matches!(constraint, Constraint::Conjunction(_)) {
                        if atomic.is_some_and(|old| old != is_atomic) {
                            return Err("native conjunction must contain only atoms or only scopes after enumeration".into());
                        }
                        atomic = Some(is_atomic);
                    }
                    dependencies.extend(child.dependencies.iter().cloned());
                    for v in &child.dependencies {
                        masks.entry(v.clone()).or_default().insert(index);
                    }
                }
            }
        }
        if dependencies
            .iter()
            .any(|id| matches!(id, VariableId::Atom(_)))
        {
            return Err("native constraint trees require integer Core variable labels".into());
        }
        let id = ConstraintId(self.nodes.len());
        self.nodes.push(ConstraintNode {
            constraint,
            dependencies,
            masks,
        });
        Ok(id)
    }
    fn solve(
        &self,
        root: ConstraintId,
        map: BindingMap,
        provider: &dyn BifResultProvider,
    ) -> Result<TreeOutcome, SolveError> {
        let mut state = State {
            tree: self,
            cache: BTreeMap::new(),
            function_cache: BTreeMap::new(),
            detected_loops: Vec::new(),
        };
        let mut frames = vec![Frame::Enter(root, map)];
        let mut result = None;
        while let Some(frame) = frames.pop() {
            match frame {
                Frame::Enter(id, map) => match &self.nodes[id.0].constraint {
                    Constraint::Atomic(a) => {
                        result = Some(a.solve(map, &self.derived, provider)?)
                    }
                    Constraint::Reference { function, .. } => {
                        let definition = &self.functions[function];
                        let body_map = restore_entry(state.function_cache.get(function), &map);
                        frames.push(Frame::ReferenceReturn(function.clone(), map));
                        if definition.self_recursive {
                            frames.push(Frame::Recursive(Recursive {
                                function: function.clone(),
                                initial_map: body_map,
                                initial_cache: state.cache.clone(),
                                initial_function_cache: state.function_cache.clone(),
                                approximation: Type::none(),
                            }));
                        } else {
                            frames.push(Frame::Enter(definition.body, body_map));
                        }
                    }
                    Constraint::Conjunction(children) => {
                        let Some(pending) = state.flags(id) else {
                            result = Some(AtomicOutcome::Failed);
                            continue;
                        };
                        let flat = children.first().is_some_and(|id| {
                            matches!(self.nodes[id.0].constraint, Constraint::Atomic(_))
                        });
                        frames.push(Frame::Conj(
                            Conj {
                                id,
                                snapshot: Snapshot::new(flat, &map),
                                last_flags: pending.identity,
                                flags: pending.indices,
                                restart: BTreeSet::new(),
                                updated: BTreeSet::new(),
                                flat,
                            },
                            map,
                        ));
                    }
                    Constraint::Disjunction(_) => {
                        let Some(pending) = state.flags(id) else {
                            result = Some(AtomicOutcome::Failed);
                            continue;
                        };
                        frames.push(Frame::Disj(Disj {
                            id,
                            map,
                            flags: pending.indices,
                            index: 0,
                            evaluated_maps: Vec::new(),
                            unevaluated_maps: Vec::new(),
                            updated: BTreeSet::new(),
                            survived: BTreeSet::new(),
                            newly_failed: false,
                        }));
                    }
                },
                Frame::ReferenceReturn(function, map) => {
                    let definition = &self.functions[&function];
                    let fun_type = match result.take().expect("function body result") {
                        AtomicOutcome::Failed => Type::function(
                            Some(
                                definition
                                    .propagated_domain
                                    .clone()
                                    .unwrap_or_else(|| vec![Type::any(); definition.arity]),
                            ),
                            Type::none(),
                        ),
                        AtomicOutcome::Solved { bindings, updated } => {
                            let ty =
                                lookup(&bindings, &BindingKey::Label(function.variable().clone()));
                            save_entry(
                                state
                                    .function_cache
                                    .entry(function.clone())
                                    .or_insert_with(empty_cache),
                                &updated,
                                &bindings,
                            );
                            ty
                        }
                    };
                    let (map, updated) = enter(function.variable(), fun_type.clone(), map)?;
                    let reference_outcome = match &definition.recursive_variable {
                        Some(variable) => crate::constraints::atomic::refine_values(
                            [(
                                BindingKey::RawVariableRecord(variable.clone()),
                                UnificationValue::Type(fun_type),
                            )],
                            map,
                            updated,
                        )?,
                        None => AtomicOutcome::Solved {
                            bindings: map,
                            updated,
                        },
                    };
                    if matches!(reference_outcome, AtomicOutcome::Failed) {
                        return Err(SolveError::Native(
                            crate::lattice::LatticeError::BadMatchNative(Box::new(
                                crate::lattice::NativeValue::Atom("error".into()),
                            )),
                        ));
                    }
                    result = Some(reference_outcome);
                }
                Frame::Recursive(rec) => {
                    let definition = &self.functions[&rec.function];
                    state.cache = rec.initial_cache.clone();
                    state.function_cache = rec.initial_function_cache.clone();
                    let (map, updated) = enter(
                        definition
                            .recursive_variable
                            .as_ref()
                            .expect("validated recursive variable"),
                        rec.approximation.limit(4)?,
                        rec.initial_map.clone(),
                    )?;
                    state.invalidate(definition.body, updated);
                    frames.push(Frame::RecursiveReturn(rec));
                    frames.push(Frame::Enter(definition.body, map));
                }
                Frame::RecursiveReturn(mut rec) => {
                    let definition = &self.functions[&rec.function];
                    match result.take().expect("recursive body result") {
                        AtomicOutcome::Failed
                            if rec.approximation.predicate(TypePredicate::None)? =>
                        {
                            rec.approximation = Type::function(
                                Some(vec![Type::any(); definition.arity]),
                                Type::unit(),
                            );
                            frames.push(Frame::Recursive(rec));
                        }
                        AtomicOutcome::Failed => result = Some(AtomicOutcome::Failed),
                        AtomicOutcome::Solved {
                            bindings,
                            mut updated,
                        } => {
                            let inferred = bindings
                                .get(&BindingKey::Label(rec.function.variable().clone()))
                                .cloned()
                                .unwrap_or_else(Type::none);
                            if inferred == rec.approximation {
                                let (bindings, changed) = enter(
                                    definition
                                        .recursive_variable
                                        .as_ref()
                                        .expect("validated recursive variable"),
                                    inferred,
                                    bindings,
                                )?;
                                updated.extend(changed);
                                result = Some(AtomicOutcome::Solved { bindings, updated });
                            } else {
                                rec.approximation = inferred;
                                frames.push(Frame::Recursive(rec));
                            }
                        }
                    }
                }
                Frame::Conj(mut c, map) => {
                    let children = children(&self.nodes[c.id.0].constraint);
                    if let Some(index) = c.flags.pop_first() {
                        let child = children[index];
                        if let Some(log) = &mut c.snapshot.log {
                            let Constraint::Atomic(a) = &self.nodes[child.0].constraint else {
                                unreachable!("flat conjunction holds atoms")
                            };
                            result =
                                Some(a.solve_logged(map, &self.derived, provider, Some(log))?);
                            frames.push(Frame::ConjReturn(c, index));
                        } else {
                            frames.push(Frame::ConjReturn(c, index));
                            frames.push(Frame::Enter(child, map));
                        }
                    } else if c.restart.is_empty() {
                        result = Some(AtomicOutcome::Solved {
                            bindings: map,
                            updated: c.updated,
                        });
                    } else if Some(&c.restart) == c.last_flags.as_ref()
                        && c.snapshot.unchanged(&map)
                    {
                        state.detected_loops.push(c.id);
                        result = Some(AtomicOutcome::Solved {
                            bindings: map,
                            updated: c.updated,
                        });
                    } else {
                        c.last_flags = Some(c.restart.clone());
                        c.snapshot.reset(&map);
                        c.flags = std::mem::take(&mut c.restart);
                        frames.push(Frame::Conj(c, map));
                    }
                }
                Frame::ConjReturn(mut c, index) => match result.take().expect("child result") {
                    AtomicOutcome::Failed => {
                        state.cache.insert(c.id, Cache::Failed);
                        result = Some(AtomicOutcome::Failed);
                    }
                    AtomicOutcome::Solved { bindings, updated } => {
                        let masks = &self.nodes[c.id.0].masks;
                        if c.flat {
                            for var in &updated {
                                if let Some(indices) = var.label().and_then(|v| masks.get(v)) {
                                    for &i in indices {
                                        if i < index {
                                            c.restart.insert(i);
                                        } else if i > index {
                                            c.flags.insert(i);
                                        }
                                    }
                                }
                            }
                        } else {
                            let mut affected: BTreeMap<usize, BTreeSet<BindingKey>> =
                                BTreeMap::new();
                            for var in &updated {
                                if let Some(indices) = var.label().and_then(|v| masks.get(v)) {
                                    for &i in indices {
                                        if i != index {
                                            affected.entry(i).or_default().insert(var.clone());
                                        }
                                    }
                                }
                            }
                            for (i, u) in affected {
                                let child = children(&self.nodes[c.id.0].constraint)[i];
                                state.invalidate(child, u);
                                if i < index {
                                    c.restart.insert(i);
                                } else {
                                    c.flags.insert(i);
                                }
                            }
                        }
                        if c.updated.is_empty() {
                            c.updated = updated;
                        } else {
                            c.updated.extend(updated);
                        }
                        frames.push(Frame::Conj(c, bindings));
                    }
                },
                Frame::Disj(mut d) => {
                    let children = children(&self.nodes[d.id.0].constraint);
                    if d.index < children.len() {
                        let i = d.index;
                        d.index += 1;
                        let child = children[i];
                        if d.flags.contains(&i) {
                            let map = state.restore(child, &d.map);
                            let was_failed = matches!(state.cache.get(&child), Some(Cache::Failed));
                            frames.push(Frame::DisjReturn(d, child, i, was_failed));
                            frames.push(Frame::Enter(child, map));
                        } else {
                            if !matches!(state.cache.get(&child), Some(Cache::Failed)) {
                                d.unevaluated_maps.push(state.restore(child, &d.map));
                                d.survived.insert(i);
                            }
                            frames.push(Frame::Disj(d));
                        }
                    } else if d.evaluated_maps.is_empty() && d.unevaluated_maps.is_empty() {
                        state.cache.insert(d.id, Cache::Failed);
                        result = Some(AtomicOutcome::Failed);
                    } else {
                        let maps: Vec<_> = d
                            .unevaluated_maps
                            .iter()
                            .chain(d.evaluated_maps.iter().rev())
                            .collect();
                        let mask = &self.nodes[d.id.0].masks;
                        let keys: Vec<_> = if d.newly_failed {
                            // Only labels in the mask survive the filter, so
                            // intersect the branch maps over the mask instead
                            // of over every SCC binding.
                            let keys = mask
                                .iter()
                                .filter(|(_, indices)| d.survived.is_subset(indices))
                                .map(|(v, _)| BindingKey::Label(v.clone()))
                                .filter(|k| maps.iter().all(|map| map.contains_key(k)))
                                .collect();
                            if crate::lattice::verify_fast_paths() {
                                let mut old: BTreeSet<_> = maps[0].keys().collect();
                                for map in &maps[1..] {
                                    old.retain(|k| map.contains_key(k));
                                }
                                let old: Vec<_> = old
                                    .into_iter()
                                    .filter(|v| {
                                        v.label()
                                            .and_then(|v| mask.get(v))
                                            .is_some_and(|indices| d.survived.is_subset(indices))
                                    })
                                    .collect();
                                assert_eq!(keys, old, "disjunction key intersection fast path");
                            }
                            keys
                        } else {
                            d.updated
                                .into_iter()
                                .filter(|v| {
                                    v.label()
                                        .and_then(|v| mask.get(v))
                                        .is_some_and(|indices| d.survived.is_subset(indices))
                                })
                                .collect()
                        };
                        let mut map = d.map.clone();
                        let mut updated = BTreeSet::new();
                        for key in keys {
                            let mut joined = lookup(maps[0], &key);
                            for branch in &maps[1..] {
                                if joined.predicate(TypePredicate::Any)? {
                                    break;
                                }
                                joined = joined.join(&lookup(branch, &key))?;
                            }
                            if joined == Type::any() {
                                if map.contains_key(&key) {
                                    map.remove(&key);
                                }
                            } else {
                                let limited = joined.limit(5)?;
                                if map.get(&key) != Some(&limited) {
                                    map.insert(key.clone(), limited);
                                }
                            }
                            if lookup(&map, &key) != lookup(&d.map, &key) {
                                updated.insert(key);
                            }
                        }
                        map.verify_shadow();
                        result = Some(AtomicOutcome::Solved {
                            bindings: map,
                            updated,
                        });
                    }
                }
                Frame::DisjReturn(mut d, child, index, was_failed) => {
                    match result.take().expect("branch result") {
                        AtomicOutcome::Failed => {
                            d.newly_failed |= !was_failed;
                        }
                        AtomicOutcome::Solved { bindings, updated } => {
                            state.save(child, &updated, &bindings);
                            d.evaluated_maps.push(bindings);
                            d.survived.insert(index);
                            d.updated.extend(updated);
                        }
                    }
                    frames.push(Frame::Disj(d));
                }
            }
        }
        if let Some(AtomicOutcome::Solved { bindings, .. }) = &result {
            bindings.verify_shadow();
        }
        Ok(TreeOutcome {
            outcome: result.expect("root result"),
            detected_loops: state.detected_loops,
        })
    }
}
fn children(c: &Constraint) -> &[ConstraintId] {
    match c {
        Constraint::Conjunction(xs) | Constraint::Disjunction(xs) => xs,
        Constraint::Atomic(_) | Constraint::Reference { .. } => {
            unreachable!("scope frame references list")
        }
    }
}
fn lookup(map: &BindingMap, key: &BindingKey) -> Type {
    map.get(key).cloned().unwrap_or_else(Type::any)
}
#[derive(Clone, Debug)]
pub struct TreeOutcome {
    pub outcome: AtomicOutcome,
    pub detected_loops: Vec<ConstraintId>,
}
#[derive(Clone, Debug)]
enum Cache {
    Failed,
    Active {
        local: BindingMap,
        dirty: BTreeSet<BindingKey>,
    },
}
struct Pending {
    indices: BTreeSet<usize>,
    identity: Option<BTreeSet<usize>>,
}
struct State<'a> {
    tree: &'a ConstraintTree,
    cache: BTreeMap<ConstraintId, Cache>,
    function_cache: BTreeMap<FunctionVariable, Cache>,
    detected_loops: Vec<ConstraintId>,
}
impl State<'_> {
    fn flags(&mut self, id: ConstraintId) -> Option<Pending> {
        match self.cache.get_mut(&id) {
            Some(Cache::Failed) => None,
            Some(Cache::Active { dirty, .. }) => {
                let u = std::mem::take(dirty);
                let masks = &self.tree.nodes[id.0].masks;
                let mut affected: BTreeMap<usize, BTreeSet<BindingKey>> = BTreeMap::new();
                for variable in u {
                    if let Some(indices) = variable.label().and_then(|v| masks.get(v)) {
                        for &index in indices {
                            affected.entry(index).or_default().insert(variable.clone());
                        }
                    }
                }
                let flags: BTreeSet<usize> = affected.keys().copied().collect();
                for (index, variables) in affected {
                    let child = children(&self.tree.nodes[id.0].constraint)[index];
                    self.invalidate(child, variables);
                }
                Some(Pending {
                    identity: Some(flags.clone()),
                    indices: flags,
                })
            }
            None => {
                self.cache.insert(
                    id,
                    Cache::Active {
                        local: BindingMap::new(),
                        dirty: BTreeSet::new(),
                    },
                );
                Some(Pending {
                    indices: (0..children(&self.tree.nodes[id.0].constraint).len()).collect(),
                    identity: None,
                })
            }
        }
    }
    fn invalidate(&mut self, mut id: ConstraintId, updated: BTreeSet<BindingKey>) {
        if let Constraint::Reference { function, .. } = &self.tree.nodes[id.0].constraint {
            id = self.tree.functions[function].body;
        }
        if let Some(Cache::Active { dirty, .. }) = self.cache.get_mut(&id) {
            dirty.extend(updated);
        }
    }
    fn restore(&self, id: ConstraintId, map: &BindingMap) -> BindingMap {
        let mut out = map.clone();
        if let Some(Cache::Active { local, dirty }) = self.cache.get(&id) {
            for (key, value) in local.iter() {
                if !dirty.contains(&key) && out.get(&key) != Some(value) {
                    out.insert(key, value.clone());
                }
            }
        }
        out
    }
    fn save(&mut self, id: ConstraintId, updated: &BTreeSet<BindingKey>, map: &BindingMap) {
        let entry = self.cache.entry(id).or_insert_with(|| Cache::Active {
            local: BindingMap::new(),
            dirty: BTreeSet::new(),
        });
        if let Cache::Active { local, dirty } = entry {
            for key in updated {
                if let Some(value) = map.get(key) {
                    local.insert(key.clone(), value.clone());
                }
            }
            dirty.clear();
        }
    }
}
struct Conj {
    id: ConstraintId,
    snapshot: Snapshot,
    last_flags: Option<BTreeSet<usize>>,
    flags: BTreeSet<usize>,
    restart: BTreeSet<usize>,
    updated: BTreeSet<BindingKey>,
    flat: bool,
}
/// The map a conjunction last (re)started from, compared to detect a loop.
/// Atom lists log the old value of each binding they write instead of holding
/// a copy, so their writes don't have to copy nodes shared with the copy.
struct Snapshot {
    map: Option<BindingMap>,
    log: Option<WriteLog>,
}
impl Snapshot {
    fn new(flat: bool, map: &BindingMap) -> Self {
        let verify = crate::lattice::verify_fast_paths();
        Self {
            map: (!flat || verify).then(|| map.clone()),
            log: flat.then(Vec::new),
        }
    }
    fn reset(&mut self, map: &BindingMap) {
        if let Some(log) = &mut self.log {
            log.clear();
        }
        if let Some(old) = &mut self.map {
            *old = map.clone();
        }
    }
    fn unchanged(&self, map: &BindingMap) -> bool {
        let Some(log) = &self.log else {
            return self.map.as_ref() == Some(map);
        };
        let mut seen = BTreeSet::new();
        let unchanged = log
            .iter()
            .all(|(key, old)| !seen.insert(key) || map.get(key) == old.as_ref());
        if let Some(old) = &self.map {
            assert_eq!(unchanged, old == map, "conjunction write log fast path");
        }
        unchanged
    }
}
struct Disj {
    id: ConstraintId,
    map: BindingMap,
    flags: BTreeSet<usize>,
    index: usize,
    evaluated_maps: Vec<BindingMap>,
    unevaluated_maps: Vec<BindingMap>,
    updated: BTreeSet<BindingKey>,
    survived: BTreeSet<usize>,
    newly_failed: bool,
}
struct Recursive {
    function: FunctionVariable,
    initial_map: BindingMap,
    initial_cache: BTreeMap<ConstraintId, Cache>,
    initial_function_cache: BTreeMap<FunctionVariable, Cache>,
    approximation: Type,
}
enum Frame {
    ReferenceReturn(FunctionVariable, BindingMap),
    Recursive(Recursive),
    RecursiveReturn(Recursive),
    Enter(ConstraintId, BindingMap),
    Conj(Conj, BindingMap),
    ConjReturn(Conj, usize),
    Disj(Disj),
    DisjReturn(Disj, ConstraintId, usize, bool),
}

pub struct PreparedTree {
    tree: ConstraintTree,
    root: ConstraintId,
}
impl PreparedTree {
    pub fn solve(
        &self,
        map: BindingMap,
        provider: &dyn BifResultProvider,
    ) -> Result<TreeOutcome, SolveError> {
        self.tree.solve(self.root, map, provider)
    }
}

fn empty_cache() -> Cache {
    Cache::Active {
        local: BindingMap::new(),
        dirty: BTreeSet::new(),
    }
}
fn restore_entry(entry: Option<&Cache>, map: &BindingMap) -> BindingMap {
    let mut out = map.clone();
    if let Some(Cache::Active { local, dirty }) = entry {
        for (key, value) in local.iter() {
            if !dirty.contains(&key) && out.get(&key) != Some(value) {
                out.insert(key, value.clone());
            }
        }
    }
    out
}
fn save_entry(entry: &mut Cache, updated: &BTreeSet<BindingKey>, map: &BindingMap) {
    if let Cache::Active { local, dirty } = entry {
        for key in updated {
            if let Some(value) = map.get(key) {
                local.insert(key.clone(), value.clone());
            }
        }
        dirty.clear();
    }
}
fn enter(
    key: &VariableId,
    value: Type,
    mut map: BindingMap,
) -> Result<(BindingMap, BTreeSet<BindingKey>), SolveError> {
    let key = BindingKey::Label(key.clone());
    let changed = if value == Type::any() {
        if map.contains_key(&key) {
            map.remove(&key);
            true
        } else {
            false
        }
    } else {
        let limited = value.limit(5)?;
        if map.get(&key) == Some(&limited) {
            false
        } else {
            map.insert(key.clone(), limited);
            true
        }
    };
    let mut updated = BTreeSet::new();
    if changed {
        updated.insert(key.clone());
    }
    Ok((map, updated))
}

/// One native multi-function SCC. Function references form lexical ownership;
/// recursive call dependencies come from body occurrences of recursive names.
pub struct PreparedScc {
    tree: PreparedTree,
    functions: Vec<FunctionVariable>,
    references: BTreeMap<FunctionVariable, ConstraintId>,
    users: BTreeMap<VariableId, BTreeSet<FunctionVariable>>,
}
#[derive(Clone, Debug)]
pub struct SccOutcome {
    pub bindings: BindingMap,
    pub iterations: usize,
    pub tried_unit: bool,
}
impl PreparedTree {
    pub fn prepare_scc(self, functions: Vec<FunctionVariable>) -> Result<PreparedScc, String> {
        if functions.len() < 2 {
            return Err("native multi-function SCC requires at least two functions".into());
        }
        if functions.windows(2).any(|p| p[0] >= p[1]) {
            return Err("SCC functions must be unique and in native label order".into());
        }
        let mut references = BTreeMap::new();
        for (i, node) in self.tree.nodes.iter().enumerate() {
            if let Constraint::Reference { function, .. } = &node.constraint {
                references
                    .entry(function.clone())
                    .or_insert(ConstraintId(i));
            }
        }
        let mut recursive_names = BTreeSet::new();
        for function in &functions {
            let definition = self
                .tree
                .functions
                .get(function)
                .ok_or("SCC function undefined")?;
            if !references.contains_key(function) {
                return Err("SCC function has no reference entry".into());
            }
            if let Some(variable) = &definition.recursive_variable {
                recursive_names.insert(variable.clone());
            }
        }
        let mut users: BTreeMap<VariableId, BTreeSet<FunctionVariable>> = BTreeMap::new();
        for function in &functions {
            let definition = &self.tree.functions[function];
            for variable in self.tree.nodes[definition.body.0]
                .dependencies
                .intersection(&recursive_names)
            {
                users
                    .entry(variable.clone())
                    .or_default()
                    .insert(function.clone());
            }
        }
        Ok(PreparedScc {
            tree: self,
            functions,
            references,
            users,
        })
    }
}
impl PreparedScc {
    pub fn solve(
        &self,
        mut map: BindingMap,
        provider: &dyn BifResultProvider,
    ) -> Result<SccOutcome, SolveError> {
        use crate::lattice::{Known, TypeGetter, TypeValue};
        let recursive: Vec<_> = self
            .functions
            .iter()
            .filter_map(|f| {
                self.tree.tree.functions[f]
                    .recursive_variable
                    .as_ref()
                    .map(|v| (f, v))
            })
            .collect();
        let mut to_solve = self.functions.clone();
        let mut tried_unit = false;
        let mut iterations = 0;
        loop {
            iterations += 1;
            let previous = map.clone();
            let approximants = recursive
                .iter()
                .map(|(f, _)| {
                    previous
                        .get(&BindingKey::Label(f.variable().clone()))
                        .cloned()
                        .unwrap_or_else(Type::none)
                        .limit(4)
                })
                .collect::<Result<Vec<_>, _>>()?;
            for f in &to_solve {
                map.remove(&BindingKey::Label(f.variable().clone()));
            }
            for ((_, variable), ty) in recursive.iter().zip(approximants) {
                map = enter(variable, ty, map)?.0;
            }
            for function in &to_solve {
                let solved =
                    self.tree
                        .tree
                        .solve(self.references[function], map.clone(), provider)?;
                let AtomicOutcome::Solved { bindings, .. } = solved.outcome else {
                    unreachable!("native function reference cannot fail")
                };
                let inferred = bindings
                    .get(&BindingKey::Label(function.variable().clone()))
                    .cloned()
                    .unwrap_or_else(Type::none)
                    .limit(4)?;
                map = enter(function.variable(), inferred.clone(), map)?.0;
                if let Some(variable) = &self.tree.tree.functions[function].recursive_variable {
                    map = enter(variable, inferred, map)?.0;
                }
            }
            let changed: Vec<_> = recursive
                .iter()
                .filter_map(|(_, v)| {
                    let key = BindingKey::Label((*v).clone());
                    (lookup(&previous, &key) != lookup(&map, &key)).then_some(*v)
                })
                .collect();
            if changed.is_empty() {
                let mut ranges = Vec::new();
                for (f, _) in &recursive {
                    let ty = map
                        .get(&BindingKey::Label(f.variable().clone()))
                        .cloned()
                        .unwrap_or_else(Type::none);
                    let TypeValue::Type(range) = ty.get(TypeGetter::FunctionRange)? else {
                        unreachable!("typed function range getter")
                    };
                    ranges.push((f, ty, range));
                }
                let any_none = ranges
                    .iter()
                    .map(|(_, _, range)| range.predicate(TypePredicate::None))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .any(|v| v);
                if any_none && !tried_unit {
                    for (f, ty, range) in ranges {
                        if range.predicate(TypePredicate::None)? {
                            let TypeValue::Types(domain) = ty.get(TypeGetter::FunctionArgs)? else {
                                unreachable!("typed function domain getter")
                            };
                            let domain = match domain {
                                Known::Known(args) => Some(args),
                                Known::Unknown => None,
                            };
                            map = enter(f.variable(), Type::function(domain, Type::unit()), map)?.0;
                        }
                    }
                    tried_unit = true;
                    to_solve = self.functions.clone();
                } else {
                    map.verify_shadow();
                    return Ok(SccOutcome {
                        bindings: map,
                        iterations,
                        tried_unit,
                    });
                }
            } else {
                let mut affected = BTreeSet::new();
                for variable in changed {
                    if let Some(users) = self.users.get(variable) {
                        affected.extend(users.iter().cloned());
                    }
                }
                to_solve = affected.into_iter().collect();
            }
        }
    }
}

#[cfg(test)]
mod unchanged_binding_mutation_tests {
    use super::*;
    fn key(n: i64) -> BindingKey {
        BindingKey::Label(VariableId::Integer(n.into()))
    }
    #[test]
    fn enter_preserves_explicit_any_removal_update_flags() {
        let variable = VariableId::Integer(1.into());
        let input = [(key(1), Type::any())].into_iter().collect();
        let (map, updated) = enter(&variable, Type::any(), input).unwrap();
        assert!(map.is_empty());
        assert_eq!(updated, [key(1)].into());
        let (map, updated) = enter(&variable, Type::any(), BindingMap::new()).unwrap();
        assert!(map.is_empty());
        assert!(updated.is_empty());
    }
    #[test]
    fn unchanged_cached_values_preserve_shared_root() {
        let mut input = BindingMap::new();
        for n in 0..300 {
            input.insert(key(n), Type::atom("payload"));
        }
        let cache = Cache::Active {
            local: input.clone(),
            dirty: BTreeSet::new(),
        };
        let restored = restore_entry(Some(&cache), &input);
        assert!(restored.ptr_eq(&input));
        assert_eq!(restored, input);
    }
    #[test]
    fn cached_any_is_inserted_into_absence_unless_dirty() {
        let cache = Cache::Active {
            local: [(key(1), Type::any())].into_iter().collect(),
            dirty: BTreeSet::new(),
        };
        let restored = restore_entry(Some(&cache), &BindingMap::new());
        assert_eq!(restored.get(&key(1)), Some(&Type::any()));
        let cache = Cache::Active {
            local: [(key(1), Type::any())].into_iter().collect(),
            dirty: [key(1)].into(),
        };
        assert!(restore_entry(Some(&cache), &BindingMap::new()).is_empty());
    }
}

use crate::constraints::atomic::{
    AtomicConstraint, AtomicOutcome, AtomicStatus, BindingKey, BindingMap, ConstraintValue, KeySet,
    SolveError, WriteLog,
};
use crate::constraints::bindings::{BindingAccess, ReadView};
use crate::constraints::derived::{BifResultProvider, DerivedArena, DerivedId, DerivedOp};
use crate::constraints::normalize::Dependencies;
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
        dependencies: Dependencies,
    },
    Conjunction(Vec<ConstraintId>),
    Disjunction(Vec<ConstraintId>),
}
#[derive(Clone, Debug)]
pub struct ConstraintNode {
    pub constraint: Constraint,
    /// Sorted and without duplicates.
    pub dependencies: Dependencies,
    pub masks: Masks,
}
/// Child positions per variable, sorted by variable, each position list
/// ascending: the solver's lookup table from an updated variable to the
/// children that read it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Masks {
    /// Each variable with the end of its run in `indices`.
    keys: Vec<(VariableId, u32)>,
    indices: Vec<u32>,
    /// The keys as machine integers, when they all are: searched instead.
    small: Option<Vec<i64>>,
}
impl Masks {
    /// The masks of a group whose children have the given dependencies.
    pub fn from_children<'a>(children: impl IntoIterator<Item = &'a [VariableId]>) -> Self {
        let mut pairs: Vec<(&VariableId, u32)> = Vec::new();
        for (index, deps) in children.into_iter().enumerate() {
            pairs.extend(deps.iter().map(|v| (v, index as u32)));
        }
        // Stable by variable: positions stay ascending within a variable.
        pairs.sort_by(|a, b| a.0.cmp(b.0));
        let mut keys: Vec<(VariableId, u32)> = Vec::new();
        let mut indices = Vec::with_capacity(pairs.len());
        for (v, index) in pairs {
            match keys.last_mut() {
                Some((last, end)) if last == v => *end += 1,
                _ => keys.push((v.clone(), indices.len() as u32 + 1)),
            }
            indices.push(index);
        }
        let small = keys
            .iter()
            .map(|(v, _)| match v {
                VariableId::Integer(n) => n.small(),
                VariableId::Atom(_) => None,
            })
            .collect();
        Self {
            keys,
            indices,
            small,
        }
    }
    pub fn get(&self, v: &VariableId) -> Option<&[u32]> {
        let at = match &self.small {
            // Small integers sort as machine integers, and below atoms.
            Some(small) => match v {
                VariableId::Integer(n) => small.binary_search(&n.small()?).ok()?,
                VariableId::Atom(_) => return None,
            },
            None => self.keys.binary_search_by(|(k, _)| k.cmp(v)).ok()?,
        };
        if crate::lattice::verify_fast_paths() {
            assert_eq!(
                Some(at),
                self.keys.binary_search_by(|(k, _)| k.cmp(v)).ok(),
                "mask lookup fast path"
            );
        }
        Some(self.run(at))
    }
    fn run(&self, at: usize) -> &[u32] {
        let start = if at == 0 { 0 } else { self.keys[at - 1].1 };
        &self.indices[start as usize..self.keys[at].1 as usize]
    }
    pub fn iter(&self) -> impl Iterator<Item = (&VariableId, &[u32])> + '_ {
        (0..self.keys.len()).map(|at| (&self.keys[at].0, self.run(at)))
    }
    pub fn len(&self) -> usize {
        self.keys.len()
    }
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}
/// A set of child positions. The first 64 live inline.
#[derive(Clone, Debug, Default)]
struct Bits {
    first: u64,
    rest: Vec<u64>,
}
impl Bits {
    fn all(n: usize) -> Self {
        let mut bits = Self::default();
        for i in 0..n {
            bits.insert(i);
        }
        bits
    }
    fn insert(&mut self, i: usize) {
        if i < 64 {
            self.first |= 1 << i;
        } else {
            let word = i / 64 - 1;
            if word >= self.rest.len() {
                self.rest.resize(word + 1, 0);
            }
            self.rest[word] |= 1 << (i % 64);
        }
    }
    fn contains(&self, i: usize) -> bool {
        if i < 64 {
            self.first & (1 << i) != 0
        } else {
            self.rest
                .get(i / 64 - 1)
                .is_some_and(|w| w & (1 << (i % 64)) != 0)
        }
    }
    fn pop_first(&mut self) -> Option<usize> {
        if self.first != 0 {
            let i = self.first.trailing_zeros() as usize;
            self.first &= self.first - 1;
            return Some(i);
        }
        for (word, w) in self.rest.iter_mut().enumerate() {
            if *w != 0 {
                let i = w.trailing_zeros() as usize;
                *w &= *w - 1;
                return Some((word + 1) * 64 + i);
            }
        }
        None
    }
    fn is_empty(&self) -> bool {
        self.first == 0 && self.rest.iter().all(|w| *w == 0)
    }
}
impl PartialEq for Bits {
    fn eq(&self, other: &Self) -> bool {
        let (short, long) = if self.rest.len() <= other.rest.len() {
            (&self.rest, &other.rest)
        } else {
            (&other.rest, &self.rest)
        };
        self.first == other.first
            && short.iter().zip(long.iter()).all(|(a, b)| a == b)
            && long[short.len()..].iter().all(|w| *w == 0)
    }
}
/// Whether every element of the ascending `small` is in the ascending `large`.
fn sorted_subset(small: &[u32], large: &[u32]) -> bool {
    let mut large = large.iter();
    small
        .iter()
        .all(|x| large.by_ref().find(|y| *y >= x).is_some_and(|y| y == x))
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
                let dependencies: BTreeSet<_> = dependencies.iter().collect();
                let body: BTreeSet<_> = self.nodes[definition.body.0].dependencies.iter().collect();
                if dependencies != body {
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
        let mut visited = vec![false; self.nodes.len()];
        let mut tasks = vec![root];
        tasks.extend(self.functions.values().map(|f| f.body));
        while let Some(id) = tasks.pop() {
            if matches!(
                self.nodes[id.0].constraint,
                Constraint::Conjunction(_) | Constraint::Disjunction(_)
            ) {
                if std::mem::replace(&mut visited[id.0], true) {
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
        let mut masks = Masks::default();
        match &constraint {
            Constraint::Reference {
                dependencies: deps, ..
            } => dependencies.extend(deps.iter().cloned()),
            Constraint::Atomic(a) => {
                a.left.append_var_names(&mut dependencies);
                match &a.right {
                    ConstraintValue::Type(ty) => ty.append_var_names(&mut dependencies),
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
                for id in children.iter() {
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
                }
                masks = Masks::from_children(
                    children.iter().map(|id| &self.nodes[id.0].dependencies[..]),
                );
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
            dependencies: dependencies.into_iter().collect(),
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
                        let c = Box::new(Conj {
                            id,
                            snapshot: Snapshot::new(flat, &map),
                            last_flags: pending.identity,
                            flags: pending.indices,
                            restart: Bits::default(),
                            updated: Vec::new(),
                            flat,
                        });
                        if flat {
                            result = Some(state.solve_flat(c, map, provider)?);
                        } else {
                            frames.push(Frame::Conj(c, map));
                        }
                    }
                    Constraint::Disjunction(_) => {
                        let Some(pending) = state.flags(id) else {
                            result = Some(AtomicOutcome::Failed);
                            continue;
                        };
                        frames.push(Frame::Disj(Box::new(Disj {
                            id,
                            map,
                            flags: pending.indices,
                            index: 0,
                            evaluated_maps: Vec::new(),
                            unevaluated_maps: Vec::new(),
                            updated: Vec::new(),
                            survived: Vec::new(),
                            newly_failed: false,
                        })));
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
                    state.invalidate(definition.body, updated.into_vec());
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
                                updated.union(changed);
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
                    let step = c.next(&map);
                    if matches!(step, ConjStep::DetectedLoop) {
                        state.detected_loops.push(c.id);
                    }
                    match step {
                        ConjStep::Child(index) => {
                            let child = children[index];
                            frames.push(Frame::ConjReturn(c, index));
                            frames.push(Frame::Enter(child, map));
                        }
                        ConjStep::Complete | ConjStep::DetectedLoop => {
                            result = Some(AtomicOutcome::Solved {
                                bindings: map,
                                updated: KeySet::from_unsorted(c.updated),
                            });
                        }
                        ConjStep::Restart => frames.push(Frame::Conj(c, map)),
                    }
                }
                Frame::ConjReturn(mut c, index) => match result.take().expect("child result") {
                    AtomicOutcome::Failed => {
                        state.cache.insert(c.id, Cache::Failed);
                        result = Some(AtomicOutcome::Failed);
                    }
                    AtomicOutcome::Solved { bindings, updated } => {
                        state.accept_conj(&mut c, index, updated);
                        frames.push(Frame::Conj(c, bindings));
                    }
                },
                Frame::Disj(mut d) => {
                    let children = children(&self.nodes[d.id.0].constraint);
                    if d.index < children.len() {
                        let i = d.index;
                        d.index += 1;
                        let child = children[i];
                        if d.flags.contains(i) {
                            let map = state.restore(child, &d.map);
                            let was_failed = matches!(state.cache.get(&child), Some(Cache::Failed));
                            frames.push(Frame::DisjReturn(d, child, i, was_failed));
                            frames.push(Frame::Enter(child, map));
                        } else {
                            if !matches!(state.cache.get(&child), Some(Cache::Failed)) {
                                d.unevaluated_maps.push(state.restore(child, &d.map));
                                d.survived.push(i as u32);
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
                                .filter(|(_, indices)| sorted_subset(&d.survived, indices))
                                .map(|(v, _)| BindingKey::Label(v.clone()))
                                .filter(|k| maps.iter().all(|map| map.contains_key(k)))
                                .collect();
                            if crate::lattice::verify_fast_paths() {
                                let detached: Vec<_> = maps
                                    .iter()
                                    .map(|map| {
                                        let mut out = (*map).clone();
                                        out.detach_trace();
                                        out
                                    })
                                    .collect();
                                let maps: Vec<_> = detached.iter().collect();
                                let mut old: BTreeSet<_> = maps[0].keys().collect();
                                for map in &maps[1..] {
                                    old.retain(|k| map.contains_key(k));
                                }
                                let old: Vec<_> = old
                                    .into_iter()
                                    .filter(|v| {
                                        v.label().and_then(|v| mask.get(v)).is_some_and(|indices| {
                                            sorted_subset(&d.survived, indices)
                                        })
                                    })
                                    .collect();
                                assert_eq!(keys, old, "disjunction key intersection fast path");
                            }
                            keys
                        } else {
                            KeySet::from_unsorted(d.updated)
                                .into_iter()
                                .filter(|v| {
                                    v.label()
                                        .and_then(|v| mask.get(v))
                                        .is_some_and(|indices| sorted_subset(&d.survived, indices))
                                })
                                .collect()
                        };
                        let mut map = d.map.clone();
                        let mut updated = KeySet::new();
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
                            d.survived.push(index as u32);
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
    indices: Bits,
    identity: Option<Bits>,
}
struct State<'a> {
    tree: &'a ConstraintTree,
    cache: BTreeMap<ConstraintId, Cache>,
    function_cache: BTreeMap<FunctionVariable, Cache>,
    detected_loops: Vec<ConstraintId>,
}
impl State<'_> {
    fn solve_flat(
        &mut self,
        c: Box<Conj>,
        map: BindingMap,
        provider: &dyn BifResultProvider,
    ) -> Result<AtomicOutcome, SolveError> {
        let mut flat = Flat {
            c,
            view: ReadView::new(map),
        };
        loop {
            let step = flat.c.next(&flat.view);
            if matches!(step, ConjStep::DetectedLoop) {
                self.detected_loops.push(flat.c.id);
            }
            match step {
                ConjStep::Child(index) => {
                    let child = children(&self.tree.nodes[flat.c.id.0].constraint)[index];
                    let Constraint::Atomic(a) = &self.tree.nodes[child.0].constraint else {
                        unreachable!("flat conjunction holds atoms")
                    };
                    let status = a.solve_access(
                        &mut flat.view,
                        &self.tree.derived,
                        provider,
                        flat.c.snapshot.log.as_mut(),
                    );
                    match status {
                        Err(error) => {
                            drop(flat);
                            return Err(error);
                        }
                        Ok(AtomicStatus::Failed) => {
                            let id = flat.c.id;
                            drop(flat);
                            self.cache.insert(id, Cache::Failed);
                            return Ok(AtomicOutcome::Failed);
                        }
                        Ok(AtomicStatus::Solved(updated)) => {
                            self.accept_conj(&mut flat.c, index, updated);
                        }
                    }
                }
                ConjStep::Restart => (),
                ConjStep::Complete | ConjStep::DetectedLoop => {
                    return Ok(AtomicOutcome::Solved {
                        bindings: flat.view.into_native(),
                        updated: KeySet::from_unsorted(flat.c.updated),
                    });
                }
            }
        }
    }
    fn accept_conj(&mut self, c: &mut Conj, index: usize, updated: KeySet) {
        let masks = &self.tree.nodes[c.id.0].masks;
        if c.flat {
            for var in &updated {
                if let Some(indices) = var.label().and_then(|v| masks.get(v)) {
                    for &i in indices {
                        let i = i as usize;
                        if i < index {
                            c.restart.insert(i);
                        } else if i > index {
                            c.flags.insert(i);
                        }
                    }
                }
            }
        } else {
            // `updated` is ascending, so each list is too.
            let mut affected: BTreeMap<usize, Vec<BindingKey>> = BTreeMap::new();
            for var in &updated {
                if let Some(indices) = var.label().and_then(|v| masks.get(v)) {
                    for &i in indices {
                        let i = i as usize;
                        if i != index {
                            affected.entry(i).or_default().push(var.clone());
                        }
                    }
                }
            }
            for (i, u) in affected {
                let child = children(&self.tree.nodes[c.id.0].constraint)[i];
                self.invalidate(child, u);
                if i < index {
                    c.restart.insert(i);
                } else {
                    c.flags.insert(i);
                }
            }
        }
        if c.updated.is_empty() {
            c.updated = updated.into_vec();
        } else {
            c.updated.extend(updated);
        }
    }
    fn flags(&mut self, id: ConstraintId) -> Option<Pending> {
        match self.cache.get_mut(&id) {
            Some(Cache::Failed) => None,
            Some(Cache::Active { dirty, .. }) => {
                let u = std::mem::take(dirty);
                let masks = &self.tree.nodes[id.0].masks;
                // `u` is ascending, so each list is too.
                let mut affected: BTreeMap<usize, Vec<BindingKey>> = BTreeMap::new();
                for variable in u {
                    if let Some(indices) = variable.label().and_then(|v| masks.get(v)) {
                        for &index in indices {
                            affected
                                .entry(index as usize)
                                .or_default()
                                .push(variable.clone());
                        }
                    }
                }
                let mut flags = Bits::default();
                for &index in affected.keys() {
                    flags.insert(index);
                }
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
                    indices: Bits::all(children(&self.tree.nodes[id.0].constraint).len()),
                    identity: None,
                })
            }
        }
    }
    fn invalidate(&mut self, mut id: ConstraintId, updated: Vec<BindingKey>) {
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
    fn save(&mut self, id: ConstraintId, updated: &KeySet, map: &BindingMap) {
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
struct Flat {
    c: Box<Conj>,
    view: ReadView,
}
struct Conj {
    id: ConstraintId,
    snapshot: Snapshot,
    last_flags: Option<Bits>,
    flags: Bits,
    restart: Bits,
    /// Every key the children updated, in any order and with repeats.
    updated: Vec<BindingKey>,
    flat: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConjStep {
    Child(usize),
    Restart,
    Complete,
    DetectedLoop,
}
impl Conj {
    fn next<A: BindingAccess>(&mut self, map: &A) -> ConjStep {
        if let Some(index) = self.flags.pop_first() {
            ConjStep::Child(index)
        } else if self.restart.is_empty() {
            ConjStep::Complete
        } else if Some(&self.restart) == self.last_flags.as_ref()
            && self.snapshot.unchanged_access(map)
        {
            ConjStep::DetectedLoop
        } else {
            self.last_flags = Some(self.restart.clone());
            self.snapshot.reset(map.native_map());
            self.flags = std::mem::take(&mut self.restart);
            ConjStep::Restart
        }
    }
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
    fn unchanged_access<A: BindingAccess>(&self, map: &A) -> bool {
        let Some(log) = &self.log else {
            return self.map.as_ref() == Some(map.native_map());
        };
        let mut seen = BTreeSet::new();
        let unchanged = log
            .iter()
            .all(|(key, old)| !seen.insert(key) || map.get(key) == old.as_ref());
        if let Some(old) = &self.map {
            let equal = {
                let mut a = old.clone();
                let mut b = map.native_map().clone();
                a.detach_trace();
                b.detach_trace();
                a == b
            };
            assert_eq!(unchanged, equal, "conjunction write log fast path");
        }
        unchanged
    }
}
struct Disj {
    id: ConstraintId,
    map: BindingMap,
    flags: Bits,
    index: usize,
    evaluated_maps: Vec<BindingMap>,
    unevaluated_maps: Vec<BindingMap>,
    /// Every key the branches updated, in any order and with repeats.
    updated: Vec<BindingKey>,
    /// Ascending positions of the branches that did not fail.
    survived: Vec<u32>,
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
    Conj(Box<Conj>, BindingMap),
    ConjReturn(Box<Conj>, usize),
    Disj(Box<Disj>),
    DisjReturn(Box<Disj>, ConstraintId, usize, bool),
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
fn save_entry(entry: &mut Cache, updated: &KeySet, map: &BindingMap) {
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
) -> Result<(BindingMap, KeySet), SolveError> {
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
    let mut updated = KeySet::new();
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
                .iter()
                .filter(|v| recursive_names.contains(*v))
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
struct RootEntry {
    certificate: crate::constraints::bindings::RootCertificate,
    updated: KeySet,
    loops: Vec<ConstraintId>,
    inferred: Type,
}
#[derive(Default)]
struct RootCache {
    entries: BTreeMap<FunctionVariable, RootEntry>,
    guard_bytes: usize,
}
impl RootCache {
    const MAX_GUARD_BYTES: usize = 8 * 1024 * 1024;
    const MAX_ENTRIES: usize = 4096;
    fn insert(&mut self, function: FunctionVariable, entry: RootEntry) -> bool {
        if let Some(old) = self.entries.remove(&function) {
            self.guard_bytes -= old.certificate.guard_buffer_bytes();
        }
        let bytes = entry.certificate.guard_buffer_bytes();
        if !entry.certificate.retainable()
            || self.entries.len() >= Self::MAX_ENTRIES
            || bytes > Self::MAX_GUARD_BYTES - self.guard_bytes
        {
            return false;
        }
        self.guard_bytes += bytes;
        self.entries.insert(function, entry);
        true
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
        let replay_enabled = self.functions.len() >= 400 && provider.replay_is_pure();
        let mutable = replay_enabled.then(|| {
            let mut keys = self
                .functions
                .iter()
                .map(|f| BindingKey::Label(f.variable().clone()))
                .collect::<Vec<_>>();
            keys.extend(
                recursive
                    .iter()
                    .map(|(_, v)| BindingKey::Label((*v).clone())),
            );
            keys.sort();
            keys.dedup();
            crate::constraints::bindings::RootCatalogue::new(keys.into())
        });
        let fixed_input =
            (replay_enabled && crate::lattice::verify_fast_paths()).then(|| map.clone());
        let mut certificates = RootCache::default();
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
                if let (Some(mutable), Some(fixed)) = (&mutable, &fixed_input) {
                    mutable.assert_fixed_input(fixed, &map);
                }
                if replay_enabled {
                    if let Some(entry) = certificates.entries.get(function) {
                        if entry.certificate.matches(&map) && !crate::lattice::verify_fast_paths() {
                            #[cfg(test)]
                            TEST_ROOT_HITS.with(|count| {
                                let (hits, checked) = count.get();
                                count.set((hits + 1, checked));
                            });
                            let inferred = entry.inferred.clone();
                            map = enter(function.variable(), inferred.clone(), map)?.0;
                            if let Some(variable) =
                                &self.tree.tree.functions[function].recursive_variable
                            {
                                map = enter(variable, inferred, map)?.0;
                            }
                            continue;
                        }
                    }
                }
                let input = if let Some(mutable) = &mutable {
                    map.trace_root(mutable.clone())
                } else {
                    map.clone()
                };
                let solved = self
                    .tree
                    .tree
                    .solve(self.references[function], input, provider)?;
                let AtomicOutcome::Solved { bindings, updated } = solved.outcome else {
                    unreachable!("native function reference cannot fail")
                };
                let inferred = bindings
                    .get(&BindingKey::Label(function.variable().clone()))
                    .cloned()
                    .unwrap_or_else(Type::none)
                    .limit(4)?;
                if replay_enabled {
                    let matched = certificates
                        .entries
                        .get(function)
                        .filter(|entry| entry.certificate.matches(&map));
                    if let Some(entry) = matched {
                        let expected = entry.certificate.replay_witness(&map);
                        let mut actual = bindings.clone();
                        actual.detach_trace();
                        assert_eq!(
                            expected, actual,
                            "complete root certificate bindings witness"
                        );
                        assert_eq!(
                            entry.updated, updated,
                            "complete root certificate updated witness"
                        );
                        assert_eq!(
                            entry.loops, solved.detected_loops,
                            "complete root certificate loop witness"
                        );
                        assert_eq!(
                            entry.inferred, inferred,
                            "complete root certificate inferred witness"
                        );
                        #[cfg(test)]
                        TEST_ROOT_HITS.with(|count| {
                            let (hits, checked) = count.get();
                            count.set((hits, checked + 1));
                        });
                    } else {
                        let certificate = bindings.certificate(&map);
                        let (updated, loops) = if crate::lattice::verify_fast_paths() {
                            (updated, solved.detected_loops.clone())
                        } else {
                            (KeySet::new(), Vec::new())
                        };
                        certificates.insert(
                            function.clone(),
                            RootEntry {
                                certificate,
                                updated,
                                loops,
                                inferred: inferred.clone(),
                            },
                        );
                    }
                }

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

#[cfg(test)]
std::thread_local! {
    pub(crate) static TEST_ROOT_HITS: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

#[cfg(test)]
mod root_cache_observation_tests {
    use super::*;
    use crate::constraints::bindings::RootCatalogue;
    #[test]
    fn native_cache_save_restore_and_dirty_keys_keep_input_guards() {
        let key = |n: i64| BindingKey::Label(VariableId::Integer(n.into()));
        let input: BindingMap = [(key(1), Type::atom("cached"))].into_iter().collect();
        let tree = ConstraintTree::default();
        let mut state = State {
            tree: &tree,
            cache: BTreeMap::new(),
            function_cache: BTreeMap::new(),
            detected_loops: vec![],
        };
        let traced = input.trace_root(RootCatalogue::new(vec![key(1), key(2)].into()));
        state.save(ConstraintId(0), &[key(1)].into(), &traced);
        let mut changed_local = traced.clone();
        changed_local.insert(key(1), Type::atom("local"));
        let restored = state.restore(ConstraintId(0), &changed_local);
        assert_eq!(restored.get(&key(1)), Some(&Type::atom("cached")));
        if let Some(Cache::Active { dirty, .. }) = state.cache.get_mut(&ConstraintId(0)) {
            dirty.insert(key(1));
        }
        let dirty = state.restore(ConstraintId(0), &changed_local);
        assert_eq!(dirty.get(&key(1)), Some(&Type::atom("local")));
        let cert = dirty.certificate(&input);
        assert_eq!(cert.slots.len(), 1);
        let mut changed_input = input.clone();
        changed_input.remove(&key(1));
        assert!(!cert.matches(&changed_input));
        assert!(state.function_cache.is_empty());
    }
}

#[cfg(test)]
mod root_cache_budget_tests {
    use super::*;
    use crate::constraints::bindings::RootCatalogue;
    fn function(n: usize) -> FunctionVariable {
        FunctionVariable::new(VariableId::Integer(n.into())).unwrap()
    }
    fn entry(guards: usize) -> RootEntry {
        let input = BindingMap::new();
        let keys = (0..guards)
            .map(|n| BindingKey::Label(VariableId::Integer(n.into())))
            .collect::<Vec<_>>();
        let traced = input.trace_root(RootCatalogue::new(keys.clone().into()));
        for key in &keys {
            traced.get(key);
        }
        RootEntry {
            certificate: traced.certificate(&input),
            updated: KeySet::new(),
            loops: vec![],
            inferred: Type::any(),
        }
    }
    #[test]
    fn aggregate_guard_bytes_replacement_rejection_and_readmission() {
        let mut cache = RootCache::default();
        let bytes = entry(128).certificate.guard_buffer_bytes();
        let count = RootCache::MAX_GUARD_BYTES / bytes;
        assert!(count < RootCache::MAX_ENTRIES);
        for n in 0..count {
            assert!(cache.insert(function(n), entry(128)));
        }
        assert_eq!(cache.guard_bytes, count * bytes);
        assert!(!cache.insert(function(count), entry(128)));
        assert_eq!(cache.entries.len(), count);
        assert_eq!(cache.guard_bytes, count * bytes);
        assert!(cache.insert(function(0), entry(0)));
        assert_eq!(cache.guard_bytes, (count - 1) * bytes);
        assert!(cache.insert(function(count), entry(128)));
        assert!(!cache.insert(function(count), entry(129)));
        assert!(!cache.entries.contains_key(&function(count)));
        assert_eq!(cache.guard_bytes, (count - 1) * bytes);
        assert!(cache.insert(function(count), entry(128)));
        assert_eq!(
            cache.guard_bytes,
            cache
                .entries
                .values()
                .map(|e| e.certificate.guard_buffer_bytes())
                .sum::<usize>()
        );
    }
    #[test]
    fn aggregate_entry_limit_allows_replacement_but_rejects_growth() {
        let mut cache = RootCache::default();
        for n in 0..RootCache::MAX_ENTRIES {
            assert!(cache.insert(function(n), entry(0)));
        }
        assert_eq!(cache.guard_bytes, 0);
        assert!(!cache.insert(function(RootCache::MAX_ENTRIES), entry(0)));
        assert_eq!(cache.entries.len(), RootCache::MAX_ENTRIES);
        assert!(cache.insert(function(0), entry(1)));
        assert_eq!(cache.entries.len(), RootCache::MAX_ENTRIES);
        assert_eq!(cache.guard_bytes, entry(1).certificate.guard_buffer_bytes());
    }
}

#[cfg(test)]
mod native_binding_access_tests {
    use super::*;
    use crate::bifs::registry::Mfa;
    use crate::constraints::atomic::{AtomicStatus, Relation};
    use crate::constraints::bindings::RootCatalogue;
    use crate::constraints::derived::EvalError;
    use crate::lattice::LatticeError;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn key(n: i64) -> BindingKey {
        BindingKey::Label(VariableId::Integer(n.into()))
    }
    fn conj(map: &BindingMap) -> Conj {
        Conj {
            id: ConstraintId(0),
            snapshot: Snapshot::new(true, map),
            last_flags: None,
            flags: Bits::default(),
            restart: Bits::default(),
            updated: vec![],
            flat: true,
        }
    }
    #[test]
    fn conj_child_complete_restart_and_loop_keep_snapshot_short_circuit_order() {
        let initial = BindingMap::new();
        let map = initial.trace_root(RootCatalogue::new(vec![key(1)].into()));
        let mut c = conj(&map);
        c.snapshot.log.as_mut().unwrap().push((key(1), None));
        c.flags.insert(2);
        c.restart.insert(0);
        c.last_flags = Some(c.restart.clone());
        assert_eq!(c.next(&map), ConjStep::Child(2));
        assert_eq!(c.snapshot.log.as_ref().unwrap().len(), 1);
        assert!(map.certificate(&initial).slots.is_empty());
        c.last_flags = None;
        assert_eq!(c.next(&map), ConjStep::Restart);
        assert!(map.certificate(&initial).slots.is_empty());
        assert!(c.snapshot.log.as_ref().unwrap().is_empty());
        assert!(c.restart.is_empty());
        assert_eq!(c.next(&map), ConjStep::Child(0));
        c.snapshot.log.as_mut().unwrap().push((key(1), None));
        assert_eq!(c.next(&map), ConjStep::Complete);
        assert!(map.certificate(&initial).slots.is_empty());
        c.restart.insert(0);
        assert_eq!(c.next(&map), ConjStep::DetectedLoop);
        assert_eq!(map.certificate(&initial).slots.len(), 1);
        assert_eq!(c.snapshot.log.as_ref().unwrap().len(), 1);
    }
    #[test]
    fn snapshot_first_old_value_and_reset_remain_native() {
        let mut map = BindingMap::new();
        let mut snapshot = Snapshot::new(true, &map);
        let old = map.insert(key(1), Type::atom("first"));
        snapshot.log.as_mut().unwrap().push((key(1), old));
        let old = map.insert(key(1), Type::atom("second"));
        snapshot.log.as_mut().unwrap().push((key(1), old));
        assert!(!snapshot.unchanged_access(&map));
        let old = map.remove(&key(1));
        snapshot.log.as_mut().unwrap().push((key(1), old));
        assert!(snapshot.unchanged_access(&map));
        snapshot.reset(&map);
        assert!(snapshot.log.as_ref().unwrap().is_empty());
        assert!(snapshot.unchanged_access(&map));
        let retained = map.clone();
        assert!(map.ptr_eq(&retained));
    }
    fn dependent_atom(tree: &mut ConstraintTree) -> ConstraintId {
        tree.push(Constraint::Atomic(AtomicConstraint {
            left: Type::variable_integer(1.into()),
            relation: Relation::Subtype,
            right: ConstraintValue::Type(Type::any()),
        }))
        .unwrap()
    }
    #[test]
    fn accept_conj_preserves_flat_masks_and_nonflat_cache_invalidation() {
        for flat in [true, false] {
            let mut tree = ConstraintTree::default();
            let mut children_ids = vec![];
            for _ in 0..3 {
                let atom = dependent_atom(&mut tree);
                children_ids.push(if flat {
                    atom
                } else {
                    tree.push(Constraint::Conjunction(vec![atom])).unwrap()
                });
            }
            let root = tree
                .push(Constraint::Conjunction(children_ids.clone()))
                .unwrap();
            let mut state = State {
                tree: &tree,
                cache: BTreeMap::new(),
                function_cache: BTreeMap::new(),
                detected_loops: vec![],
            };
            for &child in &children_ids {
                state.cache.insert(
                    child,
                    Cache::Active {
                        local: BindingMap::new(),
                        dirty: BTreeSet::new(),
                    },
                );
            }
            let mut c = conj(&BindingMap::new());
            c.id = root;
            c.flat = flat;
            let updated = KeySet::from([key(1)]);
            state.accept_conj(&mut c, 1, updated.clone());
            assert!(c.restart.contains(0));
            assert!(!c.restart.contains(1));
            assert!(c.flags.contains(2));
            assert!(!c.flags.contains(1));
            assert_eq!(c.updated, vec![key(1)]);
            state.accept_conj(&mut c, 1, updated);
            assert_eq!(c.updated, vec![key(1), key(1)]);
            for (index, child) in children_ids.iter().enumerate() {
                let Cache::Active { dirty, .. } = &state.cache[child] else {
                    panic!("active")
                };
                assert_eq!(dirty.contains(&key(1)), !flat && index != 1);
            }
        }
    }
    struct EffectProvider(Rc<RefCell<Vec<String>>>);
    impl BifResultProvider for EffectProvider {
        fn result(&self, mfa: &Mfa, arguments: &[Type]) -> Result<Type, EvalError> {
            self.0.borrow_mut().push(mfa.function.as_str().to_owned());
            assert_eq!(arguments, &[Type::any()]);
            if mfa.function.as_str() == "fail" {
                Err(EvalError::Native(LatticeError::BadArg))
            } else {
                Ok(Type::atom("written"))
            }
        }
    }
    #[test]
    fn reached_nested_frame_error_preserves_effect_order_and_stops_later_calls() {
        let calls = Rc::new(RefCell::new(vec![]));
        let provider = EffectProvider(calls.clone());
        let mut tree = ConstraintTree::default();
        let mut scopes = vec![];
        for (n, name) in [(1, "emit"), (2, "fail"), (3, "unreached")] {
            let derived = tree.push_derived(DerivedOp::BifReturn {
                mfa: Mfa {
                    module: "proof".into(),
                    function: name.into(),
                    arity: 1,
                },
                arguments: vec![Type::variable_integer(99.into())],
            });
            let atom = tree
                .push(Constraint::Atomic(AtomicConstraint {
                    left: Type::variable_integer(n.into()),
                    relation: Relation::Subtype,
                    right: ConstraintValue::Derived(derived),
                }))
                .unwrap();
            scopes.push(tree.push(Constraint::Conjunction(vec![atom])).unwrap());
        }
        let root = tree.push(Constraint::Conjunction(scopes)).unwrap();
        let input = BindingMap::from(vec![(key(100), Type::atom("retained"))]);
        assert!(matches!(
            tree.solve(root, input.clone(), &provider),
            Err(SolveError::Evaluation(EvalError::Native(
                LatticeError::BadArg
            )))
        ));
        assert_eq!(*calls.borrow(), vec!["emit".to_owned(), "fail".to_owned()]);
        assert_eq!(input.get(&key(1)), None);
        assert_eq!(input.get(&key(100)), Some(&Type::atom("retained")));
    }
    #[test]
    fn native_binding_access_layout() {
        use std::mem::{align_of, size_of};
        println!(
            concat!(
                "NATIVE_BINDING_ACCESS_LAYOUT {{",
                "\"binding_map_bytes\":{},\"binding_map_align\":{},",
                "\"binding_access_ref_bytes\":{},\"atomic_status_bytes\":{},",
                "\"atomic_outcome_bytes\":{},\"frame_bytes\":{},\"frame_align\":{},",
                "\"conj_bytes\":{},\"conj_align\":{},\"conj_box_bytes\":{},",
                "\"conj_step_bytes\":{},\"snapshot_bytes\":{},\"bits_bytes\":{}",
                "}}"
            ),
            size_of::<BindingMap>(),
            align_of::<BindingMap>(),
            size_of::<&mut BindingMap>(),
            size_of::<AtomicStatus>(),
            size_of::<AtomicOutcome>(),
            size_of::<Frame>(),
            align_of::<Frame>(),
            size_of::<Conj>(),
            align_of::<Conj>(),
            size_of::<Box<Conj>>(),
            size_of::<ConjStep>(),
            size_of::<Snapshot>(),
            size_of::<Bits>()
        );
    }
}

#[cfg(test)]
mod read_view_integration_tests {
    use super::*;
    use crate::bifs::registry::Mfa;
    use crate::constraints::atomic::Relation;
    use crate::constraints::derived::EvalError;
    use crate::lattice::LatticeError;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn key(n: i64) -> BindingKey {
        BindingKey::Label(VariableId::Integer(n.into()))
    }
    struct Effects(Rc<RefCell<Vec<(String, Vec<Type>)>>>);
    impl BifResultProvider for Effects {
        fn result(&self, mfa: &Mfa, args: &[Type]) -> Result<Type, EvalError> {
            self.0
                .borrow_mut()
                .push((mfa.function.as_str().to_owned(), args.to_vec()));
            if mfa.function.as_str() == "fail" {
                Err(EvalError::Native(LatticeError::BadArg))
            } else {
                Ok(Type::all_atoms())
            }
        }
    }
    fn derived_atom(tree: &mut ConstraintTree, name: &str, relation: Relation) -> ConstraintId {
        let derived = tree.push_derived(DerivedOp::BifReturn {
            mfa: Mfa {
                module: "proof".into(),
                function: name.into(),
                arity: 1,
            },
            arguments: vec![Type::variable_integer(1.into())],
        });
        tree.push(Constraint::Atomic(AtomicConstraint {
            left: Type::variable_integer(1.into()),
            relation,
            right: ConstraintValue::Derived(derived),
        }))
        .unwrap()
    }
    fn flat(tree: &ConstraintTree, id: ConstraintId, map: &BindingMap) -> Box<Conj> {
        Box::new(Conj {
            id,
            snapshot: Snapshot::new(true, map),
            last_flags: None,
            flags: Bits::all(children(&tree.nodes[id.0].constraint).len()),
            restart: Bits::default(),
            updated: vec![],
            flat: true,
        })
    }
    fn state(tree: &ConstraintTree) -> State<'_> {
        State {
            tree,
            cache: BTreeMap::new(),
            function_cache: BTreeMap::new(),
            detected_loops: vec![],
        }
    }
    #[test]
    fn flat_restart_reads_current_values_and_preserves_ordered_opaque_provider_effects() {
        let mut tree = ConstraintTree::default();
        let first = derived_atom(&mut tree, "emit", Relation::Subtype);
        let second = tree
            .push(Constraint::Atomic(AtomicConstraint {
                left: Type::variable_integer(1.into()),
                relation: Relation::Subtype,
                right: ConstraintValue::Type(Type::atom("narrowed")),
            }))
            .unwrap();
        let root = tree
            .push(Constraint::Conjunction(vec![first, second]))
            .unwrap();
        let effects = Rc::new(RefCell::new(vec![]));
        let input = BindingMap::new();
        let result = tree
            .solve(root, input.clone(), &Effects(effects.clone()))
            .unwrap();
        let AtomicOutcome::Solved { bindings, updated } = result.outcome else {
            panic!("solved")
        };
        assert_eq!(bindings.get(&key(1)), Some(&Type::atom("narrowed")));
        assert_eq!(updated, KeySet::from([key(1)]));
        assert_eq!(
            *effects.borrow(),
            vec![
                ("emit".to_owned(), vec![Type::any()]),
                ("emit".to_owned(), vec![Type::atom("narrowed")]),
            ]
        );
        assert!(result.detected_loops.is_empty());
        assert_eq!(input.get(&key(1)), None);
    }
    #[test]
    fn flat_failed_and_error_exits_discard_owner_before_publication_and_later_calls() {
        for error in [false, true] {
            let mut tree = ConstraintTree::default();
            let first = derived_atom(&mut tree, "emit", Relation::Subtype);
            let second = derived_atom(
                &mut tree,
                if error { "fail" } else { "equal" },
                if error {
                    Relation::Subtype
                } else {
                    Relation::Equal
                },
            );
            let third = derived_atom(&mut tree, "unreached", Relation::Subtype);
            let root = tree
                .push(Constraint::Conjunction(vec![first, second, third]))
                .unwrap();
            let input = BindingMap::new();
            let effects = Rc::new(RefCell::new(vec![]));
            let mut state = state(&tree);
            let outcome = state.solve_flat(
                flat(&tree, root, &input),
                input.clone(),
                &Effects(effects.clone()),
            );
            if error {
                assert!(matches!(
                    outcome,
                    Err(SolveError::Evaluation(EvalError::Native(
                        LatticeError::BadArg
                    )))
                ));
                assert!(!state.cache.contains_key(&root));
            } else {
                assert!(matches!(outcome, Ok(AtomicOutcome::Failed)));
                assert!(matches!(state.cache.get(&root), Some(Cache::Failed)));
            }
            assert_eq!(
                effects
                    .borrow()
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>(),
                vec!["emit", if error { "fail" } else { "equal" }]
            );
            assert_eq!(effects.borrow()[1].1, vec![Type::all_atoms()]);
            assert_eq!(input.get(&key(1)), None);
        }
    }
    #[test]
    fn flat_detected_loop_returns_native_owner_and_records_original_id() {
        let mut tree = ConstraintTree::default();
        let atom = derived_atom(&mut tree, "unreached", Relation::Subtype);
        let root = tree.push(Constraint::Conjunction(vec![atom])).unwrap();
        let input = BindingMap::from(vec![(key(1), Type::any())]);
        let mut c = flat(&tree, root, &input);
        c.flags = Bits::default();
        c.restart.insert(0);
        c.last_flags = Some(c.restart.clone());
        c.snapshot
            .log
            .as_mut()
            .unwrap()
            .push((key(1), Some(Type::any())));
        let effects = Rc::new(RefCell::new(vec![]));
        let mut state = state(&tree);
        let result = state
            .solve_flat(c, input.clone(), &Effects(effects.clone()))
            .unwrap();
        let AtomicOutcome::Solved { bindings, updated } = result else {
            panic!("solved")
        };
        assert!(bindings.ptr_eq(&input));
        assert!(updated.is_empty());
        assert_eq!(state.detected_loops, vec![root]);
        assert!(effects.borrow().is_empty());
    }
    #[test]
    fn read_view_layout() {
        use std::mem::{align_of, size_of};
        let mut values = crate::constraints::bindings::read_view_layout_values();
        let object = values.as_object_mut().unwrap();
        for (key, value) in [
            ("frame_bytes", size_of::<Frame>()),
            ("frame_align", align_of::<Frame>()),
            ("conj_bytes", size_of::<Conj>()),
            ("conj_align", align_of::<Conj>()),
            ("flat_bytes", size_of::<Flat>()),
            ("flat_align", align_of::<Flat>()),
            ("binding_map_bytes", size_of::<BindingMap>()),
        ] {
            object.insert(key.into(), serde_json::json!(value));
        }
        println!("READ_VIEW_LAYOUT {}", values);
    }
}

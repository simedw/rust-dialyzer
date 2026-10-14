use crate::constraints::atomic::{
    AtomicConstraint, AtomicOutcome, BindingMap, ConstraintValue, Relation, SolveError,
};
use crate::constraints::closure_order::ClosureOrderKey;
use crate::constraints::derived::{BifResultProvider, DerivedArena, DerivedId, DerivedOp};
use crate::constraints::tree::FunctionVariable;
use crate::lattice::{LatticeError, NativeOrderKey, Type, TypePredicate, VariableId};
use std::cmp::Ordering;
use std::sync::{Arc, OnceLock};

/// A sorted set of variables without duplicates, shared between nodes.
pub type Dependencies = Arc<[VariableId]>;

/// The union of sorted, duplicate-free runs.
fn union<'a>(runs: impl IntoIterator<Item = &'a Dependencies>) -> Dependencies {
    let mut runs = runs.into_iter().filter(|run| !run.is_empty());
    let Some(first) = runs.next() else {
        return Dependencies::default();
    };
    let Some(second) = runs.next() else {
        return first.clone();
    };
    let Some(third) = runs.next() else {
        return merge(first, second).into();
    };
    let runs: Vec<_> = [first, second, third].into_iter().chain(runs).collect();
    // Variables are nearly always machine integers, which sort as such
    // (`VariableId` puts integers before atoms), so they are sorted as keys.
    let mut small = Vec::with_capacity(runs.iter().map(|run| run.len()).sum());
    for v in runs.iter().flat_map(|run| run.iter()) {
        match v {
            VariableId::Integer(n) => match n.small() {
                Some(n) => small.push(n),
                None => return union_sorted(&runs),
            },
            VariableId::Atom(_) => return union_sorted(&runs),
        }
    }
    small.sort_unstable();
    small.dedup();
    let out: Dependencies = small
        .into_iter()
        .map(|n| VariableId::Integer(n.into()))
        .collect();
    if crate::lattice::verify_fast_paths() {
        assert_eq!(out, union_sorted(&runs), "small variable union fast path");
    }
    out
}
/// Many runs: the stable sort finds and merges them.
fn union_sorted(runs: &[&Dependencies]) -> Dependencies {
    let mut all: Vec<_> = runs.iter().flat_map(|run| run.iter()).collect();
    all.sort();
    all.dedup();
    all.into_iter().cloned().collect()
}
fn merge(a: &[VariableId], b: &[VariableId]) -> Vec<VariableId> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            Ordering::Less => {
                out.push(a[i].clone());
                i += 1;
            }
            Ordering::Greater => {
                out.push(b[j].clone());
                j += 1;
            }
            Ordering::Equal => {
                out.push(a[i].clone());
                i += 1;
                j += 1;
            }
        }
    }
    out.extend_from_slice(&a[i..]);
    out.extend_from_slice(&b[j..]);
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeConstraintId(pub usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeDerivedId(pub usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupKind {
    Conjunction,
    Disjunction,
}
#[derive(Clone, Debug)]
pub enum NativeOperand {
    Type(Type),
    Derived(NativeDerivedId),
}
#[derive(Clone, Copy, Debug)]
pub enum ClosureOrigin {
    Undefined,
    SourceLine(u32),
}
#[derive(Clone, Debug)]
struct OrderedDerived {
    semantic: DerivedId,
    key: ClosureOrderKey,
    origin: ClosureOrigin,
    deps: Dependencies,
}
#[derive(Clone, Debug)]
pub enum NativeConstraint {
    Atomic {
        left: Type,
        relation: Relation,
        right: NativeOperand,
        deps: Dependencies,
    },
    Group {
        kind: GroupKind,
        children: Vec<NativeConstraintId>,
        deps: Dependencies,
    },
    Reference {
        function: FunctionVariable,
        deps: Dependencies,
    },
}
#[derive(Clone, Debug)]
pub enum NormalizationError {
    TooManyDisjunctions,
    Lattice(LatticeError),
    Solve(SolveError),
    Unsatisfiable,
    Boundary(String),
    Comparison(String),
}
impl From<LatticeError> for NormalizationError {
    fn from(e: LatticeError) -> Self {
        Self::Lattice(e)
    }
}
impl From<SolveError> for NormalizationError {
    fn from(e: SolveError) -> Self {
        Self::Solve(e)
    }
}
#[derive(Default)]
struct AtomicOrderKeys {
    left: OnceLock<NativeOrderKey>,
    right: OnceLock<NativeOrderKey>,
}
/// The order key of `t`, from a per-thread memo: the same types recur across
/// the constraints of an SCC and across SCCs. Types are interned, so equal
/// types are one key and the key is a function of the type.
fn order_key(t: &Type) -> NativeOrderKey {
    t.native_order_key()
}

#[derive(Default)]
pub struct NativeConstraintArena {
    nodes: Vec<NativeConstraint>,
    order_keys: Vec<AtomicOrderKeys>,
    derived: Vec<OrderedDerived>,
    pub(crate) semantics: DerivedArena,
}
impl NativeConstraintArena {
    pub fn derived(
        &mut self,
        op: DerivedOp,
        key: ClosureOrderKey,
        origin: ClosureOrigin,
    ) -> NativeDerivedId {
        let semantic = self.semantics.push(op);
        let deps = self.semantics.dependencies(semantic).unwrap().iter().cloned().collect();
        let id = NativeDerivedId(self.derived.len());
        self.derived.push(OrderedDerived {
            semantic,
            key,
            origin,
            deps,
        });
        id
    }
    pub fn derived_order_key(&self, id: NativeDerivedId) -> Option<&ClosureOrderKey> {
        self.derived.get(id.0).map(|v| &v.key)
    }
    pub fn derived_capture(
        &self,
        id: NativeDerivedId,
    ) -> Option<(&ClosureOrderKey, ClosureOrigin, &Dependencies)> {
        self.derived
            .get(id.0)
            .map(|value| (&value.key, value.origin, &value.deps))
    }
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    /// A diagnostic classifier with type values and derived captures erased.
    /// Equality is not sufficient for reusing constraints or their ordering.
    pub(super) fn probe_shape(&self, start: usize) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hash = crate::fast_hash::FxHasher::default();
        for node in &self.nodes[start..] {
            std::mem::discriminant(node).hash(&mut hash);
            match node {
                NativeConstraint::Atomic { relation, right, deps, .. } => {
                    std::mem::discriminant(relation).hash(&mut hash);
                    std::mem::discriminant(right).hash(&mut hash);
                    deps.hash(&mut hash);
                    if let NativeOperand::Derived(id) = right {
                        if let Some(derived) = self.derived.get(id.0) {
                            if let Some(op) = self.semantics.get(derived.semantic) {
                                std::mem::discriminant(op).hash(&mut hash);
                            }
                        }
                    }
                }
                NativeConstraint::Group { kind, children, deps } => {
                    std::mem::discriminant(kind).hash(&mut hash);
                    for child in children { child.0.hash(&mut hash); }
                    deps.hash(&mut hash);
                }
                NativeConstraint::Reference { function, deps } => {
                    function.variable().hash(&mut hash);
                    deps.hash(&mut hash);
                }
            }
        }
        hash.finish()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
    /// Appends `other` after this arena, as if its nodes had been pushed
    /// here in order; returns the constraint id offset of its nodes.
    pub(crate) fn append(&mut self, other: NativeConstraintArena) -> usize {
        let node_offset = self.nodes.len();
        let derived_offset = self.derived.len();
        let semantic_offset = self.semantics.append(other.semantics);
        self.nodes.reserve(other.nodes.len());
        for mut node in other.nodes {
            match &mut node {
                NativeConstraint::Atomic { right: NativeOperand::Derived(id), .. } => id.0 += derived_offset,
                NativeConstraint::Group { children, .. } => {
                    for child in children {
                        child.0 += node_offset;
                    }
                }
                _ => {}
            }
            self.nodes.push(node);
        }
        self.order_keys.extend(other.order_keys);
        for mut derived in other.derived {
            derived.semantic = DerivedId(derived.semantic.0 + semantic_offset);
            self.derived.push(derived);
        }
        node_offset
    }
    /// A structural rendering of every node, for verifying that two arenas
    /// were built identically.
    pub(crate) fn debug_fingerprint(&self) -> String {
        let derived: Vec<_> = self
            .derived
            .iter()
            .map(|d| (format!("{:?}", self.semantics.get(d.semantic)), &d.key, d.origin, &d.deps))
            .collect();
        format!("{:?}\n{:?}", self.nodes, derived)
    }
    pub fn node(&self, id: NativeConstraintId) -> Option<&NativeConstraint> {
        self.nodes.get(id.0)
    }
    fn push(&mut self, node: NativeConstraint) -> NativeConstraintId {
        self.push_keyed(node, AtomicOrderKeys::default())
    }
    fn push_keyed(&mut self, node: NativeConstraint, keys: AtomicOrderKeys) -> NativeConstraintId {
        let id = NativeConstraintId(self.nodes.len());
        self.nodes.push(node);
        self.order_keys.push(keys);
        id
    }
    pub(crate) fn operand(
        &self,
        value: &NativeOperand,
    ) -> Result<ConstraintValue, NormalizationError> {
        Ok(match value {
            NativeOperand::Type(t) => ConstraintValue::Type(t.clone()),
            NativeOperand::Derived(id) => ConstraintValue::Derived(
                self.derived
                    .get(id.0)
                    .ok_or_else(|| NormalizationError::Boundary("foreign derived identity".into()))?
                    .semantic,
            ),
        })
    }
    pub fn atomic(
        &mut self,
        left: Type,
        relation: Relation,
        right: NativeOperand,
        provider: &dyn BifResultProvider,
    ) -> Result<NativeConstraintId, NormalizationError> {
        if left.predicate(TypePredicate::Any)? {
            return Ok(self.any(relation));
        }
        let right_any = match &right {
            NativeOperand::Type(t) => t.predicate(TypePredicate::Any)?,
            NativeOperand::Derived(id) => {
                self.derived.get(id.0).ok_or_else(|| {
                    NormalizationError::Boundary("foreign derived identity".into())
                })?;
                false
            }
        };
        if right_any {
            return Ok(self.any(relation));
        }
        let mut deps = std::collections::BTreeSet::new();
        left.append_var_names(&mut deps);
        let deps: Dependencies = match &right {
            NativeOperand::Type(t) => {
                t.append_var_names(&mut deps);
                deps.into_iter().collect()
            }
            NativeOperand::Derived(id) if deps.is_empty() => self.derived[id.0].deps.clone(),
            NativeOperand::Derived(id) => {
                let left: Dependencies = deps.into_iter().collect();
                union([&left, &self.derived[id.0].deps])
            }
        };
        // An equality puts its smaller operand on the left; the keys that
        // decide this are kept for sorting.
        let keys = AtomicOrderKeys::default();
        let (left, right) = match right {
            NativeOperand::Type(t) if matches!(relation, Relation::Equal) => {
                if let Some(order) = left.variable_order(&t) {
                    if order != Ordering::Less {
                        (t, NativeOperand::Type(left))
                    } else {
                        (left, NativeOperand::Type(t))
                    }
                } else {
                    let (l, r) = (order_key(&left), order_key(&t));
                    let swap = l >= r;
                    if crate::lattice::verify_fast_paths() {
                        assert_eq!(
                            swap,
                            left.to_native_value() >= t.to_native_value(),
                            "equality orientation diverged"
                        );
                    }
                    let (left, right, l, r) = if swap { (t, left, r, l) } else { (left, t, l, r) };
                    let _ = keys.left.set(l);
                    let _ = keys.right.set(r);
                    (left, NativeOperand::Type(right))
                }
            }
            right => (left, right),
        };
        if deps.is_empty() {
            let c = AtomicConstraint {
                left: left.clone(),
                relation,
                right: self.operand(&right)?,
            };
            if matches!(
                c.solve(BindingMap::new(), &self.semantics, provider)?,
                AtomicOutcome::Failed
            ) {
                return Err(NormalizationError::Unsatisfiable);
            }
        }
        Ok(self.push_keyed(
            NativeConstraint::Atomic {
                left,
                relation,
                right,
                deps,
            },
            keys,
        ))
    }
    fn any(&mut self, relation: Relation) -> NativeConstraintId {
        self.push(NativeConstraint::Atomic {
            left: Type::any(),
            relation,
            right: NativeOperand::Type(Type::any()),
            deps: Dependencies::default(),
        })
    }
    pub fn reference(
        &mut self,
        function: FunctionVariable,
        deps: Dependencies,
    ) -> NativeConstraintId {
        let deps = if deps.binary_search(function.variable()).is_ok() {
            deps.iter().filter(|v| *v != function.variable()).cloned().collect()
        } else {
            deps
        };
        self.push(NativeConstraint::Reference { function, deps })
    }
    pub fn dependencies(
        &self,
        id: NativeConstraintId,
    ) -> Result<&Dependencies, NormalizationError> {
        match self
            .nodes
            .get(id.0)
            .ok_or_else(|| NormalizationError::Boundary("foreign constraint identity".into()))?
        {
            NativeConstraint::Atomic { deps, .. }
            | NativeConstraint::Group { deps, .. }
            | NativeConstraint::Reference { deps, .. } => Ok(deps),
        }
    }
    pub fn group(
        &mut self,
        kind: GroupKind,
        children: Vec<NativeConstraintId>,
    ) -> Result<NativeConstraintId, NormalizationError> {
        // A native one-element disjunction returns its element before wrapping or flattening.
        if kind == GroupKind::Disjunction && children.len() == 1 {
            self.dependencies(children[0])?;
            return Ok(children[0]);
        }
        let mut children = children;
        if kind == GroupKind::Disjunction {
            for id in &mut children {
                if matches!(self.node(*id), Some(NativeConstraint::Atomic { .. })) {
                    *id = self.group(GroupKind::Conjunction, vec![*id])?;
                }
            }
        }
        let mut flat = std::collections::VecDeque::new();
        for id in children {
            match self
                .node(id)
                .ok_or_else(|| NormalizationError::Boundary("foreign constraint identity".into()))?
            {
                NativeConstraint::Group {
                    kind: k, children, ..
                } if *k == kind => {
                    for child in children.iter().rev() {
                        flat.push_front(*child);
                    }
                }
                _ => flat.push_front(id),
            }
        }
        let mut list = self.sorted_unique(flat.into_iter().collect())?;
        if kind == GroupKind::Conjunction {
            list.retain(|id| !self.dependencies(*id).unwrap().is_empty());
        } else if list
            .iter()
            .any(|id| self.dependencies(*id).unwrap().is_empty())
        {
            list = vec![self.any(Relation::Equal)];
        }
        let deps = union(
            list.iter()
                .map(|id| self.dependencies(*id))
                .collect::<Result<Vec<_>, _>>()?,
        );
        if deps.is_empty() {
            let tautology = self.any(Relation::Equal);
            return Ok(self.push(NativeConstraint::Group {
                kind: GroupKind::Conjunction,
                children: vec![tautology],
                deps,
            }));
        }
        Ok(self.push(NativeConstraint::Group {
            kind,
            children: list,
            deps,
        }))
    }
    pub fn disjunctive_normal_form(
        &mut self,
        root: NativeConstraintId,
    ) -> Result<NativeConstraintId, NormalizationError> {
        let expanded = self.expand(root);
        match expanded {
            Ok(list) => self.group(GroupKind::Disjunction, list),
            Err(NormalizationError::TooManyDisjunctions) => Ok(root),
            Err(e) => Err(e),
        }
    }
    fn expand(
        &mut self,
        root: NativeConstraintId,
    ) -> Result<Vec<NativeConstraintId>, NormalizationError> {
        use std::collections::BTreeMap;
        let mut values: BTreeMap<usize, Vec<NativeConstraintId>> = BTreeMap::new();
        let mut work = vec![(root, false)];
        while let Some((id, leave)) = work.pop() {
            if values.contains_key(&id.0) {
                continue;
            }
            let NativeConstraint::Group { kind, children, .. } = self
                .node(id)
                .ok_or_else(|| NormalizationError::Boundary("foreign DNF identity".into()))?
                .clone()
            else {
                return Err(NormalizationError::Boundary(
                    "DNF expansion root must be a native constraint list".into(),
                ));
            };
            if !leave {
                if kind == GroupKind::Disjunction && children.len() > 28 {
                    return Err(NormalizationError::TooManyDisjunctions);
                }
                work.push((id, true));
                for child in children.iter().rev() {
                    if matches!(self.node(*child), Some(NativeConstraint::Group { .. })) {
                        work.push((*child, false));
                    }
                }
                continue;
            }
            let mut simple = Vec::new();
            let mut groups = Vec::new();
            for child in children {
                if matches!(self.node(child), Some(NativeConstraint::Group { .. })) {
                    groups.push(values[&child.0].clone());
                } else {
                    simple.push(child);
                }
            }
            let result = match kind {
                GroupKind::Disjunction => {
                    if simple
                        .iter()
                        .any(|id| matches!(self.node(*id), Some(NativeConstraint::Atomic { .. })))
                    {
                        return Err(NormalizationError::Boundary(
                            "native disjunction contains an unwrapped atomic constraint".into(),
                        ));
                    }
                    let mut result: Vec<_> = groups.into_iter().flatten().collect();
                    result.extend(simple);
                    if result.len() > 28 {
                        return Err(NormalizationError::TooManyDisjunctions);
                    }
                    result
                }
                GroupKind::Conjunction => match groups.len() {
                    0 => vec![self.group(GroupKind::Conjunction, simple)?],
                    1 => {
                        let mut result = Vec::new();
                        for child in groups.pop().unwrap() {
                            let mut list = vec![child];
                            list.extend(&simple);
                            result.push(self.group(GroupKind::Conjunction, list)?);
                        }
                        result
                    }
                    _ => {
                        let mut groups = groups.into_iter();
                        let mut combined = groups.next().unwrap();
                        for next in groups {
                            let mut joined = Vec::new();
                            for a in &combined {
                                for b in &next {
                                    joined.push(self.group(GroupKind::Conjunction, vec![*a, *b])?);
                                }
                            }
                            if joined.len() > 28 {
                                return Err(NormalizationError::TooManyDisjunctions);
                            }
                            combined = joined;
                        }
                        let mut result = Vec::new();
                        for child in combined {
                            let prefix = self.group(GroupKind::Conjunction, simple.clone())?;
                            result.push(self.group(GroupKind::Conjunction, vec![prefix, child])?);
                        }
                        result
                    }
                },
            };
            values.insert(id.0, result);
        }
        Ok(values.remove(&root.0).unwrap())
    }
    pub fn to_value(
        &self,
        root: NativeConstraintId,
    ) -> Result<serde_json::Value, NormalizationError> {
        use serde_json::json;
        enum Action {
            Enter(NativeConstraintId),
            Leave(NativeConstraintId, usize),
        }
        let mut work = vec![Action::Enter(root)];
        let mut nodes = Vec::new();
        let mut built = Vec::new();
        let vars = |deps: &Dependencies| {
            deps.iter()
                .map(|v| match v {
                    VariableId::Integer(n) => json!({"integer":n.to_string()}),
                    VariableId::Atom(a) => json!({"atom":a}),
                })
                .collect::<Vec<_>>()
        };
        while let Some(action) = work.pop() {
            match action {
                Action::Enter(id) => match self.node(id).ok_or_else(|| {
                    NormalizationError::Boundary("foreign constraint identity".into())
                })? {
                    NativeConstraint::Group { children, .. } => {
                        work.push(Action::Leave(id, children.len()));
                        for child in children.iter().rev() {
                            work.push(Action::Enter(*child));
                        }
                    }
                    NativeConstraint::Atomic {
                        left,
                        relation,
                        right,
                        deps,
                    } => {
                        let right = match right {
                            NativeOperand::Type(t) => t.to_value(),
                            NativeOperand::Derived(id) => serde_json::json!({"derived":id.0}),
                        };
                        built.push(nodes.len());
                        nodes.push(json!({"kind":"atomic","left":left.to_value(),"relation":if matches!(relation,Relation::Equal){"eq"}else{"sub"},"right":right,"deps":vars(deps)}));
                    }
                    NativeConstraint::Reference { function, deps } => {
                        let id = match function.variable() {
                            VariableId::Integer(n) => json!({"integer":n.to_string()}),
                            VariableId::Atom(a) => json!({"atom":a}),
                        };
                        built.push(nodes.len());
                        nodes.push(json!({"kind":"ref","function":id,"deps":vars(deps)}));
                    }
                },
                Action::Leave(id, count) => {
                    let children = built.split_off(built.len() - count);
                    let NativeConstraint::Group { kind, deps, .. } = self.node(id).unwrap() else {
                        unreachable!()
                    };
                    built.push(nodes.len());
                    nodes.push(json!({"kind":if *kind==GroupKind::Conjunction{"conj"}else{"disj"},"children":children,"deps":vars(deps)}));
                }
            }
        }
        Ok(json!({"root":built[0],"nodes":nodes}))
    }
    fn compare_left(
        &self,
        ai: NativeConstraintId,
        bi: NativeConstraintId,
        a: &Type,
        b: &Type,
    ) -> Ordering {
        let key = |id: NativeConstraintId, t: &Type| {
            self.order_keys[id.0]
                .left
                .get_or_init(|| order_key(t))
        };
        // Variables are `{c, var, Id, unknown}`, so two of them are ordered by
        // their identities: integers before atoms, each in its own order.
        if let Some(order) = a.variable_order(b) {
            if crate::lattice::verify_fast_paths() {
                assert_eq!(order, key(ai, a).cmp(key(bi, b)), "variable order diverged");
            }
            return order;
        }
        // Interned types are equal exactly when they are the same term.
        let order = if a == b {
            Ordering::Equal
        } else {
            key(ai, a).cmp(key(bi, b))
        };
        if crate::lattice::verify_fast_paths() {
            assert_eq!(order, a.to_native_value().cmp(&b.to_native_value()), "left order diverged");
        }
        order
    }
    fn compare_operand(
        &self,
        ai: NativeConstraintId,
        bi: NativeConstraintId,
        a: &NativeOperand,
        b: &NativeOperand,
    ) -> Result<Ordering, NormalizationError> {
        Ok(match (a, b) {
            (NativeOperand::Type(a), NativeOperand::Type(b)) => {
                let order = if let Some(order) = a.variable_order(b) {
                    order
                } else if a == b {
                    Ordering::Equal
                } else {
                    let ka = self.order_keys[ai.0].right.get_or_init(|| order_key(a));
                    let kb = self.order_keys[bi.0].right.get_or_init(|| order_key(b));
                    ka.cmp(kb)
                };
                if crate::lattice::verify_fast_paths() {
                    assert_eq!(
                        order,
                        a.to_native_value().cmp(&b.to_native_value()),
                        "right order diverged"
                    );
                }
                order
            }
            (NativeOperand::Type(_), NativeOperand::Derived(_)) => Ordering::Less,
            (NativeOperand::Derived(_), NativeOperand::Type(_)) => Ordering::Greater,
            (NativeOperand::Derived(a), NativeOperand::Derived(b)) => {
                let a = &self.derived[a.0];
                let b = &self.derived[b.0];
                let c = a
                    .key
                    .compare(&b.key)
                    .map_err(NormalizationError::Comparison)?;
                c.then(a.deps.cmp(&b.deps))
                    .then(match (a.origin, b.origin) {
                        (ClosureOrigin::Undefined, ClosureOrigin::Undefined) => Ordering::Equal,
                        (ClosureOrigin::Undefined, _) => Ordering::Greater,
                        (_, ClosureOrigin::Undefined) => Ordering::Less,
                        (ClosureOrigin::SourceLine(a), ClosureOrigin::SourceLine(b)) => a.cmp(&b),
                    })
            }
        })
    }
    pub fn compare(
        &self,
        a: NativeConstraintId,
        b: NativeConstraintId,
    ) -> Result<Ordering, NormalizationError> {
        struct GroupCursor<'a> {
            children: std::iter::Zip<
                std::slice::Iter<'a, NativeConstraintId>,
                std::slice::Iter<'a, NativeConstraintId>,
            >,
            parents: (NativeConstraintId, NativeConstraintId),
            lengths: (usize, usize),
        }
        let mut groups = Vec::<GroupCursor<'_>>::new();
        let (mut ai, mut bi) = (a, b);
        loop {
            let order = {
                let a = self.node(ai).ok_or_else(|| {
                    NormalizationError::Boundary("foreign constraint identity".into())
                })?;
                let b = self.node(bi).ok_or_else(|| {
                    NormalizationError::Boundary("foreign constraint identity".into())
                })?;
                let arity = |n: &NativeConstraint| match n {
                    NativeConstraint::Reference { .. } => 3,
                    NativeConstraint::Atomic { .. } => 5,
                    NativeConstraint::Group { .. } => 6,
                };
                let size = arity(a).cmp(&arity(b));
                if size != Ordering::Equal {
                    return Ok(size);
                }
                match (a, b) {
                    (
                        NativeConstraint::Reference {
                            function: a,
                            deps: ad,
                        },
                        NativeConstraint::Reference {
                            function: b,
                            deps: bd,
                        },
                    ) => a.variable().cmp(b.variable()).then(ad.cmp(bd)),
                    (
                        NativeConstraint::Atomic {
                            left: a,
                            relation: ar,
                            right: av,
                            deps: ad,
                        },
                        NativeConstraint::Atomic {
                            left: b,
                            relation: br,
                            right: bv,
                            deps: bd,
                        },
                    ) => {
                        let c = self.compare_left(ai, bi, a, b).then(match (ar, br) {
                            (Relation::Equal, Relation::Subtype) => Ordering::Less,
                            (Relation::Subtype, Relation::Equal) => Ordering::Greater,
                            _ => Ordering::Equal,
                        });
                        if c != Ordering::Equal {
                            return Ok(c);
                        }
                        self.compare_operand(ai, bi, av, bv)?.then(ad.cmp(bd))
                    }
                    (
                        NativeConstraint::Group {
                            kind: ak,
                            children: ac,
                            ..
                        },
                        NativeConstraint::Group {
                            kind: bk,
                            children: bc,
                            ..
                        },
                    ) => {
                        let kind = match (ak, bk) {
                            (GroupKind::Conjunction, GroupKind::Disjunction) => Ordering::Less,
                            (GroupKind::Disjunction, GroupKind::Conjunction) => Ordering::Greater,
                            _ => Ordering::Equal,
                        };
                        if kind != Ordering::Equal {
                            return Ok(kind);
                        }
                        // Visit children lexicographically without queuing unused siblings.
                        let mut children = ac.iter().zip(bc.iter());
                        if let Some((&a, &b)) = children.next() {
                            groups.push(GroupCursor {
                                children,
                                parents: (ai, bi),
                                lengths: (ac.len(), bc.len()),
                            });
                            (ai, bi) = (a, b);
                            continue;
                        }
                        let length = ac.len().cmp(&bc.len());
                        if length != Ordering::Equal {
                            return Ok(length);
                        }
                        self.dependencies(ai)?.cmp(self.dependencies(bi)?)
                    }
                    _ => unreachable!("same native tuple arity selects same constraint record"),
                }
            };
            if order != Ordering::Equal {
                return Ok(order);
            }
            loop {
                let Some(group) = groups.last_mut() else {
                    return Ok(Ordering::Equal);
                };
                if let Some((&a, &b)) = group.children.next() {
                    (ai, bi) = (a, b);
                    break;
                }
                let length = group.lengths.0.cmp(&group.lengths.1);
                if length != Ordering::Equal {
                    return Ok(length);
                }
                let order = self
                    .dependencies(group.parents.0)?
                    .cmp(self.dependencies(group.parents.1)?);
                if order != Ordering::Equal {
                    return Ok(order);
                }
                groups.pop();
            }
        }
    }
    fn sorted_unique(
        &self,
        list: Vec<NativeConstraintId>,
    ) -> Result<Vec<NativeConstraintId>, NormalizationError> {
        // Lists mostly arrive sorted, or as a few sorted runs. The order is
        // total, so stably merging the runs gives the list the bottom-up
        // merge sort gives, and for a single run the comparisons that find
        // it also find its duplicates.
        let mut runs = vec![0];
        let mut unique = Vec::with_capacity(list.len());
        for (index, &id) in list.iter().enumerate() {
            if index > 0 {
                match self.compare(list[index - 1], id)? {
                    Ordering::Greater => runs.push(index),
                    Ordering::Equal if runs.len() == 1 => continue,
                    _ => {}
                }
            }
            if runs.len() == 1 {
                unique.push(id);
            }
        }
        if runs.len() > 1 {
            unique.clear();
            for id in self.merge_runs(&list, runs)? {
                if let Some(last) = unique.last()
                    && self.compare(*last, id)? == Ordering::Equal
                {
                    continue;
                }
                unique.push(id);
            }
        }
        if crate::lattice::verify_fast_paths() {
            let merged = self.merge_sorted_unique(list);
            assert!(
                matches!(&merged, Ok(merged) if *merged == unique),
                "natural merge diverged from merge sort: {unique:?} {merged:?}"
            );
        }
        Ok(unique)
    }
    /// Merges adjacent runs, given by their starts, pairwise and stably.
    fn merge_runs(
        &self,
        list: &[NativeConstraintId],
        mut runs: Vec<usize>,
    ) -> Result<Vec<NativeConstraintId>, NormalizationError> {
        let n = list.len();
        let mut list = list.to_vec();
        let mut merged = Vec::with_capacity(n);
        while runs.len() > 1 {
            let bound = |i: usize| runs.get(i).copied().unwrap_or(n);
            let mut next = Vec::with_capacity(runs.len().div_ceil(2));
            for i in (0..runs.len()).step_by(2) {
                let (start, middle, end) = (bound(i), bound(i + 1), bound(i + 2));
                next.push(start);
                let (mut a, mut b) = (start, middle);
                while a < middle && b < end {
                    if self.compare(list[a], list[b])? != Ordering::Greater {
                        merged.push(list[a]);
                        a += 1;
                    } else {
                        merged.push(list[b]);
                        b += 1;
                    }
                }
                merged.extend_from_slice(&list[a..middle]);
                merged.extend_from_slice(&list[b..end]);
            }
            std::mem::swap(&mut list, &mut merged);
            merged.clear();
            runs = next;
        }
        Ok(list)
    }
    fn merge_sorted_unique(
        &self,
        mut list: Vec<NativeConstraintId>,
    ) -> Result<Vec<NativeConstraintId>, NormalizationError> {
        let n = list.len();
        let mut width = 1;
        while width < n {
            let mut merged = Vec::with_capacity(n);
            let mut start = 0;
            while start < n {
                let middle = (start + width).min(n);
                let end = (middle + width).min(n);
                let (mut a, mut b) = (start, middle);
                while a < middle && b < end {
                    if self.compare(list[a], list[b])? != Ordering::Greater {
                        merged.push(list[a]);
                        a += 1;
                    } else {
                        merged.push(list[b]);
                        b += 1;
                    }
                }
                merged.extend_from_slice(&list[a..middle]);
                merged.extend_from_slice(&list[b..end]);
                start = end;
            }
            list = merged;
            width = width.saturating_mul(2);
        }
        let mut unique = Vec::with_capacity(n);
        for id in list {
            if let Some(last) = unique.last()
                && self.compare(*last, id)? == Ordering::Equal
            {
                continue;
            }
            unique.push(id);
        }
        Ok(unique)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraints::derived::NoBifProvider;
    use num_bigint::BigInt;
    fn leaf(a: &mut NativeConstraintArena, variable: i64, name: &str) -> NativeConstraintId {
        a.atomic(
            Type::variable_integer(variable.into()),
            Relation::Equal,
            NativeOperand::Type(Type::atom(name)),
            &NoBifProvider,
        )
        .unwrap()
    }
    #[test]
    fn atomic_order_keys_are_lazy_on_the_reached_operand() {
        let mut arena = NativeConstraintArena::default();
        let mut atom = |left: &str| {
            arena
                .atomic(
                    Type::atom(left),
                    Relation::Subtype,
                    NativeOperand::Type(Type::variable_integer(1.into())),
                    &NoBifProvider,
                )
                .unwrap()
        };
        let first = atom("a");
        let second = atom("b");
        assert!(arena.order_keys[first.0].left.get().is_none());
        assert!(arena.order_keys[first.0].right.get().is_none());
        assert_eq!(arena.compare(first, second).unwrap(), Ordering::Less);
        assert!(arena.order_keys[first.0].left.get().is_some());
        assert!(arena.order_keys[second.0].left.get().is_some());
        assert!(arena.order_keys[first.0].right.get().is_none());
        assert!(arena.order_keys[second.0].right.get().is_none());
        let left = arena.order_keys[first.0].left.get().unwrap() as *const NativeOrderKey;
        assert_eq!(arena.compare(first, second).unwrap(), Ordering::Less);
        assert_eq!(
            left,
            arena.order_keys[first.0].left.get().unwrap() as *const NativeOrderKey
        );
    }

    #[test]
    fn variable_left_sides_compare_by_identity() {
        let mut arena = NativeConstraintArena::default();
        let mut atom = |left: Type| {
            arena
                .atomic(
                    left,
                    Relation::Subtype,
                    NativeOperand::Type(Type::atom("payload")),
                    &NoBifProvider,
                )
                .unwrap()
        };
        let ids = [
            atom(Type::variable_integer((-3).into())),
            atom(Type::variable_integer(2.into())),
            atom(Type::variable_integer(BigInt::from(1u8) << 70)),
            atom(Type::variable_atom("A")),
            atom(Type::variable_atom("B")),
        ];
        for (i, a) in ids.iter().enumerate() {
            for (j, b) in ids.iter().enumerate() {
                assert_eq!(arena.compare(*a, *b).unwrap(), i.cmp(&j));
            }
        }
        if !crate::lattice::verify_fast_paths() {
            assert!(arena.order_keys[ids[0].0].left.get().is_none());
        }
        for (i, a) in ids.iter().enumerate() {
            for (j, b) in ids.iter().enumerate() {
                let NativeConstraint::Atomic { left: l, .. } = arena.node(*a).unwrap() else {
                    unreachable!()
                };
                let NativeConstraint::Atomic { left: r, .. } = arena.node(*b).unwrap() else {
                    unreachable!()
                };
                assert_eq!(l.to_native_value().cmp(&r.to_native_value()), i.cmp(&j));
            }
        }
    }

    #[test]
    fn foreign_constraint_error_precedes_order_key_initialization() {
        let mut arena = NativeConstraintArena::default();
        let valid = leaf(&mut arena, 1, "a");
        // Equalities key their operands when built, to orient them.
        let keys = |arena: &NativeConstraintArena| {
            let keys = &arena.order_keys[valid.0];
            (keys.left.get().cloned(), keys.right.get().cloned())
        };
        let before = keys(&arena);
        for (a, b) in [
            (NativeConstraintId(usize::MAX), valid),
            (valid, NativeConstraintId(usize::MAX)),
        ] {
            assert!(
                matches!(arena.compare(a, b), Err(NormalizationError::Boundary(error)) if error == "foreign constraint identity")
            );
        }
        assert_eq!(keys(&arena), before);
    }

    #[test]
    fn immutable_order_keys_preserve_arena_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<NativeConstraintArena>();
    }

    #[test]
    fn constant_failure_is_rejected_at_construction() {
        let mut a = NativeConstraintArena::default();
        assert!(matches!(
            a.atomic(
                Type::atom("a"),
                Relation::Equal,
                NativeOperand::Type(Type::atom("b")),
                &NoBifProvider
            ),
            Err(NormalizationError::Unsatisfiable)
        ));
    }
    #[test]
    fn equalities_deduplicate_after_native_side_orientation() {
        let mut a = NativeConstraintArena::default();
        let first = leaf(&mut a, 1, "a");
        let reversed = a
            .atomic(
                Type::atom("a"),
                Relation::Equal,
                NativeOperand::Type(Type::variable_integer(1.into())),
                &NoBifProvider,
            )
            .unwrap();
        let root = a
            .group(GroupKind::Conjunction, vec![first, reversed])
            .unwrap();
        let Some(NativeConstraint::Group { children, .. }) = a.node(root) else {
            panic!()
        };
        assert_eq!(children.len(), 1);
    }
    fn product(a: &mut NativeConstraintArena, x: usize, y: usize) -> NativeConstraintId {
        let mut groups = Vec::new();
        for (variable, count) in [(1, x), (2, y)] {
            let children = (0..count)
                .map(|n| leaf(a, variable, &format!("a{n}")))
                .collect();
            groups.push(a.group(GroupKind::Disjunction, children).unwrap());
        }
        a.group(GroupKind::Conjunction, groups).unwrap()
    }
    #[test]
    fn native_dnf_boundary_is_twenty_eight_and_fallback_preserves_root() {
        let mut a = NativeConstraintArena::default();
        let root = product(&mut a, 7, 4);
        let normalized = a.disjunctive_normal_form(root).unwrap();
        let Some(NativeConstraint::Group { kind, children, .. }) = a.node(normalized) else {
            panic!()
        };
        assert_eq!(*kind, GroupKind::Disjunction);
        assert_eq!(children.len(), 28);
        let overflow = product(&mut a, 10, 3);
        assert_eq!(a.disjunctive_normal_form(overflow).unwrap(), overflow);
    }
    #[test]
    fn conjunction_tautology_is_filtered_but_disjunction_tautology_wins() {
        let mut a = NativeConstraintArena::default();
        let tautology = a.group(GroupKind::Conjunction, vec![]).unwrap();
        let atom = leaf(&mut a, 1, "a");
        let conjunction = a
            .group(GroupKind::Conjunction, vec![tautology, atom])
            .unwrap();
        assert_eq!(a.dependencies(conjunction).unwrap().len(), 1);
        let disjunction = a
            .group(GroupKind::Disjunction, vec![tautology, atom])
            .unwrap();
        assert!(a.dependencies(disjunction).unwrap().is_empty());
    }
    #[test]
    fn flat_serialization_and_drop_accept_deep_alternating_groups() {
        let mut a = NativeConstraintArena::default();
        let mut root = leaf(&mut a, 1, "first");
        for n in 0..1000 {
            let other = leaf(&mut a, 1, &format!("a{n}"));
            let branch = a.group(GroupKind::Disjunction, vec![root, other]).unwrap();
            root = a
                .group(GroupKind::Conjunction, vec![branch, other])
                .unwrap();
        }
        assert!(a.to_value(root).unwrap()["nodes"].as_array().unwrap().len() > 1000);
    }
    #[test]
    fn differing_left_does_not_read_forged_derived_operand() {
        let mut a = NativeConstraintArena::default();
        let mut raw = |n: i64| {
            a.push(NativeConstraint::Atomic {
                left: Type::variable_integer(n.into()),
                relation: Relation::Equal,
                right: NativeOperand::Derived(NativeDerivedId(usize::MAX)),
                deps: Dependencies::default(),
            })
        };
        let l = raw(1);
        let r = raw(2);
        assert_eq!(a.compare(l, r).unwrap(), Ordering::Less);
        assert!(a.order_keys[l.0].right.get().is_none());
    }
    #[test]
    fn deeply_nested_equal_groups_are_iterative() {
        let mut a = NativeConstraintArena::default();
        let mut root = a.push(NativeConstraint::Group {
            kind: GroupKind::Conjunction,
            children: vec![],
            deps: Dependencies::default(),
        });
        for _ in 0..4096 {
            root = a.push(NativeConstraint::Group {
                kind: GroupKind::Conjunction,
                children: vec![root],
                deps: Dependencies::default(),
            });
        }
        assert_eq!(a.compare(root, root).unwrap(), Ordering::Equal);
    }
}

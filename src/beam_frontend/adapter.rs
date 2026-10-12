//! In-memory compiler output to the analyzer's compact Core arenas.
//! The selected runtime atom scope determines map layout, never the compiler.
use super::{
    Compilation,
    clean_view::{CleanView, GroupChildren, NodeShape, ViewRef},
};
use crate::{
    core::{
        CoreExpr, CoreModule, CoreNode, ErlangTerm as T, ErlangTerms, NodeId, RawNode, TermId,
        build_node,
    },
    fast_hash::{FastHashMap, FxHasher},
    native_form_environment::RawFormTerm,
    native_runtime_hash::{NativeAtomLayout, NativeHashKey},
    shared_vec::SharedVec,
    term_order::TermOrderArena,
};
use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
    sync::Arc,
};

fn tuple(input: &ErlangTerms, id: TermId) -> Result<&[TermId], String> {
    match input.terms.get(id.0) {
        Some(T::Tuple(f)) => Ok(f),
        _ => Err(format!("native adapter: expected tuple at {}", id.0)),
    }
}
fn atom(input: &ErlangTerms, id: TermId) -> Option<&str> {
    match input.terms.get(id.0) {
        Some(T::Atom(a)) => Some(a),
        _ => None,
    }
}
fn list(input: &ErlangTerms, mut id: TermId) -> Result<Vec<TermId>, String> {
    let mut items = Vec::new();
    loop {
        match input.terms.get(id.0) {
            Some(T::Nil) => return Ok(items),
            Some(T::List { items: next, tail }) => {
                items.extend_from_slice(next);
                id = *tail;
            }
            _ => return Err(format!("native adapter: expected proper list at {}", id.0)),
        }
    }
}
fn pairs(input: &ErlangTerms, id: TermId) -> Result<Vec<TermId>, String> {
    let mut out = Vec::new();
    for pair in list(input, id)? {
        let fields = tuple(input, pair)?;
        if fields.len() != 2 {
            return Err("native adapter: expected definition pair".into());
        }
        out.extend_from_slice(fields);
    }
    Ok(out)
}
fn map_pairs(
    input: &Arc<ErlangTerms>,
    entries: &[(TermId, TermId)],
    layout: &NativeAtomLayout,
) -> Result<Vec<(TermId, TermId)>, String> {
    if entries.len() < 2 {
        return Ok(entries.to_vec());
    }
    let roots = entries.iter().map(|(key, _)| *key).collect::<Vec<_>>();
    let order = TermOrderArena::reachable(input, &roots)?;
    let mut positions = BTreeMap::new();
    for &(key, value) in entries {
        let raw = RawFormTerm::new(input.clone(), key)?;
        let hash_key =
            NativeHashKey::from_raw_with_order(&raw, &order).map_err(|e| e.to_string())?;
        if positions.insert(hash_key, (key, value)).is_some() {
            return Err("native adapter: duplicate map key".into());
        }
    }
    crate::native_map_order::ordered_keys(layout, positions.keys().cloned())
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|key| {
            positions
                .remove(&key)
                .ok_or("native adapter: map order lost key".into())
        })
        .collect()
}

struct Builder<'a> {
    layout: &'a NativeAtomLayout,
    terms: SharedVec<T>,
    nodes: SharedVec<CoreNode>,
    /// The first term interned with each hash; later unequal terms with the
    /// same hash (true collisions) go to `collisions`, in interning order.
    hashes: FastHashMap<u64, TermId>,
    collisions: FastHashMap<u64, Vec<TermId>>,
    atoms: FastHashMap<String, TermId>,
    orders: BTreeMap<TermId, Vec<usize>>,
    values: FastHashMap<(usize, TermId), TermId>,
}
impl<'a> Builder<'a> {
    fn intern(&mut self, term: T, order: Option<Vec<usize>>) -> TermId {
        let mut hasher = FxHasher::default();
        term.hash(&mut hasher);
        order.hash(&mut hasher);
        let hash = hasher.finish();
        let same = |id: &TermId| self.terms[id.0] == term && self.orders.get(id) == order.as_ref();
        let collided = match self.hashes.get(&hash) {
            None => false,
            Some(id) if same(id) => return *id,
            Some(_) => {
                if let Some(id) = self.collisions.get(&hash).and_then(|ids| ids.iter().find(|id| same(id))) {
                    return *id;
                }
                true
            }
        };
        let id = TermId(self.terms.len());
        self.terms.push(term);
        if let Some(order) = order {
            self.orders.insert(id, order);
        }
        if collided {
            self.collisions.entry(hash).or_default().push(id);
        } else {
            self.hashes.insert(hash, id);
        }
        id
    }
    fn atom(&mut self, name: &str) -> TermId {
        if let Some(&id) = self.atoms.get(name) {
            return id;
        }
        let id = self.intern(T::Atom(name.into()), None);
        self.atoms.insert(name.into(), id);
        id
    }
    fn source_atoms(&mut self, input: &Arc<ErlangTerms>) -> Result<(), String> {
        let mut pending = vec![input.root];
        while let Some(id) = pending.pop() {
            match &input.terms[id.0] {
                T::Atom(name) => {
                    self.atom(name);
                }
                T::Tuple(items) => pending.extend(items.iter().rev().copied()),
                T::List { items, tail } => {
                    pending.push(*tail);
                    pending.extend(items.iter().rev().copied());
                }
                T::Map(entries) => {
                    for (key, value) in map_pairs(input, entries, self.layout)?.into_iter().rev() {
                        pending.extend([value, key]);
                    }
                }
                _ => (),
            }
        }
        Ok(())
    }
    fn value(&mut self, input: &Arc<ErlangTerms>, root: TermId) -> Result<TermId, String> {
        let namespace = Arc::as_ptr(input) as usize;
        enum Task {
            Enter(TermId),
            Finish(TermId, T, Option<Vec<usize>>),
        }
        let mut pending = vec![Task::Enter(root)];
        while let Some(task) = pending.pop() {
            match task {
                Task::Enter(id) => {
                    if self.values.contains_key(&(namespace, id)) {
                        continue;
                    }
                    let (term, order) = match &input.terms[id.0] {
                        T::Atom(name) => {
                            let mapped = self.atom(name);
                            self.values.insert((namespace, id), mapped);
                            continue;
                        }
                        T::Tuple(items) => {
                            pending.push(Task::Finish(id, T::Tuple(items.clone()), None));
                            pending.extend(items.iter().rev().map(|&child| Task::Enter(child)));
                            continue;
                        }
                        T::List { items, tail } => {
                            let mut items = items.clone();
                            let mut tail = *tail;
                            while let T::List {
                                items: more,
                                tail: next,
                            } = &input.terms[tail.0]
                            {
                                items.extend_from_slice(more);
                                tail = *next;
                            }
                            if items.is_empty() {
                                pending.push(Task::Finish(id, T::List { items, tail }, None));
                                pending.push(Task::Enter(tail));
                                continue;
                            }
                            (T::List { items, tail }, None)
                        }
                        T::Map(entries) => {
                            let entries = map_pairs(input, entries, self.layout)?;
                            let order = if entries.len() > 32 {
                                (0..entries.len()).rev().collect()
                            } else {
                                (0..entries.len()).collect()
                            };
                            (T::Map(entries), Some(order))
                        }
                        T::Bitstring { bytes, bits } if *bits == bytes.len() as u64 * 8 => {
                            (T::Binary(bytes.clone()), None)
                        }
                        term => (term.clone(), None),
                    };
                    let children = match &term {
                        T::Tuple(items) => items.clone(),
                        T::List { items, tail } => {
                            let mut children = items.clone();
                            children.push(*tail);
                            children
                        }
                        T::Map(entries) => entries.iter().flat_map(|&(k, v)| [k, v]).collect(),
                        _ => vec![],
                    };
                    pending.push(Task::Finish(id, term, order));
                    pending.extend(children.into_iter().rev().map(Task::Enter));
                }
                Task::Finish(id, term, order) => {
                    let mapped = |id: TermId| {
                        self.values
                            .get(&(namespace, id))
                            .copied()
                            .ok_or("native adapter: missing child value".to_owned())
                    };
                    let term = match term {
                        T::Tuple(items) => {
                            T::Tuple(items.into_iter().map(mapped).collect::<Result<_, _>>()?)
                        }
                        T::List { items, tail } => {
                            let tail = mapped(tail)?;
                            if items.is_empty() {
                                self.values.insert((namespace, id), tail);
                                continue;
                            }
                            T::List {
                                items: items.into_iter().map(mapped).collect::<Result<_, _>>()?,
                                tail,
                            }
                        }
                        T::Map(entries) => T::Map(
                            entries
                                .into_iter()
                                .map(|(k, v)| Ok((mapped(k)?, mapped(v)?)))
                                .collect::<Result<_, String>>()?,
                        ),
                        term => term,
                    };
                    let mapped = self.intern(term, order);
                    self.values.insert((namespace, id), mapped);
                }
            }
        }
        self.values
            .get(&(namespace, root))
            .copied()
            .ok_or("native adapter: value root missing".into())
    }
    fn core(&mut self, input: &Arc<ErlangTerms>) -> Result<NodeId, String> {
        self.core_mode(input, false)
    }
    fn core_mode(&mut self, input: &Arc<ErlangTerms>, direct: bool) -> Result<NodeId, String> {
        struct Frame {
            kind: String,
            annotations: TermId,
            value: Option<TermId>,
            groups: Vec<Vec<TermId>>,
            map_arg: Option<TermId>,
            pattern: Option<bool>,
            source_children: Vec<TermId>,
            at: usize,
            children: Vec<NodeId>,
        }
        fn shape(
            input: &ErlangTerms,
            id: TermId,
        ) -> Result<
            (
                String,
                TermId,
                Option<TermId>,
                Vec<Vec<TermId>>,
                Option<TermId>,
                Option<bool>,
            ),
            String,
        > {
            let f = tuple(input, id)?;
            let tag = f
                .first()
                .and_then(|&id| atom(input, id))
                .ok_or("native adapter: node tag missing")?;
            let kind = tag
                .strip_prefix("c_")
                .ok_or("native adapter: expected Core record")?
                .to_owned();
            let arity = match kind.as_str() {
                "literal" | "var" | "opaque" | "binary" | "tuple" | "values" | "catch" => 3,
                "alias" | "apply" | "case" | "cons" | "fun" | "letrec" | "primop" | "seq" => 4,
                "call" | "clause" | "let" | "map" | "map_pair" | "receive" => 5,
                "module" => 6,
                "bitstr" | "try" => 7,
                _ => return Err(format!("native adapter: unknown record {tag}")),
            };
            if f.len() != arity {
                return Err(format!("native adapter: wrong record arity for {tag}"));
            }
            let mut map_arg = None;
            let mut pattern = None;
            let mut value = None;
            let groups = match kind.as_str() {
                "literal" | "var" | "opaque" => {
                    value = Some(f[2]);
                    vec![]
                }
                "binary" | "tuple" | "values" => vec![list(input, f[2])?],
                "alias" | "cons" | "seq" => vec![vec![f[2]], vec![f[3]]],
                "apply" | "primop" | "case" => vec![vec![f[2]], list(input, f[3])?],
                "bitstr" => (2..7).map(|i| vec![f[i]]).collect(),
                "call" => vec![vec![f[2]], vec![f[3]], list(input, f[4])?],
                "clause" | "let" | "receive" => vec![list(input, f[2])?, vec![f[3]], vec![f[4]]],
                "fun" => vec![list(input, f[2])?, vec![f[3]]],
                "catch" => vec![vec![f[2]]],
                "try" => vec![
                    vec![f[2]],
                    list(input, f[3])?,
                    vec![f[4]],
                    list(input, f[5])?,
                    vec![f[6]],
                ],
                "letrec" => vec![pairs(input, f[2])?, vec![f[3]]],
                "module" => vec![
                    vec![f[2]],
                    list(input, f[3])?,
                    pairs(input, f[4])?,
                    pairs(input, f[5])?,
                ],
                "map_pair" => vec![vec![f[2]], vec![f[3]], vec![f[4]]],
                "map" => {
                    map_arg = Some(f[2]);
                    pattern = Some(match atom(input, f[4]) {
                        Some("true") => true,
                        Some("false") => false,
                        _ => return Err("native adapter: map pattern flag invalid".into()),
                    });
                    vec![list(input, f[3])?]
                }
                _ => unreachable!(),
            };
            Ok((kind, f[1], value, groups, map_arg, pattern))
        }
        let make = |this: &mut Self, id| -> Result<Frame, String> {
            let (kind, ann, value, groups, map_arg, pattern) = shape(input, id)?;
            let source_children = groups
                .iter()
                .flat_map(|g| g.iter().copied())
                .chain(map_arg)
                .collect();
            Ok(Frame {
                kind,
                annotations: this.value(input, ann)?,
                value: value.map(|v| this.value(input, v)).transpose()?,
                groups,
                map_arg,
                pattern,
                source_children,
                at: 0,
                children: vec![],
            })
        };
        let mut stack = vec![make(self, input.root)?];
        loop {
            let frame = stack.last_mut().ok_or("native adapter: missing frame")?;
            let child = frame.source_children.get(frame.at).copied();
            if let Some(child) = child {
                frame.at += 1;
                stack.push(make(self, child)?);
                continue;
            }
            let frame = stack.pop().unwrap();
            let id = NodeId(self.nodes.len());
            let node = if direct {
                direct_node(
                    &frame.kind,
                    frame.annotations,
                    frame.value,
                    frame.groups.iter().map(Vec::len),
                    frame.children,
                    frame.pattern,
                    id,
                )?
            } else {
                let mut children = frame.children.into_iter();
                let groups = frame
                    .groups
                    .iter()
                    .map(|g| {
                        g.iter()
                            .map(|_| children.next().unwrap().0 as u64)
                            .collect()
                    })
                    .collect();
                let raw = RawNode {
                    kind: &frame.kind,
                    annotations: frame.annotations.0 as u64,
                    groups,
                    value: frame.value.map(|v| v.0 as u64),
                    map_arg: frame.map_arg.map(|_| children.next().unwrap().0 as u64),
                    map_pattern: frame.pattern,
                };
                build_node(
                    &raw,
                    format_args!("native.nodes[{}]", id.0),
                    id.0,
                    &self.nodes,
                    &self.terms,
                )?
            };
            self.nodes.push(node);
            if let Some(parent) = stack.last_mut() {
                parent.children.push(id);
            } else {
                return Ok(id);
            }
        }
    }
    fn core_view(&mut self, view: &CleanView) -> Result<NodeId, String> {
        struct Frame<'a> {
            shape: NodeShape<'a>,
            annotations: TermId,
            value: Option<TermId>,
            group_at: usize,
            cursor: Option<GroupChildren<'a>>,
            counts: [usize; 5],
            children: Vec<NodeId>,
        }
        impl Frame<'_> {
            fn next_child(&mut self) -> Result<Option<ViewRef>, String> {
                while self.group_at < self.shape.group_count() {
                    let cursor = self
                        .cursor
                        .get_or_insert_with(|| self.shape.group_children(self.group_at));
                    if let Some(child) = cursor.next() {
                        let child = child?;
                        self.counts[self.group_at] += 1;
                        return Ok(Some(child));
                    }
                    self.cursor = None;
                    self.group_at += 1;
                }
                // Match compiler transport: map pairs are numbered before base.
                Ok(self.shape.map_arg.take())
            }
        }
        let input = view.arena();
        let make = |this: &mut Self, reference| -> Result<Frame<'_>, String> {
            let shape = view.shape(reference)?;
            let annotations = this.value(input, shape.annotations)?;
            let value = shape.value.map(|id| this.value(input, id)).transpose()?;
            Ok(Frame {
                shape,
                annotations,
                value,
                group_at: 0,
                cursor: None,
                counts: [0; 5],
                children: Vec::new(),
            })
        };
        let mut stack = vec![make(self, view.root())?];
        loop {
            let child = stack
                .last_mut()
                .ok_or("native view: missing frame")?
                .next_child()?;
            if let Some(child) = child {
                stack.push(make(self, child)?);
                continue;
            }
            let frame = stack.pop().unwrap();
            let id = NodeId(self.nodes.len());
            let node = direct_node(
                frame.shape.kind,
                frame.annotations,
                frame.value,
                frame.counts[..frame.shape.group_count()].iter().copied(),
                frame.children,
                frame.shape.pattern,
                id,
            )?;
            self.nodes.push(node);
            if let Some(parent) = stack.last_mut() {
                parent.children.push(id);
            } else {
                return Ok(id);
            }
        }
    }
    fn contracts(&mut self, input: &Arc<ErlangTerms>) -> Result<BTreeMap<TermId, TermId>, String> {
        fn variable(input: &ErlangTerms, id: TermId) -> Option<TermId> {
            let f = tuple(input, id).ok()?;
            match f.first().and_then(|&v| atom(input, v)) {
                Some("var") if f.len() == 3 => match &input.terms[f[2].0] {
                    T::Atom(a) if a != "_" => Some(f[2]),
                    T::Integer(_) => Some(f[2]),
                    _ => None,
                },
                Some("ann_type") if f.len() == 3 => {
                    let fields = list(input, f[2]).ok()?;
                    if fields.len() == 2 {
                        variable(input, fields[1])
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }
        let mut output = BTreeMap::new();
        let mut pending = vec![input.root];
        while let Some(id) = pending.pop() {
            if let T::Tuple(f) = &input.terms[id.0] {
                if f.len() == 4
                    && atom(input, f[0]) == Some("type")
                    && atom(input, f[2]) == Some("bounded_fun")
                {
                    let mut variables = vec![];
                    let mut valid = true;
                    if let Ok(args) = list(input, f[3]) {
                        if args.len() == 2 {
                            if let Ok(constraints) = list(input, args[1]) {
                                for constraint in constraints {
                                    let Ok(fields) = tuple(input, constraint) else {
                                        valid = false;
                                        break;
                                    };
                                    if fields.len() != 4
                                        || atom(input, fields[0]) != Some("type")
                                        || atom(input, fields[2]) != Some("constraint")
                                    {
                                        valid = false;
                                        break;
                                    }
                                    let Ok(args) = list(input, fields[3]) else {
                                        valid = false;
                                        break;
                                    };
                                    if args.len() != 2 {
                                        valid = false;
                                        break;
                                    }
                                    let Ok(op) = tuple(input, args[0]) else {
                                        valid = false;
                                        break;
                                    };
                                    let Ok(types) = list(input, args[1]) else {
                                        valid = false;
                                        break;
                                    };
                                    if op.len() != 3
                                        || atom(input, op[0]) != Some("atom")
                                        || atom(input, op[2]) != Some("is_subtype")
                                        || types.len() != 2
                                    {
                                        valid = false;
                                        break;
                                    }
                                    let Some(var) = variable(input, types[0]) else {
                                        valid = false;
                                        break;
                                    };
                                    variables.push(var);
                                }
                            } else {
                                valid = false
                            }
                        } else {
                            valid = false
                        }
                    } else {
                        valid = false
                    }
                    if valid {
                        let form = self.value(input, id)?;
                        let nil = self.atom("nil");
                        let mut entries = BTreeMap::new();
                        for var in variables {
                            let key = self.value(input, var)?;
                            entries.insert(key, nil);
                        }
                        // Order a generated map through the same runtime layout.
                        let raw = ErlangTerms {
                            root: TermId(self.terms.len()),
                            terms: {
                                self.terms.freeze_shared();
                                self.terms.clone()
                            },
                        };
                        let mut terms = raw.terms;
                        terms.push(T::Map(entries.into_iter().collect()));
                        terms.freeze_shared();
                        let generated = Arc::new(ErlangTerms {
                            root: raw.root,
                            terms,
                        });
                        let map = self.value(&generated, generated.root)?;
                        output.insert(form, map);
                        let ns = Arc::as_ptr(&generated) as usize;
                        if let T::Map(entries) = &generated.terms[generated.root.0] {
                            for &(key, value) in entries {
                                self.values.remove(&(ns, key));
                                self.values.remove(&(ns, value));
                            }
                        }
                        self.values.remove(&(ns, generated.root));
                    }
                }
                pending.extend(f.iter().rev().copied());
            } else if let T::List { items, tail } = &input.terms[id.0] {
                pending.push(*tail);
                pending.extend(items.iter().rev().copied());
            }
        }
        Ok(output)
    }
}

/// Native compilation already fixes record shapes. Build the typed node once;
/// cross-node semantics are checked by CoreProgram's final validation gate.
fn direct_node(
    kind: &str,
    annotations: TermId,
    value: Option<TermId>,
    groups: impl Iterator<Item = usize>,
    children: Vec<NodeId>,
    pattern: Option<bool>,
    id: NodeId,
) -> Result<CoreNode, String> {
    struct Children<I> {
        groups: I,
        nodes: std::vec::IntoIter<NodeId>,
    }
    impl<I: Iterator<Item = usize>> Children<I> {
        fn count(&mut self) -> usize {
            self.groups.next().unwrap()
        }
        fn one(&mut self) -> NodeId {
            let count = self.count();
            debug_assert_eq!(count, 1);
            self.nodes.next().unwrap()
        }
        fn group(&mut self) -> Vec<NodeId> {
            let count = self.count();
            self.nodes.by_ref().take(count).collect()
        }
        fn pairs(&mut self) -> Vec<(NodeId, NodeId)> {
            let count = self.count();
            debug_assert_eq!(count % 2, 0);
            (0..count / 2)
                .map(|_| (self.nodes.next().unwrap(), self.nodes.next().unwrap()))
                .collect()
        }
    }
    let mut children = Children {
        groups,
        nodes: children.into_iter(),
    };
    let expr = match kind {
        "literal" => CoreExpr::Literal(value.ok_or("native direct: missing literal value")?),
        "var" => CoreExpr::Var(value.ok_or("native direct: missing variable value")?),
        "opaque" => CoreExpr::Opaque(value.ok_or("native direct: missing opaque value")?),
        "values" => CoreExpr::Values(children.group()),
        "tuple" => CoreExpr::Tuple(children.group()),
        "binary" => CoreExpr::Binary(children.group()),
        "bitstr" => CoreExpr::Bitstr {
            value: children.one(),
            size: children.one(),
            unit: children.one(),
            typ: children.one(),
            flags: children.one(),
        },
        "cons" => CoreExpr::Cons {
            head: children.one(),
            tail: children.one(),
        },
        "map" => {
            // Compiler transport numbers map pairs before its base argument.
            let pairs = children.group();
            CoreExpr::Map {
                base: children
                    .nodes
                    .next()
                    .ok_or("native direct: missing map base")?,
                pairs,
                pattern: pattern.ok_or("native direct: missing map pattern flag")?,
            }
        }
        "map_pair" => CoreExpr::MapPair {
            operation: children.one(),
            key: children.one(),
            value: children.one(),
        },
        "let" => CoreExpr::Let {
            vars: children.group(),
            argument: children.one(),
            body: children.one(),
        },
        "seq" => CoreExpr::Seq {
            argument: children.one(),
            body: children.one(),
        },
        "apply" => CoreExpr::Apply {
            operator: children.one(),
            args: children.group(),
        },
        "call" => CoreExpr::Call {
            module: children.one(),
            name: children.one(),
            args: children.group(),
        },
        "primop" => CoreExpr::Primop {
            name: children.one(),
            args: children.group(),
        },
        "case" => CoreExpr::Case {
            argument: children.one(),
            clauses: children.group(),
        },
        "clause" => CoreExpr::Clause {
            patterns: children.group(),
            guard: children.one(),
            body: children.one(),
        },
        "alias" => CoreExpr::Alias {
            variable: children.one(),
            pattern: children.one(),
        },
        "fun" => CoreExpr::Fun {
            params: children.group(),
            body: children.one(),
        },
        "receive" => CoreExpr::Receive {
            clauses: children.group(),
            timeout: children.one(),
            action: children.one(),
        },
        "try" => CoreExpr::Try {
            argument: children.one(),
            vars: children.group(),
            body: children.one(),
            exception_vars: children.group(),
            handler: children.one(),
        },
        "catch" => CoreExpr::Catch {
            body: children.one(),
        },
        "letrec" => CoreExpr::Letrec {
            definitions: children.pairs(),
            body: children.one(),
        },
        "module" => CoreExpr::Module {
            name: children.one(),
            exports: children.group(),
            attributes: children.pairs(),
            definitions: children.pairs(),
        },
        _ => return Err(format!("native direct: unknown Core kind {kind}")),
    };
    debug_assert!(children.groups.next().is_none());
    debug_assert!(children.nodes.next().is_none());
    Ok(CoreNode {
        id,
        annotations,
        expr,
    })
}

/// Convert without serialization, with maps ordered in the selected atom scope.
/// The compact cache representation intentionally preserves source atom order
/// and declarations in Core attributes rather than retaining expanded ASTs.
pub fn module(compilation: Compilation, layout: &NativeAtomLayout) -> Result<CoreModule, String> {
    let forms = Arc::new(compilation.source_forms);
    let cleaned = Arc::new(compilation.cleaned_core);
    let mut builder = Builder {
        layout,
        terms: SharedVec::new(),
        nodes: SharedVec::new(),
        hashes: FastHashMap::default(),
        collisions: FastHashMap::default(),
        atoms: FastHashMap::default(),
        orders: BTreeMap::new(),
        values: FastHashMap::default(),
    };
    let source_forms = builder.intern(T::Nil, None);
    builder.source_atoms(&forms)?;
    let core = builder.core(&cleaned)?;
    let contract_variable_maps = builder.contracts(&forms)?;
    builder.terms.freeze_shared();
    builder.nodes.freeze_shared();
    Ok(CoreModule {
        name: compilation.module,
        source_forms,
        core,
        terms: builder.terms,
        nodes: builder.nodes,
        map_iterator_orders: builder.orders,
        contract_variable_maps,
    })
}

/// Emit typed nodes directly. Callers must apply CoreProgram validation before analysis.
pub fn module_direct(
    compilation: Compilation,
    layout: &NativeAtomLayout,
) -> Result<CoreModule, String> {
    let forms = Arc::new(compilation.source_forms);
    let cleaned = Arc::new(compilation.cleaned_core);
    let mut builder = Builder {
        layout,
        terms: SharedVec::new(),
        nodes: SharedVec::new(),
        hashes: FastHashMap::default(),
        collisions: FastHashMap::default(),
        atoms: FastHashMap::default(),
        orders: BTreeMap::new(),
        values: FastHashMap::default(),
    };
    let source_forms = builder.intern(T::Nil, None);
    builder.source_atoms(&forms)?;
    let core = builder.core_mode(&cleaned, true)?;
    let contract_variable_maps = builder.contracts(&forms)?;
    builder.terms.freeze_shared();
    builder.nodes.freeze_shared();
    Ok(CoreModule {
        name: compilation.module,
        source_forms,
        core,
        terms: builder.terms,
        nodes: builder.nodes,
        map_iterator_orders: builder.orders,
        contract_variable_maps,
    })
}

/// Fuse lazy cleanup with typed emission; validate the assembled program before analysis.
pub fn module_view(
    module: String,
    source_forms: ErlangTerms,
    view: &CleanView,
    layout: &NativeAtomLayout,
) -> Result<CoreModule, String> {
    let forms = Arc::new(source_forms);
    let mut builder = Builder {
        layout,
        terms: SharedVec::new(),
        nodes: SharedVec::new(),
        hashes: FastHashMap::default(),
        collisions: FastHashMap::default(),
        atoms: FastHashMap::default(),
        orders: BTreeMap::new(),
        values: FastHashMap::default(),
    };
    let source_forms = builder.intern(T::Nil, None);
    builder.source_atoms(&forms)?;
    let core = builder.core_view(view)?;
    let contract_variable_maps = builder.contracts(&forms)?;
    builder.terms.freeze_shared();
    builder.nodes.freeze_shared();
    Ok(CoreModule {
        name: module,
        source_forms,
        core,
        terms: builder.terms,
        nodes: builder.nodes,
        map_iterator_orders: builder.orders,
        contract_variable_maps,
    })
}

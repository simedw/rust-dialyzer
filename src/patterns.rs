use crate::{
    core::{CoreExpr, ErlangTerm, TermId},
    dataflow_key::{DataflowKey, SyntheticLabels},
    dataflow_nodes::DataflowNodes,
    program::{ModuleId, NodeRef},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatternRef {
    Core(NodeRef),
    Refold(usize),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiteralRef {
    Term {
        module: ModuleId,
        term: TermId,
    },
    ListSuffix {
        module: ModuleId,
        term: TermId,
        index: usize,
    },
}
#[derive(Clone, Copy, Debug)]
pub enum PairOperation {
    Node(PatternRef),
    Exact,
}
#[cfg(otp_domain = "29")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeRecordIdentity {
    Any,
    Local(String),
    Qualified { module: String, name: String },
}
#[cfg(otp_domain = "29")]
impl NativeRecordIdentity {
    pub fn parse(terms: &crate::shared_vec::SharedVec<ErlangTerm>, id: TermId) -> Result<Self, String> {
        Ok(match terms.get(id.0) {
            Some(ErlangTerm::Nil) => Self::Any,
            Some(ErlangTerm::Atom(name)) => Self::Local(name.clone()),
            Some(ErlangTerm::Tuple(pair)) if pair.len() == 2 => match (&terms[pair[0].0], &terms[pair[1].0]) {
                (ErlangTerm::Atom(module), ErlangTerm::Atom(name)) => Self::Qualified { module: module.clone(), name: name.clone() },
                _ => return Err("native record qualified id must contain atoms".into()),
            },
            _ => return Err("native record id must be [], an atom or module/name".into()),
        })
    }
}
#[derive(Clone, Debug)]
pub enum Shape {
    Alias {
        variable: PatternRef,
        pattern: PatternRef,
    },
    Binary(Vec<PatternRef>),
    Segment {
        value: PatternRef,
        size: PatternRef,
        unit: PatternRef,
        typ: PatternRef,
        flags: PatternRef,
    },
    Cons {
        head: PatternRef,
        tail: PatternRef,
    },
    Literal(LiteralRef),
    Map(Vec<PatternRef>),
    Pair {
        operation: PairOperation,
        key: PatternRef,
        value: PatternRef,
    },
    Tuple(Vec<PatternRef>),
    Values(Vec<PatternRef>),
    Variable {
        key: DataflowKey,
        original: NodeRef,
    },
    Other,
    #[cfg(otp_domain = "29")]
    Record { identity: NativeRecordIdentity, pairs: Vec<PatternRef> },
    #[cfg(otp_domain = "29")]
    RecordPair { key: PatternRef, value: PatternRef },
}
#[derive(Clone, Debug)]
pub struct RefoldNode {
    pub shape: Shape,
    pub key: Option<DataflowKey>,
    pub annotation: Option<NodeRef>,
    pub origin: LiteralRef,
}
#[derive(Clone, Debug)]
pub enum RefoldError {
    Boundary(String),
    ResourceLimit { depth: usize, maximum: usize },
}
impl From<String> for RefoldError {
    fn from(e: String) -> Self {
        Self::Boundary(e)
    }
}
impl From<&str> for RefoldError {
    fn from(e: &str) -> Self {
        Self::Boundary(e.into())
    }
}
pub struct Patterns<'a, 'b> {
    pub nodes: &'a DataflowNodes<'b>,
    pub refolded: Vec<RefoldNode>,
    labels: SyntheticLabels,
    depths: std::collections::BTreeMap<ModuleId, Vec<usize>>,
    suffixes: std::collections::BTreeMap<(ModuleId, TermId), Vec<usize>>,
}
impl<'a, 'b> Patterns<'a, 'b> {
    pub fn new(nodes: &'a DataflowNodes<'b>) -> Self {
        Self::with_allocator(nodes, SyntheticLabels::default())
    }
    pub fn with_allocator(nodes: &'a DataflowNodes<'b>, labels: SyntheticLabels) -> Self {
        Self {
            nodes,
            refolded: Vec::new(),
            labels,
            depths: std::collections::BTreeMap::new(),
            suffixes: std::collections::BTreeMap::new(),
        }
    }
    pub fn allocation_count(&self) -> u64 {
        self.labels.allocations()
    }
    pub fn allocator(&self) -> &SyntheticLabels {
        &self.labels
    }
    pub fn set_allocator(&mut self, labels: SyntheticLabels) {
        self.labels = labels;
    }
    pub fn allocation_profile(&self) -> crate::dataflow_key::SyntheticAllocationProfile {
        self.labels.profile()
    }
    fn push(
        &mut self,
        shape: Shape,
        origin: LiteralRef,
        labeled: bool,
    ) -> Result<PatternRef, String> {
        let key = if labeled {
            Some(self.labels.allocate()?.into())
        } else {
            None
        };
        let id = self.refolded.len();
        self.refolded.push(RefoldNode {
            shape,
            key,
            annotation: None,
            origin,
        });
        Ok(PatternRef::Refold(id))
    }
    pub fn shape(&self, pat: PatternRef) -> Result<Shape, String> {
        Ok(match pat {
            PatternRef::Refold(id) => self
                .refolded
                .get(id)
                .ok_or("refold node outside arena")?
                .shape
                .clone(),
            PatternRef::Core(node) => {
                let p = |id: crate::core::NodeId| {
                    PatternRef::Core(NodeRef {
                        module: node.module,
                        node: id.0,
                    })
                };
                match self.nodes.expression(node)? {
                    CoreExpr::Alias { variable, pattern } => Shape::Alias {
                        variable: p(*variable),
                        pattern: p(*pattern),
                    },
                    CoreExpr::Binary(xs) => Shape::Binary(xs.iter().map(|&x| p(x)).collect()),
                    CoreExpr::Bitstr {
                        value,
                        size,
                        unit,
                        typ,
                        flags,
                    } => Shape::Segment {
                        value: p(*value),
                        size: p(*size),
                        unit: p(*unit),
                        typ: p(*typ),
                        flags: p(*flags),
                    },
                    CoreExpr::Cons { head, tail } => Shape::Cons {
                        head: p(*head),
                        tail: p(*tail),
                    },
                    CoreExpr::Literal(term) => Shape::Literal(LiteralRef::Term {
                        module: node.module,
                        term: *term,
                    }),
                    CoreExpr::Map { pairs, .. } => {
                        Shape::Map(pairs.iter().map(|&x| p(x)).collect())
                    }
                    #[cfg(otp_domain = "29")]
                    CoreExpr::Record { id, pairs, .. } => {
                        let CoreExpr::Literal(term) = self.nodes.expression(NodeRef { module: node.module, node: id.0 })? else { return Err("native record identity is not literal".into()); };
                        Shape::Record { identity: NativeRecordIdentity::parse(&self.nodes.module(node.module)?.terms, *term)?, pairs: pairs.iter().copied().map(p).collect() }
                    }
                    #[cfg(otp_domain = "29")]
                    CoreExpr::RecordPair { key, value } => Shape::RecordPair { key: p(*key), value: p(*value) },
                    CoreExpr::MapPair {
                        operation,
                        key,
                        value,
                    } => Shape::Pair {
                        operation: PairOperation::Node(p(*operation)),
                        key: p(*key),
                        value: p(*value),
                    },
                    CoreExpr::Tuple(xs) => Shape::Tuple(xs.iter().map(|&x| p(x)).collect()),
                    CoreExpr::Values(xs) => Shape::Values(xs.iter().map(|&x| p(x)).collect()),
                    CoreExpr::Var(_) => Shape::Variable {
                        key: self.nodes.label(node)?.into(),
                        original: node,
                    },
                    _ => Shape::Other,
                }
            }
        })
    }
    pub fn annotation(&self, pat: PatternRef) -> Option<NodeRef> {
        match pat {
            PatternRef::Core(n) => Some(n),
            PatternRef::Refold(id) => self.refolded[id].annotation,
        }
    }
    #[cfg(otp_domain = "29")]
    pub fn qualified_native_record(&mut self, source: PatternRef, identity: NativeRecordIdentity, pairs: Vec<PatternRef>) -> Result<PatternRef, String> {
        let origin = match source {
            PatternRef::Core(node) => LiteralRef::Term { module: node.module, term: self.nodes.module(node.module)?.source_forms },
            PatternRef::Refold(index) => self.refolded.get(index).ok_or("native record source outside patterns")?.origin,
        };
        // cerl:c_record/2 has no annotations or label. It is compiler-generated
        // and must not allocate a synthetic variable identity.
        self.push(Shape::Record { identity, pairs }, origin, false)
    }
    pub fn term(&self, module: ModuleId, id: TermId) -> Result<&ErlangTerm, String> {
        self.nodes
            .module(module)?
            .terms
            .get(id.0)
            .ok_or("literal term outside arena".into())
    }
    pub fn native_subtrees(&self, pat: PatternRef) -> Result<Vec<PatternRef>, String> {
        if let PatternRef::Core(node) = pat {
            return Ok(self
                .nodes
                .native_subtrees(node)?
                .into_iter()
                .map(PatternRef::Core)
                .collect());
        }
        Ok(match self.shape(pat)? {
            Shape::Alias { variable, pattern } => vec![variable, pattern],
            Shape::Binary(xs) | Shape::Map(xs) | Shape::Tuple(xs) | Shape::Values(xs) => xs,
            Shape::Cons { head, tail } => vec![head, tail],
            #[cfg(otp_domain = "29")]
            Shape::Record { pairs, .. } => pairs,
            #[cfg(otp_domain = "29")]
            Shape::RecordPair { key, value } => vec![key, value],
            Shape::Segment {
                value,
                size,
                unit,
                typ,
                flags,
            } => vec![value, size, unit, typ, flags],
            Shape::Pair {
                operation,
                key,
                value,
            } => {
                let mut xs = vec![];
                if let PairOperation::Node(op) = operation {
                    xs.push(op)
                }
                xs.extend([key, value]);
                xs
            }
            _ => vec![],
        })
    }
    pub fn literal_type(&self, lit: LiteralRef) -> Result<crate::lattice::Type, String> {
        match lit {
            LiteralRef::Term { module, term } => self.nodes.literal_type(module, term),
            LiteralRef::ListSuffix {
                module,
                term,
                index,
            } => {
                let ErlangTerm::List { items, tail } = self.term(module, term)? else {
                    return Err("invalid list suffix".into());
                };
                let suffix = items.get(index..).ok_or("list suffix outside term")?;
                let mut out = self.nodes.literal_type(module, *tail)?;
                for id in suffix.iter().rev() {
                    out = crate::lattice::Type::cons(self.nodes.literal_type(module, *id)?, out)?
                }
                Ok(out)
            }
        }
    }
    pub fn has_record_annotation(&self, pat: PatternRef) -> Result<bool, String> {
        let Some(node) = self.annotation(pat) else {
            return Ok(false);
        };
        let module = self.nodes.module(node.module)?;
        let ann = module
            .nodes
            .get(node.node)
            .ok_or("node outside arena")?
            .annotations;
        let ErlangTerm::List { items, .. } = self.term(node.module, ann)? else {
            return Ok(false);
        };
        for id in items {
            if matches!(self.term(node.module,*id)?,ErlangTerm::Atom(s) if s=="record") {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn literal(&self, pat: PatternRef) -> Result<LiteralRef, String> {
        if let Shape::Literal(lit) = self.shape(pat)? {
            Ok(lit)
        } else {
            Err("expected literal pattern node".into())
        }
    }
    pub fn contains_map(&self, lit: LiteralRef) -> Result<bool, String> {
        match lit {
            LiteralRef::Term { module, term } => self.nodes.contains_map(module, term),
            LiteralRef::ListSuffix {
                module,
                term,
                index,
            } => {
                let ErlangTerm::List { items, tail } = self.term(module, term)? else {
                    return Err("invalid list suffix".into());
                };
                let suffix = items.get(index..).ok_or("list suffix outside term")?;
                if self.nodes.contains_map(module, *tail)? {
                    return Ok(true);
                };
                for id in suffix {
                    if self.nodes.contains_map(module, *id)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        }
    }
    fn children(&self, lit: LiteralRef) -> Result<(Vec<LiteralRef>, u8), String> {
        let (module, term, start) = match lit {
            LiteralRef::Term { module, term } => (module, term, 0),
            LiteralRef::ListSuffix {
                module,
                term,
                index,
            } => (module, term, index),
        };
        let tref = |term| LiteralRef::Term { module, term };
        Ok(match self.term(module, term)? {
            ErlangTerm::Tuple(xs) => (xs.iter().map(|&id| tref(id)).collect(), 0),
            ErlangTerm::List { items, tail } if start < items.len() => {
                let rest = if start + 1 == items.len() {
                    tref(*tail)
                } else {
                    LiteralRef::ListSuffix {
                        module,
                        term,
                        index: start + 1,
                    }
                };
                (vec![tref(items[start]), rest], 1)
            }
            ErlangTerm::Map(_) => (
                self.nodes
                    .module(module)?
                    .map_iterator_pairs(term)?
                    .map(|(_, val)| tref(val))
                    .collect(),
                2,
            ),
            _ => return Err("refold requested for nonstructural literal".into()),
        })
    }
    pub fn refold_depth(&mut self, lit: LiteralRef) -> Result<usize, String> {
        let (module, term, index) = match lit {
            LiteralRef::Term { module, term } => (module, term, None),
            LiteralRef::ListSuffix {
                module,
                term,
                index,
            } => (module, term, Some(index)),
        };
        if !self.depths.contains_key(&module) {
            let arena = self.nodes.module(module)?;
            let mut depths = Vec::with_capacity(arena.terms.len());
            for (i, value) in arena.terms.iter().enumerate() {
                let id = TermId(i);
                let depth = if !self.nodes.contains_map(module, id)? {
                    0
                } else {
                    match value {
                        ErlangTerm::Map(entries) => {
                            1 + entries.iter().map(|(_, v)| depths[v.0]).max().unwrap_or(0)
                        }
                        ErlangTerm::Tuple(items) => {
                            1 + items.iter().map(|v| depths[v.0]).max().unwrap_or(0)
                        }
                        ErlangTerm::List { items, tail } => {
                            let mut ds = vec![0; items.len() + 1];
                            ds[items.len()] = depths[tail.0];
                            let mut contains = self.nodes.contains_map(module, *tail)?;
                            for ix in (0..items.len()).rev() {
                                contains =
                                    contains || self.nodes.contains_map(module, items[ix])?;
                                ds[ix] = if contains {
                                    1 + depths[items[ix].0].max(ds[ix + 1])
                                } else {
                                    0
                                };
                            }
                            let depth = ds[0];
                            self.suffixes.insert((module, id), ds);
                            depth
                        }
                        _ => 0,
                    }
                };
                depths.push(depth.min(129));
            }
            self.depths.insert(module, depths);
        }
        if let Some(index) = index {
            self.suffixes
                .get(&(module, term))
                .and_then(|xs| xs.get(index))
                .copied()
                .ok_or("list suffix depth outside term".into())
        } else {
            self.depths[&module]
                .get(term.0)
                .copied()
                .ok_or("term depth outside arena".into())
        }
    }
    pub fn refold(&mut self, pat: PatternRef) -> Result<PatternRef, RefoldError> {
        let Shape::Literal(root) = self.shape(pat)? else {
            return Ok(pat);
        };
        if !self.contains_map(root)? {
            return Ok(pat);
        };
        let depth = self.refold_depth(root)?;
        if depth > 128 {
            return Err(RefoldError::ResourceLimit {
                depth,
                maximum: 128,
            });
        };
        enum Task {
            Visit(LiteralRef),
            Finish {
                lit: LiteralRef,
                kind: u8,
                count: usize,
            },
        }
        let mut pending = vec![Task::Visit(root)];
        let mut results = Vec::new();
        while let Some(task) = pending.pop() {
            match task {
                Task::Visit(lit) => {
                    if !self.contains_map(lit)? {
                        results.push(self.push(Shape::Literal(lit), lit, false)?)
                    } else {
                        let (children, kind) = self.children(lit)?;
                        pending.push(Task::Finish {
                            lit,
                            kind,
                            count: children.len(),
                        });
                        pending.extend(children.into_iter().rev().map(Task::Visit));
                    }
                }
                Task::Finish { lit, kind, count } => {
                    let start = results
                        .len()
                        .checked_sub(count)
                        .ok_or("refold result stack")?;
                    let children = results.split_off(start);
                    let shape = match kind {
                        0 => Shape::Tuple(children),
                        1 => Shape::Cons {
                            head: children[0],
                            tail: children[1],
                        },
                        2 => {
                            let LiteralRef::Term { module, term } = lit else {
                                return Err("map suffix".into());
                            };
                            let ErlangTerm::Map(_) = self.term(module, term)? else {
                                return Err("map refold shape".into());
                            };
                            let keys: Vec<_> = self
                                .nodes
                                .module(module)?
                                .map_iterator_pairs(term)?
                                .map(|(key, _)| key)
                                .collect();
                            let mut pairs = Vec::new();
                            for (key, value) in keys.into_iter().zip(children) {
                                let keylit = LiteralRef::Term { module, term: key };
                                let key = self.push(Shape::Literal(keylit), keylit, false)?;
                                let pair = self.push(
                                    Shape::Pair {
                                        operation: PairOperation::Exact,
                                        key,
                                        value,
                                    },
                                    lit,
                                    false,
                                )?;
                                pairs.push(pair)
                            }
                            Shape::Map(pairs)
                        }
                        _ => return Err("refold constructor".into()),
                    };
                    results.push(self.push(shape, lit, true)?);
                }
            }
        }
        let output = results.pop().ok_or("empty refold result")?;
        if let PatternRef::Refold(id) = output {
            self.refolded[id].annotation = self.annotation(pat);
            if let PatternRef::Core(node) = pat
                && let Some(label) = self.nodes.optional_label(node)?
            {
                self.refolded[id].key = Some(label.into())
            }
        }
        Ok(output)
    }
}

impl std::hash::Hash for PatternRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Self::Core(n) => {
                0u8.hash(state);
                n.module.0.hash(state);
                n.node.hash(state)
            }
            Self::Refold(n) => {
                1u8.hash(state);
                n.hash(state)
            }
        }
    }
}

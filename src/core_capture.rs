use crate::{
    core::{CoreExpr, CoreModule, CoreNode, CoreProgram, ErlangTerm, ErlangTerms, NodeId, TermId},
    labels::Labels,
    program::{ModuleId, NodeRef},
    term_order::{CaptureEnvironment, CaptureId, CaptureTerm, TermOrderArena},
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
/// Exact labeled Cerl record values, with flat ownership and shared ordinary terms.
pub struct CoreCaptures {
    terms: Vec<CaptureTerm>,
    nodes: Vec<CaptureId>,
}
impl CoreCaptures {
    pub fn build(core: &CoreProgram, labels: &Labels, module: ModuleId) -> Result<Self, String> {
        let source = core
            .modules
            .get(module.0)
            .ok_or("capture module outside Core arena")?;
        let mut terms = source
            .terms
            .iter()
            .map(|term| source_term(term, |id| CaptureId(id.0)))
            .collect();
        let mut writer = RecordWriter::new(&mut terms);
        let mut nodes = Vec::with_capacity(source.nodes.len());
        for node in &source.nodes {
            let id = writer.record(labels, module, source, node, &nodes, |id| CaptureId(id.0))?;
            nodes.push(id);
        }
        Ok(Self { terms, nodes })
    }
    /// `get(module)?.append_selected(nodes, target)` for a module that passes
    /// `check_module`, building only the records and source terms the selected
    /// nodes reach. Source terms keep their relative order and precede the
    /// synthetic ones, so conversion fails on the same term; synthetic terms
    /// may be laid out differently but are equal under term order.
    pub fn append_selected_from_source(
        core: &CoreProgram,
        labels: &Labels,
        module: ModuleId,
        nodes: &[NodeId],
        target: &mut Vec<CaptureTerm>,
    ) -> Result<Vec<CaptureId>, String> {
        let source = core
            .modules
            .get(module.0)
            .ok_or("capture module outside Core arena")?;
        if nodes.iter().any(|n| n.0 >= source.nodes.len()) {
            return Err("capture node outside Core arena".into());
        }
        let mut needed = HashSet::new();
        let mut todo = nodes.to_vec();
        while let Some(id) = todo.pop() {
            if needed.insert(id.0) {
                todo.extend(record_children(&source.nodes[id.0].expr));
            }
        }
        let mut needed = needed.into_iter().collect::<Vec<_>>();
        needed.sort_unstable();
        let mut reached = HashSet::new();
        let mut todo = Vec::new();
        for &index in &needed {
            let node = &source.nodes[index];
            if let CoreExpr::Literal(id) | CoreExpr::Var(id) | CoreExpr::Opaque(id) = node.expr {
                todo.push(id);
            }
            if labels.is_reannotated(NodeRef {
                module,
                node: node.id.0,
            }) {
                annotation_items(source, node.annotations, |item| todo.push(item))?;
            } else {
                todo.push(node.annotations);
            }
        }
        while let Some(id) = todo.pop() {
            if !reached.insert(id.0) {
                continue;
            }
            match &source.terms[id.0] {
                ErlangTerm::Tuple(xs) => todo.extend(xs),
                ErlangTerm::List { items, tail } => {
                    todo.extend(items);
                    todo.push(*tail);
                }
                ErlangTerm::Map(ps) => {
                    for (k, v) in ps {
                        todo.extend([*k, *v]);
                    }
                }
                _ => {}
            }
        }
        let mut reached = reached.into_iter().collect::<Vec<_>>();
        reached.sort_unstable();
        let mut remap = HashMap::with_capacity(reached.len());
        for index in reached {
            let term = source_term(&source.terms[index], |id| remap[&id.0]);
            remap.insert(index, CaptureId(target.len()));
            target.push(term);
        }
        let mut writer = RecordWriter::new(target);
        let mut records = vec![CaptureId(usize::MAX); source.nodes.len()];
        for index in needed {
            records[index] =
                writer.record(labels, module, source, &source.nodes[index], &records, |id| {
                    remap[&id.0]
                })?;
        }
        Ok(nodes.iter().map(|n| records[n.0]).collect())
    }
    /// The error `build` would return for this module, without building it.
    pub fn check_module(core: &CoreProgram, labels: &Labels, module: ModuleId) -> Result<(), String> {
        let source = core
            .modules
            .get(module.0)
            .ok_or("capture module outside Core arena")?;
        for node in &source.nodes {
            let reference = NodeRef {
                module,
                node: node.id.0,
            };
            if !labels.is_reannotated(reference) {
                continue;
            }
            labels
                .label(reference)
                .ok_or("reannotated Core node has no label")?;
            let mut cursor = node.annotations;
            loop {
                match &source.terms[cursor.0] {
                    ErlangTerm::Nil => break,
                    ErlangTerm::List { tail, .. } => cursor = *tail,
                    _ => return Err("Core capture annotations are not a proper list".into()),
                }
            }
        }
        Ok(())
    }
    /// For a variable node of a module that passes `check_module`: the result of
    /// `append_selected(&[variable])` (Err) or the conversion error
    /// `TermOrderArena::from_captures` would raise on the copied terms (Ok).
    /// Only source leaves can fail conversion; terms `build` adds never do.
    pub fn selected_variable_error(
        core: &CoreProgram,
        labels: &Labels,
        module: ModuleId,
        variable: NodeId,
    ) -> Result<Option<String>, String> {
        let source = &core.modules[module.0];
        let node = source
            .nodes
            .get(variable.0)
            .ok_or("capture node outside Core arena")?;
        let CoreExpr::Var(name) = node.expr else {
            return Err("capture selection is not a variable".into());
        };
        let mut todo = vec![name];
        if labels.is_reannotated(NodeRef {
            module,
            node: node.id.0,
        }) {
            let mut cursor = node.annotations;
            while let ErlangTerm::List { items, tail } = &source.terms[cursor.0] {
                todo.extend(items.iter().copied().filter(|item| !matches!(&source.terms[item.0], ErlangTerm::Tuple(pair) if pair.len() == 2 && matches!(&source.terms[pair[0].0], ErlangTerm::Atom(a) if a == "label"))));
                cursor = *tail;
            }
        } else {
            todo.push(node.annotations);
        }
        let mut visited = HashSet::new();
        while let Some(id) = todo.pop() {
            if !visited.insert(id.0) {
                continue;
            }
            match &source.terms[id.0] {
                ErlangTerm::Tuple(xs) => todo.extend(xs),
                ErlangTerm::List { items, tail } => {
                    todo.extend(items);
                    todo.push(*tail);
                }
                ErlangTerm::Map(ps) => {
                    for (k, v) in ps {
                        todo.extend([*k, *v]);
                    }
                }
                _ => {}
            }
        }
        let mut indices = visited.into_iter().collect::<Vec<_>>();
        indices.sort_unstable();
        Ok(indices.into_iter().find_map(|index| match &source.terms[index] {
            ErlangTerm::Integer(n) if n.parse::<num_bigint::BigInt>().is_err() => {
                Some("invalid capture integer".to_owned())
            }
            ErlangTerm::Float(bits) if !f64::from_bits(*bits).is_finite() => {
                Some("nonfinite capture float".to_owned())
            }
            ErlangTerm::Bitstring { bytes, bits } if *bits > bytes.len() as u64 * 8 => {
                Some("capture bitstring outside bytes".to_owned())
            }
            _ => None,
        }))
    }
    /// Copies only the selected source values and their reachable dependencies.
    pub fn append_selected(
        &self,
        nodes: &[NodeId],
        target: &mut Vec<CaptureTerm>,
    ) -> Result<Vec<CaptureId>, String> {
        let roots = nodes
            .iter()
            .map(|n| {
                self.nodes
                    .get(n.0)
                    .copied()
                    .ok_or("capture node outside Core arena".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut visited = HashSet::new();
        let mut todo = roots.clone();
        while let Some(id) = todo.pop() {
            if !visited.insert(id.0) {
                continue;
            }
            match &self.terms[id.0] {
                CaptureTerm::Tuple(xs) => todo.extend(xs),
                CaptureTerm::List { items, tail } => {
                    todo.extend(items);
                    todo.push(*tail);
                }
                CaptureTerm::Map(ps) => {
                    for (k, v) in ps {
                        todo.extend([*k, *v]);
                    }
                }
                _ => {}
            }
        }
        let mut indices = visited.into_iter().collect::<Vec<_>>();
        indices.sort_unstable();
        let mut remap = HashMap::new();
        for index in indices {
            let id = |id: CaptureId| remap[&id.0];
            let value = match &self.terms[index] {
                CaptureTerm::Tuple(xs) => CaptureTerm::Tuple(xs.iter().copied().map(id).collect()),
                CaptureTerm::List { items, tail } => CaptureTerm::List {
                    items: items.iter().copied().map(id).collect(),
                    tail: id(*tail),
                },
                CaptureTerm::Map(ps) => {
                    CaptureTerm::Map(ps.iter().map(|(k, v)| (id(*k), id(*v))).collect())
                }
                value => value.clone(),
            };
            let id = CaptureId(target.len());
            target.push(value);
            remap.insert(index, id);
        }
        Ok(roots.iter().map(|id| remap[&id.0]).collect())
    }
    pub fn source_terms(&self, nodes: &[NodeId]) -> Result<ErlangTerms, String> {
        let mut terms = vec![CaptureTerm::Nil];
        let items = self.append_selected(nodes, &mut terms)?;
        let root = TermId(terms.len());
        terms.push(CaptureTerm::List {
            items,
            tail: CaptureId(0),
        });
        let id = |id: CaptureId| TermId(id.0);
        let terms = terms
            .into_iter()
            .map(|term| {
                Ok(match term {
                    CaptureTerm::Integer(n) => ErlangTerm::Integer(n),
                    CaptureTerm::Float(n) => ErlangTerm::Float(n),
                    CaptureTerm::Atom(a) => ErlangTerm::Atom(a),
                    CaptureTerm::Tuple(xs) => ErlangTerm::Tuple(xs.into_iter().map(id).collect()),
                    CaptureTerm::Map(ps) => {
                        ErlangTerm::Map(ps.into_iter().map(|(k, v)| (id(k), id(v))).collect())
                    }
                    CaptureTerm::Nil => ErlangTerm::Nil,
                    CaptureTerm::List { items, tail } => ErlangTerm::List {
                        items: items.into_iter().map(id).collect(),
                        tail: id(tail),
                    },
                    CaptureTerm::Binary(bytes) => ErlangTerm::Binary(bytes),
                    CaptureTerm::Bitstring { bytes, bits } => ErlangTerm::Bitstring { bytes, bits },
                    CaptureTerm::Function(crate::term_order::CapturedFunction::External {
                        module,
                        function,
                        arity,
                    }) => ErlangTerm::ExternalFun(Box::new(crate::core::ExternalFun {
                        module,
                        function,
                        arity: u8::try_from(arity)
                            .map_err(|_| "external function arity exceeds 255")?,
                    })),
                    CaptureTerm::Function(_) | CaptureTerm::Runtime { .. } => {
                        return Err("Core source capture contains a runtime handle".into());
                    }
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(ErlangTerms { root, terms: terms.into() })
    }
    pub fn environment(&self, nodes: &[NodeId]) -> Result<CaptureEnvironment, String> {
        let mut terms = vec![CaptureTerm::Nil];
        let items = self.append_selected(nodes, &mut terms)?;
        let root = CaptureId(terms.len());
        terms.push(CaptureTerm::List {
            items,
            tail: CaptureId(0),
        });
        CaptureEnvironment::new(Arc::new(TermOrderArena::from_captures(&terms)?), root)
    }
}
fn source_term(term: &ErlangTerm, id: impl Fn(TermId) -> CaptureId) -> CaptureTerm {
    match term {
        ErlangTerm::ExternalFun(external_fun_payload) => {
            let crate::core::ExternalFun {
                module,
                function,
                arity,
            } = external_fun_payload.as_ref();
            CaptureTerm::Function(crate::term_order::CapturedFunction::External {
                module: module.clone(),
                function: function.clone(),
                arity: usize::from(*arity),
            })
        }
        ErlangTerm::Atom(a) => CaptureTerm::Atom(a.clone()),
        ErlangTerm::Integer(n) => CaptureTerm::Integer(n.clone()),
        ErlangTerm::Float(n) => CaptureTerm::Float(*n),
        ErlangTerm::Binary(bytes) => CaptureTerm::Binary(bytes.clone()),
        ErlangTerm::Bitstring { bytes, bits } => CaptureTerm::Bitstring {
            bytes: bytes.clone(),
            bits: *bits,
        },
        ErlangTerm::Nil => CaptureTerm::Nil,
        ErlangTerm::Tuple(xs) => CaptureTerm::Tuple(xs.iter().copied().map(id).collect()),
        ErlangTerm::List { items, tail } => CaptureTerm::List {
            items: items.iter().copied().map(&id).collect(),
            tail: id(*tail),
        },
        ErlangTerm::Map(ps) => CaptureTerm::Map(ps.iter().map(|(k, v)| (id(*k), id(*v))).collect()),
    }
}
/// The nodes whose records a node's record refers to.
fn record_children(expr: &CoreExpr) -> Vec<NodeId> {
    match expr {
        CoreExpr::Map {
            pairs,
            pattern: true,
            ..
        } => pairs.clone(),
        _ => expr.children(),
    }
}
/// Annotation items kept on a reannotated node: all but `{label, _}` pairs.
fn annotation_items(
    source: &CoreModule,
    annotations: TermId,
    mut keep: impl FnMut(TermId),
) -> Result<(), String> {
    let mut cursor = annotations;
    loop {
        match &source.terms[cursor.0] {
            ErlangTerm::Nil => return Ok(()),
            ErlangTerm::List { items, tail } => {
                items
                    .iter()
                    .copied()
                    .filter(|item| !matches!(&source.terms[item.0], ErlangTerm::Tuple(pair) if pair.len() == 2 && matches!(&source.terms[pair[0].0], ErlangTerm::Atom(a) if a == "label")))
                    .for_each(&mut keep);
                cursor = *tail;
            }
            _ => return Err("Core capture annotations are not a proper list".into()),
        }
    }
}
/// Appends labeled Cerl node records to a capture term list.
struct RecordWriter<'t> {
    terms: &'t mut Vec<CaptureTerm>,
    nil: CaptureId,
    label_tag: CaptureId,
    empty_map_literal: CaptureId,
    tags: HashMap<&'static str, CaptureId>,
}
impl<'t> RecordWriter<'t> {
    fn new(terms: &'t mut Vec<CaptureTerm>) -> Self {
        let mut push = |value| {
            terms.push(value);
            CaptureId(terms.len() - 1)
        };
        let nil = push(CaptureTerm::Nil);
        let label_tag = push(CaptureTerm::Atom("label".into()));
        let empty_map = push(CaptureTerm::Map(vec![]));
        let literal_tag = push(CaptureTerm::Atom("c_literal".into()));
        let empty_map_literal = push(CaptureTerm::Tuple(vec![literal_tag, nil, empty_map]));
        Self {
            terms,
            nil,
            label_tag,
            empty_map_literal,
            tags: HashMap::new(),
        }
    }
    fn push(&mut self, value: CaptureTerm) -> CaptureId {
        let id = CaptureId(self.terms.len());
        self.terms.push(value);
        id
    }
    fn list(&mut self, items: Vec<CaptureId>) -> CaptureId {
        self.push(CaptureTerm::List {
            items,
            tail: self.nil,
        })
    }
    fn pairs(&mut self, pairs: Vec<(CaptureId, CaptureId)>) -> CaptureId {
        let pairs = pairs
            .into_iter()
            .map(|(a, b)| self.push(CaptureTerm::Tuple(vec![a, b])))
            .collect();
        self.list(pairs)
    }
    /// The record of `node`; `records` holds the records of its children and
    /// `term` maps source terms to their copies.
    fn record(
        &mut self,
        labels: &Labels,
        module: ModuleId,
        source: &CoreModule,
        node: &CoreNode,
        records: &[CaptureId],
        term: impl Fn(TermId) -> CaptureId,
    ) -> Result<CaptureId, String> {
        let reference = NodeRef {
            module,
            node: node.id.0,
        };
        let annotation = if labels.is_reannotated(reference) {
            let label = labels
                .label(reference)
                .ok_or("reannotated Core node has no label")?;
            let number = self.push(CaptureTerm::Integer(label.0.to_string()));
            let label = self.push(CaptureTerm::Tuple(vec![self.label_tag, number]));
            let mut annotations = vec![label];
            annotation_items(source, node.annotations, |item| annotations.push(term(item)))?;
            self.list(annotations)
        } else {
            term(node.annotations)
        };
        let kind = node.expr.kind();
        let tag = match self.tags.get(kind) {
            Some(tag) => *tag,
            None => {
                let tag = self.push(CaptureTerm::Atom(format!("c_{kind}")));
                self.tags.insert(kind, tag);
                tag
            }
        };
        let child = |id: NodeId| records[id.0];
        let fields = match &node.expr {
            CoreExpr::Literal(id) | CoreExpr::Var(id) | CoreExpr::Opaque(id) => {
                vec![term(*id)]
            }
            CoreExpr::Values(xs) | CoreExpr::Tuple(xs) | CoreExpr::Binary(xs) => {
                let xs = xs.iter().copied().map(child).collect();
                vec![self.list(xs)]
            }
            CoreExpr::Bitstr {
                value,
                size,
                unit,
                typ,
                flags,
            } => vec![
                child(*value),
                child(*size),
                child(*unit),
                child(*typ),
                child(*flags),
            ],
            CoreExpr::Cons { head, tail } => vec![child(*head), child(*tail)],
            CoreExpr::Map {
                base,
                pairs,
                pattern,
            } => {
                let base = if *pattern {
                    self.empty_map_literal
                } else {
                    child(*base)
                };
                let pairs = pairs.iter().copied().map(child).collect();
                let pairs = self.list(pairs);
                let pattern = self.push(CaptureTerm::Atom(pattern.to_string()));
                vec![base, pairs, pattern]
            }
            CoreExpr::MapPair {
                operation,
                key,
                value,
            } => vec![child(*operation), child(*key), child(*value)],
            CoreExpr::Let {
                vars,
                argument,
                body,
            } => {
                let argument = child(*argument);
                let body = child(*body);
                let vars = vars.iter().copied().map(child).collect();
                vec![self.list(vars), argument, body]
            }
            CoreExpr::Seq { argument, body } => vec![child(*argument), child(*body)],
            CoreExpr::Apply { operator, args } => {
                let operator = child(*operator);
                let args = args.iter().copied().map(child).collect();
                vec![operator, self.list(args)]
            }
            CoreExpr::Call { module, name, args } => {
                let module = child(*module);
                let name = child(*name);
                let args = args.iter().copied().map(child).collect();
                vec![module, name, self.list(args)]
            }
            CoreExpr::Primop { name, args } => {
                let name = child(*name);
                let args = args.iter().copied().map(child).collect();
                vec![name, self.list(args)]
            }
            CoreExpr::Case { argument, clauses } => {
                let argument = child(*argument);
                let clauses = clauses.iter().copied().map(child).collect();
                vec![argument, self.list(clauses)]
            }
            CoreExpr::Clause {
                patterns,
                guard,
                body,
            } => {
                let guard = child(*guard);
                let body = child(*body);
                let patterns = patterns.iter().copied().map(child).collect();
                vec![self.list(patterns), guard, body]
            }
            CoreExpr::Alias { variable, pattern } => vec![child(*variable), child(*pattern)],
            CoreExpr::Fun { params, body } => {
                let body = child(*body);
                let params = params.iter().copied().map(child).collect();
                vec![self.list(params), body]
            }
            CoreExpr::Receive {
                clauses,
                timeout,
                action,
            } => {
                let timeout = child(*timeout);
                let action = child(*action);
                let clauses = clauses.iter().copied().map(child).collect();
                vec![self.list(clauses), timeout, action]
            }
            CoreExpr::Try(try_payload) => {
                let crate::core::TryExpr {
                    argument,
                    vars,
                    body,
                    exception_vars,
                    handler,
                } = try_payload.as_ref();

                let argument = child(*argument);
                let body = child(*body);
                let handler = child(*handler);
                let vars = vars.iter().copied().map(child).collect();
                let exceptions = exception_vars.iter().copied().map(child).collect();
                let vars = self.list(vars);
                let exceptions = self.list(exceptions);
                vec![argument, vars, body, exceptions, handler]
            }
            CoreExpr::Catch { body } => vec![child(*body)],
            CoreExpr::Letrec { definitions, body } => {
                let body = child(*body);
                let definitions = definitions
                    .iter()
                    .map(|(v, f)| (child(*v), child(*f)))
                    .collect::<Vec<_>>();
                vec![self.pairs(definitions), body]
            }
            CoreExpr::Module(module_payload) => {
                let crate::core::ModuleExpr {
                    name,
                    exports,
                    attributes,
                    definitions,
                } = module_payload.as_ref();

                let name = child(*name);
                let exports = exports.iter().copied().map(child).collect();
                let attributes = attributes
                    .iter()
                    .map(|(k, v)| (child(*k), child(*v)))
                    .collect::<Vec<_>>();
                let definitions = definitions
                    .iter()
                    .map(|(v, f)| (child(*v), child(*f)))
                    .collect::<Vec<_>>();
                let exports = self.list(exports);
                let attributes = self.pairs(attributes);
                let definitions = self.pairs(definitions);
                vec![name, exports, attributes, definitions]
            }
        };
        let mut record = vec![tag, annotation];
        record.extend(fields);
        Ok(self.push(CaptureTerm::Tuple(record)))
    }
}

mod clean;
pub mod frontend;
use firefly_binary::{BinaryEntrySpecifier as Spec, Bitstring, Endianness};
use firefly_diagnostics::{CodeMap, SourceId, SourceSpan, Spanned};
use firefly_syntax_base::{Annotated, Annotations, Lit, Literal, MapOp, Var};
use firefly_syntax_core::{Clause, Expr, Module};
use native_beam_seed::{PreparedCoreSeed, SourceOrigin};
use rust_dialyzer_poc::core::{CoreExpr, CoreModule, CoreNode, ErlangTerm, NodeId, TermId};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Seed contains untouched original terms/source_forms and independently reconstructed
/// raw-form attributes. Seed nodes and core are never consulted.
pub fn adapt(
    module: &Module,
    codemap: &CodeMap,
    seed: PreparedCoreSeed,
) -> Result<CoreModule, String> {
    let source_forms = seed.source_forms;
    let attrs = seed.attributes;
    let mut a = Adapter {
        codemap,
        source_column_offset: seed.source_column_offset,
        terms: seed.terms,
        emitted_terms: HashMap::new(),
        filename_terms: HashMap::new(),
        nodes: vec![],
        variables: BTreeMap::new(),
        next_variable: 0,
        source_variables: seed.source_variables,
        source_origins: seed.source_origins,
    };
    let empty = a.list(vec![]);
    let name_term = a.atom(&module.name.name.to_string());
    let name = a.literal_node(name_term, empty);
    let mut exports: Vec<_> = module.exports.iter().collect();
    exports.sort_by_key(|e| (e.item.function.to_string(), e.item.arity));
    let mut export_nodes = vec![];
    for e in exports {
        export_nodes.push(a.function_var(
            &e.item.function.to_string(),
            e.item.arity as usize,
            empty,
        ));
    }
    let mut attribute_nodes = vec![];
    for attr in attrs {
        let key = attr.name.as_str();
        let value = attr.value.0;
        let ann = attr.annotations.0;
        if value >= a.terms.len() || ann >= a.terms.len() {
            return Err("attribute term outside seed arena".into());
        }
        let key = a.atom(key);
        attribute_nodes.push(a.literal_node(key, ann));
        attribute_nodes.push(a.literal_node(value, ann));
    }
    let mut definitions = vec![];
    for (name, function) in &module.functions {
        definitions.push(a.function_var(&name.function.to_string(), name.arity as usize, empty));
        definitions.push(
            if name.function.to_string() == "module_info" && name.arity <= 1 {
                a.predefined_module_info(&module.name.name.to_string(), name.arity as usize)?
            } else {
                a.expr(&Expr::Fun(function.fun.clone()))?
            },
        );
    }
    let root = a.node(
        "module",
        empty,
        vec![vec![name], export_nodes, attribute_nodes, definitions],
    );
    let mut map_iterator_orders = seed.map_iterator_orders;
    for (i, term) in a.terms.iter().enumerate() {
        if matches!(term,ErlangTerm::Map(v) if v.is_empty()) {
            map_iterator_orders.entry(TermId(i)).or_default();
        }
    }
    Ok(CoreModule {
        name: module.name.name.to_string(),
        source_forms,
        core: NodeId(root),
        terms: a.terms,
        nodes: a.nodes,
        contract_variable_maps: seed.contract_variable_maps,
        map_iterator_orders,
    })
}

struct Adapter<'a> {
    codemap: &'a CodeMap,
    source_column_offset: usize,
    terms: Vec<ErlangTerm>,
    emitted_terms: HashMap<ErlangTerm, usize>,
    filename_terms: HashMap<SourceId, usize>,
    nodes: Vec<CoreNode>,
    variables: BTreeMap<String, usize>,
    next_variable: usize,
    source_variables: BTreeSet<String>,
    source_origins: Vec<SourceOrigin>,
}
impl Adapter<'_> {
    fn source_column(&self, synthetic_column: usize) -> Option<usize> {
        (synthetic_column + 1)
            .checked_sub(self.source_column_offset)
            .filter(|column| *column != 0)
    }
    fn term(&mut self, value: ErlangTerm) -> usize {
        if let Some(&id) = self.emitted_terms.get(&value) {
            return id;
        }
        let id = self.terms.len();
        self.emitted_terms.insert(value.clone(), id);
        self.terms.push(value);
        id
    }
    fn atom(&mut self, value: &str) -> usize {
        self.term(ErlangTerm::Atom(value.into()))
    }
    fn integer(&mut self, value: impl ToString) -> usize {
        self.term(ErlangTerm::Integer(value.to_string()))
    }
    fn list(&mut self, items: Vec<usize>) -> usize {
        let nil = self.term(ErlangTerm::Nil);
        if items.is_empty() {
            nil
        } else {
            self.term(ErlangTerm::List {
                items: items.into_iter().map(TermId).collect(),
                tail: TermId(nil),
            })
        }
    }
    fn tuple(&mut self, items: Vec<usize>) -> usize {
        self.term(ErlangTerm::Tuple(items.into_iter().map(TermId).collect()))
    }
    fn node(&mut self, kind: &str, annotations: usize, groups: Vec<Vec<usize>>) -> usize {
        let id = self.nodes.len();
        let g = groups
            .into_iter()
            .map(|g| g.into_iter().map(NodeId).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        let one = |i: usize| g[i][0];
        let pairs = |i: usize| g[i].chunks_exact(2).map(|p| (p[0], p[1])).collect();
        let expr = match kind {
            "values" => CoreExpr::Values(g[0].clone()),
            "binary" => CoreExpr::Binary(g[0].clone()),
            "tuple" => CoreExpr::Tuple(g[0].clone()),
            "bitstr" => CoreExpr::Bitstr {
                value: one(0),
                size: one(1),
                unit: one(2),
                typ: one(3),
                flags: one(4),
            },
            "cons" => CoreExpr::Cons {
                head: one(0),
                tail: one(1),
            },
            "seq" => CoreExpr::Seq {
                argument: one(0),
                body: one(1),
            },
            "alias" => CoreExpr::Alias {
                variable: one(0),
                pattern: one(1),
            },
            "let" => CoreExpr::Let {
                vars: g[0].clone(),
                argument: one(1),
                body: one(2),
            },
            "apply" => CoreExpr::Apply {
                operator: one(0),
                args: g[1].clone(),
            },
            "call" => CoreExpr::Call {
                module: one(0),
                name: one(1),
                args: g[2].clone(),
            },
            "primop" => CoreExpr::Primop {
                name: one(0),
                args: g[1].clone(),
            },
            "case" => CoreExpr::Case {
                argument: one(0),
                clauses: g[1].clone(),
            },
            "clause" => CoreExpr::Clause {
                patterns: g[0].clone(),
                guard: one(1),
                body: one(2),
            },
            "fun" => CoreExpr::Fun {
                params: g[0].clone(),
                body: one(1),
            },
            "receive" => CoreExpr::Receive {
                clauses: g[0].clone(),
                timeout: one(1),
                action: one(2),
            },
            "try" => CoreExpr::Try {
                argument: one(0),
                vars: g[1].clone(),
                body: one(2),
                exception_vars: g[3].clone(),
                handler: one(4),
            },
            "catch" => CoreExpr::Catch { body: one(0) },
            "letrec" => CoreExpr::Letrec {
                definitions: pairs(0),
                body: one(1),
            },
            "map_pair" => CoreExpr::MapPair {
                operation: one(0),
                key: one(1),
                value: one(2),
            },
            "module" => CoreExpr::Module {
                name: one(0),
                exports: g[1].clone(),
                attributes: pairs(2),
                definitions: pairs(3),
            },
            _ => unreachable!("internal adapter node kind {kind}"),
        };
        self.nodes.push(CoreNode {
            id: NodeId(id),
            annotations: TermId(annotations),
            expr,
        });
        id
    }
    fn leaf(&mut self, expr: CoreExpr, ann: usize) -> usize {
        let id = self.nodes.len();
        self.nodes.push(CoreNode {
            id: NodeId(id),
            annotations: TermId(ann),
            expr,
        });
        id
    }
    fn literal_node(&mut self, term: usize, ann: usize) -> usize {
        self.leaf(CoreExpr::Literal(TermId(term)), ann)
    }
    fn function_var(&mut self, name: &str, arity: usize, ann: usize) -> usize {
        let name = self.atom(name);
        let arity = self.integer(arity);
        let term = self.tuple(vec![name, arity]);
        self.var_node(term, ann)
    }
    fn var_node(&mut self, term: usize, ann: usize) -> usize {
        self.leaf(CoreExpr::Var(TermId(term)), ann)
    }
    fn filename_term(&mut self, source: SourceId, filename: &str) -> usize {
        if let Some(&term) = self.filename_terms.get(&source) {
            return term;
        }
        let chars = filename.chars().map(|c| self.integer(c as u32)).collect();
        let term = self.list(chars);
        self.filename_terms.insert(source, term);
        term
    }
    fn annotations(
        &mut self,
        span: SourceSpan,
        annotations: &Annotations,
    ) -> Result<usize, String> {
        let mut items = vec![];
        if !span.is_unknown() {
            let loc = self
                .codemap
                .location_for_span(span)
                .map_err(|e| format!("source location: {e}"))?;
            let source_line = loc.line.to_usize() + 1;
            let source_column = self.source_column(loc.column.to_usize());
            let filename = self
                .codemap
                .name_for_span(span)
                .map_err(|e| format!("source name: {e}"))?
                .to_string();
            let line = self.integer(source_line);
            if let Some(column) = source_column {
                let col = self.integer(column);
                items.push(self.tuple(vec![line, col]));
            } else {
                items.push(line);
            }
            let filename = self.filename_term(span.source_id(), &filename);
            let file = self.atom("file");
            items.push(self.tuple(vec![file, filename]));
        }
        if span.is_unknown()
            || annotations
                .iter()
                .any(|(key, _)| key.to_string() == "compiler_generated")
        {
            items.push(self.atom("compiler_generated"));
        }
        Ok(self.list(items))
    }
    fn literal(&mut self, lit: &Literal) -> Result<usize, String> {
        Ok(match &lit.value {
            Lit::Atom(s) => self.atom(&s.to_string()),
            Lit::Integer(i) => self.integer(i),
            Lit::Float(f) => self.term(ErlangTerm::Float(f.inner().to_bits())),
            Lit::Nil => self.term(ErlangTerm::Nil),
            Lit::Cons(h, t) => {
                let h = self.literal(h)?;
                let t = self.literal(t)?;
                self.term(ErlangTerm::List {
                    items: vec![TermId(h)],
                    tail: TermId(t),
                })
            }
            Lit::Tuple(xs) => {
                let ids = xs
                    .iter()
                    .map(|x| self.literal(x))
                    .collect::<Result<Vec<_>, _>>()?;
                self.tuple(ids)
            }
            Lit::Map(xs) => {
                if !xs.is_empty() {
                    return Err(
                        "unsupported literal map: native iterator provenance unavailable".into(),
                    );
                }
                self.term(ErlangTerm::Map(vec![]))
            }
            Lit::Binary(b) => {
                let data: Vec<u8> = b.bytes().collect();
                if b.bit_size() % 8 == 0 {
                    self.term(ErlangTerm::Binary(data))
                } else {
                    self.term(ErlangTerm::Bitstring {
                        bytes: data,
                        bits: b.bit_size() as u64,
                    })
                }
            }
        })
    }
    fn var(&mut self, v: &Var) -> Result<usize, String> {
        let ann = self.annotations(v.span(), v.annotations())?;
        Ok(if let Some(arity) = v.arity {
            self.function_var(&v.name.name.to_string(), arity, ann)
        } else {
            let name = v.name.name.to_string();
            if name != "_" && self.source_variables.contains(&name) {
                let term = self.atom(&name);
                return Ok(self.var_node(term, ann));
            }
            let id = if name == "_" {
                let id = self.next_variable;
                self.next_variable += 1;
                id
            } else if let Some(id) = self.variables.get(&name) {
                *id
            } else {
                let id = self.next_variable;
                self.next_variable += 1;
                self.variables.insert(name, id);
                id
            };
            let t = self.integer(id);
            self.var_node(t, ann)
        })
    }
    fn vars(&mut self, vs: &[Var]) -> Result<Vec<usize>, String> {
        vs.iter().map(|v| self.var(v)).collect()
    }
    fn exprs(&mut self, xs: &[Expr]) -> Result<Vec<usize>, String> {
        xs.iter().map(|x| self.expr(x)).collect()
    }
    fn clause(&mut self, c: &Clause) -> Result<usize, String> {
        let ann = self.annotations(c.span, c.annotations())?;
        let patterns = self.exprs(&c.patterns)?;
        let guard = match &c.guard {
            Some(g) => self.expr(g)?,
            None => {
                let t = self.atom("true");
                self.literal_node(t, ann)
            }
        };
        let body = self.expr(&c.body)?;
        Ok(self.node("clause", ann, vec![patterns, vec![guard], vec![body]]))
    }
    fn constant(&mut self, value: &str, ann: usize) -> usize {
        let t = self.atom(value);
        self.literal_node(t, ann)
    }
    fn predefined_module_info(&mut self, name: &str, arity: usize) -> Result<usize, String> {
        // OTP erl_internal:predefined_functions/1 emits erlang:get_module_info(M[,X]).
        // Firefly injects runtime-specific bodies; construct the OTP function independently.
        let generated = self.atom("compiler_generated");
        let zero = self.integer(0);
        let ann = self.list(vec![zero, generated]);
        let erlang = self.constant("erlang", ann);
        let function = self.constant("get_module_info", ann);
        let module = self.constant(name, ann);
        let mut args = vec![module];
        let mut params = vec![];
        if arity == 1 {
            let id = self.next_variable;
            self.next_variable += 1;
            let term = self.integer(id);
            let var = self.var_node(term, ann);
            params.push(var);
            args.push(var);
        }
        let call = self.node("call", ann, vec![vec![erlang], vec![function], args]);
        Ok(self.node("fun", ann, vec![params, vec![call]]))
    }
    fn bitstr(&mut self, b: &firefly_syntax_core::Bitstring) -> Result<usize, String> {
        let ann = self.annotations(b.span, b.annotations())?;
        let value = self.expr(&b.value)?;
        let (typ, unit, signed, endian, default) = match b.spec {
            Spec::Integer {
                signed,
                endianness,
                unit,
            } => ("integer", unit, signed, Some(endianness), Some(8)),
            Spec::Float { endianness, unit } => ("float", unit, false, Some(endianness), Some(64)),
            Spec::Binary { unit } => ("binary", unit, false, None, None),
            Spec::Utf8 => ("utf8", 1, false, None, None),
            Spec::Utf16 { endianness } => ("utf16", 1, false, Some(endianness), None),
            Spec::Utf32 { endianness } => ("utf32", 1, false, Some(endianness), None),
        };
        let size = match &b.size {
            Some(s) => self.expr(s)?,
            None => match default {
                Some(n) => {
                    let t = self.integer(n);
                    self.literal_node(t, ann)
                }
                None => self.constant(if typ == "binary" { "all" } else { "undefined" }, ann),
            },
        };
        let unit_t = if matches!(b.spec, Spec::Utf8 | Spec::Utf16 { .. } | Spec::Utf32 { .. }) {
            self.atom("undefined")
        } else {
            self.integer(unit)
        };
        let unit = self.literal_node(unit_t, ann);
        let typ = self.constant(typ, ann);
        let flags = vec![
            self.atom(if signed { "signed" } else { "unsigned" }),
            self.atom(match endian.unwrap_or(Endianness::Big) {
                Endianness::Big => "big",
                Endianness::Little => "little",
                Endianness::Native => "native",
            }),
        ];
        let flags = self.list(flags);
        let flags = self.literal_node(flags, ann);
        Ok(self.node(
            "bitstr",
            ann,
            vec![vec![value], vec![size], vec![unit], vec![typ], vec![flags]],
        ))
    }
    fn expr(&mut self, e: &Expr) -> Result<usize, String> {
        let mut ann = self.annotations(e.span(), e.annotations())?;
        if let Expr::Map(map) = e {
            if !map.is_pattern && !e.span().is_unknown() {
                let loc = self
                    .codemap
                    .location_for_span(e.span())
                    .map_err(|e| format!("map source location: {e}"))?;
                let file = self
                    .codemap
                    .name_for_span(e.span())
                    .map_err(|e| format!("map source name: {e}"))?
                    .to_string();
                let mut matches: Vec<_> = self
                    .source_origins
                    .iter()
                    .filter(|o| {
                        o.matches_update(
                            &file,
                            loc.line.to_usize() + 1,
                            self.source_column(loc.column.to_usize()).unwrap_or(1),
                        )
                    })
                    .cloned()
                    .collect();
                matches.sort_by_key(SourceOrigin::position);
                matches.dedup_by_key(|o| o.position());
                if matches.len() > 1 {
                    return Err(format!(
                        "ambiguous map-update source origin {file}:{}:{}",
                        loc.line.to_usize() + 1,
                        self.source_column(loc.column.to_usize()).unwrap_or(1)
                    ));
                }
                if let Some(origin) = matches.first() {
                    let (line, col) = origin.position();
                    let line = self.integer(line);
                    let col = self.integer(col);
                    let location = self.tuple(vec![line, col]);
                    let filename = self.filename_term(e.span().source_id(), &file);
                    let tag = self.atom("file");
                    let file = self.tuple(vec![tag, filename]);
                    let mut items = vec![location, file];
                    if e.is_compiler_generated() {
                        items.push(self.atom("compiler_generated"));
                    }
                    ann = self.list(items);
                }
            }
        }
        let (kind, groups) = match e {
            Expr::Literal(l) => {
                let t = self.literal(l)?;
                return Ok(self.literal_node(t, ann));
            }
            Expr::Var(v) => return self.var(v),
            Expr::Alias(x) => (
                "alias",
                vec![vec![self.var(&x.var)?], vec![self.expr(&x.pattern)?]],
            ),
            Expr::Apply(x) => (
                "apply",
                vec![vec![self.expr(&x.callee)?], self.exprs(&x.args)?],
            ),
            Expr::Binary(x) => (
                "binary",
                vec![
                    x.segments
                        .iter()
                        .map(|b| self.bitstr(b))
                        .collect::<Result<Vec<_>, _>>()?,
                ],
            ),
            Expr::Call(x) => (
                "call",
                vec![
                    vec![self.expr(&x.module)?],
                    vec![self.expr(&x.function)?],
                    self.exprs(&x.args)?,
                ],
            ),
            Expr::Case(x) => (
                "case",
                vec![
                    vec![self.expr(&x.arg)?],
                    x.clauses
                        .iter()
                        .map(|c| self.clause(c))
                        .collect::<Result<Vec<_>, _>>()?,
                ],
            ),
            Expr::Catch(x) => ("catch", vec![vec![self.expr(&x.body)?]]),
            Expr::Cons(x) => (
                "cons",
                vec![vec![self.expr(&x.head)?], vec![self.expr(&x.tail)?]],
            ),
            Expr::Fun(x) => ("fun", vec![self.vars(&x.vars)?, vec![self.expr(&x.body)?]]),
            Expr::If(x) => {
                let guard = self.expr(&x.guard)?;
                let mut clauses = vec![];
                for (atom, body) in [("true", &x.then_body), ("false", &x.else_body)] {
                    let p = self.constant(atom, ann);
                    let g = self.constant("true", ann);
                    let b = self.expr(body)?;
                    clauses.push(self.node("clause", ann, vec![vec![p], vec![g], vec![b]]));
                }
                ("case", vec![vec![guard], clauses])
            }
            Expr::Let(x) => (
                "let",
                vec![
                    self.vars(&x.vars)?,
                    vec![self.expr(&x.arg)?],
                    vec![self.expr(&x.body)?],
                ],
            ),
            Expr::LetRec(x) => {
                let mut defs = vec![];
                for (v, e) in &x.defs {
                    defs.push(self.var(v)?);
                    defs.push(self.expr(e)?);
                }
                ("letrec", vec![defs, vec![self.expr(&x.body)?]])
            }
            Expr::Map(x) => {
                let base = self.expr(&x.arg)?;
                let mut pairs = vec![];
                for p in &x.pairs {
                    // Firefly dropped MapField.span; restore exact raw-field origin when
                    // supplied, otherwise use the surviving key origin, never the enclosing map.
                    let mut pair_ann = self.annotations(p.key.span(), p.key.annotations())?;
                    if !p.key.span().is_unknown() {
                        let loc = self
                            .codemap
                            .location_for_span(p.key.span())
                            .map_err(|e| format!("map key location: {e}"))?;
                        let file = self
                            .codemap
                            .name_for_span(p.key.span())
                            .map_err(|e| format!("map key file: {e}"))?
                            .to_string();
                        let kind = match p.op {
                            MapOp::Assoc => "map_field_assoc",
                            MapOp::Exact => "map_field_exact",
                        };
                        let mut origins: Vec<_> = self
                            .source_origins
                            .iter()
                            .filter(|o| {
                                o.matches_field(
                                    kind,
                                    &file,
                                    loc.line.to_usize() + 1,
                                    self.source_column(loc.column.to_usize()).unwrap_or(1),
                                )
                            })
                            .cloned()
                            .collect();
                        origins.sort_by_key(SourceOrigin::position);
                        origins.dedup_by_key(|o| o.position());
                        if origins.len() > 1 {
                            return Err(format!("ambiguous map field source origin {file}"));
                        }
                        if let Some(origin) = origins.first() {
                            let (line, col) = origin.position();
                            let line = self.integer(line);
                            let col = self.integer(col);
                            let location = self.tuple(vec![line, col]);
                            let chars = file.chars().map(|c| self.integer(c as u32)).collect();
                            let filename = self.list(chars);
                            let tag = self.atom("file");
                            let file = self.tuple(vec![tag, filename]);
                            pair_ann = self.list(vec![location, file]);
                        }
                    }
                    let generated =
                        self.annotations(SourceSpan::UNKNOWN, &Annotations::default())?;
                    let op = self.constant(
                        match p.op {
                            MapOp::Assoc => "assoc",
                            MapOp::Exact => "exact",
                        },
                        generated,
                    );
                    let k = self.expr(&p.key)?;
                    let v = self.expr(&p.value)?;
                    pairs.push(self.node("map_pair", pair_ann, vec![vec![op], vec![k], vec![v]]));
                }
                let id = self.leaf(
                    CoreExpr::Map {
                        base: NodeId(base),
                        pairs: pairs.into_iter().map(NodeId).collect(),
                        pattern: x.is_pattern,
                    },
                    ann,
                );
                return Ok(id);
            }
            Expr::PrimOp(x) => {
                let name = x.name.to_string();
                let name = if name == "bits_init_writable" {
                    "bs_init_writable"
                } else {
                    name.as_str()
                };
                if name == "raise"
                    && x.annotations
                        .iter()
                        .any(|(key, _)| key.to_string() == "generated_rethrow")
                {
                    if x.args.len() != 3 {
                        return Err("generated rethrow must have class,value,info".into());
                    }
                    (
                        "primop",
                        vec![
                            vec![self.constant("raise", ann)],
                            vec![self.expr(&x.args[2])?, self.expr(&x.args[1])?],
                        ],
                    )
                } else if matches!(name, "error" | "exit" | "throw" | "nif_error")
                    || (matches!(name, "raise" | "make_fun") && x.args.len() == 3)
                {
                    (
                        "call",
                        vec![
                            vec![self.constant("erlang", ann)],
                            vec![self.constant(&name, ann)],
                            self.exprs(&x.args)?,
                        ],
                    )
                } else {
                    (
                        "primop",
                        vec![vec![self.constant(&name, ann)], self.exprs(&x.args)?],
                    )
                }
            }
            Expr::Receive(x) => (
                "receive",
                vec![
                    x.clauses
                        .iter()
                        .map(|c| self.clause(c))
                        .collect::<Result<Vec<_>, _>>()?,
                    vec![self.expr(&x.timeout)?],
                    vec![self.expr(&x.action)?],
                ],
            ),
            Expr::Seq(x) => (
                "seq",
                vec![vec![self.expr(&x.arg)?], vec![self.expr(&x.body)?]],
            ),
            Expr::Try(x) => (
                "try",
                vec![
                    vec![self.expr(&x.arg)?],
                    self.vars(&x.vars)?,
                    vec![self.expr(&x.body)?],
                    self.vars(&x.evars)?,
                    vec![self.expr(&x.handler)?],
                ],
            ),
            Expr::Tuple(x) => ("tuple", vec![self.exprs(&x.elements)?]),
            Expr::Values(x) => ("values", vec![self.exprs(&x.values)?]),
        };
        Ok(self.node(kind, ann, groups))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    fn encode64(data: &[u8]) -> String {
        rust_dialyzer_poc::type_exports::term_to_value(&ErlangTerm::Binary(data.to_vec()), |id| {
            id.0
        })["value"]
            .as_str()
            .unwrap()
            .to_owned()
    }
    fn snapshot(a: &Adapter<'_>) -> Value {
        CoreModule {
            name: String::new(),
            source_forms: TermId(0),
            core: NodeId(0),
            terms: a.terms.clone(),
            nodes: a.nodes.clone(),
            contract_variable_maps: BTreeMap::new(),
            map_iterator_orders: BTreeMap::new(),
        }
        .to_wire_value()
    }
    fn nodes(a: &Adapter<'_>) -> Value {
        snapshot(a)["nodes"].take()
    }
    fn terms(a: &Adapter<'_>) -> Value {
        snapshot(a)["terms"].take()
    }
    use firefly_intern::Symbol;
    fn adapter(codemap: &CodeMap) -> Adapter<'_> {
        Adapter {
            codemap,
            source_column_offset: 0,
            terms: vec![],
            emitted_terms: HashMap::new(),
            filename_terms: HashMap::new(),
            nodes: vec![],
            variables: BTreeMap::new(),
            next_variable: 0,
            source_variables: BTreeSet::new(),
            source_origins: vec![],
        }
    }
    #[test]
    fn strict_seed_boundary_rejects_invalid_and_null_metadata() {
        for seed in [
            Value::Null,
            json!("scalar"),
            json!([]),
            json!({}),
            json!({"terms":null}),
        ] {
            assert_eq!(
                PreparedCoreSeed::from_wire_value(&seed).unwrap_err(),
                "seed.terms missing"
            );
        }
        let seed = json!({"terms":[{"kind":"nil"}],"source_forms":0,"attributes":[],"contract_variable_maps":null});
        assert_eq!(
            PreparedCoreSeed::from_wire_value(&seed).unwrap_err(),
            "seed.contract_variable_maps: expected array"
        );
        let mut invalid = seed.clone();
        invalid["source_forms"] = json!(1);
        assert_eq!(
            PreparedCoreSeed::from_wire_value(&invalid).unwrap_err(),
            "source_forms outside terms"
        );
        let mut invalid = seed;
        invalid.as_object_mut().unwrap().remove("attributes");
        assert_eq!(
            PreparedCoreSeed::from_wire_value(&invalid).unwrap_err(),
            "seed.attributes missing: need raw-form attributes independent of oracle Core"
        );
    }
    #[test]
    fn encoded_spans_distinguish_line_only_from_explicit_column_one() {
        let map = CodeMap::new();
        let file = map.add(std::path::PathBuf::from("fixture.ex"), "  \n".to_owned());
        let missing = map.line_column_to_span(file, 0u32, 0u32).unwrap();
        let column_one = map.line_column_to_span(file, 0u32, 1u32).unwrap();
        let mut a = adapter(&map);
        a.source_column_offset = 1;
        let absent = a.annotations(missing, &Annotations::default()).unwrap();
        let explicit = a.annotations(column_one, &Annotations::default()).unwrap();
        let location =
            |a: &Adapter<'_>, ann: usize| terms(&a)[ann]["items"][0].as_u64().unwrap() as usize;
        let absent = location(&a, absent);
        let explicit = location(&a, explicit);
        assert_eq!(terms(&a)[absent], json!({"kind":"integer","value":"1"}));
        assert_eq!(terms(&a)[explicit]["kind"], "tuple");
        assert_eq!(terms(&a)[explicit]["items"], json!([absent, absent]));
    }
    #[test]
    fn emitted_terms_share_ids_without_changing_seed_terms() {
        let map = CodeMap::new();
        let mut a = adapter(&map);
        let seed = json!({"kind":"atom","value":"ok"});
        a.terms.push(ErlangTerm::Atom("ok".into()));
        let first = a.atom("ok");
        let second = a.atom("ok");
        let distinct = a.atom("error");
        assert_eq!(first, 1);
        assert_eq!(first, second);
        assert_ne!(first, distinct);
        let list = a.list(vec![first, distinct]);
        assert_eq!(list, a.list(vec![second, distinct]));
        assert_eq!(terms(&a)[0], seed);
        assert_eq!(a.terms.len(), 5);
    }
    #[test]
    fn conditional_becomes_two_boolean_clauses() {
        let map = CodeMap::new();
        let mut a = adapter(&map);
        let atom = |name| Expr::Literal(Literal::atom(SourceSpan::UNKNOWN, Symbol::intern(name)));
        let id = a
            .expr(&Expr::If(firefly_syntax_core::If::new(
                SourceSpan::UNKNOWN,
                atom("true"),
                atom("ok"),
                atom("error"),
            )))
            .unwrap();
        assert_eq!(nodes(&a)[id]["kind"], "case");
        let clauses = nodes(&a)[id]["groups"][1].as_array().unwrap().clone();
        assert_eq!(clauses.len(), 2);
        for (clause, name) in clauses.iter().zip(["true", "false"]) {
            let pattern = nodes(&a)[clause.as_u64().unwrap() as usize]["groups"][0][0]
                .as_u64()
                .unwrap() as usize;
            let value = nodes(&a)[pattern]["value"].as_u64().unwrap() as usize;
            assert_eq!(terms(&a)[value]["value"], name);
        }
    }
    #[test]
    fn aliases_share_integer_variable_identity() {
        let map = CodeMap::new();
        let mut a = adapter(&map);
        let v = Var::new(firefly_intern::Ident::from_str("$temporary"));
        let first = a.var(&v).unwrap();
        let second = a.var(&v).unwrap();
        let term =
            |node: usize| terms(&a)[nodes(&a)[node]["value"].as_u64().unwrap() as usize].clone();
        assert_eq!(term(first), term(second));
        assert_eq!(term(first)["kind"], "integer");
    }
    #[test]
    fn anonymous_binders_receive_distinct_identities() {
        let map = CodeMap::new();
        let mut a = adapter(&map);
        let v = Var::new(firefly_intern::Ident::from_str("_"));
        let first = a.var(&v).unwrap();
        let second = a.var(&v).unwrap();
        let value = |node: usize| {
            terms(&a)[nodes(&a)[node]["value"].as_u64().unwrap() as usize]["value"].clone()
        };
        assert_ne!(value(first), value(second));
    }
    #[test]
    fn source_variable_name_survives_diagnostic_adaptation() {
        let map = CodeMap::new();
        let mut a = adapter(&map);
        a.source_variables.insert("M".into());
        let v = Var::new(firefly_intern::Ident::from_str("M"));
        let node = a.var(&v).unwrap();
        let term = nodes(&a)[node]["value"].as_u64().unwrap() as usize;
        assert_eq!(terms(&a)[term], json!({"kind":"atom","value":"M"}));
    }
    #[test]
    fn marked_rethrow_uses_otp_info_value_order() {
        let map = CodeMap::new();
        let mut a = adapter(&map);
        let args = ["error", "reason", "info"]
            .into_iter()
            .map(|n| Expr::Literal(Literal::atom(SourceSpan::UNKNOWN, Symbol::intern(n))))
            .collect();
        let mut op =
            firefly_syntax_core::PrimOp::new(SourceSpan::UNKNOWN, Symbol::intern("raise"), args);
        op.annotations.set(Symbol::intern("generated_rethrow"));
        op.mark_compiler_generated();
        let node = a.expr(&Expr::PrimOp(op)).unwrap();
        assert_eq!(nodes(&a)[node]["kind"], "primop");
        let args = nodes(&a)[node]["groups"][1].as_array().unwrap().clone();
        assert_eq!(args.len(), 2);
        let names: Vec<_> = args
            .iter()
            .map(|n| {
                let term = nodes(&a)[n.as_u64().unwrap() as usize]["value"]
                    .as_u64()
                    .unwrap() as usize;
                terms(&a)[term]["value"].clone()
            })
            .collect();
        assert_eq!(names, vec![json!("info"), json!("reason")]);
    }
    #[test]
    fn map_field_origin_is_independent_of_enclosing_map() {
        let map = CodeMap::new();
        let file = map.add(
            std::path::PathBuf::from("fixture.erl"),
            "                \n".into(),
        );
        let map_span = map.line_column_to_span(file, 0u32, 2u32).unwrap();
        let key_span = map.line_column_to_span(file, 0u32, 4u32).unwrap();
        let mut a = adapter(&map);
        a.source_origins.push(SourceOrigin::MapFieldExact {
            file: "fixture.erl".into(),
            key_line: 1,
            key_column: 5,
            line: 1,
            column: 9,
        });
        let key = Expr::Literal(Literal::atom(key_span, Symbol::intern("missing")));
        let value = Expr::Literal(Literal::integer(key_span, 2));
        let base = Expr::Literal(Literal::atom(map_span, Symbol::intern("base")));
        let expr = Expr::Map(firefly_syntax_core::Map::update(
            map_span,
            base,
            vec![firefly_syntax_core::MapPair {
                op: MapOp::Exact,
                key: Box::new(key),
                value: Box::new(value),
            }],
        ));
        let node = a.expr(&expr).unwrap();
        let pair = nodes(&a)[node]["groups"][0][0].as_u64().unwrap() as usize;
        let ann = nodes(&a)[pair]["annotations"].as_u64().unwrap() as usize;
        let location = terms(&a)[ann]["items"][0].as_u64().unwrap() as usize;
        let col = terms(&a)[location]["items"][1].as_u64().unwrap() as usize;
        assert_eq!(terms(&a)[col]["value"], "9");
        let op = nodes(&a)[pair]["groups"][0][0].as_u64().unwrap() as usize;
        let ann = nodes(&a)[op]["annotations"].as_u64().unwrap() as usize;
        let generated = terms(&a)[ann]["items"][0].as_u64().unwrap() as usize;
        assert_eq!(terms(&a)[generated]["value"], "compiler_generated");
    }
    #[test]
    fn writable_binary_primop_uses_otp_name_and_preserves_size_hint() {
        let map = CodeMap::new();
        let mut a = adapter(&map);
        for name in ["bits_init_writable", "bs_init_writable"] {
            let expr = Expr::PrimOp(firefly_syntax_core::PrimOp::new(
                SourceSpan::UNKNOWN,
                Symbol::intern(name),
                vec![Expr::Literal(Literal::integer(SourceSpan::UNKNOWN, 256))],
            ));
            let node = a.expr(&expr).unwrap();
            assert_eq!(nodes(&a)[node]["kind"], "primop");
            let op = nodes(&a)[node]["groups"][0][0].as_u64().unwrap() as usize;
            let term = nodes(&a)[op]["value"].as_u64().unwrap() as usize;
            assert_eq!(terms(&a)[term]["value"], "bs_init_writable");
            let args = nodes(&a)[node]["groups"][1].as_array().unwrap().clone();
            assert_eq!(args.len(), 1);
            let arg = args[0].as_u64().unwrap() as usize;
            let term = nodes(&a)[arg]["value"].as_u64().unwrap() as usize;
            assert_eq!(terms(&a)[term], json!({"kind": "integer", "value": "256"}));
        }
    }
    #[test]
    fn native_bif_primops_become_calls_while_raise2_stays_core() {
        let map = CodeMap::new();
        let mut a = adapter(&map);
        for (name, arity, kind) in [
            ("error", 1, "call"),
            ("exit", 1, "call"),
            ("throw", 1, "call"),
            ("nif_error", 1, "call"),
            ("raise", 3, "call"),
            ("make_fun", 3, "call"),
            ("raise", 2, "primop"),
        ] {
            let args = (0..arity)
                .map(|_| Expr::Literal(Literal::nil(SourceSpan::UNKNOWN)))
                .collect();
            let expr = Expr::PrimOp(firefly_syntax_core::PrimOp::new(
                SourceSpan::UNKNOWN,
                Symbol::intern(name),
                args,
            ));
            let node = a.expr(&expr).unwrap();
            assert_eq!(nodes(&a)[node]["kind"], kind);
            if kind == "call" {
                let module = nodes(&a)[node]["groups"][0][0].as_u64().unwrap() as usize;
                let term = nodes(&a)[module]["value"].as_u64().unwrap() as usize;
                assert_eq!(terms(&a)[term]["value"], "erlang");
            }
        }
    }
    #[test]
    fn nonempty_literal_map_requires_order_evidence() {
        let map = CodeMap::new();
        let mut a = adapter(&map);
        let lit = Literal::map(
            SourceSpan::UNKNOWN,
            vec![(
                Literal::integer(SourceSpan::UNKNOWN, 1),
                Literal::integer(SourceSpan::UNKNOWN, 2),
            )],
        );
        assert!(a.literal(&lit).unwrap_err().contains("provenance"));
    }
    #[test]
    fn predefined_module_info_uses_otp_runtime_call() {
        let map = CodeMap::new();
        let mut a = adapter(&map);
        for arity in [0, 1] {
            let fun = a.predefined_module_info("fixture", arity).unwrap();
            assert_eq!(nodes(&a)[fun]["kind"], "fun");
            assert_eq!(nodes(&a)[fun]["groups"][0].as_array().unwrap().len(), arity);
            let body = nodes(&a)[fun]["groups"][1][0].as_u64().unwrap() as usize;
            assert_eq!(nodes(&a)[body]["kind"], "call");
            assert_eq!(
                nodes(&a)[body]["groups"][2].as_array().unwrap().len(),
                arity + 1
            );
            let atom_at = |group: usize| {
                let node = nodes(&a)[body]["groups"][group][0].as_u64().unwrap() as usize;
                let term = nodes(&a)[node]["value"].as_u64().unwrap() as usize;
                terms(&a)[term]["value"].clone()
            };
            assert_eq!(atom_at(0), "erlang");
            assert_eq!(atom_at(1), "get_module_info");
            if arity == 1 {
                assert_eq!(
                    nodes(&a)[fun]["groups"][0][0],
                    nodes(&a)[body]["groups"][2][1]
                );
            }
        }
    }
    #[test]
    fn byte_encoding_preserves_padding() {
        assert_eq!(encode64(&[0xff]), "/w==");
        assert_eq!(encode64(&[0xff, 0xee]), "/+4=");
        assert_eq!(encode64(&[0xff, 0xee, 0xdd]), "/+7d");
    }
}

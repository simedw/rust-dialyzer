//! Native forms-to-Core compiler slice, following OTP 28.3 v3_core.erl,
//! erl_internal.erl and core_parse.hrl (Apache-2.0; Ericsson AB).
//! Every function is compiled; unsupported constructs are errors, never dropped.
use super::term::Arena;
use crate::core::{ErlangTerm, ErlangTerms, TermId};
use std::collections::{BTreeSet, HashSet};
mod binary;
mod comprehension;
mod constant;
mod control;
mod exception;
mod exports;
#[cfg(test)]
mod guard_analysis;
mod literal;
mod matches;
mod pattern;
mod receive;
mod shadow;
mod split;

type Result<T> = std::result::Result<T, String>;
#[derive(Clone)]
struct Expr {
    value: TermId,
    pre: Vec<Pre>,
}
#[derive(Clone)]
enum Pre {
    Set(TermId, TermId),
    Effect(TermId),
    Match {
        ann: TermId,
        arg: TermId,
        pattern: TermId,
        raw_pattern: TermId,
        guard: TermId,
        failure: TermId,
        allocations: Vec<TermId>,
    },
}
// Canonical identity for literal map keys. This order is only for identity;
// OTP Core map pattern ordering is handled separately.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum LiteralKey {
    Integer(String),
    Float(u64),
    Atom(String),
    ExternalFun(String, String, u8),
    Tuple(Vec<LiteralKey>),
    Map(Vec<(LiteralKey, LiteralKey)>),
    Nil,
    List(Vec<LiteralKey>, Box<LiteralKey>),
    Bits(Vec<u8>, u64),
}
struct Lower {
    a: Arena,
    module: String,
    forms_root: TermId,
    nifs: Option<BTreeSet<(String, i64)>>,
    load_nif: bool,
    file: Option<TermId>,
    counter: i64,
    fun_counter: u32,
    goto_counter: u32,
    function: String,
    depth: usize,
    wanted: bool,
    in_guard: bool,
    known_vars: HashSet<String>,
    case_patterns: bool,
    pin_tests: Vec<TermId>,
    generated_guard_equalities: BTreeSet<TermId>,
    deferred_vars: BTreeSet<TermId>,
    deferred_aliases: std::collections::BTreeMap<TermId, TermId>,
    guard_allocations: std::collections::BTreeMap<TermId, Vec<TermId>>,
    wildcard_renames: std::collections::BTreeMap<TermId, TermId>,
    let_annotations: std::collections::BTreeMap<TermId, TermId>,
    protected_guards: HashSet<TermId>,
    source_annotations: std::collections::BTreeMap<TermId, TermId>,
    shadowed_funs: std::collections::BTreeMap<TermId, HashSet<String>>,
    export_memo: exports::ExportMemo,
}
pub fn lower(forms: &ErlangTerms, module: &str) -> Result<ErlangTerms> {
    let mut l = Lower {
        a: Arena::from_terms(forms),
        module: module.into(),
        forms_root: forms.root,
        nifs: None,
        load_nif: false,
        file: None,
        counter: 0,
        fun_counter: 0,
        goto_counter: 0,
        function: String::new(),
        depth: 0,
        wanted: true,
        in_guard: false,
        known_vars: HashSet::new(),
        case_patterns: false,
        pin_tests: Vec::new(),
        generated_guard_equalities: BTreeSet::new(),
        deferred_vars: BTreeSet::new(),
        deferred_aliases: std::collections::BTreeMap::new(),
        guard_allocations: std::collections::BTreeMap::new(),
        wildcard_renames: std::collections::BTreeMap::new(),
        let_annotations: std::collections::BTreeMap::new(),
        protected_guards: HashSet::new(),
        source_annotations: std::collections::BTreeMap::new(),
        shadowed_funs: std::collections::BTreeMap::new(),
        export_memo: Default::default(),
    };
    let mut input = l.a.list_items(forms.root)?;
    input.extend(l.predefined()?);
    let export_all = input.iter().any(|form| {
        l.a.tuple_fields(*form).ok().is_some_and(|f| {
            f.len() == 4
                && l.a.tag(*form) == Some("attribute")
                && l.a.as_atom(f[2]) == Some("compile")
                && (l.a.as_atom(f[3]) == Some("export_all")
                    || l.a.list_items(f[3]).ok().is_some_and(|opts| {
                        opts.iter()
                            .any(|opt| l.a.as_atom(*opt) == Some("export_all"))
                    }))
        })
    });
    let mut exports = BTreeSet::new();
    if export_all {
        for form in &input {
            if l.a.tag(*form) == Some("function") {
                let f = l.a.tuple_fields(*form)?;
                exports.insert((
                    l.a.as_atom(f[2])
                        .ok_or("lower: export_all function name")?
                        .to_owned(),
                    l.a.as_integer(f[3])
                        .ok_or("lower: export_all function arity")?,
                ));
            }
        }
    }
    let mut attributes = Vec::new();
    let mut definitions = Vec::new();
    for form in input {
        let f = l.a.tuple_fields(form)?.to_vec();
        match l.a.tag(form) {
            Some("attribute") if f.len() == 4 => {
                let kind =
                    l.a.as_atom(f[2])
                        .ok_or("lower: non-atom attribute")?
                        .to_owned();
                match kind.as_str() {
                    "module" => {
                        if l.a.as_atom(f[3]) != Some(module) {
                            return Err("lower: module identity mismatch".into());
                        }
                    }
                    "export" => {
                        for fa in l.a.list_items(f[3])? {
                            let pair = l.a.tuple_fields(fa)?;
                            if pair.len() != 2 {
                                return Err("lower: malformed export".into());
                            }
                            exports.insert((
                                l.a.as_atom(pair[0]).ok_or("lower: export name")?.to_owned(),
                                l.a.as_integer(pair[1]).ok_or("lower: export arity")?,
                            ));
                        }
                    }
                    "nifs" => {
                        let mut declarations = l.nifs.take().unwrap_or_default();
                        for declaration in l.a.list_items(f[3])? {
                            let fa = l.a.tuple_fields(declaration)?;
                            if fa.len() != 2 {
                                return Err(l.err("malformed NIF declaration"));
                            }
                            declarations.insert((
                                l.a.as_atom(fa[0]).ok_or("lower: NIF name")?.to_owned(),
                                l.a.as_integer(fa[1]).ok_or("lower: NIF arity")?,
                            ));
                        }
                        l.nifs = Some(declarations);
                    }
                    "import" => (),
                    "file" => {
                        let pair = l.a.tuple_fields(f[3])?.to_vec();
                        if pair.len() != 2 {
                            return Err("lower: malformed file attribute".into());
                        }
                        l.file = Some(pair[0]);
                        attributes.push(l.attribute(&f)?);
                    }
                    "record" => attributes.push(l.attribute(&f)?),
                    _ => attributes.push(l.attribute(&f)?),
                }
            }
            Some("function") if f.len() == 5 => definitions.push(l.function(&f)?),
            Some("eof") => (),
            tag => return Err(format!("lower: unsupported form {tag:?}")),
        }
    }
    // OTP inserts the entry marker in every function only when load_nif/2 is
    // used without an explicit -nifs declaration. A present empty declaration
    // is intentionally different from an absent declaration.
    if l.load_nif && l.nifs.is_none() {
        for definition in &mut definitions {
            *definition = l.insert_nif_start(*definition)?;
        }
    }
    let ann = l.a.nil();
    let name_value = l.a.atom(module);
    let name = l.literal(ann, name_value);
    let mut export_nodes = Vec::new();
    for (name, arity) in exports {
        let n = l.a.atom(name);
        let ar = l.a.integer(arity);
        let pair = l.a.tuple(vec![n, ar]);
        let ann = l.a.nil();
        export_nodes.push(l.a.tagged("c_var", vec![ann, pair]));
    }
    let exports = l.a.list(export_nodes);
    let attributes = l.a.list(attributes);
    let definitions = l.a.list(definitions);
    let root = l.a.tagged(
        "c_module",
        vec![ann, name, exports, attributes, definitions],
    );
    Ok(l.a.finish(root))
}
impl Lower {
    fn err(&self, tag: &str) -> String {
        format!("lower {}:{}: unsupported {tag}", self.module, self.function)
    }
    fn expression_annotation(&self, node: TermId) -> Result<TermId> {
        self.source_annotations
            .get(&node)
            .copied()
            .map(Ok)
            .unwrap_or_else(|| self.a.tuple_fields(node).map(|f| f[1]))
    }
    fn empty(&mut self) -> TermId {
        self.a.nil()
    }
    fn literal(&mut self, ann: TermId, value: TermId) -> TermId {
        self.a.tagged("c_literal", vec![ann, value])
    }
    fn atom_literal(&mut self, name: &str) -> TermId {
        let ann = self.empty();
        let v = self.a.atom(name);
        self.literal(ann, v)
    }
    fn variable(&mut self, ann: TermId, name: TermId) -> TermId {
        self.a.tagged("c_var", vec![ann, name])
    }
    fn fresh(&mut self, ann: TermId) -> TermId {
        let n = self.a.integer(self.counter);
        self.counter += 1;
        self.variable(ann, n)
    }
    fn annotation(&mut self, annotation: TermId) -> Result<TermId> {
        let (location, generated) = match self.a.get(annotation) {
            ErlangTerm::Integer(_) => (annotation, false),
            ErlangTerm::Tuple(xs) if xs.len() == 2 && self.a.as_integer(xs[0]).is_some() => {
                (annotation, false)
            }
            _ => {
                let loc = self
                    .a
                    .keyword(annotation, "location")
                    .or_else(|| self.a.keyword(annotation, "line"))
                    .ok_or("lower: unsupported erl_anno")?;
                let generated = self
                    .a
                    .keyword(annotation, "generated")
                    .is_some_and(|x| self.a.as_atom(x) == Some("true"));
                (loc, generated)
            }
        };
        let mut out = vec![location];
        if let Some(file) = self.file {
            let key = self.a.atom("file");
            out.push(self.a.tuple(vec![key, file]));
        }
        if generated {
            out.push(self.a.atom("compiler_generated"));
        }
        Ok(self.a.list(out))
    }
    fn record_annotation(&mut self, source: TermId) -> Result<TermId> {
        let ann = self.annotation(source)?;
        if self
            .a
            .keyword(source, "record")
            .is_some_and(|v| self.a.as_atom(v) == Some("true"))
        {
            let record = self.a.atom("record");
            let mut items = vec![record];
            items.extend(self.a.list_items(ann)?);
            Ok(self.a.list(items))
        } else {
            self.full_annotation(ann)
        }
    }
    fn attribute(&mut self, f: &[TermId]) -> Result<TermId> {
        let location = match self.a.get(f[1]) {
            ErlangTerm::Integer(_) | ErlangTerm::Tuple(_) => f[1],
            _ => self
                .a
                .keyword(f[1], "location")
                .ok_or("lower: attribute annotation")?,
        };
        let ann = self.a.list(vec![location]);
        let name = self.literal(ann, f[2]);
        let val = if matches!(self.a.get(f[3]), ErlangTerm::List { .. } | ErlangTerm::Nil) {
            f[3]
        } else {
            self.a.list(vec![f[3]])
        };
        let value = self.literal(ann, val);
        Ok(self.a.tuple(vec![name, value]))
    }
    fn a_root(&self) -> TermId {
        self.forms_root
    }
    fn abstract_fa_list(&mut self, entries: Vec<TermId>, line: TermId) -> Result<TermId> {
        let mut tail = self.a.tagged("nil", vec![line]);
        for entry in entries.into_iter().rev() {
            let pair = self.a.tuple_fields(entry)?.to_vec();
            if pair.len() != 2 {
                return Err(self.err("callback name/arity"));
            }
            let name = self.a.tagged("atom", vec![line, pair[0]]);
            let arity = self.a.tagged("integer", vec![line, pair[1]]);
            let elements = self.a.list(vec![name, arity]);
            let head = self.a.tagged("tuple", vec![line, elements]);
            tail = self.a.tagged("cons", vec![line, head, tail]);
        }
        Ok(tail)
    }
    fn predefined(&mut self) -> Result<Vec<TermId>> {
        let mut forms = Vec::new();
        let zero = self.a.integer(0);
        let module = self.a.atom(self.module.clone());
        let mut exports = Vec::new();
        let mut callbacks = Vec::new();
        let mut optional = Vec::new();
        // erl_internal:predefined_functions reads callback rows in attribute order.
        for form in self.a.list_items(self.a_root())? {
            if self.a.tag(form) != Some("attribute") {
                continue;
            }
            let fields = self.a.tuple_fields(form)?.to_vec();
            if fields.len() != 4 {
                continue;
            }
            match self.a.as_atom(fields[2]) {
                Some("callback") => {
                    let row = self.a.tuple_fields(fields[3])?;
                    if row.len() != 2 {
                        return Err(self.err("callback attribute shape"));
                    }
                    let fa = self.a.tuple_fields(row[0])?;
                    if fa.len() != 2
                        || self.a.as_atom(fa[0]).is_none()
                        || self.a.as_integer(fa[1]).is_none()
                    {
                        return Err(self.err("callback name/arity"));
                    }
                    callbacks.push(row[0]);
                }
                Some("optional_callbacks") => optional.extend(self.a.list_items(fields[3])?),
                _ => (),
            }
        }
        if !callbacks.is_empty() {
            let mut clauses = Vec::new();
            for (kind, entries) in [("callbacks", callbacks), ("optional_callbacks", optional)] {
                let body = self.abstract_fa_list(entries, zero)?;
                let kind = self.a.atom(kind);
                let pattern = self.a.tagged("atom", vec![zero, kind]);
                let patterns = self.a.list(vec![pattern]);
                let guards = self.empty();
                let body = self.a.list(vec![body]);
                clauses.push(self.a.tagged("clause", vec![zero, patterns, guards, body]));
            }
            let name = self.a.atom("behaviour_info");
            let arity = self.a.integer(1);
            exports.push(self.a.tuple(vec![name, arity]));
            let clauses = self.a.list(clauses);
            forms.push(self.a.tagged("function", vec![zero, name, arity, clauses]));
        }
        for arity in [0, 1] {
            let name = self.a.atom("module_info");
            let n = self.a.integer(arity);
            exports.push(self.a.tuple(vec![name, n]));
            let erlang = self.a.atom("erlang");
            let get = self.a.atom("get_module_info");
            let m = self.a.tagged("atom", vec![zero, module]);
            let erlang = self.a.tagged("atom", vec![zero, erlang]);
            let get = self.a.tagged("atom", vec![zero, get]);
            let remote = self.a.tagged("remote", vec![zero, erlang, get]);
            let mut args = vec![m];
            let mut pats = Vec::new();
            if arity == 1 {
                let x = self.a.atom("X");
                let x = self.a.tagged("var", vec![zero, x]);
                args.push(x);
                pats.push(x);
            }
            let args = self.a.list(args);
            let call = self.a.tagged("call", vec![zero, remote, args]);
            let pats = self.a.list(pats);
            let guards = self.empty();
            let body = self.a.list(vec![call]);
            let clause = self.a.tagged("clause", vec![zero, pats, guards, body]);
            let clauses = self.a.list(vec![clause]);
            forms.push(self.a.tagged("function", vec![zero, name, n, clauses]));
        }
        let export = self.a.atom("export");
        let exports = self.a.list(exports);
        let attr = self.a.tagged("attribute", vec![zero, export, exports]);
        forms.insert(0, attr);
        Ok(forms)
    }
    fn function(&mut self, f: &[TermId]) -> Result<TermId> {
        let name = self
            .a
            .as_atom(f[2])
            .ok_or("lower: function name")?
            .to_owned();
        let arity = self.a.as_integer(f[3]).ok_or("lower: function arity")?;
        if !(0..=255).contains(&arity) {
            return Err("lower: invalid function arity".into());
        }
        self.function = format!("{name}/{arity}");
        self.counter = 0;
        self.fun_counter = 0;
        self.goto_counter = 0;
        self.wanted = true;
        self.wildcard_renames.clear();
        self.known_vars.clear();
        self.deferred_vars.clear();
        self.deferred_aliases.clear();
        self.guard_allocations.clear();
        self.generated_guard_equalities.clear();
        let source = self.a.list_items(f[4])?;
        let first = *source.first().ok_or("lower: empty function")?;
        let first_fields = self.a.tuple_fields(first)?.to_vec();
        let line_ann = self.annotation(first_fields[1])?;
        let fa = self.a.tuple(vec![f[2], f[3]]);
        let function_atom = self.a.atom("function");
        let marker = self.a.tuple(vec![function_atom, fa]);
        let mut anns = vec![marker];
        anns.extend(self.a.list_items(line_ann)?);
        let fun_ann = self.a.list(anns);
        let mut params = Vec::new();
        for _ in 0..arity {
            params.push(self.fresh(line_ann));
        }
        let mut clauses = Vec::new();
        for c in source {
            clauses.push(self.clause(c)?);
        }
        let mut fail_params = Vec::new();
        for _ in 0..arity {
            let ann = self.empty();
            fail_params.push(self.fresh(ann));
        }
        fail_params.reverse();
        let fail = self.failure(fail_params, fun_ann, "function_clause")?;
        clauses.push(fail);
        for c in &mut clauses {
            *c = self.rename_wildcards(*c)?;
        }
        let arg = if params.len() == 1 {
            let mut fs = self.a.tuple_fields(params[0])?.to_vec();
            fs[1] = fun_ann;
            self.a.tuple(fs)
        } else {
            let ps = self.a.list(params.clone());
            self.a.tagged("c_values", vec![fun_ann, ps])
        };
        let clauses = self.a.list(clauses);
        let case = self.a.tagged("c_case", vec![fun_ann, arg, clauses]);
        let case = if self
            .nifs
            .as_ref()
            .is_some_and(|nifs| nifs.contains(&(name.clone(), arity)))
        {
            self.nif_start(case)
        } else {
            case
        };
        let params = self.a.list(params);
        let fun = self.a.tagged("c_fun", vec![fun_ann, params, case]);
        let fun = self.finish_named_funs(fun)?;
        let fun = self.export_liveness(fun)?;
        let fun = self.lower_final(fun)?;
        let ann = self.empty();
        let binder = self.variable(ann, fa);
        Ok(self.a.tuple(vec![binder, fun]))
    }
    fn nif_start(&mut self, body: TermId) -> TermId {
        let ann = self.empty();
        let name = self.atom_literal("nif_start");
        let args = self.empty();
        let start = self.a.tagged("c_primop", vec![ann, name, args]);
        self.a.tagged("c_seq", vec![ann, start, body])
    }
    fn insert_nif_start(&mut self, definition: TermId) -> Result<TermId> {
        let mut pair = self.a.tuple_fields(definition)?.to_vec();
        let mut fun = self.a.tuple_fields(pair[1])?.to_vec();
        let body = fun[3];
        if self.a.tag(body) == Some("c_seq") {
            let seq = self.a.tuple_fields(body)?;
            if self.a.tag(seq[2]) == Some("c_primop") {
                let primop = self.a.tuple_fields(seq[2])?;
                if self.a.as_atom(self.a.tuple_fields(primop[2])?[2]) == Some("nif_start") {
                    return Ok(definition);
                }
            }
        }
        fun[3] = if self.a.tag(body) == Some("c_letrec") {
            let mut letrec = self.a.tuple_fields(body)?.to_vec();
            let mut defs = Vec::new();
            for def in self.a.list_items(letrec[2])? {
                defs.push(self.insert_nif_start(def)?);
            }
            letrec[2] = self.a.list(defs);
            letrec[3] = self.nif_start(letrec[3]);
            self.a.tuple(letrec)
        } else {
            self.nif_start(body)
        };
        pair[1] = self.a.tuple(fun);
        Ok(self.a.tuple(pair))
    }
    fn failure(&mut self, pats: Vec<TermId>, ann: TermId, reason: &str) -> Result<TermId> {
        let reason = self.atom_literal(reason);
        let mut es = vec![reason];
        es.extend(pats.iter().copied());
        let es = self.a.list(es);
        let tuple_ann =
            if self.a.as_atom(self.a.tuple_fields(reason)?[2]) == Some("function_clause") {
                ann
            } else {
                self.empty()
            };
        let elements = self.a.list_items(es)?;
        let all_literal = elements.iter().all(|&e| self.a.tag(e) == Some("c_literal"));
        let tuple = if all_literal {
            let values = elements
                .iter()
                .map(|&e| self.a.tuple_fields(e).map(|f| f[2]))
                .collect::<Result<Vec<_>>>()?;
            let value = self.a.tuple(values);
            self.literal(tuple_ann, value)
        } else {
            self.a.tagged("c_tuple", vec![tuple_ann, es])
        };
        let name = self.atom_literal("match_fail");
        let args = self.a.list(vec![tuple]);
        let primop = self.a.tagged("c_primop", vec![ann, name, args]);
        let generated = self.a.atom("compiler_generated");
        let gen_ann = self.a.list(vec![generated]);
        let pats = self.a.list(pats);
        let truth = self.atom_literal("true");
        Ok(self
            .a
            .tagged("c_clause", vec![gen_ann, pats, truth, primop]))
    }
    fn clause(&mut self, node: TermId) -> Result<TermId> {
        let f = self.a.tuple_fields(node)?.to_vec();
        if self.a.tag(node) != Some("clause") || f.len() != 5 {
            return Err(self.err("malformed clause"));
        }
        let ann = self.annotation(f[1])?;
        let outer_known = self.known_vars.clone();
        let outer_pins = std::mem::take(&mut self.pin_tests);
        let mut pats = Vec::new();
        let mut bound = HashSet::new();
        for p in self.a.list_items(f[2])? {
            pats.push(self.pattern(p, &mut bound)?);
        }
        let mut guard = self.guard(f[3])?;
        let pins = std::mem::take(&mut self.pin_tests);
        let (composed, allocations) =
            self.compose_pattern_guard(pins, guard, self.a.list_items(f[3])?.is_empty())?;
        guard = composed;
        self.known_vars.extend(bound.iter().cloned());
        bound.extend(outer_known.iter().cloned());
        let case_patterns = self.case_patterns;
        self.case_patterns = false;
        let body = self.body(f[4], &mut bound);
        self.case_patterns = case_patterns;
        self.known_vars = outer_known;
        self.pin_tests = outer_pins;
        let body = body?;
        let pats = self.a.list(pats);
        let clause = self.a.tagged("c_clause", vec![ann, pats, guard, body]);
        if !allocations.is_empty() {
            self.guard_allocations.insert(clause, allocations);
        }
        Ok(clause)
    }
    fn deferred_fresh(&mut self, ann: TermId) -> TermId {
        let marker = self.a.atom(format!("$native_late_{}", self.a.terms.len()));
        let variable = self.variable(ann, marker);
        self.deferred_vars.insert(variable);
        variable
    }
    fn compose_pattern_guard(
        &mut self,
        mut pins: Vec<TermId>,
        mut guard: TermId,
        empty_guard: bool,
    ) -> Result<(TermId, Vec<TermId>)> {
        if pins.is_empty() {
            return Ok((guard, vec![]));
        }
        if empty_guard {
            guard = pins.pop().unwrap();
        }
        // OTP uguard appends to the guard expression sequence, rather than
        // treating a precompiled sequence as one nested argument.
        let mut pre = Vec::new();
        loop {
            let f = self.a.tuple_fields(guard)?.to_vec();
            match self.a.tag(guard) {
                Some("c_let") => {
                    let vars = self.a.list_items(f[2])?;
                    if vars.len() != 1 {
                        break;
                    }
                    self.let_annotations.insert(vars[0], f[1]);
                    pre.push(Pre::Set(vars[0], f[3]));
                    guard = f[4];
                }
                Some("c_seq") => {
                    pre.push(Pre::Effect(f[2]));
                    guard = f[3];
                }
                _ => break,
            }
        }
        let mut allocations = Vec::new();
        for pin in pins.into_iter().rev() {
            let empty = self.empty();
            let left = self.deferred_fresh(empty);
            let right = self.deferred_fresh(empty);
            allocations.extend([left, right]);
            let mut next = vec![Pre::Set(left, pin)];
            next.extend(pre);
            next.push(Pre::Set(right, guard));
            pre = next;
            let erlang = self.atom_literal("erlang");
            let op = self.atom_literal("and");
            let args = self.a.list(vec![left, right]);
            guard = self.a.tagged("c_call", vec![empty, erlang, op, args]);
        }
        Ok((self.wrap(pre, guard), allocations))
    }
    fn pattern(&mut self, node: TermId, bound: &mut HashSet<String>) -> Result<TermId> {
        let raw = self.raw_pattern(node)?;
        self.normalize_pattern(raw, bound)
    }
    fn raw_pattern(&mut self, node: TermId) -> Result<TermId> {
        let f = self.a.tuple_fields(node)?.to_vec();
        let tag = self
            .a
            .tag(node)
            .ok_or("lower: malformed pattern")?
            .to_owned();
        let ann = self.annotation(f[1])?;
        match tag.as_str() {
            "var" => Ok(self.variable(ann, f[2])),
            "atom" | "integer" | "char" | "float" | "string" => Ok(self.literal(ann, f[2])),
            "nil" => {
                let nil = self.empty();
                Ok(self.literal(ann, nil))
            }
            "tuple" => {
                let ann = self.record_annotation(f[1])?;
                let mut es = Vec::new();
                for p in self.a.list_items(f[2])? {
                    es.push(self.raw_pattern(p)?);
                }
                let es = self.a.list(es);
                Ok(self.a.tagged("c_tuple", vec![ann, es]))
            }
            "cons" => {
                let h = self.raw_pattern(f[2])?;
                let t = self.raw_pattern(f[3])?;
                Ok(self.a.tagged("c_cons", vec![ann, h, t]))
            }
            "bin" => self.binary_pattern(&f, ann, &mut HashSet::new()),
            "map" => {
                let mut rows = Vec::new();
                for pair in self.a.list_items(f[2])? {
                    let pf = self.a.tuple_fields(pair)?.to_vec();
                    if self.a.tag(pair) != Some("map_field_exact") || pf.len() != 4 {
                        return Err(self.err("map pattern association"));
                    }
                    let key = self.partial_eval(pf[2])?;
                    let key_expr = self.expr(key)?;
                    let key_value = self.wrap(key_expr.pre, key_expr.value);
                    let value = self.raw_pattern(pf[3])?;
                    let op = self.atom_literal("exact");
                    let pair_ann = self.annotation(pf[1])?;
                    let lowered = self
                        .a
                        .tagged("c_map_pair", vec![pair_ann, op, key_value, value]);
                    rows.push(lowered);
                }
                let ordered = self.sort_pattern_map_pairs(rows)?;
                let pairs = self.a.list(ordered);
                let empty_ann = self.empty();
                let empty_map = self.a.push(ErlangTerm::Map(vec![]));
                let base = self.literal(empty_ann, empty_map);
                let pattern = self.a.atom("true");
                Ok(self.a.tagged("c_map", vec![ann, base, pairs, pattern]))
            }
            "match" => {
                let left = self.raw_pattern(f[2])?;
                let right = self.raw_pattern(f[3])?;
                self.alias_patterns(left, right)
            }
            "op" => {
                if let Some(folded) = self.constant_pattern(node)? {
                    self.raw_pattern(folded)
                } else {
                    Err(self.err("nonconstant operator pattern"))
                }
            }
            _ => Err(self.err(&format!("pattern {tag}"))),
        }
    }
    fn rename_wildcards(&mut self, node: TermId) -> Result<TermId> {
        // Matched by reference: most visits reach shared leaves, which are
        // returned unchanged and need no copy.
        match self.a.get(node) {
            ErlangTerm::Tuple(f) => {
                let mut f = f.clone();
                if let Some(known) = self.shadowed_funs.remove(&node) {
                    let renamed = self.rename_fun_shadowing(node, known)?;
                    return self.rename_wildcards(renamed);
                }
                if let Some(allocations) = self.guard_allocations.remove(&node) {
                    f[2] = self.rename_wildcards(f[2])?;
                    for variable in allocations {
                        self.rename_wildcards(variable)?;
                    }
                    f[3] = self.rename_wildcards(f[3])?;
                    f[4] = self.rename_wildcards(f[4])?;
                    return Ok(self.a.tuple(f));
                }
                if let Some(original) = self.deferred_aliases.get(&node).copied() {
                    let variable = self.rename_wildcards(original)?;
                    f[2] = self.a.tuple_fields(variable)?[2];
                    return Ok(self.a.tuple(f));
                }
                if self.a.tag(node) == Some("c_var")
                    && f.len() == 3
                    && (self.a.as_atom(f[2]) == Some("_") || self.deferred_vars.contains(&node))
                {
                    if let Some(renamed) = self.wildcard_renames.get(&node) {
                        return Ok(*renamed);
                    }
                    let renamed = self.fresh(f[1]);
                    self.wildcard_renames.insert(node, renamed);
                    return Ok(renamed);
                }
                if self.a.tag(node) == Some("c_literal") {
                    return Ok(node);
                }
                for child in &mut f {
                    *child = self.rename_wildcards(*child)?;
                }
                Ok(self.a.rebuild_tuple(node, f))
            }
            ErlangTerm::List { items, tail } => {
                let (items, tail) = (items.clone(), *tail);
                let mut out = Vec::with_capacity(items.len());
                for x in items {
                    out.push(self.rename_wildcards(x)?);
                }
                let tail = self.rename_wildcards(tail)?;
                Ok(self.a.rebuild_list(node, out, tail))
            }
            _ => Ok(node),
        }
    }
    fn protected_guard(&mut self, ann: TermId, expression: Expr) -> TermId {
        if expression.pre.is_empty() {
            return expression.value;
        }
        let arg = self.wrap(expression.pre, expression.value);
        let empty = self.empty();
        let name = self.a.atom("Try");
        let try_var = self.variable(empty, name);
        let name = self.a.atom("T");
        let t = self.variable(empty, name);
        let name = self.a.atom("R");
        let r = self.variable(empty, name);
        let vars = self.a.list(vec![try_var]);
        let evars = self.a.list(vec![t, r]);
        let handler = self.atom_literal("false");
        let node = self
            .a
            .tagged("c_try", vec![ann, arg, vars, try_var, evars, handler]);
        self.protected_guards.insert(node);
        node
    }
    fn guard(&mut self, node: TermId) -> Result<TermId> {
        let alternatives = self.a.list_items(node)?;
        if alternatives.is_empty() {
            return Ok(self.atom_literal("true"));
        }
        let old_in_guard = self.in_guard;
        self.in_guard = true;
        let expression = self.guard_alternatives(&alternatives);
        self.in_guard = old_in_guard;
        let expression = expression?;
        Ok(self.wrap(expression.pre, expression.value))
    }
    fn guard_alternatives(&mut self, alternatives: &[TermId]) -> Result<Expr> {
        if alternatives.len() > 1024 {
            return Err(self.err("guard alternatives depth limit"));
        }
        let tests = self.a.list_items(alternatives[0])?;
        let first = *tests.first().ok_or("lower: empty guard alternative")?;
        let f = self.a.tuple_fields(first)?.to_vec();
        let ann = self.annotation(f[1])?;
        let value = {
            let mut combined = *tests.last().unwrap();
            for left in tests[..tests.len() - 1].iter().rev() {
                let op = self.a.atom("and");
                combined = self.a.tagged("op", vec![f[1], op, *left, combined]);
            }
            let (expression, bools) = self.known_guard_expr(combined)?;
            let expression = self.force_guard_booleans(expression, bools)?;
            self.protected_guard(ann, expression)
        };
        let left = Expr { value, pre: vec![] };
        if alternatives.len() == 1 {
            return Ok(left);
        }
        let left = self.guard_safe(left)?;
        let right = self.guard_alternatives(&alternatives[1..])?;
        let right = self.guard_safe(right)?;
        let mut pre = left.pre;
        pre.extend(right.pre);
        let value = self.guard_call(ann, "or", vec![left.value, right.value]);
        Ok(Expr { value, pre })
    }
    fn guard_safe(&mut self, mut expression: Expr) -> Result<Expr> {
        if !matches!(
            self.a.tag(expression.value),
            Some("c_literal" | "c_var" | "c_cons" | "c_tuple")
        ) {
            let ann = if self.generated_guard_equalities.contains(&expression.value) {
                let pass = self.a.atom("v3_core");
                let compiler = self.a.atom("compiler_generated");
                self.a.list(vec![pass, compiler])
            } else {
                self.expression_annotation(expression.value)?
            };
            let variable = self.fresh(ann);
            expression.pre.push(Pre::Set(variable, expression.value));
            expression.value = variable;
        }
        Ok(expression)
    }
    fn guard_call(&mut self, ann: TermId, name: &str, args: Vec<TermId>) -> TermId {
        let erlang = self.a.atom("erlang");
        let erlang = self.literal(ann, erlang);
        let name = self.a.atom(name);
        let name = self.literal(ann, name);
        let args = self.a.list(args);
        self.a.tagged("c_call", vec![ann, erlang, name, args])
    }
    fn known_guard_expr(&mut self, node: TermId) -> Result<(Expr, Vec<TermId>)> {
        let f = self.a.tuple_fields(node)?.to_vec();
        if self.a.tag(node) == Some("atom")
            && matches!(self.a.as_atom(f[2]), Some("true" | "false"))
        {
            let ann = self.annotation(f[1])?;
            let value = self.literal(ann, f[2]);
            return Ok((Expr { value, pre: vec![] }, vec![]));
        }
        let mut operation = None;
        if self.a.tag(node) == Some("op") {
            operation = self
                .a
                .as_atom(f[2])
                .map(|op| (op.to_owned(), f[3..].to_vec()));
        }
        if self.a.tag(node) == Some("call") && self.a.tag(f[2]) == Some("remote") {
            let remote = self.a.tuple_fields(f[2])?;
            if self.a.tag(remote[2]) == Some("atom")
                && self.a.as_atom(self.a.tuple_fields(remote[2])?[2]) == Some("erlang")
                && self.a.tag(remote[3]) == Some("atom")
            {
                let name = self
                    .a
                    .as_atom(self.a.tuple_fields(remote[3])?[2])
                    .unwrap_or("")
                    .to_owned();
                operation = Some((name, self.a.list_items(f[3])?));
            }
        }
        if let Some((op, args)) = &operation {
            if matches!(op.as_str(), "and" | "or" | "xor") && args.len() == 2 {
                let (left, mut bools) = self.known_guard_expr(args[0])?;
                let left = self.guard_safe(left)?;
                let (right, rb) = self.known_guard_expr(args[1])?;
                let right = self.guard_safe(right)?;
                bools.extend(rb);
                let mut pre = left.pre;
                pre.extend(right.pre);
                let ann = self.annotation(f[1])?;
                let value = self.guard_call(ann, op, vec![left.value, right.value]);
                return Ok((Expr { value, pre }, bools));
            }
            if op == "not" && args.len() == 1 {
                let (mut inner, bools) = self.known_guard_expr(args[0])?;
                let fields = self.a.tuple_fields(inner.value)?.to_vec();
                if self.a.tag(inner.value) == Some("c_call")
                    && self.generated_guard_equalities.contains(&inner.value)
                    && self.a.as_atom(self.a.tuple_fields(fields[3])?[2]) == Some("=:=")
                    && self
                        .a
                        .list_items(fields[1])?
                        .iter()
                        .any(|ann| self.a.as_atom(*ann) == Some("compiler_generated"))
                {
                    let mut call_args = self.a.list_items(fields[4])?;
                    call_args[1] = self.atom_literal("false");
                    let mut fields = fields;
                    fields[4] = self.a.list(call_args);
                    inner.value = self.a.tuple(fields);
                    self.generated_guard_equalities.insert(inner.value);
                    return Ok((self.guard_safe(inner)?, bools));
                }
                let inner = self.guard_safe(inner)?;
                let ann = self.annotation(f[1])?;
                let value = self.guard_call(ann, "not", vec![inner.value]);
                return Ok((
                    Expr {
                        value,
                        pre: inner.pre,
                    },
                    bools,
                ));
            }
        }
        let mut expression = self.expr(node)?;
        if self.boolean_test(expression.value)? {
            return Ok((expression, vec![]));
        }
        let special_is_function = operation
            .as_ref()
            .is_some_and(|(op, args)| op == "is_function" && args.len() == 2);
        let simple = matches!(
            self.a.tag(expression.value),
            Some("c_literal" | "c_var" | "c_cons" | "c_tuple")
        );
        if !simple {
            let ann = if self.generated_guard_equalities.contains(&expression.value) {
                let pass = self.a.atom("v3_core");
                let compiler = self.a.atom("compiler_generated");
                self.a.list(vec![pass, compiler])
            } else {
                self.expression_annotation(expression.value)?
            };
            let variable = self.fresh(ann);
            let let_ann = if self.a.tag(expression.value) == Some("c_call") {
                ann
            } else {
                let compiler = self.a.atom("compiler_generated");
                self.a.list(vec![compiler])
            };
            self.let_annotations.insert(variable, let_ann);
            expression.pre.push(Pre::Set(variable, expression.value));
            expression.value = variable;
        }
        let operand = expression.value;
        let compiler = self.a.atom("compiler_generated");
        let ann = self.a.list(vec![compiler]);
        let erlang = self.atom_literal("erlang");
        let name = self.atom_literal("=:=");
        let truth = self.atom_literal("true");
        let args = self.a.list(vec![operand, truth]);
        expression.value = self.a.tagged("c_call", vec![ann, erlang, name, args]);
        self.generated_guard_equalities.insert(expression.value);
        Ok((
            expression,
            if special_is_function {
                vec![]
            } else {
                vec![operand]
            },
        ))
    }
    fn same_guard_operand(&self, a: TermId, b: TermId) -> Result<bool> {
        if a == b {
            return Ok(true);
        }
        if self.a.tag(a) != self.a.tag(b) {
            return Ok(false);
        }
        if matches!(self.a.tag(a), Some("c_var" | "c_literal")) {
            return Ok(
                self.a.get(self.a.tuple_fields(a)?[2]) == self.a.get(self.a.tuple_fields(b)?[2])
            );
        }
        Ok(false)
    }
    fn unforce_guard(
        &self,
        value: TermId,
        pre: &[Pre],
        bools: &mut Vec<TermId>,
        depth: usize,
    ) -> Result<()> {
        if depth > 1024 {
            return Err(self.err("guard substitution depth limit"));
        }
        if self.a.tag(value) == Some("c_var") {
            for instruction in pre.iter().rev() {
                if let Pre::Set(variable, arg) = instruction {
                    if self.same_guard_operand(value, *variable)? {
                        return self.unforce_guard(*arg, pre, bools, depth + 1);
                    }
                }
            }
            return Ok(());
        }
        if self.a.tag(value) != Some("c_call") {
            return Ok(());
        }
        let f = self.a.tuple_fields(value)?;
        if self.a.as_atom(self.a.tuple_fields(f[2])?[2]) != Some("erlang") {
            return Ok(());
        }
        let name = self.a.as_atom(self.a.tuple_fields(f[3])?[2]);
        let args = self.a.list_items(f[4])?;
        if name == Some("and") && args.len() == 2 {
            self.unforce_guard(args[0], pre, bools, depth + 1)?;
            self.unforce_guard(args[1], pre, bools, depth + 1)?;
        } else if name == Some("=:=")
            && args.len() == 2
            && self.a.tag(args[1]) == Some("c_literal")
            && matches!(
                self.a.as_atom(self.a.tuple_fields(args[1])?[2]),
                Some("true" | "false")
            )
        {
            let mut kept = Vec::new();
            let mut removed = false;
            for variable in bools.iter().copied() {
                if !removed && self.same_guard_operand(variable, args[0])? {
                    removed = true;
                } else {
                    kept.push(variable);
                }
            }
            *bools = kept;
        }
        Ok(())
    }
    fn force_guard_booleans(
        &mut self,
        mut expression: Expr,
        mut bools: Vec<TermId>,
    ) -> Result<Expr> {
        self.unforce_guard(expression.value, &expression.pre, &mut bools, 0)?;
        for operand in bools.into_iter().rev() {
            expression = self.guard_safe(expression)?;
            let mut f = self.a.tuple_fields(operand)?.to_vec();
            f[1] = self.empty();
            let operand = self.a.tuple(f);
            let compiler = self.a.atom("compiler_generated");
            let ann = self.a.list(vec![compiler]);
            let erlang = self.atom_literal("erlang");
            let name = self.atom_literal("is_boolean");
            let args = self.a.list(vec![operand]);
            let call = self.a.tagged("c_call", vec![ann, erlang, name, args]);
            let empty = self.empty();
            let variable = self.fresh(empty);
            expression.pre.push(Pre::Set(variable, call));
            let erlang = self.atom_literal("erlang");
            let name = self.atom_literal("and");
            let args = self.a.list(vec![expression.value, variable]);
            expression.value = self.a.tagged("c_call", vec![ann, erlang, name, args]);
        }
        Ok(expression)
    }
    fn boolean_test(&self, node: TermId) -> Result<bool> {
        let f = self.a.tuple_fields(node)?;
        if self.a.tag(node) == Some("c_literal") {
            return Ok(matches!(self.a.as_atom(f[2]), Some("true" | "false")));
        }
        if self.a.tag(node) != Some("c_call") {
            return Ok(false);
        }
        let m = self.a.tuple_fields(f[2])?;
        let n = self.a.tuple_fields(f[3])?;
        if self.a.as_atom(m[2]) != Some("erlang") {
            return Ok(false);
        }
        let name = self.a.as_atom(n[2]).unwrap_or("");
        let arity = self.a.list_items(f[4])?.len();
        Ok((name == "is_record" && matches!(arity, 2 | 3))
            || (arity == 1
                && matches!(
                    name,
                    "is_atom"
                        | "is_binary"
                        | "is_bitstring"
                        | "is_boolean"
                        | "is_float"
                        | "is_function"
                        | "is_integer"
                        | "is_list"
                        | "is_map"
                        | "is_number"
                        | "is_pid"
                        | "is_port"
                        | "is_reference"
                        | "is_tuple"
                ))
            || (arity == 2
                && matches!(
                    name,
                    "==" | "/=" | "=:=" | "=/=" | "<" | "=<" | ">" | ">=" | "and" | "or" | "xor"
                )))
    }
    fn body(&mut self, node: TermId, bound: &mut HashSet<String>) -> Result<TermId> {
        let (steps, _) = self.body_steps(node, bound)?;
        self.assemble_steps(steps, None)
    }
    fn preamble_ir_size(&self, pre: &[Pre]) -> Result<usize> {
        pre.iter().try_fold(0, |sum, instruction| {
            Ok(sum
                + match instruction {
                    Pre::Set(_, arg) | Pre::Effect(arg) => self.expression_ir_size(*arg)?,
                    Pre::Match { guard, .. } => {
                        1 + if self.a.tag(*guard) == Some("c_literal") {
                            0
                        } else {
                            self.expression_ir_size(*guard)?
                        }
                    }
                })
        })
    }
    fn expression_ir_size(&self, node: TermId) -> Result<usize> {
        let f = self.a.tuple_fields(node)?;
        Ok(match self.a.tag(node) {
            Some("c_let") => self.expression_ir_size(f[3])? + self.expression_ir_size(f[4])?,
            Some("c_seq") => self.expression_ir_size(f[2])? + self.expression_ir_size(f[3])?,
            Some("c_case") => {
                let mut n = 1;
                let clauses = self.a.list_items(f[3])?;
                for clause in clauses.iter().take(clauses.len().saturating_sub(1)) {
                    n += self.expression_ir_size(*clause)?;
                }
                n
            }
            Some("c_fun") => self.expression_ir_size(f[3])?,
            Some("c_clause") => {
                1 + self.expression_ir_size(f[4])?
                    + if self.a.tag(f[3]) == Some("c_literal") {
                        0
                    } else {
                        self.expression_ir_size(f[3])?
                    }
            }
            Some("c_receive") => {
                let mut n = 1;
                for clause in self.a.list_items(f[2])? {
                    n += self.expression_ir_size(clause)?;
                }
                n
            }
            Some("c_catch") => 1 + self.expression_ir_size(f[2])?,
            Some("c_try") if self.protected_guards.contains(&node) => {
                1 + self.expression_ir_size(f[2])?
            }
            Some("c_try") => 1 + self.expression_ir_size(f[4])? + self.expression_ir_size(f[6])?,
            _ => 1,
        })
    }
    fn body_steps(
        &mut self,
        node: TermId,
        bound: &mut HashSet<String>,
    ) -> Result<(Vec<(Expr, bool)>, usize)> {
        let expressions = self.a.list_items(node)?;
        if expressions.is_empty() {
            return Err(self.err("empty body"));
        }
        let mut built = Vec::new();
        let mut ir_size = 0;
        for e in expressions.iter().copied() {
            if self.a.tag(e) == Some("match") {
                let mode = if e == *expressions.last().unwrap() {
                    matches::MatchMode::Last
                } else {
                    matches::MatchMode::Discard
                };
                let out = self.matching(e, bound, mode)?;
                ir_size += self.preamble_ir_size(&out.pre)?;
                built.push((out, true));
            } else {
                let out = self.expr(e)?;
                let exports = self.source_export_names(e)?;
                bound.extend(exports.iter().cloned());
                self.known_vars.extend(exports);
                ir_size += self.preamble_ir_size(&out.pre)? + self.expression_ir_size(out.value)?;
                built.push((out, false));
            }
        }
        Ok((built, ir_size))
    }
    fn assemble_steps(
        &mut self,
        mut built: Vec<(Expr, bool)>,
        tail: Option<TermId>,
    ) -> Result<TermId> {
        let mut result = if let Some(tail) = tail {
            tail
        } else {
            let (last, _) = built.pop().ok_or_else(|| self.err("empty body"))?;
            self.wrap(last.pre, last.value)
        };
        for (e, assigned) in built.into_iter().rev() {
            if assigned {
                result = self.wrap(e.pre, result);
                continue;
            }
            let ann = self.empty();
            let seq = self.a.tagged("c_seq", vec![ann, e.value, result]);
            result = self.wrap(e.pre, seq);
        }
        Ok(result)
    }
    fn match_return_value(&mut self, expression: &mut Expr) -> Result<TermId> {
        if !matches!(
            self.a.tag(expression.value),
            Some("c_literal" | "c_var" | "c_tuple" | "c_cons")
        ) {
            let ann = self.expression_annotation(expression.value)?;
            let marker = self.a.atom(format!("$native_safe_{}", self.a.terms.len()));
            let var = self.variable(ann, marker);
            self.deferred_vars.insert(var);
            expression.pre.push(Pre::Set(var, expression.value));
            expression.value = var;
        }
        self.mark_match_return(expression.value)
    }
    fn mark_match_return(&mut self, node: TermId) -> Result<TermId> {
        let mut fields = self.a.tuple_fields(node)?.to_vec();
        let marker = self.a.atom("compiler_generated");
        let mut ann = vec![marker];
        ann.extend(self.a.list_items(fields[1])?);
        fields[1] = self.a.list(ann);
        match self.a.tag(node) {
            Some("c_tuple") => {
                let mut es = Vec::new();
                for e in self.a.list_items(fields[2])? {
                    es.push(self.mark_match_return(e)?);
                }
                if es.iter().all(|e| self.a.tag(*e) == Some("c_literal")) {
                    let values = es
                        .iter()
                        .map(|e| self.a.tuple_fields(*e).map(|f| f[2]))
                        .collect::<Result<Vec<_>>>()?;
                    let value = self.a.tuple(values);
                    return Ok(self.literal(fields[1], value));
                }
                fields[2] = self.a.list(es);
            }
            Some("c_cons") => {
                fields[2] = self.mark_match_return(fields[2])?;
                fields[3] = self.mark_match_return(fields[3])?;
                if self.a.tag(fields[2]) == Some("c_literal")
                    && self.a.tag(fields[3]) == Some("c_literal")
                {
                    let head = self.a.tuple_fields(fields[2])?[2];
                    let tail = self.a.tuple_fields(fields[3])?[2];
                    let value = self.a.push(ErlangTerm::List {
                        items: vec![head],
                        tail,
                    });
                    return Ok(self.literal(fields[1], value));
                }
            }
            _ => (),
        }
        let marked = self.a.tuple(fields);
        if self.deferred_vars.contains(&node) {
            // The marked return is the same binder, only with another annotation.
            self.deferred_aliases.insert(marked, node);
        }
        Ok(marked)
    }
    fn wrap(&mut self, pre: Vec<Pre>, mut result: TermId) -> TermId {
        for instruction in pre.into_iter().rev() {
            match instruction {
                Pre::Effect(arg) => {
                    let ann = self.empty();
                    result = self.a.tagged("c_seq", vec![ann, arg, result]);
                }
                Pre::Set(var, arg) => {
                    let ann = self
                        .let_annotations
                        .get(&var)
                        .copied()
                        .unwrap_or_else(|| self.empty());
                    let vars = self.a.list(vec![var]);
                    result = self.a.tagged("c_let", vec![ann, vars, arg, result]);
                }
                Pre::Match {
                    ann,
                    arg,
                    pattern,
                    guard,
                    failure,
                    allocations,
                    ..
                } => {
                    let patterns = self.a.list(vec![pattern]);
                    let success = self
                        .a
                        .tagged("c_clause", vec![ann, patterns, guard, result]);
                    if !allocations.is_empty() {
                        self.guard_allocations.insert(success, allocations);
                    }
                    let clauses = self.a.list(vec![success, failure]);
                    result = self.a.tagged("c_case", vec![ann, arg, clauses]);
                }
            }
        }
        result
    }
    // Source lowering allocates right-to-left; binding discovery executes left-to-right.
    // Each operand starts in the group entry scope, so siblings cannot capture its new bindings.
    fn safe_list(&mut self, input: Vec<TermId>) -> Result<(Vec<TermId>, Vec<Pre>)> {
        let entry = self.known_vars.clone();
        let mut chunks = Vec::new();
        let mut values = Vec::new();
        for node in input.into_iter().rev() {
            self.known_vars = entry.clone();
            let expr = self.source_safe(node)?;
            values.push(expr.value);
            chunks.push(expr.pre);
        }
        values.reverse();
        chunks.reverse();
        self.known_vars = entry.clone();
        let mut known = entry;
        let mut pre = Vec::new();
        for chunk in chunks {
            for instruction in chunk {
                let instruction = match instruction {
                    Pre::Match {
                        ann,
                        arg,
                        raw_pattern,
                        failure,
                        ..
                    } => {
                        let old_case = self.case_patterns;
                        self.case_patterns = true;
                        self.known_vars = known.clone();
                        let old_pins = std::mem::take(&mut self.pin_tests);
                        let pattern = self.normalize_pattern(raw_pattern, &mut known)?;
                        self.case_patterns = old_case;
                        let pins = std::mem::take(&mut self.pin_tests);
                        let truth = self.atom_literal("true");
                        let (guard, allocations) = self.compose_pattern_guard(pins, truth, true)?;
                        self.pin_tests = old_pins;
                        Pre::Match {
                            ann,
                            arg,
                            pattern,
                            raw_pattern,
                            guard,
                            failure,
                            allocations,
                        }
                    }
                    Pre::Set(var, arg) => {
                        self.collect_raw_pattern_names(var, &mut known)?;
                        Pre::Set(var, arg)
                    }
                    other => other,
                };
                pre.push(instruction);
            }
        }
        self.known_vars = known;
        Ok((values, pre))
    }
    fn full_annotation(&mut self, ann: TermId) -> Result<TermId> {
        if self.wanted {
            return Ok(ann);
        }
        let mark = self.a.atom("result_not_wanted");
        let mut items = vec![mark];
        items.extend(self.a.list_items(ann)?);
        Ok(self.a.list(items))
    }
    fn right_assoc(&mut self, mut node: TermId, operation: &str) -> Result<TermId> {
        loop {
            let f = self.a.tuple_fields(node)?.to_vec();
            if f.len() != 5 || self.a.tag(f[3]) != Some("op") {
                return Ok(node);
            }
            let left = self.a.tuple_fields(f[3])?.to_vec();
            if left.len() != 5 || self.a.as_atom(left[2]) != Some(operation) {
                return Ok(node);
            }
            let right = self.a.tagged("op", vec![f[1], f[2], left[4], f[4]]);
            node = self.a.tagged("op", vec![left[1], left[2], left[3], right]);
        }
    }
    fn boolean_switch(&mut self, f: &[TermId], orelse: bool) -> Result<TermId> {
        let ann = self.annotation(f[1])?;
        let fresh = self.fresh(ann);
        let variable_name = self.a.tuple_fields(fresh)?[2];
        let source_var = self.a.tagged("var", vec![f[1], variable_name]);
        let generated_atom = self.a.atom("generated");
        let true_atom = self.a.atom("true");
        let generated = self.a.tuple(vec![generated_atom, true_atom]);
        let location_atom = self.a.atom("location");
        let location = match self.a.get(f[1]) {
            ErlangTerm::List { .. } => self
                .a
                .keyword(f[1], "location")
                .ok_or("lower: boolean location")?,
            _ => f[1],
        };
        let location = self.a.tuple(vec![location_atom, location]);
        let negative = self.a.list(vec![generated, location]);
        let mut clauses = Vec::new();
        for (atom, body) in [
            (
                "true",
                if orelse {
                    let a = self.a.atom("true");
                    self.a.tagged("atom", vec![f[1], a])
                } else {
                    f[4]
                },
            ),
            (
                "false",
                if orelse {
                    f[4]
                } else {
                    let a = self.a.atom("false");
                    self.a.tagged("atom", vec![f[1], a])
                },
            ),
        ] {
            let atom = self.a.atom(atom);
            let pattern = self.a.tagged("atom", vec![negative, atom]);
            let patterns = self.a.list(vec![pattern]);
            let guards = self.empty();
            let body = self.a.list(vec![body]);
            clauses.push(
                self.a
                    .tagged("clause", vec![negative, patterns, guards, body]),
            );
        }
        let badarg = self.a.atom("badarg");
        let badarg = self.a.tagged("atom", vec![negative, badarg]);
        let tuple_args = self.a.list(vec![badarg, source_var]);
        let tuple = self.a.tagged("tuple", vec![negative, tuple_args]);
        let erlang = self.a.atom("erlang");
        let erlang = self.a.tagged("atom", vec![negative, erlang]);
        let error = self.a.atom("error");
        let error = self.a.tagged("atom", vec![negative, error]);
        let remote = self.a.tagged("remote", vec![negative, erlang, error]);
        let args = self.a.list(vec![tuple]);
        let call = self.a.tagged("call", vec![negative, remote, args]);
        let body = self.a.list(vec![call]);
        let patterns = self.a.list(vec![source_var]);
        let guards = self.empty();
        clauses.push(
            self.a
                .tagged("clause", vec![negative, patterns, guards, body]),
        );
        let clauses = self.a.list(clauses);
        Ok(self.a.tagged("case", vec![negative, f[3], clauses]))
    }
    fn literal_key(&self, node: TermId, depth: usize) -> Result<LiteralKey> {
        if depth > 1024 {
            return Err(self.err("literal map key depth limit"));
        }
        Ok(match self.a.get(node) {
            ErlangTerm::Atom(a) => LiteralKey::Atom(a.clone()),
            ErlangTerm::Integer(n) => LiteralKey::Integer(
                n.parse::<num_bigint::BigInt>()
                    .map_err(|_| self.err("invalid map integer"))?
                    .to_string(),
            ),
            ErlangTerm::Float(bits) => LiteralKey::Float(*bits),
            ErlangTerm::ExternalFun(external_fun_payload) => {
                let crate::core::ExternalFun {
                    module,
                    function,
                    arity,
                } = external_fun_payload.as_ref();
                LiteralKey::ExternalFun(module.clone(), function.clone(), *arity)
            }
            ErlangTerm::Binary(bytes) => LiteralKey::Bits(bytes.clone(), (bytes.len() as u64) * 8),
            ErlangTerm::Bitstring { bytes, bits } => LiteralKey::Bits(bytes.clone(), *bits),
            ErlangTerm::Nil => LiteralKey::Nil,
            ErlangTerm::Tuple(fields) => LiteralKey::Tuple(
                fields
                    .iter()
                    .map(|x| self.literal_key(*x, depth + 1))
                    .collect::<Result<Vec<_>>>()?,
            ),
            ErlangTerm::List { .. } => {
                let mut current = node;
                let mut values = Vec::new();
                while let ErlangTerm::List { items, tail } = self.a.get(current) {
                    for value in items {
                        values.push(self.literal_key(*value, depth + 1)?);
                    }
                    current = *tail;
                }
                if values.is_empty() {
                    return self.literal_key(current, depth + 1);
                }
                LiteralKey::List(values, Box::new(self.literal_key(current, depth + 1)?))
            }
            ErlangTerm::Map(pairs) => {
                let mut canonical = std::collections::BTreeMap::new();
                for (k, v) in pairs {
                    canonical.insert(
                        self.literal_key(*k, depth + 1)?,
                        self.literal_key(*v, depth + 1)?,
                    );
                }
                LiteralKey::Map(canonical.into_iter().collect())
            }
        })
    }
    fn build_map_pairs(&mut self, source: TermId, base: TermId, ann: TermId) -> Result<Expr> {
        let mut pairs = Vec::new();
        let mut pre = Vec::new();
        let mut literal = if self.a.tag(base) == Some("c_literal") {
            if let ErlangTerm::Map(pairs) = self.a.get(self.a.tuple_fields(base)?[2]) {
                Some(pairs.clone())
            } else {
                None
            }
        } else {
            None
        };
        for pair in self.a.list_items(source)? {
            let pf = self.a.tuple_fields(pair)?.to_vec();
            let op = match self.a.tag(pair) {
                Some("map_field_assoc") => "assoc",
                Some("map_field_exact") => "exact",
                _ => return Err(self.err("map association")),
            };
            if pf.len() != 4 {
                return Err(self.err("malformed map association"));
            }
            let (key, key_pre) = self.safe_list(vec![pf[2]])?;
            pre.extend(key_pre);
            let (value, value_pre) = self.safe_list(vec![pf[3]])?;
            pre.extend(value_pre);
            let key = key[0];
            let value = value[0];
            if let Some(current) = &mut literal {
                if self.a.tag(key) == Some("c_literal") && self.a.tag(value) == Some("c_literal") {
                    let k = self.a.tuple_fields(key)?[2];
                    let v = self.a.tuple_fields(value)?[2];
                    let identity = self.literal_key(k, 0)?;
                    let mut existing = None;
                    for (index, (old, _)) in current.iter().enumerate() {
                        if self.literal_key(*old, 0)? == identity {
                            existing = Some(index);
                            break;
                        }
                    }
                    if let Some(index) = existing {
                        current[index] = (k, v);
                    } else if op == "assoc" {
                        current.push((k, v));
                    } else {
                        literal = None;
                    }
                } else {
                    literal = None;
                }
            }
            let pair_ann = self.annotation(pf[1])?;
            let op = self.atom_literal(op);
            pairs.push(self.a.tagged("c_map_pair", vec![pair_ann, op, key, value]));
        }
        let value = if let Some(pairs) = literal {
            let map = self.a.push(ErlangTerm::Map(pairs));
            self.literal(ann, map)
        } else {
            let pairs = self.a.list(pairs);
            let pattern = self.a.atom("false");
            self.a.tagged("c_map", vec![ann, base, pairs, pattern])
        };
        Ok(Expr { value, pre })
    }
    fn literal_expression(&self, root: TermId) -> Result<bool> {
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            let fields = self.a.tuple_fields(node)?;
            match self.a.tag(node) {
                Some("atom" | "integer" | "char" | "float" | "string" | "nil") => (),
                Some("tuple") if fields.len() == 3 => pending.extend(self.a.list_items(fields[2])?),
                Some("cons") if fields.len() == 4 => pending.extend([fields[2], fields[3]]),
                _ => return Ok(false),
            }
        }
        Ok(true)
    }
    fn literal_cons(&mut self, root: TermId) -> Result<Expr> {
        let mut spine = Vec::new();
        let mut tail = root;
        while self.a.tag(tail) == Some("cons") {
            let fields = self.a.tuple_fields(tail)?.to_vec();
            let ann = self.annotation(fields[1])?;
            let ann = self.full_annotation(ann)?;
            spine.push((ann, fields[2]));
            tail = fields[3];
        }
        let mut result = self.expr(tail)?;
        for (ann, head) in spine.into_iter().rev() {
            let head = self.expr(head)?;
            debug_assert!(head.pre.is_empty() && result.pre.is_empty());
            result.value = self.a.tagged("c_cons", vec![ann, head.value, result.value]);
        }
        Ok(result)
    }
    fn expr(&mut self, node: TermId) -> Result<Expr> {
        if self.a.tag(node) == Some("cons") && self.literal_expression(node)? {
            return self.literal_cons(node);
        }
        if self.depth >= 1024 {
            return Err(self.err("expression depth limit"));
        }
        self.depth += 1;
        let r = self.expr_inner(node);
        self.depth -= 1;
        r
    }
    fn expr_inner(&mut self, node: TermId) -> Result<Expr> {
        let f = self.a.tuple_fields(node)?.to_vec();
        let tag = self
            .a
            .tag(node)
            .ok_or("lower: malformed expression")?
            .to_owned();
        if f.len() < 2 {
            return Err(self.err("malformed expression"));
        }
        let line_ann = self.annotation(f[1])?;
        let ann = if !self.wanted && !matches!(tag.as_str(), "var" | "case" | "if" | "block") {
            let mark = self.a.atom("result_not_wanted");
            let mut annotations = vec![mark];
            annotations.extend(self.a.list_items(line_ann)?);
            self.a.list(annotations)
        } else {
            line_ann
        };
        let (value, pre) = match tag.as_str() {
            "named_fun" => return self.named_fun_expr(&f, ann),
            "maybe" => return self.maybe_expr(&f, line_ann),
            "match" => {
                let mut bound = self.known_vars.clone();
                return self.matching(node, &mut bound, matches::MatchMode::Last);
            }
            "var" => (self.variable(ann, f[2]), vec![]),
            "atom" | "integer" | "char" | "float" | "string" => (self.literal(ann, f[2]), vec![]),
            "nil" => {
                let nil = self.empty();
                (self.literal(ann, nil), vec![])
            }
            "tuple" => {
                let ann = self.record_annotation(f[1])?;
                let input = self.a.list_items(f[2])?;
                let (es, pre) = self.safe_list(input)?;
                let es = self.a.list(es);
                (self.a.tagged("c_tuple", vec![ann, es]), pre)
            }
            "cons" => {
                let (es, pre) = self.safe_list(vec![f[2], f[3]])?;
                (self.a.tagged("c_cons", vec![ann, es[0], es[1]]), pre)
            }
            "call" => {
                let call = self.a.tuple_fields(f[2])?.to_vec();
                let args = self.a.list_items(f[3])?;
                if self.a.tag(f[2]) == Some("remote") {
                    if self.a.tag(call[2]) == Some("atom")
                        && self.a.tag(call[3]) == Some("atom")
                        && self.a.as_atom(self.a.tuple_fields(call[2])?[2]) == Some("erlang")
                        && self.a.as_atom(self.a.tuple_fields(call[3])?[2]) == Some("load_nif")
                        && args.len() == 2
                    {
                        self.load_nif = true;
                    }
                    let mut input = vec![call[2], call[3]];
                    input.extend(args);
                    let (mut es, pre) = self.safe_list(input)?;
                    let module = es.remove(0);
                    let name = es.remove(0);
                    let badrecord =
                        es.len() == 1
                            && self.a.tag(es[0]) == Some("c_tuple")
                            && self
                                .a
                                .list_items(self.a.tuple_fields(es[0])?[2])?
                                .first()
                                .is_some_and(|first| {
                                    self.a.tag(*first) == Some("c_literal")
                                        && self.a.tuple_fields(*first).ok().is_some_and(|f| {
                                            self.a.as_atom(f[2]) == Some("badrecord")
                                        })
                                });
                    let is_error = self.a.tag(module) == Some("c_literal")
                        && self.a.tag(name) == Some("c_literal")
                        && self.a.as_atom(self.a.tuple_fields(module)?[2]) == Some("erlang")
                        && self.a.as_atom(self.a.tuple_fields(name)?[2]) == Some("error");
                    let args = self.a.list(es);
                    if badrecord && is_error {
                        let name = self.atom_literal("match_fail");
                        (self.a.tagged("c_primop", vec![ann, name, args]), pre)
                    } else {
                        (self.a.tagged("c_call", vec![ann, module, name, args]), pre)
                    }
                } else if self.a.tag(f[2]) == Some("atom") {
                    let arity = self.a.integer(args.len());
                    let pair = self.a.tuple(vec![call[2], arity]);
                    let op_ann = self.annotation(call[1])?;
                    let op = self.variable(op_ann, pair);
                    let (es, pre) = self.safe_list(args)?;
                    let args = self.a.list(es);
                    (self.a.tagged("c_apply", vec![line_ann, op, args]), pre)
                } else {
                    let mut operator = self.expr(f[2])?;
                    if self.a.tag(operator.value) != Some("c_var") {
                        let op_ann = self.expression_annotation(operator.value)?;
                        let variable = self.fresh(op_ann);
                        operator.pre.push(Pre::Set(variable, operator.value));
                        operator.value = variable;
                    }
                    let (values, pre) = self.safe_list(args)?;
                    operator.pre.extend(pre);
                    let args = self.a.list(values);
                    (
                        self.a
                            .tagged("c_apply", vec![line_ann, operator.value, args]),
                        operator.pre,
                    )
                }
            }
            "op" => {
                let op = self.a.as_atom(f[2]).ok_or("lower: operator")?.to_owned();
                if op == "++" && self.a.tag(f[3]) == Some("lc") {
                    return self.comprehension_append(f[3], f[4]);
                }
                if matches!(op.as_str(), "andalso" | "orelse") {
                    let associated = self.right_assoc(node, &op)?;
                    let fields = self.a.tuple_fields(associated)?.to_vec();
                    let source = self.boolean_switch(&fields, op == "orelse")?;
                    return self.expr(source);
                }
                let (es, pre) = self.safe_list(f[3..].to_vec())?;
                let module = self.a.atom("erlang");
                let module = self.literal(ann, module);
                let name = self.literal(ann, f[2]);
                let args = self.a.list(es);
                (self.a.tagged("c_call", vec![ann, module, name, args]), pre)
            }
            "block" => {
                let mut bound = self.known_vars.clone();
                let (mut steps, _) = self.body_steps(f[2], &mut bound)?;
                let (last, _) = steps.pop().ok_or_else(|| self.err("empty block"))?;
                let mut pre = Vec::new();
                for (step, assigned) in steps {
                    pre.extend(step.pre);
                    if !assigned {
                        pre.push(Pre::Effect(step.value));
                    }
                }
                pre.extend(last.pre);
                (last.value, pre)
            }
            "if" => {
                let mut clauses = Vec::new();
                for clause in self.a.list_items(f[2])? {
                    clauses.push(self.clause(clause)?);
                }
                let reason = self.atom_literal("if_clause");
                let name = self.atom_literal("match_fail");
                let args = self.a.list(vec![reason]);
                let fail = self.a.tagged("c_primop", vec![ann, name, args]);
                let compiler = self.a.atom("compiler_generated");
                let generated = self.a.list(vec![compiler]);
                let pats = self.empty();
                let truth = self.atom_literal("true");
                clauses.push(
                    self.a
                        .tagged("c_clause", vec![generated, pats, truth, fail]),
                );
                let empty = self.empty();
                let values = self.empty();
                let arg = self.a.tagged("c_values", vec![empty, values]);
                let clauses = self.a.list(clauses);
                (self.a.tagged("c_case", vec![ann, arg, clauses]), vec![])
            }
            "case" => {
                let mut input = self.source_novars(f[2])?;
                // v3_core evaluates case inputs through novars before clauses.
                if !matches!(
                    self.a.tag(input.value),
                    Some(
                        "c_apply"
                            | "c_call"
                            | "c_fun"
                            | "c_binary"
                            | "c_map"
                            | "c_literal"
                            | "c_var"
                            | "c_cons"
                            | "c_tuple"
                    )
                ) {
                    let annotation = self.expression_annotation(input.value)?;
                    let temporary = self.fresh(annotation);
                    input.pre.push(Pre::Set(temporary, input.value));
                    input.value = temporary;
                }
                let mut cs = Vec::new();
                for c in self.a.list_items(f[3])? {
                    let old = self.case_patterns;
                    self.case_patterns = true;
                    let clause = self.clause(c);
                    self.case_patterns = old;
                    cs.push(clause?);
                }
                let empty = self.empty();
                let failvar = self.fresh(empty);
                cs.push(self.failure(vec![failvar], ann, "case_clause")?);
                let cs = self.a.list(cs);
                (
                    self.a.tagged("c_case", vec![ann, input.value, cs]),
                    input.pre,
                )
            }
            "map" if f.len() == 3 => {
                let empty = self.empty();
                let map = self.a.push(ErlangTerm::Map(vec![]));
                let base = self.literal(empty, map);
                let expression = self.build_map_pairs(f[2], base, ann)?;
                (expression.value, expression.pre)
            }
            "map" if f.len() == 4 => {
                let base = self.expr(f[2])?;
                let mut base = self.guard_safe(base)?;
                let base_is_map = self.a.tag(base.value) == Some("c_literal")
                    && matches!(
                        self.a.get(self.a.tuple_fields(base.value)?[2]),
                        ErlangTerm::Map(_)
                    );
                if self.a.tag(base.value) != Some("c_var") && !base_is_map {
                    let empty = self.empty();
                    let variable = self.fresh(empty);
                    let annotation = self.expression_annotation(base.value)?;
                    self.let_annotations.insert(variable, annotation);
                    base.pre.push(Pre::Set(variable, base.value));
                    base.value = variable;
                }
                let expression = self.build_map_pairs(f[3], base.value, ann)?;
                base.pre.extend(expression.pre);
                let body = if self.a.list_items(f[3])?.is_empty() {
                    base.value
                } else {
                    expression.value
                };
                let guard = self.guard_call(line_ann, "is_map", vec![base.value]);
                let generated = self.a.atom("compiler_generated");
                let mut generated_ann = vec![generated];
                generated_ann.extend(self.a.list_items(line_ann)?);
                let generated_ann = self.a.list(generated_ann);
                let empty = self.empty();
                let clause = self
                    .a
                    .tagged("c_clause", vec![generated_ann, empty, guard, body]);
                let badmap = self.atom_literal("badmap");
                let tuple_ann = self.empty();
                let reason_items = self.a.list(vec![badmap, base.value]);
                let reason = if self.in_guard {
                    badmap
                } else if self.a.tag(base.value) == Some("c_literal") {
                    let badmap_value = self.a.tuple_fields(badmap)?[2];
                    let base_value = self.a.tuple_fields(base.value)?[2];
                    let tuple = self.a.tuple(vec![badmap_value, base_value]);
                    self.literal(tuple_ann, tuple)
                } else {
                    self.a.tagged("c_tuple", vec![tuple_ann, reason_items])
                };
                let eval_failure = self.a.atom("eval_failure");
                let badmap_atom = self.a.atom("badmap");
                let eval_failure = self.a.tuple(vec![eval_failure, badmap_atom]);
                let mut fail_ann = vec![eval_failure];
                fail_ann.extend(self.a.list_items(line_ann)?);
                let fail_ann = self.a.list(fail_ann);
                let name = self.atom_literal("match_fail");
                let args = self.a.list(vec![reason]);
                let fail = self.a.tagged("c_primop", vec![fail_ann, name, args]);
                let generated = self.a.atom("compiler_generated");
                let generated = self.a.list(vec![generated]);
                let empty = self.empty();
                let truth = self.atom_literal("true");
                let fail = self
                    .a
                    .tagged("c_clause", vec![generated, empty, truth, fail]);
                let empty = self.empty();
                let values = self.empty();
                let argument = self.a.tagged("c_values", vec![empty, values]);
                let clauses = self.a.list(vec![clause, fail]);
                (
                    self.a.tagged("c_case", vec![line_ann, argument, clauses]),
                    base.pre,
                )
            }
            "fun" => {
                let fun = self.a.tuple_fields(f[2])?.to_vec();
                match self.a.as_atom(fun[0]) {
                    Some("function") if fun.len() == 3 => {
                        let name = self.a.tuple(vec![fun[1], fun[2]]);
                        (self.variable(ann, name), vec![])
                    }
                    Some("function") if fun.len() == 4 => {
                        let (args, pre) = self.safe_list(fun[1..].to_vec())?;
                        let module = self.atom_literal("erlang");
                        let name = self.atom_literal("make_fun");
                        let args = self.a.list(args);
                        (self.a.tagged("c_call", vec![ann, module, name, args]), pre)
                    }
                    Some("clauses") if fun.len() == 2 => {
                        let source = self.a.list_items(fun[1])?;
                        let first = *source.first().ok_or("lower: empty anonymous fun")?;
                        let arity = self.a.list_items(self.a.tuple_fields(first)?[2])?.len();
                        let mut clauses = Vec::new();
                        for c in source {
                            clauses.push(self.clause(c)?);
                        }
                        let mut params = Vec::new();
                        for _ in 0..arity {
                            let empty = self.empty();
                            params.push(self.fresh(empty));
                        }
                        params.reverse();
                        let mut failed = Vec::new();
                        for _ in 0..arity {
                            let empty = self.empty();
                            failed.push(self.fresh(empty));
                        }
                        failed.reverse();
                        clauses.push(self.failure(failed, ann, "function_clause")?);
                        let generated_name =
                            format!("-{}-fun-{}-", self.function, self.fun_counter);
                        self.fun_counter += 1;
                        let zero = self.a.integer(0);
                        let name = self.a.atom(generated_name);
                        let id = self.a.tuple(vec![zero, zero, name]);
                        let id_key = self.a.atom("id");
                        let marker = self.a.tuple(vec![id_key, id]);
                        let mut annotations = vec![marker];
                        annotations.extend(self.a.list_items(ann)?);
                        let fun_ann = self.a.list(annotations);
                        let argument = if params.len() == 1 {
                            let mut variable = self.a.tuple_fields(params[0])?.to_vec();
                            variable[1] = ann;
                            self.a.tuple(variable)
                        } else {
                            let values = self.a.list(params.clone());
                            self.a.tagged("c_values", vec![ann, values])
                        };
                        let clauses = self.a.list(clauses);
                        let case = self.a.tagged("c_case", vec![ann, argument, clauses]);
                        let params = self.a.list(params);
                        let fun = self.a.tagged("c_fun", vec![fun_ann, params, case]);
                        self.source_annotations.insert(fun, ann);
                        (self.register_fun_shadowing(fun), vec![])
                    }
                    _ => return Err(self.err("fun descriptor")),
                }
            }
            "receive" => {
                let e = self.receive_expr(&f, line_ann)?;
                (e.value, e.pre)
            }
            "lc" | "bc" | "mc" => return self.comprehension(node),
            "try" => {
                let e = self.try_expr(&f, line_ann)?;
                (e.value, e.pre)
            }
            "catch" => {
                let e = self.catch_expr(&f)?;
                (e.value, e.pre)
            }
            "bin" => {
                let binary = self.binary_expr(&f, ann)?;
                (binary.value, binary.pre)
            }
            _ => return Err(self.err(&tag)),
        };
        Ok(Expr { value, pre })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_map_identity_keeps_numeric_and_signed_zero_keys_distinct() {
        let mut a = Arena::default();
        let line = a.integer(1);
        let value = a.atom("v");
        let value = a.tagged("atom", vec![line, value]);
        let mut pairs = Vec::new();
        for literal in [
            ErlangTerm::Integer("1".into()),
            ErlangTerm::Float(1.0f64.to_bits()),
            ErlangTerm::Float(0.0f64.to_bits()),
            ErlangTerm::Float((-0.0f64).to_bits()),
            ErlangTerm::Integer("1".into()),
        ] {
            let tag = if matches!(literal, ErlangTerm::Integer(_)) {
                "integer"
            } else {
                "float"
            };
            let raw = a.push(literal);
            let key = a.tagged(tag, vec![line, raw]);
            pairs.push(a.tagged("map_field_assoc", vec![line, key, value]));
        }
        let pairs = a.list(pairs);
        let map = a.tagged("map", vec![line, pairs]);
        let body = a.list(vec![map]);
        let empty = a.nil();
        let clause = a.tagged("clause", vec![line, empty, empty, body]);
        let clauses = a.list(vec![clause]);
        let name = a.atom("f");
        let arity = a.integer(0);
        let function = a.tagged("function", vec![line, name, arity, clauses]);
        let root = a.list(vec![function]);
        let core = lower(&a.finish(root), "m").unwrap();
        let a = Arena::from_terms(&core);
        let defs = a.list_items(a.tuple_fields(core.root).unwrap()[5]).unwrap();
        let fun = a.tuple_fields(defs[0]).unwrap()[1];
        let case = a.tuple_fields(fun).unwrap()[3];
        let clause = a.list_items(a.tuple_fields(case).unwrap()[3]).unwrap()[0];
        let literal = a.tuple_fields(clause).unwrap()[4];
        assert_eq!(a.tag(literal), Some("c_literal"));
        let raw = a.tuple_fields(literal).unwrap()[2];
        let ErlangTerm::Map(pairs) = a.get(raw) else {
            panic!("expected literal map")
        };
        assert_eq!(pairs.len(), 4);
    }
    #[test]
    fn unsupported_function_is_an_error_instead_of_an_omission() {
        let mut a = Arena::default();
        let line = a.integer(1);
        let name = a.atom("f");
        let zero = a.integer(0);
        let args = a.nil();
        let guards = a.nil();
        let bad = a.tagged("unsupported_future_construct", vec![line, args]);
        let body = a.list(vec![bad]);
        let clause = a.tagged("clause", vec![line, args, guards, body]);
        let clauses = a.list(vec![clause]);
        let fun = a.tagged("function", vec![line, name, zero, clauses]);
        let root = a.list(vec![fun]);
        let error = lower(&a.finish(root), "m").unwrap_err();
        assert!(error.contains("m:f/0"));
        assert!(error.contains("unsupported_future_construct"));
    }
    fn assignment_fixture(name: &str) -> ErlangTerms {
        let mut a = Arena::default();
        let line = a.integer(1);
        let variable_name = a.atom(name);
        let variable = a.tagged("var", vec![line, variable_name]);
        let erlang = a.atom("erlang");
        let erlang = a.tagged("atom", vec![line, erlang]);
        let self_name = a.atom("self");
        let self_name = a.tagged("atom", vec![line, self_name]);
        let remote = a.tagged("remote", vec![line, erlang, self_name]);
        let args = a.nil();
        let call = a.tagged("call", vec![line, remote, args]);
        let assignment = a.tagged("match", vec![line, variable, call]);
        let done = a.atom("done");
        let done = a.tagged("atom", vec![line, done]);
        let body = a.list(vec![assignment, done]);
        let guards = a.nil();
        let clause = a.tagged("clause", vec![line, args, guards, body]);
        let clauses = a.list(vec![clause]);
        let fun_name = a.atom("f");
        let zero = a.integer(0);
        let function = a.tagged("function", vec![line, fun_name, zero, clauses]);
        let forms = a.list(vec![function]);
        a.finish(forms)
    }
    #[test]
    fn fresh_assignment_does_not_emit_a_sequence_of_its_variable() {
        let core = lower(&assignment_fixture("X"), "m").unwrap();
        let a = Arena::from_terms(&core);
        assert!(
            !(0..a.terms.len())
                .map(TermId)
                .any(|id| a.tag(id) == Some("c_seq"))
        );
    }
    #[test]
    fn underscore_assignment_suppresses_only_its_rhs_annotations() {
        let core = lower(&assignment_fixture("_ignored"), "m").unwrap();
        let a = Arena::from_terms(&core);
        let call = (0..a.terms.len())
            .map(TermId)
            .find(|id| a.tag(*id) == Some("c_call"))
            .unwrap();
        let call_ann = a.tuple_fields(call).unwrap()[1];
        let first_ann = a.list_items(call_ann).unwrap()[0];
        assert_eq!(a.as_atom(first_ann), Some("result_not_wanted"));
        let done_literal = (0..a.terms.len())
            .map(TermId)
            .find(|id| {
                a.tag(*id) == Some("c_literal")
                    && a.as_atom(a.tuple_fields(*id).unwrap()[2]) == Some("done")
            })
            .unwrap();
        let done_ann = a.tuple_fields(done_literal).unwrap()[1];
        assert!(
            a.list_items(done_ann)
                .unwrap()
                .iter()
                .all(|id| a.as_atom(*id) != Some("result_not_wanted"))
        );
    }
    #[test]
    fn case_pin_is_an_equality_test_with_delayed_fresh_variable() {
        let mut a = Arena::default();
        let line = a.integer(1);
        let x = a.atom("X");
        let x = a.tagged("var", vec![line, x]);
        let truth = a.atom("true");
        let truth = a.tagged("atom", vec![line, truth]);
        let body = a.list(vec![truth]);
        let patterns = a.list(vec![x]);
        let guards = a.nil();
        let clause = a.tagged("clause", vec![line, patterns, guards, body]);
        let clauses = a.list(vec![clause]);
        let case = a.tagged("case", vec![line, x, clauses]);
        let body = a.list(vec![case]);
        let clause = a.tagged("clause", vec![line, patterns, guards, body]);
        let clauses = a.list(vec![clause]);
        let name = a.atom("f");
        let arity = a.integer(1);
        let function = a.tagged("function", vec![line, name, arity, clauses]);
        let forms = a.list(vec![function]);
        let core = lower(&a.finish(forms), "m").unwrap();
        let a = Arena::from_terms(&core);
        let defs = a.list_items(a.tuple_fields(core.root).unwrap()[5]).unwrap();
        let fun = a.tuple_fields(defs[0]).unwrap()[1];
        let top_case = a.tuple_fields(fun).unwrap()[3];
        let top_clause = a.list_items(a.tuple_fields(top_case).unwrap()[3]).unwrap()[0];
        let inner_case = a.tuple_fields(top_clause).unwrap()[4];
        let inner_clause = a
            .list_items(a.tuple_fields(inner_case).unwrap()[3])
            .unwrap()[0];
        let pin = a
            .list_items(a.tuple_fields(inner_clause).unwrap()[2])
            .unwrap()[0];
        let pf = a.tuple_fields(pin).unwrap();
        assert_eq!(a.as_integer(pf[2]), Some(3));
        assert!(a.list_items(pf[1]).unwrap().is_empty());
        let guard = a.tuple_fields(inner_clause).unwrap()[3];
        let gf = a.tuple_fields(guard).unwrap();
        assert_eq!(a.tag(guard), Some("c_call"));
        assert_eq!(a.as_atom(a.tuple_fields(gf[3]).unwrap()[2]), Some("=:="));
        let reference = a.list_items(gf[4]).unwrap()[0];
        assert_eq!(a.tag(reference), Some("c_var"));
        let rf = a.tuple_fields(reference).unwrap();
        assert_eq!(a.as_integer(rf[2]), a.as_integer(pf[2]));
        assert!(a.list_items(rf[1]).unwrap().is_empty());
    }
}

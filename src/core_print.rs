use std::collections::HashMap;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrintError {
    Boundary(String),
    ResourceLimit,
    Unsupported(String),
}
pub struct CorePrinter<'a, 'b, 'c> {
    pub patterns: &'a crate::patterns::Patterns<'b, 'c>,
}
impl CorePrinter<'_, '_, '_> {
    fn planner(&self) -> Planner<'_, '_, '_> {
        Planner {
            patterns: self.patterns,
            cache: std::cell::RefCell::new(HashMap::new()),
            reads: std::cell::RefCell::new(Vec::new()),
        }
    }
    pub fn format_patterns(
        &self,
        pats: &[crate::patterns::PatternRef],
    ) -> Result<String, PrintError> {
        self.planner().format_patterns(pats).map_err(public_error)
    }
    pub fn format_cerl(&self, node: crate::program::NodeRef) -> Result<String, PrintError> {
        self.planner().format_cerl(node).map_err(public_error)
    }
}
fn public_error(e: PlanError) -> PrintError {
    match e {
        PlanError::Boundary(s) => PrintError::Boundary(s),
        PlanError::ResourceLimit => PrintError::ResourceLimit,
        PlanError::Unsupported(s) => PrintError::Unsupported(s),
        PlanError::Need(_) => PrintError::Boundary("unresolved Core document dependency".into()),
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct DocKey {
    pat: crate::patterns::PatternRef,
    strip: bool,
    root: bool,
    line: i64,
    hook: bool,
}
use crate::{
    core::{CoreExpr, ErlangTerm, TermId},
    patterns::{LiteralRef, PairOperation, PatternRef, Patterns, Shape},
    program::NodeRef,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum PlanError {
    Boundary(String),
    ResourceLimit,
    Unsupported(String),
    Need(DocKey),
}
impl From<String> for PlanError {
    fn from(s: String) -> Self {
        Self::Boundary(s)
    }
}
const MAX_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;
#[derive(Clone, Debug)]
struct Doc(Vec<String>, usize, usize);
impl Doc {
    fn limited() -> Self {
        Self(vec![String::new()], 0, MAX_DOCUMENT_BYTES + 1)
    }
    fn from_lines(lines: Vec<String>, indent: usize) -> Self {
        let size = lines
            .iter()
            .fold(lines.len().saturating_sub(1), |sum, line| {
                sum.saturating_add(line.len())
            });
        if size > MAX_DOCUMENT_BYTES {
            Self::limited()
        } else {
            Self(lines, indent, size)
        }
    }
    fn text(s: impl Into<String>) -> Self {
        let text = s.into();
        if text.len() > MAX_DOCUMENT_BYTES {
            Self::limited()
        } else {
            let size = text.len();
            Self(vec![text], 0, size)
        }
    }
    fn string(text: String) -> Self {
        if text.len() > MAX_DOCUMENT_BYTES {
            return Self::limited();
        }
        let mut lines = Vec::new();
        let mut chars = text.chars().collect::<Vec<_>>();
        let width = 66666isize;
        let mut length = chars.len();
        loop {
            if length <= width as usize {
                lines.push(chars.iter().collect());
                break;
            }
            let mut i = 0usize;
            let mut remaining = length as isize;
            let mut target = width - 1;
            let mut split = None;
            while i < chars.len() {
                let c = chars[i];
                if matches!(c, ' ' | '\t' | '\n') && target <= 0 && remaining >= 5 {
                    split = Some(i + 1);
                    break;
                }
                if c == '\\' {
                    i += 1;
                    target -= 1;
                    remaining -= 1;
                    if i >= chars.len() {
                        break;
                    }
                    let count = if chars[i] == '^' && i + 1 < chars.len() {
                        2
                    } else if chars[i..].iter().take(3).count() == 3
                        && chars[i..i + 3].iter().all(|c| ('0'..='7').contains(c))
                    {
                        3
                    } else if i + 1 < chars.len()
                        && chars[i..i + 2].iter().all(|c| ('0'..='7').contains(c))
                    {
                        2
                    } else {
                        1
                    };
                    i += count;
                    target -= count as isize;
                    remaining -= count as isize;
                    continue;
                }
                if target <= -10 && remaining >= 5 {
                    split = Some(i);
                    break;
                }
                i += 1;
                target -= 1;
                remaining -= 1
            }
            match split {
                None => {
                    lines.push(chars.iter().collect());
                    break;
                }
                Some(i) => {
                    let mut line = chars[..i].iter().collect::<String>();
                    line.push('"');
                    lines.push(line);
                    let mut next = vec!['"'];
                    next.extend_from_slice(&chars[i..]);
                    chars = next;
                    length = length.saturating_sub(width as usize - 1);
                }
            }
        }
        Self::from_lines(lines, 0)
    }
    fn beside(self, rhs: Self) -> Self {
        if self.2 > MAX_DOCUMENT_BYTES || rhs.2 > MAX_DOCUMENT_BYTES {
            return Self::limited();
        }
        let width = self.0.last().map_or(0, |s| s.chars().count());
        let projected = self
            .2
            .saturating_add(rhs.2)
            .saturating_add(width.saturating_mul(rhs.0.len().saturating_sub(1)))
            .saturating_sub(rhs.1);
        if projected > MAX_DOCUMENT_BYTES {
            return Self::limited();
        }
        if self.0.is_empty() {
            return rhs;
        }
        if rhs.0.is_empty() {
            return self;
        }
        let mut a = self.0;
        let width = a.last().map_or(0, |s| s.chars().count());
        let mut b = rhs.0.into_iter();
        let first = b.next().unwrap_or_default();
        a.last_mut().unwrap().push_str(&first[rhs.1..]);
        a.extend(b.map(|s| format!("{}{}", " ".repeat(width), s)));
        Self::from_lines(a, self.1)
    }
    fn follow(self, rhs: Self) -> Self {
        self.follow_at(rhs, 0)
    }
    fn follow_at(self, rhs: Self, n: usize) -> Self {
        if self.0.len() == 1 {
            self.beside(Self::text(" ")).beside(rhs)
        } else {
            self.above(Self::text("").nest(n)).beside(rhs)
        }
    }
    fn nest(self, n: usize) -> Self {
        let projected = self.2.saturating_add(n.saturating_mul(self.0.len()));
        if projected > MAX_DOCUMENT_BYTES {
            return Self::limited();
        }
        Self(
            self.0
                .into_iter()
                .map(|s| format!("{}{}", " ".repeat(n), s))
                .collect(),
            self.1.saturating_add(n),
            projected,
        )
    }
    fn above(self, rhs: Self) -> Self {
        if self.2 > MAX_DOCUMENT_BYTES || rhs.2 > MAX_DOCUMENT_BYTES {
            return Self::limited();
        }
        if self.0.is_empty() {
            return rhs;
        }
        if rhs.0.is_empty() {
            return self;
        }
        if self.2.saturating_add(rhs.2).saturating_add(1) > MAX_DOCUMENT_BYTES {
            return Self::limited();
        }
        let mut a = self.0;
        a.extend(rhs.0);
        Self::from_lines(a, self.1)
    }
    fn sep(xs: Vec<Self>, indent: usize) -> Self {
        if xs.iter().any(|d| d.2 > MAX_DOCUMENT_BYTES) {
            return Self::limited();
        }
        if xs.is_empty() {
            return Self::text("");
        }
        let horizontal = xs.iter().all(|d| d.0.len() <= 1);
        let xs = if horizontal {
            xs.into_iter()
                .map(|d| if d.0.is_empty() { Self::text("") } else { d })
                .collect::<Vec<_>>()
        } else {
            xs.into_iter()
                .filter(|d| !d.0.is_empty())
                .collect::<Vec<_>>()
        };
        let mut xs = xs.into_iter().map(|mut d| {
            if horizontal {
                d.0[0].drain(..d.1);
                d.2 = d.2.saturating_sub(d.1);
                d.1 = 0
            }
            d
        });
        let mut out = xs.next().unwrap_or_else(|| Self(Vec::new(), 0, 0));
        for d in xs {
            out = if horizontal {
                out.beside(Self::text(" ")).beside(d)
            } else {
                out.above(d.nest(indent))
            }
        }
        out
    }

    fn par(xs: Vec<Self>, indent: usize) -> Self {
        if xs.iter().any(|d| d.2 > MAX_DOCUMENT_BYTES) {
            return Self::limited();
        }
        let xs = xs
            .into_iter()
            .filter(|d| !d.0.is_empty())
            .collect::<Vec<_>>();
        if xs.is_empty() {
            return Self::text("");
        }
        let mut blocks = Vec::new();
        let mut run: Option<Self> = None;
        for mut d in xs {
            if d.0.len() == 1 {
                d.0[0].drain(..d.1);
                d.2 = d.2.saturating_sub(d.1);
                d.1 = 0;
                run = Some(match run {
                    None => d,
                    Some(r) => r.beside(Self::text(" ")).beside(d),
                })
            } else {
                if let Some(r) = run.take() {
                    blocks.push(r)
                }
                blocks.push(d)
            }
        }
        if let Some(r) = run {
            blocks.push(r)
        }
        let mut xs = blocks.into_iter();
        let mut out = xs.next().unwrap();
        for d in xs {
            out = out.above(d.nest(indent))
        }
        out
    }

    fn comma(xs: Vec<Self>) -> Self {
        let count = xs.len();
        Self::par(
            xs.into_iter()
                .enumerate()
                .map(|(i, d)| {
                    if i + 1 < count {
                        d.beside(Self::text(","))
                    } else {
                        d
                    }
                })
                .collect(),
            0,
        )
    }
    fn surround(self, left: &str, right: &str) -> Self {
        Self::text(left).beside(self).beside(Self::text(right))
    }
    fn finish(self) -> Result<String, PlanError> {
        if self.2 > MAX_DOCUMENT_BYTES {
            return Err(PlanError::ResourceLimit);
        }
        if self.0.iter().any(|line| line.chars().count() > 100000) {
            return Err(PlanError::Unsupported(
                "prettypr width wrapping beyond 100000 columns".into(),
            ));
        }
        Ok(self.0.join("\n"))
    }
}
struct Planner<'a, 'b, 'c> {
    patterns: &'a Patterns<'b, 'c>,
    cache: std::cell::RefCell<std::collections::HashMap<DocKey, Doc>>,
    reads: std::cell::RefCell<Vec<DocKey>>,
}
impl Planner<'_, '_, '_> {
    fn doc(
        &self,
        pat: PatternRef,
        strip: bool,
        root: bool,
        line: i64,
        _depth: usize,
        hook: bool,
    ) -> Result<Doc, PlanError> {
        let key = DocKey {
            pat,
            strip,
            root,
            line,
            hook,
        };
        if let Some(doc) = self.cache.borrow().get(&key) {
            self.reads.borrow_mut().push(key);
            Ok(doc.clone())
        } else {
            Err(PlanError::Need(key))
        }
    }
    fn run(&self, root: impl Fn() -> Result<Doc, PlanError>) -> Result<Doc, PlanError> {
        let mut pending = Vec::<DocKey>::new();
        let mut evaluations = 0usize;
        loop {
            self.reads.borrow_mut().clear();
            let result = match pending.last() {
                Some(key) => self.doc_body(key.pat, key.strip, key.root, key.line, 0, key.hook),
                None => root(),
            };
            evaluations += 1;
            if evaluations > 5_000_000 {
                return Err(PlanError::ResourceLimit);
            }
            match result {
                Ok(doc) => {
                    if doc.2 > MAX_DOCUMENT_BYTES {
                        return Err(PlanError::ResourceLimit);
                    }
                    let reads = std::mem::take(&mut *self.reads.borrow_mut());
                    let mut cache = self.cache.borrow_mut();
                    for key in reads {
                        cache.remove(&key);
                    }
                    match pending.pop() {
                        Some(key) => {
                            cache.insert(key, doc);
                        }
                        None => return Ok(doc),
                    }
                }
                Err(PlanError::Need(key)) => {
                    if pending.last() == Some(&key) {
                        return Err(PlanError::Boundary(
                            "cyclic Core document dependency".into(),
                        ));
                    }
                    pending.push(key)
                }
                Err(e) => return Err(e),
            }
        }
    }

    fn format_patterns(&self, pats: &[PatternRef]) -> Result<String, PlanError> {
        let variable = pats.len() == 1 && self.variable_prefix(pats[0])?;
        let doc = self.run(|| self.values(pats, true, 0, 0))?;
        Ok(format!(
            "{}{}",
            if variable { "variable " } else { "pattern " },
            doc.finish()?
        ))
    }
    fn format_cerl(&self, node: NodeRef) -> Result<String, PlanError> {
        self.run(|| self.doc(PatternRef::Core(node), false, true, 0, 0, true))?
            .finish()
    }
    fn variable_prefix(&self, pat: PatternRef) -> Result<bool, PlanError> {
        if let PatternRef::Core(node) = pat
            && let CoreExpr::Var(term) = self.patterns.nodes.expression(node)?
        {
            return Ok(
                !matches!(self.patterns.term(node.module,*term)?,ErlangTerm::Atom(s)if s.is_empty()),
            );
        }
        Ok(false)
    }
    fn var(&self, node: NodeRef, strip: bool) -> Result<Doc, PlanError> {
        let CoreExpr::Var(term) = self.patterns.nodes.expression(node)? else {
            return Err(PlanError::Boundary("expected var".into()));
        };
        Ok(Doc::text(match self.patterns.term(node.module, *term)? {
            ErlangTerm::Atom(s) => {
                let s = if strip
                    && (s.starts_with('@') || s.starts_with("cor") || s.starts_with("rec"))
                {
                    ""
                } else {
                    s
                };
                let first = s.chars().next();
                if first.is_some_and(|c| {
                    c.is_ascii_uppercase() || ('À'..='Þ').contains(&c) && c != '×' || c == '_'
                }) {
                    s.to_owned()
                } else {
                    format!("_{s}")
                }
            }
            ErlangTerm::Integer(n) => {
                if strip {
                    "_".into()
                } else {
                    format!("_{n}")
                }
            }
            ErlangTerm::Tuple(xs) if xs.len() == 2 => {
                let ErlangTerm::Atom(name) = self.patterns.term(node.module, xs[0])? else {
                    return Err(PlanError::Boundary("function variable name".into()));
                };
                let ErlangTerm::Integer(arity) = self.patterns.term(node.module, xs[1])? else {
                    return Err(PlanError::Boundary("function variable arity".into()));
                };
                if strip {
                    "_".into()
                } else {
                    format!("{}/{arity}", quoted(name, '\''))
                }
            }
            _ => return Err(PlanError::Unsupported("invalid Core variable name".into())),
        }))
    }
    fn line(&self, pat: PatternRef) -> Result<Option<i64>, PlanError> {
        let Some(node) = self.patterns.annotation(pat) else {
            return Ok(None);
        };
        let ann = self.patterns.nodes.module(node.module)?.nodes[node.node].annotations;
        let ErlangTerm::List { items, .. } = self.patterns.term(node.module, ann)? else {
            return Ok(None);
        };
        for &id in items {
            match self.patterns.term(node.module, id)? {
                ErlangTerm::Integer(n) => {
                    return Ok(Some(n.parse().map_err(|_| {
                        PlanError::Boundary("annotation line outside i64".into())
                    })?));
                }
                ErlangTerm::Tuple(xs) if xs.len() == 2 => {
                    if let ErlangTerm::Integer(n) = self.patterns.term(node.module, xs[0])? {
                        return Ok(Some(n.parse().map_err(|_| {
                            PlanError::Boundary("annotation line outside i64".into())
                        })?));
                    }
                }
                _ => {}
            }
        }
        Ok(None)
    }
    fn values(
        &self,
        xs: &[PatternRef],
        strip: bool,
        line: i64,
        depth: usize,
    ) -> Result<Doc, PlanError> {
        let ds = xs
            .iter()
            .map(|&p| self.doc(p, strip, false, line, depth + 1, true))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(if ds.len() == 1 {
            ds.into_iter().next().unwrap()
        } else {
            Doc::comma(ds).surround("<", ">")
        })
    }
    fn doc_body(
        &self,
        pat: PatternRef,
        strip: bool,
        root: bool,
        mut line: i64,
        depth: usize,
        hook: bool,
    ) -> Result<Doc, PlanError> {
        let ann = if !strip && !root {
            self.line(pat)?.filter(|&n| n > line)
        } else {
            None
        };
        if let Some(n) = ann {
            line = n
        }
        let p = |id: crate::core::NodeId, module| PatternRef::Core(NodeRef { module, node: id.0 });
        let child = |x| self.doc(x, strip, false, line, depth + 1, true);
        let cont = |x| self.doc(x, strip, true, line, depth + 1, false);
        let out = match self.patterns.shape(pat)? {
            Shape::Literal(lit) => self.literal(lit, depth + 1, hook)?,
            Shape::Variable { original, .. } => self.var(original, strip)?,
            Shape::Alias { variable, pattern } => child(variable)?
                .beside(Doc::text(" ="))
                .follow_at(child(pattern)?, 4),
            Shape::Values(xs) => self.values(&xs, strip, line, depth + 1)?,
            Shape::Tuple(xs) => {
                Doc::comma(xs.into_iter().map(child).collect::<Result<Vec<_>, _>>()?)
                    .surround("{", "}")
            }
            Shape::Cons { head, tail } => {
                if strip && let Some(chars) = self.folded_chars(pat)? {
                    return Ok(Doc::string(quoted(&chars, '"')));
                }
                let mut elements = vec![child(head)?];
                let mut rest = tail;
                loop {
                    match self.patterns.shape(rest)? {
                        Shape::Cons { head, tail } => {
                            elements.last_mut().unwrap().0.last_mut().unwrap().push(',');
                            elements.push(child(head)?);
                            rest = tail
                        }
                        Shape::Literal(lit) => {
                            let (module, term, index) = literal_parts(lit);
                            match self.patterns.term(module, term)? {
                                ErlangTerm::Nil => break,
                                ErlangTerm::List { items, tail } => {
                                    let xs = items
                                        .get(index..)
                                        .ok_or_else(|| PlanError::Boundary("list suffix".into()))?;
                                    for &id in xs {
                                        elements
                                            .last_mut()
                                            .unwrap()
                                            .0
                                            .last_mut()
                                            .unwrap()
                                            .push(',');
                                        elements.push(self.literal(
                                            LiteralRef::Term { module, term: id },
                                            depth + 1,
                                            true,
                                        )?)
                                    }
                                    if !matches!(
                                        self.patterns.term(module, *tail)?,
                                        ErlangTerm::Nil
                                    ) {
                                        elements.push(Doc::text("| ").beside(self.literal(
                                            LiteralRef::Term {
                                                module,
                                                term: *tail,
                                            },
                                            depth + 1,
                                            true,
                                        )?))
                                    }
                                    break;
                                }
                                _ => {
                                    elements.push(Doc::text("| ").beside(child(rest)?));
                                    break;
                                }
                            }
                        }
                        _ => {
                            elements.push(Doc::text("| ").beside(child(rest)?));
                            break;
                        }
                    }
                }
                Doc::par(elements, 0).surround("[", "]")
            }
            Shape::Map(pairs) => {
                let body = Doc::comma(pairs.into_iter().map(cont).collect::<Result<Vec<_>, _>>()?);
                let base = if let PatternRef::Core(node) = pat
                    && let CoreExpr::Map { base: argument, .. } =
                        self.patterns.nodes.expression(node)?
                {
                    Some(p(*argument, node.module))
                } else {
                    None
                };
                let prefix = if let Some(base) = base
                    && !self.empty_map(base)?
                {
                    if hook {
                        self.doc(base, false, true, line, depth + 1, false)?
                            .beside(Doc::text("#{"))
                    } else {
                        Doc::text("~{")
                    }
                } else {
                    Doc::text(if hook { "#{" } else { "~{" })
                };
                let suffix = if !hook {
                    if let Some(base) = base
                        && !self.empty_map(base)?
                    {
                        Doc::text(" | ")
                            .beside(self.doc(base, false, false, line, depth + 1, true)?)
                            .beside(Doc::text("}~"))
                    } else {
                        Doc::text("}~")
                    }
                } else {
                    Doc::text("}")
                };
                prefix.beside(body).beside(suffix)
            }
            Shape::Pair {
                operation,
                key,
                value,
            } => {
                let op = match operation {
                    PairOperation::Exact => ":=",
                    PairOperation::Node(n) => match self.atom(n)?.as_str() {
                        "assoc" => "=>",
                        "exact" => ":=",
                        _ => return Err(PlanError::Unsupported("map operation".into())),
                    },
                };
                child(key)?.beside(Doc::text(op)).beside(child(value)?)
            }
            Shape::Binary(xs) => {
                if hook {
                    let ds = xs
                        .into_iter()
                        .map(|x| self.segment(x, strip, line, depth + 1))
                        .collect::<Result<Vec<_>, _>>()?;
                    let mut out = Doc::text("");
                    for (i, d) in ds.into_iter().enumerate() {
                        if i != 0 {
                            out = out.beside(Doc::text(","))
                        }
                        out = out.beside(d)
                    }
                    out.surround("<<", ">>")
                } else {
                    Doc::comma(
                        xs.into_iter()
                            .map(|x| self.doc(x, strip, false, line, depth + 1, false))
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                    .surround("#{", "}#")
                }
            }
            Shape::Segment {
                value,
                size,
                unit,
                typ,
                flags,
            } => {
                if hook {
                    self.segment(pat, strip, line, depth + 1)?
                } else {
                    child(value)?.surround("#<", ">").beside(
                        Doc::comma(
                            [size, unit, typ, flags]
                                .into_iter()
                                .map(child)
                                .collect::<Result<Vec<_>, _>>()?,
                        )
                        .surround("(", ")"),
                    )
                }
            }
            Shape::Other => {
                let PatternRef::Core(node) = pat else {
                    return Err(PlanError::Unsupported("synthetic expression".into()));
                };
                match self.patterns.nodes.expression(node)? {
                    CoreExpr::Let {
                        vars,
                        argument,
                        body,
                    } => {
                        let vs = self.values(
                            &vars
                                .iter()
                                .map(|&id| p(id, node.module))
                                .collect::<Vec<_>>(),
                            strip,
                            line,
                            depth + 1,
                        )?;
                        let arg = child(p(*argument, node.module))?;
                        let head = Doc::par(
                            vec![
                                Doc::text("let").follow_at(vs.beside(Doc::text(" =")), 2),
                                arg,
                            ],
                            4,
                        );
                        let b = child(p(*body, node.module))?;
                        if matches!(
                            self.patterns.nodes.expression(NodeRef {
                                module: node.module,
                                node: body.0
                            })?,
                            CoreExpr::Let { .. }
                        ) {
                            Doc::sep(vec![head.beside(Doc::text(" in")), b], 0)
                        } else {
                            Doc::sep(vec![head, Doc::text("in ").beside(b)], 0)
                        }
                    }
                    CoreExpr::Seq { argument, body } => {
                        let b = child(p(*body, node.module))?;
                        Doc::sep(
                            vec![
                                Doc::text("do ").beside(child(p(*argument, node.module))?),
                                if matches!(
                                    self.patterns.nodes.expression(NodeRef {
                                        module: node.module,
                                        node: body.0
                                    })?,
                                    CoreExpr::Seq { .. }
                                ) {
                                    b
                                } else {
                                    b.nest(3)
                                },
                            ],
                            0,
                        )
                    }
                    CoreExpr::Apply { operator, args } => Doc::text("apply")
                        .follow(child(p(*operator, node.module))?)
                        .beside(
                            Doc::comma(
                                args.iter()
                                    .map(|&id| child(p(id, node.module)))
                                    .collect::<Result<Vec<_>, _>>()?,
                            )
                            .surround("(", ")"),
                        ),
                    CoreExpr::Call { module, name, args } => Doc::text("call")
                        .follow_at(
                            child(p(*module, node.module))?
                                .beside(Doc::text(":"))
                                .beside(child(p(*name, node.module))?),
                            2,
                        )
                        .beside(
                            Doc::comma(
                                args.iter()
                                    .map(|&id| child(p(id, node.module)))
                                    .collect::<Result<Vec<_>, _>>()?,
                            )
                            .surround("(", ")"),
                        ),
                    CoreExpr::Primop { name, args } => Doc::text("primop")
                        .follow_at(child(p(*name, node.module))?, 2)
                        .beside(
                            Doc::comma(
                                args.iter()
                                    .map(|&id| child(p(id, node.module)))
                                    .collect::<Result<Vec<_>, _>>()?,
                            )
                            .surround("(", ")"),
                        ),
                    CoreExpr::Case { argument, clauses } => {
                        let cs = clauses
                            .iter()
                            .map(|&id| child(p(id, node.module)))
                            .collect::<Result<Vec<_>, _>>()?;
                        Doc::sep(
                            vec![
                                Doc::par(
                                    vec![
                                        Doc::text("case")
                                            .follow_at(child(p(*argument, node.module))?, 2),
                                        Doc::text("of"),
                                    ],
                                    2,
                                ),
                                vertical(cs).nest(2),
                                Doc::text("end"),
                            ],
                            0,
                        )
                    }
                    CoreExpr::Clause {
                        patterns,
                        guard,
                        body,
                    } => {
                        let pats = self.values(
                            &patterns
                                .iter()
                                .map(|&id| p(id, node.module))
                                .collect::<Vec<_>>(),
                            strip,
                            line,
                            depth + 1,
                        )?;
                        let head = Doc::par(
                            vec![
                                pats,
                                Doc::text("when")
                                    .follow_at(child(p(*guard, node.module))?, 2)
                                    .follow(Doc::text("->")),
                            ],
                            2,
                        );
                        Doc::par(vec![head, child(p(*body, node.module))?], 4)
                    }
                    CoreExpr::Fun { params, body } => {
                        let vars = Doc::comma(
                            params
                                .iter()
                                .map(|&id| child(p(id, node.module)))
                                .collect::<Result<Vec<_>, _>>()?,
                        )
                        .surround("(", ") ->");
                        Doc::par(
                            vec![
                                Doc::text("fun").follow_at(vars, 2),
                                child(p(*body, node.module))?,
                            ],
                            4,
                        )
                    }
                    CoreExpr::Catch { body } => {
                        Doc::text("catch").follow_at(child(p(*body, node.module))?, 2)
                    }
                    CoreExpr::Try(try_payload) => {
                        let crate::core::TryExpr {
                            argument,
                            vars,
                            body,
                            exception_vars,
                            handler,
                        } = try_payload.as_ref();

                        let vs = self.values(
                            &vars
                                .iter()
                                .map(|&id| p(id, node.module))
                                .collect::<Vec<_>>(),
                            strip,
                            line,
                            depth + 1,
                        )?;
                        let es = self.values(
                            &exception_vars
                                .iter()
                                .map(|&id| p(id, node.module))
                                .collect::<Vec<_>>(),
                            strip,
                            line,
                            depth + 1,
                        )?;
                        Doc::sep(
                            vec![
                                Doc::text("try").follow_at(child(p(*argument, node.module))?, 4),
                                Doc::text("of ")
                                    .beside(vs)
                                    .beside(Doc::text(" ->"))
                                    .follow_at(child(p(*body, node.module))?, 4),
                                Doc::text("catch ")
                                    .beside(es)
                                    .beside(Doc::text(" ->"))
                                    .follow_at(child(p(*handler, node.module))?, 4),
                            ],
                            0,
                        )
                    }
                    CoreExpr::Receive {
                        clauses,
                        timeout,
                        action,
                    } => {
                        let cs = vertical(
                            clauses
                                .iter()
                                .map(|&id| child(p(id, node.module)))
                                .collect::<Result<Vec<_>, _>>()?,
                        )
                        .nest(2);
                        Doc::sep(
                            vec![
                                Doc::text("receive"),
                                cs,
                                Doc::sep(
                                    vec![
                                        Doc::text("after").follow_at(
                                            child(p(*timeout, node.module))?
                                                .beside(Doc::text(" ->")),
                                            2,
                                        ),
                                        child(p(*action, node.module))?.nest(2),
                                    ],
                                    0,
                                ),
                            ],
                            0,
                        )
                    }
                    CoreExpr::Letrec { definitions, body } => {
                        let ds = definitions
                            .iter()
                            .map(|&(n, f)| {
                                Ok(Doc::par(
                                    vec![
                                        child(p(n, node.module))?.beside(Doc::text(" =")),
                                        child(p(f, node.module))?,
                                    ],
                                    4,
                                ))
                            })
                            .collect::<Result<Vec<_>, PlanError>>()?;
                        Doc::sep(
                            vec![
                                Doc::text("letrec"),
                                vertical(ds).nest(2),
                                Doc::text("in ").beside(child(p(*body, node.module))?),
                            ],
                            0,
                        )
                    }
                    CoreExpr::Module(module_payload) => {
                        let crate::core::ModuleExpr {
                            name,
                            exports,
                            attributes,
                            definitions,
                        } = module_payload.as_ref();

                        let xs = Doc::comma(
                            exports
                                .iter()
                                .map(|&id| child(p(id, node.module)))
                                .collect::<Result<Vec<_>, _>>()?,
                        )
                        .surround("[", "]");
                        let attrs = Doc::comma(
                            attributes
                                .iter()
                                .map(|&(k, v)| {
                                    Ok(child(p(k, node.module))?
                                        .beside(Doc::text(" ="))
                                        .follow_at(child(p(v, node.module))?, 4))
                                })
                                .collect::<Result<Vec<_>, PlanError>>()?,
                        )
                        .surround("[", "]");
                        let defs = vertical(
                            definitions
                                .iter()
                                .map(|&(n, f)| {
                                    Ok(Doc::par(
                                        vec![
                                            child(p(n, node.module))?.beside(Doc::text(" =")),
                                            child(p(f, node.module))?,
                                        ],
                                        4,
                                    ))
                                })
                                .collect::<Result<Vec<_>, PlanError>>()?,
                        )
                        .nest(2);
                        Doc::sep(
                            vec![
                                Doc::text("module")
                                    .follow_at(child(p(*name, node.module))?.follow(xs), 2),
                                Doc::text("attributes").follow_at(attrs, 2).nest(2),
                                defs,
                                Doc::text("end"),
                            ],
                            0,
                        )
                    }
                    _ => return Err(PlanError::Unsupported("opaque Core expression".into())),
                }
            }
        };
        Ok(if let Some(n) = ann {
            Doc::text(format!("% Line {n}")).above(out)
        } else {
            out
        })
    }
    fn folded_chars(&self, pat: PatternRef) -> Result<Option<String>, PlanError> {
        let mut out = String::new();
        let mut tail = pat;
        let mut count = 0;
        loop {
            count += 1;
            if count > 1_000_000 {
                return Err(PlanError::ResourceLimit);
            }
            match self.patterns.shape(tail)? {
                Shape::Cons { head, tail: next } => {
                    let Shape::Literal(LiteralRef::Term { module, term }) =
                        self.patterns.shape(head)?
                    else {
                        return Ok(None);
                    };
                    let ErlangTerm::Integer(n) = self.patterns.term(module, term)? else {
                        return Ok(None);
                    };
                    let Some(c) = n
                        .parse::<u32>()
                        .ok()
                        .filter(|&n| print_char(n))
                        .and_then(char::from_u32)
                    else {
                        return Ok(None);
                    };
                    out.push(c);
                    tail = next
                }
                Shape::Literal(lit) => {
                    let (module, term, start) = literal_parts(lit);
                    match self.patterns.term(module, term)? {
                        ErlangTerm::Nil => return Ok((!out.is_empty()).then_some(out)),
                        ErlangTerm::List { items, tail }
                            if matches!(self.patterns.term(module, *tail)?, ErlangTerm::Nil) =>
                        {
                            for &id in items.get(start..).ok_or_else(|| {
                                PlanError::Boundary("list suffix outside term".into())
                            })? {
                                let ErlangTerm::Integer(n) = self.patterns.term(module, id)? else {
                                    return Ok(None);
                                };
                                let Some(c) = n
                                    .parse::<u32>()
                                    .ok()
                                    .filter(|&n| print_char(n))
                                    .and_then(char::from_u32)
                                else {
                                    return Ok(None);
                                };
                                out.push(c)
                            }
                            return Ok((!out.is_empty()).then_some(out));
                        }
                        _ => return Ok(None),
                    }
                }
                _ => return Ok(None),
            }
        }
    }
    fn empty_map(&self, pat: PatternRef) -> Result<bool, PlanError> {
        Ok(
            matches!(self.patterns.shape(pat)?,Shape::Literal(LiteralRef::Term{module,term}) if matches!(self.patterns.term(module,term)?,ErlangTerm::Map(xs)if xs.is_empty())),
        )
    }
    fn atom(&self, pat: PatternRef) -> Result<String, PlanError> {
        let LiteralRef::Term { module, term } = self.patterns.literal(pat)? else {
            return Err(PlanError::Boundary("expected atom".into()));
        };
        if let ErlangTerm::Atom(a) = self.patterns.term(module, term)? {
            Ok(a.clone())
        } else {
            Err(PlanError::Boundary("expected atom".into()))
        }
    }
    fn literal(&self, lit: LiteralRef, depth: usize, hook: bool) -> Result<Doc, PlanError> {
        if depth > 128 {
            return Err(PlanError::ResourceLimit);
        }
        let (module, term, start) = literal_parts(lit);
        let value = self.patterns.term(module, term)?;
        let child = |id| self.literal(LiteralRef::Term { module, term: id }, depth + 1, true);
        Ok(match value {
            ErlangTerm::ExternalFun(_) => {
                return Err(PlanError::Unsupported(
                    "cerl_prettypr external function literal".into(),
                ));
            }
            ErlangTerm::Atom(s) => Doc::text(quoted(s, '\'')),
            ErlangTerm::Integer(n) => Doc::text(n),
            ErlangTerm::Float(bits) => Doc::text(core_float(*bits)?),
            ErlangTerm::Nil => Doc::text("[]"),
            ErlangTerm::Tuple(xs) => Doc::comma(
                xs.iter()
                    .map(|&id| child(id))
                    .collect::<Result<Vec<_>, _>>()?,
            )
            .surround("{", "}"),
            ErlangTerm::List { items, tail } => {
                let xs = items
                    .get(start..)
                    .ok_or_else(|| PlanError::Boundary("list suffix outside term".into()))?;
                let nil = matches!(self.patterns.term(module, *tail)?, ErlangTerm::Nil);
                let mut cs = String::new();
                let string = nil
                    && !xs.is_empty()
                    && xs.iter().all(|&id| match self.patterns.term(module, id) {
                        Ok(ErlangTerm::Integer(s)) => s.parse::<u32>().ok().is_some_and(|n| {
                            print_char(n)
                                && char::from_u32(n).is_some_and(|c| {
                                    cs.push(c);
                                    true
                                })
                        }),
                        _ => false,
                    });
                if string {
                    Doc::string(quoted(&cs, '"'))
                } else {
                    let mut ds = xs
                        .iter()
                        .map(|&id| child(id))
                        .collect::<Result<Vec<_>, _>>()?;
                    for i in 0..ds.len().saturating_sub(1) {
                        ds[i] = ds[i].clone().beside(Doc::text(","))
                    }
                    if !nil {
                        ds.push(Doc::text("| ").beside(child(*tail)?))
                    }
                    Doc::par(ds, 0).surround("[", "]")
                }
            }
            ErlangTerm::Map(entries) => {
                let terms = crate::core::ErlangTerms {
                    root: term,
                    terms: self.patterns.nodes.module(module)?.terms.clone(),
                };
                let order = crate::term_order::TermOrderArena::new(&terms)?;
                let mut es = order.canonical_map_pairs(entries)?;
                let mut error = None;
                es.sort_by(|&(ak, av), &(bk, bv)| {
                    match order.compare(ak, &order, bk).and_then(|cmp| {
                        if cmp.is_eq() {
                            order.compare(av, &order, bv)
                        } else {
                            Ok(cmp)
                        }
                    }) {
                        Ok(cmp) => cmp,
                        Err(e) => {
                            error = Some(e);
                            std::cmp::Ordering::Equal
                        }
                    }
                });
                if let Some(e) = error {
                    return Err(e.into());
                }
                Doc::comma(
                    es.into_iter()
                        .map(|(k, v)| Ok(child(k)?.beside(Doc::text("=>")).beside(child(v)?)))
                        .collect::<Result<Vec<_>, PlanError>>()?,
                )
                .surround(
                    if hook { "#{" } else { "~{" },
                    if hook { "}" } else { "}~" },
                )
            }
            ErlangTerm::Binary(bytes) => bit_doc(bytes, (bytes.len() * 8) as u64, hook),
            ErlangTerm::Bitstring { bytes, bits } => bit_doc(bytes, *bits, hook),
        })
    }
    fn segment(
        &self,
        pat: PatternRef,
        strip: bool,
        line: i64,
        depth: usize,
    ) -> Result<Doc, PlanError> {
        let Shape::Segment {
            value,
            size,
            unit,
            typ,
            flags,
        } = self.patterns.shape(pat)?
        else {
            return Err(PlanError::Boundary("expected segment".into()));
        };
        let cont = |x| self.doc(x, strip, true, line, depth + 1, false);
        let kind = self.atom(typ)?;
        let unit_value = match self.patterns.literal(unit) {
            Ok(LiteralRef::Term { module, term }) => match self.patterns.term(module, term)? {
                ErlangTerm::Integer(n) => Some(n.clone()),
                _ => None,
            },
            _ => None,
        };
        let fs = self.atoms(flags)?;
        let size_doc = if self.atom(size).is_ok() {
            Doc::text("")
        } else {
            Doc::text(":").beside(cont(size)?)
        };
        let suffix = if unit_value.as_deref() == Some("1")
            && kind == "integer"
            && fs == ["unsigned", "big"]
        {
            if matches!(self.patterns.literal(size),Ok(LiteralRef::Term{module,term})if matches!(self.patterns.term(module,term)?,ErlangTerm::Integer(n)if n=="8"))
            {
                Doc::text("")
            } else {
                size_doc
            }
        } else if unit_value.as_deref() == Some("8")
            && kind == "binary"
            && fs == ["unsigned", "big"]
        {
            size_doc.beside(Doc::text("/binary"))
        } else {
            let fs = fs
                .iter()
                .filter(|f| match kind.as_str() {
                    "binary" | "utf8" | "utf16" | "utf32" => false,
                    "float" => ["little", "native"].contains(&f.as_str()),
                    "integer" => ["little", "native", "signed"].contains(&f.as_str()),
                    _ => false,
                })
                .map(|s| format!("-{s}"))
                .collect::<String>();
            let d = size_doc.beside(Doc::text(format!("/{kind}{fs}")));
            if unit_value.is_some() {
                d.beside(Doc::text("-unit:")).beside(cont(unit)?)
            } else {
                d
            }
        };
        Ok(cont(value)?.beside(suffix))
    }
    fn atoms(&self, pat: PatternRef) -> Result<Vec<String>, PlanError> {
        let LiteralRef::Term { module, term } = self.patterns.literal(pat)? else {
            return Err(PlanError::Boundary("expected flags literal".into()));
        };
        match self.patterns.term(module, term)? {
            ErlangTerm::Nil => Ok(vec![]),
            ErlangTerm::List { items, tail }
                if matches!(self.patterns.term(module, *tail)?, ErlangTerm::Nil) =>
            {
                items
                    .iter()
                    .map(|&id| match self.patterns.term(module, id)? {
                        ErlangTerm::Atom(a) => Ok(a.clone()),
                        _ => Err(PlanError::Boundary("expected atom flag".into())),
                    })
                    .collect()
            }
            _ => Err(PlanError::Boundary("expected flags list".into())),
        }
    }
}
fn literal_parts(lit: LiteralRef) -> (crate::program::ModuleId, TermId, usize) {
    match lit {
        LiteralRef::Term { module, term } => (module, term, 0),
        LiteralRef::ListSuffix {
            module,
            term,
            index,
        } => (module, term, index),
    }
}
fn vertical(xs: Vec<Doc>) -> Doc {
    let mut xs = xs.into_iter();
    let mut d = xs.next().unwrap_or_else(|| Doc(Vec::new(), 0, 0));
    for x in xs {
        d = d.above(x)
    }
    d
}
fn quoted(s: &str, q: char) -> String {
    let mut out = String::new();
    out.push(q);
    for c in s.chars() {
        match c {
            c if c == q => {
                out.push('\\');
                out.push(c)
            }
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{b}' => out.push_str("\\v"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\u{1b}' => out.push_str("\\e"),
            '\u{7f}' => out.push_str("\\d"),
            c if (c as u32) < 32 || (127..160).contains(&(c as u32)) => {
                out.push_str(&format!("\\{:03o}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push(q);
    out
}
fn print_char(n: u32) -> bool {
    matches!(n,8..=13|27|32..=127|160..=255)
}
fn bit_doc(bytes: &[u8], bits: u64, hook: bool) -> Doc {
    if (bits / 8).saturating_mul(if hook { 2 } else { 32 }) > MAX_DOCUMENT_BYTES as u64 {
        return Doc::limited();
    }
    let full = bits / 8;
    let rem = bits % 8;
    let mut segs = bytes
        .iter()
        .take(full as usize)
        .map(|n| {
            if hook {
                n.to_string()
            } else {
                format!("#<{n}>(8, 1, 'integer', ['unsigned', 'big'])")
            }
        })
        .collect::<Vec<_>>();
    if rem != 0 {
        let n = bytes[full as usize] >> (8 - rem);
        segs.push(if hook {
            format!("{n}:{rem}")
        } else {
            format!("#<{n}>({rem}, 1, 'integer', ['unsigned', 'big'])")
        })
    }
    Doc::text(if hook {
        format!("<<{}>>", segs.join(","))
    } else {
        format!("#{{{}}}#", segs.join(", "))
    })
}
fn core_float(bits: u64) -> Result<String, PlanError> {
    let v = f64::from_bits(bits);
    if !v.is_finite() {
        return Err(PlanError::Boundary("nonfinite float".into()));
    }
    let s = format!("{v:.20e}");
    let (m, e) = s.split_once('e').unwrap();
    let mut cut = m.to_string();
    if let Some(dot) = cut.find('.')
        && let Some(zeros) = cut[dot + 2..].find("000")
    {
        cut.truncate(dot + 2 + zeros)
    }
    let exponent = e
        .parse::<i32>()
        .map_err(|e| PlanError::Boundary(e.to_string()))?;
    if exponent != 0 {
        cut.push_str(&format!("e{exponent:+}"))
    }
    Ok(cut)
}

#[cfg(test)]
mod tests {
    use super::Doc;
    #[test]
    fn expanded_documents_reject_before_allocating_exponential_output() {
        let mut doc = Doc::text("x".repeat(1024));
        for _ in 0..17 {
            doc = doc.clone().beside(doc);
        }
        assert!(matches!(doc.finish(), Err(super::PlanError::ResourceLimit)));
        let doc = Doc::text("x").above(Doc::text("y")).nest(usize::MAX);
        assert!(matches!(doc.finish(), Err(super::PlanError::ResourceLimit)));
        let doc = Doc::par(vec![Doc::text("prefix"), Doc::limited()], 0);
        assert!(matches!(doc.finish(), Err(super::PlanError::ResourceLimit)));
    }
    #[test]
    fn empty_clause_layout_preserves_horizontal_spacing() {
        assert_eq!(
            Doc::sep(
                vec![
                    Doc::text("case X of"),
                    Doc(Vec::new(), 0, 0),
                    Doc::text("end")
                ],
                0
            )
            .finish()
            .unwrap(),
            "case X of  end"
        );
        assert_eq!(
            Doc::sep(
                vec![
                    Doc::text("case X").above(Doc::text("of")),
                    Doc(Vec::new(), 0, 0),
                    Doc::text("end")
                ],
                0
            )
            .finish()
            .unwrap(),
            "case X\nof\nend"
        );
    }
    #[test]
    fn paragraph_packs_prefix_before_multiline_annotation() {
        assert_eq!(
            Doc::par(
                vec![
                    Doc::text("A,"),
                    Doc::text("B,"),
                    Doc::text("% Line 2").above(Doc::text("C"))
                ],
                0
            )
            .finish()
            .unwrap(),
            "A, B,\n% Line 2\nC"
        );
        assert_eq!(
            Doc::sep(
                vec![
                    Doc::text("A,"),
                    Doc::text("B,"),
                    Doc::text("% Line 2").above(Doc::text("C"))
                ],
                0
            )
            .finish()
            .unwrap(),
            "A,\nB,\n% Line 2\nC"
        );
    }
    #[test]
    fn string_split_keeps_native_declared_length_recurrence() {
        let text = format!("\"{}\"", "a ".repeat(100000));
        let rendered = Doc::string(text).finish().unwrap();
        assert_eq!(rendered.len(), 200011);
        assert!(rendered.ends_with("\"\n\"a \""));
    }
}

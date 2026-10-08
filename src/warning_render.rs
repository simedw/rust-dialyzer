use crate::{
    bifs::boundary::NativeTerm,
    core::{ErlangTerm, ErlangTerms, TermId},
    lattice::{PresentationContext, TypePredicate},
};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::{collections::BTreeMap, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilenameOption {
    Basename,
    Fullpath,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocationOption {
    Line,
    Column,
}
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub indent: bool,
    pub filename: FilenameOption,
    pub location: LocationOption,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            indent: true,
            filename: FilenameOption::Basename,
            location: LocationOption::Column,
        }
    }
}
#[derive(Debug, Clone)]
pub enum RenderError {
    Malformed(String),
    ResourceLimit { depth: usize, expanded_nodes: usize },
    Syntax(crate::erlang_syntax::syntax::SyntaxError),
    NoMatchingClause { function: String },
    UnsupportedTerm(String),
}
impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for RenderError {}
type Result<T> = std::result::Result<T, RenderError>;
#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Atom(String),
    Integer(BigInt),
    Float(u64),
    Binary(Vec<u8>),
    Tuple(Vec<Rc<Value>>),
    List(Vec<Rc<Value>>),
    Improper(Vec<Rc<Value>>, Rc<Value>),
    Other(&'static str),
}
type V = Rc<Value>;
type Env = BTreeMap<&'static str, V>;
fn text(s: &str) -> V {
    Rc::new(Value::List(
        s.chars()
            .map(|c| Rc::new(Value::Integer(BigInt::from(c as u32))))
            .collect(),
    ))
}
fn boolean(b: bool) -> V {
    Rc::new(Value::Atom(if b { "true" } else { "false" }.into()))
}
fn list(v: &V) -> Result<&[V]> {
    if let Value::List(v) = v.as_ref() {
        Ok(v)
    } else {
        Err(RenderError::Malformed("expected proper list".into()))
    }
}
fn string(v: &V) -> Result<String> {
    let mut out = String::new();
    let mut pending = vec![v.as_ref()];
    while let Some(v) = pending.pop() {
        match v {
            Value::List(items) => pending.extend(items.iter().rev().map(AsRef::as_ref)),
            Value::Integer(n) => out.push(
                n.to_u32()
                    .and_then(char::from_u32)
                    .ok_or_else(|| RenderError::Malformed("invalid character".into()))?,
            ),
            Value::Binary(b) => out.push_str(
                std::str::from_utf8(b).map_err(|e| RenderError::Malformed(e.to_string()))?,
            ),
            _ => return Err(RenderError::Malformed("expected character data".into())),
        }
    }
    Ok(out)
}
fn native(v: &V) -> Result<NativeTerm> {
    Ok(match v.as_ref() {
        Value::Atom(a) => NativeTerm::Atom { value: a.clone() },
        Value::Integer(n) => NativeTerm::Integer {
            value: n.to_string(),
        },
        Value::Tuple(t) => NativeTerm::Tuple {
            items: t.iter().map(native).collect::<Result<_>>()?,
        },
        Value::List(t) if t.is_empty() => NativeTerm::Nil,
        Value::List(t) => NativeTerm::List {
            items: t.iter().map(native).collect::<Result<_>>()?,
            tail: Box::new(NativeTerm::Nil),
        },
        _ => {
            return Err(RenderError::UnsupportedTerm(
                "canonical type contains unsupported value".into(),
            ));
        }
    })
}
// Value equality and canonical-type conversion use recursive borrowed trees. Bound
// their external depth and expanded size before constructing those trees.
fn validate_render_arena(arena: &ErlangTerms) -> Result<()> {
    const MAX_DEPTH: usize = 128;
    const MAX_EXPANDED: usize = 1_000_000;
    let mut measures: Vec<(usize, usize)> = Vec::with_capacity(arena.terms.len());
    for term in &arena.terms {
        let mut depth = 1usize;
        let mut expanded = 1usize;
        let mut add = |id: TermId| -> Result<()> {
            let &(d, n) = measures
                .get(id.0)
                .ok_or_else(|| RenderError::Malformed("invalid arena reference".into()))?;
            depth = depth.max(d.saturating_add(1));
            expanded = expanded.saturating_add(n);
            Ok(())
        };
        match term {
            ErlangTerm::Tuple(items) => {
                for &id in items {
                    add(id)?;
                }
            }
            ErlangTerm::List { items, tail } => {
                for &id in items {
                    add(id)?;
                }
                add(*tail)?;
            }
            ErlangTerm::Map(entries) => {
                for &(key, value) in entries {
                    add(key)?;
                    add(value)?;
                }
            }
            _ => {}
        }
        if depth > MAX_DEPTH || expanded > MAX_EXPANDED {
            return Err(RenderError::ResourceLimit {
                depth,
                expanded_nodes: expanded,
            });
        }
        measures.push((depth, expanded));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Function {
    Message,
    Arguments,
    Code,
    CallReason,
    Contract,
    IsOpaque,
    TypeString,
    FieldDiffs,
    Expected,
    ExpectedWithoutOpaque,
    Positions,
    OpaquePositions,
    InvalidDetails,
    Format,
    IsInteger,
    Flatten,
    Reverse,
    Min,
    Ordinal,
    Position,
    PatternString,
    RecordString,
    Signature,
    Split,
    TypeLayout,
}
#[derive(Clone, Copy, Debug)]
enum Operator {
    Append,
    Equal,
    Or,
    Remainder,
}
#[derive(Clone, Debug)]
enum Pattern {
    Var(&'static str),
    Atom(&'static str),
    Integer(&'static str),
    Nil,
    Cons(Box<Pattern>, Box<Pattern>),
    Tuple(Vec<Pattern>),
    Text(&'static str),
    Prefix(Box<Pattern>, Box<Pattern>),
    All(Vec<Pattern>),
}
#[derive(Clone, Debug)]
enum Expr {
    Var(&'static str),
    Atom(&'static str),
    Integer(&'static str),
    Text(&'static str),
    Nil,
    Cons(Box<Expr>, Box<Expr>),
    Tuple(Vec<Expr>),
    Match(Pattern, Box<Expr>),
    Call(Function, Vec<Expr>),
    Op(Operator, Vec<Expr>),
    Case(Box<Expr>, Vec<Clause>),
    If(Vec<Clause>),
    Comprehension(Box<Expr>, Vec<Qualifier>),
}
#[derive(Clone, Debug)]
enum Qualifier {
    Generate(Pattern, Expr),
}
#[derive(Clone, Debug)]
struct Clause {
    patterns: Vec<Pattern>,
    guards: Vec<Vec<Expr>>,
    body: Vec<Expr>,
}
#[path = "render_source.rs"]
mod source;

fn bind(pattern: &Pattern, value: &V, env: &mut Env) -> Result<bool> {
    Ok(match pattern {
        Pattern::Var("_") => true,
        Pattern::Var(name) => match env.get(name) {
            Some(old) => old == value,
            None => {
                env.insert(name, value.clone());
                true
            }
        },
        Pattern::Atom(a) => matches!(value.as_ref(),Value::Atom(v) if v==a),
        Pattern::Integer(n) => matches!(value.as_ref(),Value::Integer(v) if v.to_string()==*n),
        Pattern::Nil => matches!(value.as_ref(),Value::List(v) if v.is_empty()),
        Pattern::Text(s) => value == &text(s),
        Pattern::Tuple(patterns) => {
            if let Value::Tuple(values) = value.as_ref() {
                if patterns.len() != values.len() {
                    false
                } else {
                    let mut result = true;
                    for (p, v) in patterns.iter().zip(values) {
                        result &= bind(p, v, env)?;
                    }
                    result
                }
            } else {
                false
            }
        }
        Pattern::Cons(head, tail) => {
            if let Value::List(values) = value.as_ref() {
                if let Some(first) = values.first() {
                    bind(head, first, env)?
                        && bind(tail, &Rc::new(Value::List(values[1..].to_vec())), env)?
                } else {
                    false
                }
            } else {
                false
            }
        }
        Pattern::Prefix(prefix, rest) => {
            let Pattern::Text(prefix) = prefix.as_ref() else {
                return Err(RenderError::Malformed("nonliteral pattern prefix".into()));
            };
            let values = match value.as_ref() {
                Value::List(values) => values,
                _ => return Ok(false),
            };
            let prefix = list(&text(prefix))?.to_vec();
            values.starts_with(&prefix)
                && bind(
                    rest,
                    &Rc::new(Value::List(values[prefix.len()..].to_vec())),
                    env,
                )?
        }
        Pattern::All(patterns) => {
            let mut result = true;
            for p in patterns {
                result &= bind(p, value, env)?;
            }
            result
        }
    })
}
fn truth(value: &V) -> bool {
    matches!(value.as_ref(),Value::Atom(a) if a=="true")
}

pub struct Renderer {
    functions: BTreeMap<(Function, usize), Vec<Clause>>,
    context: PresentationContext,
}
impl Default for Renderer {
    fn default() -> Self {
        Self {
            functions: source::source()
                .into_iter()
                .map(|(n, a, c)| ((n, a), c))
                .collect(),
            context: PresentationContext::default(),
        }
    }
}
impl Renderer {
    fn sequence(&self, body: &[Expr], env: &mut Env, options: Options) -> Result<V> {
        let mut value = Rc::new(Value::List(vec![]));
        for expression in body {
            value = self.eval(expression, env, options)?;
        }
        Ok(value)
    }
    fn clauses(
        &self,
        clauses: &[Clause],
        args: &[V],
        parent: &Env,
        options: Options,
    ) -> Result<Option<V>> {
        for clause in clauses {
            if clause.patterns.len() != args.len() {
                continue;
            }
            let mut env = parent.clone();
            let mut matches = true;
            for (p, v) in clause.patterns.iter().zip(args) {
                if !bind(p, v, &mut env)? {
                    matches = false;
                    break;
                }
            }
            if !matches {
                continue;
            }
            let mut guard = clause.guards.is_empty();
            for conjunction in &clause.guards {
                let mut okay = true;
                for test in conjunction {
                    if !truth(&self.eval(test, &mut env, options)?) {
                        okay = false;
                        break;
                    }
                }
                guard |= okay;
            }
            if guard {
                return self.sequence(&clause.body, &mut env, options).map(Some);
            }
        }
        Ok(None)
    }
    fn eval(&self, expr: &Expr, env: &mut Env, options: Options) -> Result<V> {
        match expr {
            Expr::Var(name) => env
                .get(name)
                .cloned()
                .ok_or_else(|| RenderError::Malformed(format!("unbound source variable {name}"))),
            Expr::Atom(a) => Ok(Rc::new(Value::Atom((*a).into()))),
            Expr::Integer(n) => {
                Ok(Rc::new(Value::Integer(n.parse().map_err(|_| {
                    RenderError::Malformed("source integer".into())
                })?)))
            }
            Expr::Text(s) => Ok(text(s)),
            Expr::Nil => Ok(Rc::new(Value::List(vec![]))),
            Expr::Cons(head, tail) => {
                let h = self.eval(head, env, options)?;
                let t = self.eval(tail, env, options)?;
                let mut values = vec![h];
                values.extend(list(&t)?.iter().cloned());
                Ok(Rc::new(Value::List(values)))
            }
            Expr::Tuple(items) => Ok(Rc::new(Value::Tuple(
                items
                    .iter()
                    .map(|x| self.eval(x, env, options))
                    .collect::<Result<_>>()?,
            ))),
            Expr::Match(pattern, value) => {
                let value = self.eval(value, env, options)?;
                if !bind(pattern, &value, env)? {
                    return Err(RenderError::Malformed("source match failed".into()));
                }
                Ok(value)
            }
            Expr::Call(function, args) => {
                let args = args
                    .iter()
                    .map(|x| self.eval(x, env, options))
                    .collect::<Result<Vec<_>>>()?;
                self.call(*function, &args, options)
            }
            Expr::Op(op, args) => {
                let left = self.eval(&args[0], env, options)?;
                if matches!(op, Operator::Or) && truth(&left) {
                    return Ok(boolean(true));
                }
                let right = self.eval(&args[1], env, options)?;
                match op {
                    Operator::Append => {
                        let mut values = list(&left)?.to_vec();
                        values.extend(list(&right)?.iter().cloned());
                        Ok(Rc::new(Value::List(values)))
                    }
                    Operator::Equal => Ok(boolean(left == right)),
                    Operator::Or => Ok(boolean(truth(&right))),
                    Operator::Remainder => {
                        let (Value::Integer(a), Value::Integer(b)) =
                            (left.as_ref(), right.as_ref())
                        else {
                            return Err(RenderError::Malformed("remainder operands".into()));
                        };
                        Ok(Rc::new(Value::Integer(a % b)))
                    }
                }
            }
            Expr::Case(input, clauses) => {
                let input = self.eval(input, env, options)?;
                self.clauses(clauses, &[input], env, options)?
                    .ok_or_else(|| RenderError::NoMatchingClause {
                        function: "case".into(),
                    })
            }
            Expr::If(clauses) => self.clauses(clauses, &[], env, options)?.ok_or_else(|| {
                RenderError::NoMatchingClause {
                    function: "if".into(),
                }
            }),
            Expr::Comprehension(body, qualifiers) => {
                let mut values = Vec::new();
                self.comprehend(body, qualifiers, env, options, &mut values)?;
                Ok(Rc::new(Value::List(values)))
            }
        }
    }
    fn comprehend(
        &self,
        body: &Expr,
        qualifiers: &[Qualifier],
        env: &Env,
        options: Options,
        out: &mut Vec<V>,
    ) -> Result<()> {
        let Some((qualifier, rest)) = qualifiers.split_first() else {
            let mut env = env.clone();
            out.push(self.eval(body, &mut env, options)?);
            return Ok(());
        };
        let mut scope = env.clone();
        match qualifier {
            Qualifier::Generate(pattern, input) => {
                let input = self.eval(input, &mut scope, options)?;
                for value in list(&input)? {
                    let mut next = scope.clone();
                    if bind(pattern, value, &mut next)? {
                        self.comprehend(body, rest, &next, options, out)?;
                    }
                }
            }
        }
        Ok(())
    }
    fn call(&self, function: Function, args: &[V], options: Options) -> Result<V> {
        if let Some(clauses) = self.functions.get(&(function, args.len())) {
            return self
                .clauses(clauses, args, &Env::new(), options)?
                .ok_or_else(|| RenderError::NoMatchingClause {
                    function: format!("{function:?}/{}", args.len()),
                });
        }
        match function {
            Function::TypeLayout | Function::Signature | Function::FieldDiffs => {
                let value = string(&args[0])?;
                if !options.indent {
                    return Ok(args[0].clone());
                }
                use crate::erlang_syntax::syntax::{
                    format_field_diffs, format_signature, format_type_or_literal,
                };
                let rendered = match function {
                    Function::TypeLayout => format_type_or_literal(&value),
                    Function::Signature => format_signature(&value),
                    _ => format_field_diffs(&value),
                }
                .map_err(RenderError::Syntax)?;
                Ok(text(&rendered))
            }
            Function::Format => self.format(&string(&args[0])?, list(&args[1])?),
            Function::IsInteger => Ok(boolean(matches!(args[0].as_ref(), Value::Integer(_)))),
            Function::Reverse => {
                let mut values = list(&args[0])?.to_vec();
                values.reverse();
                Ok(Rc::new(Value::List(values)))
            }
            Function::Flatten => Ok(text(&string(&args[0])?)),
            Function::Min => {
                let (Value::Integer(a), Value::Integer(b)) = (args[0].as_ref(), args[1].as_ref())
                else {
                    return Err(RenderError::Malformed("min operands".into()));
                };
                Ok(Rc::new(Value::Integer(a.min(b).clone())))
            }
            Function::Position => self.position(&args[0], options),
            Function::Split => {
                let source = string(&args[0])?;
                let separator = string(&args[1])?;
                Ok(Rc::new(Value::List(
                    source.split(&separator).map(text).collect(),
                )))
            }
            Function::IsOpaque => {
                let ty = native(&args[0])?
                    .type_value()
                    .map_err(RenderError::Malformed)?;
                Ok(boolean(
                    ty.predicate(TypePredicate::Opaque)
                        .map_err(|e| RenderError::Malformed(e.to_string()))?,
                ))
            }
            Function::TypeString => {
                let ty = native(&args[0])?
                    .type_value()
                    .map_err(RenderError::Malformed)?;
                Ok(text(
                    &self
                        .context
                        .type_to_string(&ty)
                        .map_err(|e| RenderError::Malformed(e.to_string()))?,
                ))
            }
            _ => Err(RenderError::NoMatchingClause {
                function: format!("{function:?}/{}", args.len()),
            }),
        }
    }
    fn format(&self, format: &str, args: &[V]) -> Result<V> {
        let mut output = String::new();
        let mut chars = format.chars();
        let mut index = 0;
        while let Some(c) = chars.next() {
            if c != '~' {
                output.push(c);
                continue;
            }
            let mut unicode = false;
            let mut control = chars
                .next()
                .ok_or_else(|| RenderError::Malformed("format ends with ~".into()))?;
            if control == 't' {
                unicode = true;
                control = chars
                    .next()
                    .ok_or_else(|| RenderError::Malformed("format ends with ~t".into()))?
            }
            if control == 'n' {
                output.push('\n');
                continue;
            }
            let value = args
                .get(index)
                .ok_or_else(|| RenderError::Malformed("missing format argument".into()))?;
            index += 1;
            match control {
                's' => match value.as_ref() {
                    Value::Atom(a) => output.push_str(a),
                    _ => output.push_str(&string(value)?),
                },
                'w' | 'B' => match value.as_ref() {
                    Value::Atom(a) if unicode => output.push_str(&self.context.atom_to_string(a)),
                    Value::Atom(a) => output.push_str(
                        self.context
                            .qualified_name(a, "a")
                            .strip_suffix(":a")
                            .unwrap_or(a),
                    ),
                    Value::Integer(n) => output.push_str(&n.to_string()),
                    _ => {
                        return Err(RenderError::UnsupportedTerm(
                            "term writer requires non-scalar".into(),
                        ));
                    }
                },
                _ => {
                    return Err(RenderError::UnsupportedTerm(format!(
                        "io format control {control}"
                    )));
                }
            }
        }
        if index != args.len() {
            return Err(RenderError::Malformed("excess format arguments".into()));
        }
        Ok(text(&output))
    }
    fn position(&self, value: &V, options: Options) -> Result<V> {
        let position = match value.as_ref() {
            Value::Integer(n) => n.to_string(),
            Value::Tuple(items) if items.len() == 2 => {
                let Value::Integer(line) = items[0].as_ref() else {
                    return Err(RenderError::Malformed("invalid source line".into()));
                };
                if options.location == LocationOption::Line {
                    line.to_string()
                } else {
                    let Value::Integer(column) = items[1].as_ref() else {
                        return Err(RenderError::Malformed("invalid source column".into()));
                    };
                    format!("{line}:{column}")
                }
            }
            _ => return Err(RenderError::Malformed("invalid source position".into())),
        };
        Ok(text(&position))
    }
    pub fn format_warning(
        &self,
        arena: &ErlangTerms,
        root: TermId,
        options: Options,
    ) -> Result<String> {
        validate_render_arena(arena)?;
        let mut values: Vec<V> = Vec::with_capacity(arena.terms.len());
        for term in &arena.terms {
            let value = match term {
                ErlangTerm::Atom(a) => Value::Atom(a.clone()),
                ErlangTerm::Integer(n) => Value::Integer(
                    n.parse()
                        .map_err(|_| RenderError::Malformed("arena integer".into()))?,
                ),
                ErlangTerm::Float(f) => Value::Float(*f),
                ErlangTerm::Binary(b) => Value::Binary(b.clone()),
                ErlangTerm::Tuple(items) => {
                    Value::Tuple(items.iter().map(|id| values[id.0].clone()).collect())
                }
                ErlangTerm::Nil => Value::List(vec![]),
                ErlangTerm::List { items, tail } => {
                    let mut result: Vec<_> = items.iter().map(|id| values[id.0].clone()).collect();
                    match values[tail.0].as_ref() {
                        Value::List(t) => {
                            result.extend(t.iter().cloned());
                            Value::List(result)
                        }
                        _ => Value::Improper(result, values[tail.0].clone()),
                    }
                }
                ErlangTerm::ExternalFun { .. } => Value::Other("function"),
                ErlangTerm::Bitstring { .. } => Value::Other("bitstring"),
                ErlangTerm::Map(_) => Value::Other("map"),
            };
            values.push(Rc::new(value));
        }
        let value = values
            .get(root.0)
            .ok_or_else(|| RenderError::Malformed("root outside arena".into()))?;
        let Value::Tuple(warning) = value.as_ref() else {
            return Err(RenderError::Malformed("warning is not a tuple".into()));
        };
        if warning.len() != 3 {
            return Err(RenderError::Malformed("warning tuple arity".into()));
        }
        let Value::Tuple(info) = warning[1].as_ref() else {
            return Err(RenderError::Malformed("warning info is not a tuple".into()));
        };
        if !matches!(info.len(), 2 | 3) {
            return Err(RenderError::Malformed("warning info tuple arity".into()));
        }
        if !matches!(info[0].as_ref(), Value::List(_)) {
            return Err(RenderError::Malformed(
                "warning filename must be a proper list".into(),
            ));
        }
        let file = string(&info[0])?;
        let file = if options.filename == FilenameOption::Fullpath {
            file
        } else {
            let stripped = file.trim_end_matches('/');
            stripped.rsplit('/').next().unwrap_or("").into()
        };
        let message = self.call(
            Function::Message,
            &[
                warning[2].clone(),
                boolean(options.indent),
                Rc::new(Value::Atom(
                    if options.location == LocationOption::Column {
                        "column"
                    } else {
                        "line"
                    }
                    .into(),
                )),
            ],
            options,
        )?;
        Ok(format!(
            "{file}:{}: {}",
            string(&self.position(&info[1], options)?)?,
            string(&message)?
        ))
    }
}

/// Apply native stable key sorts and adjacent exact full-warning deduplication.
pub fn process_warnings(arena: &ErlangTerms, warnings: &[TermId]) -> Result<Vec<TermId>> {
    use crate::term_order::TermOrderArena;
    let order = TermOrderArena::new(arena).map_err(RenderError::Malformed)?;
    use std::cmp::Ordering;
    let mut rows = Vec::with_capacity(warnings.len());
    for root in warnings {
        let Some(ErlangTerm::Tuple(fields)) = arena.terms.get(root.0) else {
            return Err(RenderError::Malformed("warning must be a tuple".into()));
        };
        if fields.len() != 3 {
            return Err(RenderError::Malformed("warning tuple arity".into()));
        }
        rows.push((*root, fields[1], fields[2]));
    }
    fn stable_sort<T: Copy, F>(rows: &mut Vec<T>, mut compare: F) -> Result<()>
    where
        F: FnMut(T, T) -> Result<Ordering>,
    {
        let mut width = 1;
        while width < rows.len() {
            let mut output = Vec::with_capacity(rows.len());
            for start in (0..rows.len()).step_by(width * 2) {
                let middle = (start + width).min(rows.len());
                let end = (start + width * 2).min(rows.len());
                let (mut left, mut right) = (start, middle);
                while left < middle && right < end {
                    if compare(rows[left], rows[right])? != Ordering::Greater {
                        output.push(rows[left]);
                        left += 1;
                    } else {
                        output.push(rows[right]);
                        right += 1;
                    }
                }
                output.extend_from_slice(&rows[left..middle]);
                output.extend_from_slice(&rows[right..end]);
            }
            *rows = output;
            width *= 2;
        }
        Ok(())
    }
    stable_sort(&mut rows, |a, b| {
        order
            .compare(a.2, &order, b.2)
            .map_err(RenderError::Malformed)
    })?;
    stable_sort(&mut rows, |a, b| {
        order
            .compare(a.1, &order, b.1)
            .map_err(RenderError::Malformed)
    })?;
    let mut result = Vec::with_capacity(rows.len());
    for (root, _, _) in rows {
        let duplicate = if let Some(previous) = result.last() {
            order
                .compare_map_keys(*previous, &order, root)
                .map_err(RenderError::Malformed)?
                == Ordering::Equal
        } else {
            false
        };
        if !duplicate {
            result.push(root);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn rejects_deep_and_exponential_dag_before_recursive_values_exist() {
        for (count, copies) in [(100_000, 1), (30, 2)] {
            let mut terms = vec![ErlangTerm::Nil];
            for i in 0..count {
                terms.push(ErlangTerm::Tuple(vec![TermId(i); copies]));
            }
            let arena = ErlangTerms {
                root: TermId(count),
                terms,
            };
            assert!(matches!(
                Renderer::default().format_warning(&arena, arena.root, Options::default()),
                Err(RenderError::ResourceLimit { .. })
            ));
        }
    }
    #[test]
    fn root_directory_basename_is_empty() {
        let arena=ErlangTerms::parse(&json!({"root":8,"terms":[
            {"kind":"atom","value":"warn_matching"},{"kind":"integer","value":"47"},{"kind":"nil"},
            {"kind":"list","items":[1],"tail":2},{"kind":"integer","value":"3"},{"kind":"tuple","items":[3,4]},
            {"kind":"atom","value":"guard_fail"},{"kind":"tuple","items":[6,2]},{"kind":"tuple","items":[0,5,7]}
        ]})).unwrap();
        let options = Options {
            indent: false,
            ..Options::default()
        };
        assert_eq!(
            Renderer::default()
                .format_warning(&arena, arena.root, options)
                .unwrap(),
            ":3: Clause guard cannot succeed.\n"
        );
    }
    #[test]
    fn stable_ties_keep_nonadjacent_duplicate_warnings() {
        let arena = ErlangTerms::parse(&json!({"root":5,"terms":[
            {"kind":"atom","value":"a"},{"kind":"atom","value":"b"},{"kind":"nil"},
            {"kind":"tuple","items":[0,2,2]},{"kind":"tuple","items":[1,2,2]},
            {"kind":"list","items":[3,4,3,3],"tail":2}
        ]}))
        .unwrap();
        assert_eq!(
            process_warnings(&arena, &[TermId(3), TermId(4), TermId(3), TermId(3)]).unwrap(),
            vec![TermId(3), TermId(4), TermId(3)]
        );
    }
}

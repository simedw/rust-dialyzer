use crate::ir::{Expr, Function, Key, Program};
use crate::types::Type;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

type Env = BTreeMap<String, Type>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    pub args: Vec<Type>,
    pub ret: Type,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Warning {
    pub module: String,
    pub function: String,
    pub arity: usize,
    pub line: u64,
    pub category: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Output {
    pub warnings: Vec<Warning>,
    pub signatures: BTreeMap<String, Signature>,
    pub evaluations: usize,
}

fn boolean() -> Type {
    Type::Atom(Some("true".into())).union(Type::Atom(Some("false".into())))
}

fn builtin(key: &Key) -> Option<Signature> {
    let Key(module, name, arity) = key;
    if module != "erlang" {
        return None;
    }
    let (args, ret) = match (name.as_str(), *arity) {
        ("+" | "-" | "*", 2) => (vec![Type::number(); 2], Type::number()),
        ("+" | "-", 1) => (vec![Type::number()], Type::number()),
        ("/", 2) => (vec![Type::number(); 2], Type::Float),
        ("div" | "rem", 2) => (vec![Type::Integer; 2], Type::Integer),
        ("length", 1) => (vec![Type::List(Box::new(Type::Any))], Type::Integer),
        ("byte_size" | "bit_size", 1) => (vec![Type::Binary], Type::Integer),
        ("hd", 1) => (vec![Type::List(Box::new(Type::Any))], Type::Any),
        ("tl", 1) => (
            vec![Type::List(Box::new(Type::Any))],
            Type::List(Box::new(Type::Any)),
        ),
        (
            "is_integer" | "is_float" | "is_number" | "is_atom" | "is_binary" | "is_list"
            | "is_boolean",
            1,
        ) => (vec![Type::Any], boolean()),
        ("==" | "=:=" | "/=" | "=/=" | "<" | ">" | "=<" | ">=", 2) => {
            (vec![Type::Any; 2], boolean())
        }
        _ => return None,
    };
    Some(Signature { args, ret })
}

fn predicate(name: &str) -> Option<Type> {
    Some(match name {
        "is_integer" => Type::Integer,
        "is_float" => Type::Float,
        "is_number" => Type::number(),
        "is_atom" => Type::Atom(None),
        "is_binary" => Type::Binary,
        "is_list" => Type::List(Box::new(Type::Any)),
        "is_boolean" => boolean(),
        _ => return None,
    })
}

fn bind(pattern: &Expr, ty: &Type, env: &mut Env) -> Type {
    match pattern {
        Expr::Var(name) => {
            let narrowed = env.get(name).unwrap_or(&Type::Any).intersect(ty);
            env.insert(name.clone(), narrowed.clone());
            narrowed
        }
        Expr::Literal(t) => t.intersect(ty),
        Expr::Tuple(xs) => {
            let fields = match ty {
                Type::Any => vec![Type::Any; xs.len()],
                Type::Tuple(fields) if fields.len() == xs.len() => fields.clone(),
                _ => return Type::None,
            };
            let bound: Vec<_> = xs
                .iter()
                .zip(fields)
                .map(|(x, t)| bind(x, &t, env))
                .collect();
            if bound.contains(&Type::None) {
                Type::None
            } else {
                Type::Tuple(bound)
            }
        }
        Expr::Cons(head, tail) => {
            let elem = match ty {
                Type::Any => Type::Any,
                Type::List(elem) => *elem.clone(),
                _ => return Type::None,
            };
            let h = bind(head, &elem, env);
            let t = bind(tail, &Type::List(Box::new(elem)), env);
            if h == Type::None || t == Type::None {
                Type::None
            } else {
                Type::List(Box::new(h))
            }
        }
        Expr::Match(a, b) => {
            let t = bind(b, ty, env);
            bind(a, &t, env)
        }
        Expr::Call(..) => Type::None,
    }
}

fn pattern_type(pattern: &Expr, env: &Env) -> Type {
    match pattern {
        Expr::Var(n) => env.get(n).cloned().unwrap_or(Type::Any),
        Expr::Literal(t) => t.clone(),
        Expr::Tuple(xs) => Type::Tuple(xs.iter().map(|x| pattern_type(x, env)).collect()),
        Expr::Cons(x, _) => Type::List(Box::new(pattern_type(x, env))),
        Expr::Match(a, _) => pattern_type(a, env),
        Expr::Call(..) => Type::None,
    }
}

struct Evaluation<'a> {
    program: &'a Program,
    summaries: &'a [Signature],
    function: &'a Function,
    warnings: BTreeSet<Warning>,
}

impl Evaluation<'_> {
    fn warning(&mut self, line: u64, category: &str) {
        let Key(m, f, a) = &self.function.key;
        self.warnings.insert(Warning {
            module: m.clone(),
            function: f.clone(),
            arity: *a,
            line,
            category: category.into(),
        });
    }

    fn eval(&mut self, expr: &Expr, env: &mut Env) -> Type {
        match expr {
            Expr::Var(n) => env.get(n).cloned().unwrap_or(Type::Any),
            Expr::Literal(t) => t.clone(),
            Expr::Tuple(xs) => {
                let ts: Vec<_> = xs.iter().map(|x| self.eval(x, env)).collect();
                if ts.contains(&Type::None) {
                    Type::None
                } else {
                    Type::Tuple(ts)
                }
            }
            Expr::Cons(h, t) => {
                let h = self.eval(h, env);
                let t = self.eval(t, env);
                if h == Type::None || t == Type::None {
                    return Type::None;
                }
                match t {
                    Type::List(elem) => Type::List(Box::new(h.union(*elem))),
                    _ => Type::None,
                }
            }
            Expr::Match(p, x) => {
                let t = self.eval(x, env);
                bind(p, &t, env)
            }
            Expr::Call(key, args, line) => {
                let sig = self
                    .program
                    .index
                    .get(key)
                    .map(|i| self.summaries[*i].clone())
                    .or_else(|| builtin(key))
                    .expect("validated call");
                let mut actual = Vec::new();
                let mut bad = false;
                for (expr, expected) in args.iter().zip(&sig.args) {
                    let t = self.eval(expr, env);
                    if t == Type::None {
                        return Type::None;
                    }
                    let narrowed = t.intersect(expected);
                    bad |= narrowed == Type::None;
                    if matches!(expr, Expr::Var(_)) {
                        bind(expr, &narrowed, env);
                    }
                    actual.push(narrowed);
                }
                if bad {
                    self.warning(*line, "call");
                    return Type::None;
                }
                if key.0 == "erlang" {
                    match key.1.as_str() {
                        "+" | "-" | "*" => {
                            if actual.iter().all(|x| x == &Type::Integer) {
                                return Type::Integer;
                            }
                            if actual.contains(&Type::Float) {
                                return Type::Float;
                            }
                        }
                        "hd" => {
                            if let Type::List(t) = &actual[0] {
                                return *t.clone();
                            }
                        }
                        "tl" => return actual[0].clone(),
                        _ => {}
                    }
                }
                sig.ret
            }
        }
    }

    fn infer(&mut self) -> Signature {
        let mut result = Signature {
            args: vec![Type::None; self.function.key.2],
            ret: Type::None,
        };
        for clause in &self.function.clauses {
            let mut env = Env::new();
            let mut viable = true;
            for p in &clause.patterns {
                viable &= bind(p, &Type::Any, &mut env) != Type::None;
            }
            for guard in &clause.guards {
                if let Expr::Call(Key(module, name, _), args, _) = guard
                    && module == "erlang"
                    && let Some(t) = predicate(name)
                {
                    viable &= bind(&args[0], &t, &mut env) != Type::None;
                    continue;
                }
                viable &= self.eval(guard, &mut env) != Type::None;
            }
            if !viable {
                continue;
            }
            let mut ret = Type::None;
            for body in &clause.body {
                ret = self.eval(body, &mut env);
                if ret == Type::None {
                    break;
                }
            }
            if ret == Type::None {
                continue;
            }
            result.ret = result.ret.union(ret);
            for (i, p) in clause.patterns.iter().enumerate() {
                result.args[i] = result.args[i].clone().union(pattern_type(p, &env));
            }
        }
        result
    }
}

fn callees(expr: &Expr, output: &mut BTreeSet<Key>) {
    match expr {
        Expr::Call(key, xs, _) => {
            output.insert(key.clone());
            for x in xs {
                callees(x, output);
            }
        }
        Expr::Tuple(xs) => {
            for x in xs {
                callees(x, output);
            }
        }
        Expr::Cons(a, b) | Expr::Match(a, b) => {
            callees(a, output);
            callees(b, output);
        }
        _ => {}
    }
}

pub fn analyze(program: &Program, worklist: bool) -> Result<Output, String> {
    let n = program.functions.len();
    let mut callers = vec![BTreeSet::new(); n];
    for (i, f) in program.functions.iter().enumerate() {
        let mut calls = BTreeSet::new();
        for c in &f.clauses {
            for x in c.guards.iter().chain(&c.body) {
                callees(x, &mut calls);
            }
        }
        for key in calls {
            if let Some(j) = program.index.get(&key) {
                callers[*j].insert(i);
            } else if builtin(&key).is_none() {
                return Err(format!("unsupported external call {key:?}"));
            }
        }
    }
    let mut summaries: Vec<_> = program
        .functions
        .iter()
        .map(|f| Signature {
            args: vec![Type::Any; f.key.2],
            ret: Type::None,
        })
        .collect();
    let mut evaluations = 0;
    if worklist {
        let mut queue: VecDeque<_> = (0..n).collect();
        let mut queued = vec![true; n];
        while let Some(i) = queue.pop_front() {
            queued[i] = false;
            let mut eval = Evaluation {
                program,
                summaries: &summaries,
                function: &program.functions[i],
                warnings: BTreeSet::new(),
            };
            let next = eval.infer();
            evaluations += 1;
            if evaluations > n.max(1) * 1000 {
                return Err("inference did not converge".into());
            }
            if summaries[i] != next {
                summaries[i] = next;
                for &caller in &callers[i] {
                    if !queued[caller] {
                        queue.push_back(caller);
                        queued[caller] = true;
                    }
                }
            }
        }
    } else {
        loop {
            let mut changed = false;
            for (i, f) in program.functions.iter().enumerate() {
                let mut eval = Evaluation {
                    program,
                    summaries: &summaries,
                    function: f,
                    warnings: BTreeSet::new(),
                };
                let next = eval.infer();
                evaluations += 1;
                changed |= summaries[i] != next;
                summaries[i] = next;
            }
            if !changed {
                break;
            }
            if evaluations > n.max(1) * 1000 {
                return Err("inference did not converge".into());
            }
        }
    }
    let mut warnings = BTreeSet::new();
    let mut signatures = BTreeMap::new();
    for (i, f) in program.functions.iter().enumerate() {
        let mut eval = Evaluation {
            program,
            summaries: &summaries,
            function: f,
            warnings: BTreeSet::new(),
        };
        eval.infer();
        let sig = &summaries[i];
        if sig.ret == Type::None {
            eval.warning(f.line, "no_return");
        }
        if let Some(c) = &f.contract
            && sig.ret != Type::None
            && (c.ret.intersect(&sig.ret) == Type::None
                || c.args
                    .iter()
                    .zip(&sig.args)
                    .any(|(a, b)| a.intersect(b) == Type::None))
        {
            eval.warning(c.line, "invalid_contract");
        }
        warnings.extend(eval.warnings);
        signatures.insert(format!("{}:{}/{}", f.key.0, f.key.1, f.key.2), sig.clone());
    }
    Ok(Output {
        warnings: warnings.into_iter().collect(),
        signatures,
        evaluations,
    })
}

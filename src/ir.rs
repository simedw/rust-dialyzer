use crate::types::Type;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Key(pub String, pub String, pub usize);

#[derive(Clone, Debug)]
pub enum Expr {
    Var(String),
    Literal(Type),
    Tuple(Vec<Expr>),
    Cons(Box<Expr>, Box<Expr>),
    Call(Key, Vec<Expr>, u64),
    Match(Box<Expr>, Box<Expr>),
}

#[derive(Clone, Debug)]
pub struct Clause {
    pub patterns: Vec<Expr>,
    pub guards: Vec<Expr>,
    pub body: Vec<Expr>,
}

#[derive(Clone, Debug)]
pub struct Contract {
    pub args: Vec<Type>,
    pub ret: Type,
    pub line: u64,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub key: Key,
    pub line: u64,
    pub clauses: Vec<Clause>,
    pub contract: Option<Contract>,
}

pub struct Program {
    pub functions: Vec<Function>,
    pub index: BTreeMap<Key, usize>,
}

#[derive(Deserialize)]
struct Export {
    version: u32,
    modules: Vec<Module>,
}

#[derive(Deserialize)]
struct Module {
    name: String,
    forms: Vec<Value>,
}

fn array(v: &Value) -> Result<&[Value]> {
    v.as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("expected array: {v}"))
}
fn at(v: &Value, n: usize) -> Result<&Value> {
    array(v)?
        .get(n)
        .ok_or_else(|| format!("missing field {n}: {v}"))
}
fn string(v: &Value) -> Result<String> {
    v.as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("expected string: {v}"))
}
fn tag(v: &Value) -> Result<String> {
    string(at(v, 0)?)
}
fn line(v: &Value) -> u64 {
    v.as_u64()
        .or_else(|| v.get(0).and_then(Value::as_u64))
        .unwrap_or(0)
}
fn node_line(v: &Value) -> u64 {
    v.get(1).map(line).unwrap_or(0)
}
fn parse_many(v: &Value, module: &str) -> Result<Vec<Expr>> {
    array(v)?.iter().map(|v| expr(v, module)).collect()
}

fn expr(v: &Value, module: &str) -> Result<Expr> {
    Ok(match tag(v)?.as_str() {
        "var" => Expr::Var(string(at(v, 2)?)?),
        "integer" | "char" => Expr::Literal(Type::Integer),
        "float" => Expr::Literal(Type::Float),
        "atom" => Expr::Literal(Type::Atom(Some(string(at(v, 2)?)?))),
        "nil" => Expr::Literal(Type::List(Box::new(Type::None))),
        "string" => Expr::Literal(Type::List(Box::new(Type::Integer))),
        "bin" => {
            for item in array(at(v, 2)?)? {
                if tag(at(item, 2)?)? != "string" {
                    return Err(format!("only literal binaries supported: {v}"));
                }
            }
            Expr::Literal(Type::Binary)
        }
        "tuple" => Expr::Tuple(parse_many(at(v, 2)?, module)?),
        "cons" => Expr::Cons(
            Box::new(expr(at(v, 2)?, module)?),
            Box::new(expr(at(v, 3)?, module)?),
        ),
        "match" => Expr::Match(
            Box::new(expr(at(v, 2)?, module)?),
            Box::new(expr(at(v, 3)?, module)?),
        ),
        "op" => {
            let args = array(v)?
                .get(3..)
                .ok_or("malformed operator")?
                .iter()
                .map(|v| expr(v, module))
                .collect::<Result<Vec<_>>>()?;
            Expr::Call(
                Key("erlang".into(), string(at(v, 2)?)?, args.len()),
                args,
                node_line(v),
            )
        }
        "call" => {
            let callee = at(v, 2)?;
            let (m, f) = match tag(callee)?.as_str() {
                "atom" => (module.to_owned(), string(at(callee, 2)?)?),
                "remote" => {
                    if tag(at(callee, 2)?)? != "atom" || tag(at(callee, 3)?)? != "atom" {
                        return Err(format!("dynamic remote call unsupported: {v}"));
                    }
                    (
                        string(at(at(callee, 2)?, 2)?)?,
                        string(at(at(callee, 3)?, 2)?)?,
                    )
                }
                _ => return Err(format!("dynamic call unsupported: {v}")),
            };
            let args = parse_many(at(v, 3)?, module)?;
            Expr::Call(Key(m, f, args.len()), args, node_line(v))
        }
        kind => return Err(format!("unsupported expression {kind}: {v}")),
    })
}

fn ty(v: &Value) -> Result<Type> {
    match tag(v)?.as_str() {
        "atom" => Ok(Type::Atom(Some(string(at(v, 2)?)?))),
        "integer" => Ok(Type::Integer),
        "ann_type" => ty(at(at(v, 2)?, 1)?),
        "type" => {
            let name = string(at(v, 2)?)?;
            let args = at(v, 3)?;
            Ok(match name.as_str() {
                "term" | "any" => Type::Any,
                "none" | "no_return" => Type::None,
                "integer" | "non_neg_integer" | "pos_integer" | "neg_integer" => Type::Integer,
                "float" => Type::Float,
                "number" => Type::number(),
                "atom" => Type::Atom(None),
                "boolean" => {
                    Type::Atom(Some("true".into())).union(Type::Atom(Some("false".into())))
                }
                "binary" | "bitstring" => Type::Binary,
                "list" | "nonempty_list" => Type::List(Box::new(if array(args)?.is_empty() {
                    Type::Any
                } else {
                    ty(at(args, 0)?)?
                })),
                "nil" => Type::List(Box::new(Type::None)),
                "tuple" => Type::Tuple(array(args)?.iter().map(ty).collect::<Result<Vec<_>>>()?),
                "union" => array(args)?
                    .iter()
                    .map(ty)
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .fold(Type::None, Type::union),
                _ => return Err(format!("unsupported type {name}: {v}")),
            })
        }
        _ => Err(format!("unsupported type: {v}")),
    }
}

impl Program {
    pub fn parse(input: &str) -> Result<Self> {
        let export: Export = serde_json::from_str(input).map_err(|e| e.to_string())?;
        if export.version != 1 {
            return Err("unsupported export version".into());
        }
        let mut functions = Vec::new();
        for module in export.modules {
            let mut contracts = BTreeMap::new();
            for form in &module.forms {
                if tag(form)? == "attribute" && at(form, 2)?.as_str() == Some("spec") {
                    let spec = at(form, 3)?;
                    let key = at(spec, 0)?;
                    let name = string(at(key, 0)?)?;
                    if name == "__info__" {
                        continue;
                    }
                    let alternatives = array(at(spec, 1)?)?;
                    if alternatives.len() != 1 {
                        return Err("overloaded specs unsupported".into());
                    }
                    let fun = at(&alternatives[0], 3)?;
                    let product = at(at(fun, 0)?, 3)?;
                    contracts.insert(
                        (
                            name,
                            at(key, 1)?.as_u64().ok_or("invalid spec arity")? as usize,
                        ),
                        Contract {
                            args: array(product)?.iter().map(ty).collect::<Result<Vec<_>>>()?,
                            ret: ty(at(fun, 1)?)?,
                            line: node_line(form),
                        },
                    );
                }
            }
            for form in &module.forms {
                if tag(form)? != "function" {
                    continue;
                }
                let name = string(at(form, 2)?)?;
                if name == "__info__" || name.starts_with("MACRO-") || name == "module_info" {
                    continue;
                }
                let arity = at(form, 3)?.as_u64().ok_or("invalid arity")? as usize;
                let mut clauses = Vec::new();
                for c in array(at(form, 4)?)? {
                    let guard_groups = array(at(c, 3)?)?;
                    if guard_groups.len() > 1 {
                        return Err("disjunctive guards unsupported".into());
                    }
                    clauses.push(Clause {
                        patterns: parse_many(at(c, 2)?, &module.name)?,
                        guards: if guard_groups.is_empty() {
                            vec![]
                        } else {
                            parse_many(&guard_groups[0], &module.name)?
                        },
                        body: parse_many(at(c, 4)?, &module.name)?,
                    });
                }
                if clauses.is_empty()
                    || clauses
                        .iter()
                        .any(|c| c.patterns.len() != arity || c.body.is_empty())
                {
                    return Err(format!("malformed clauses for {name}/{arity}"));
                }
                let contract = contracts.remove(&(name.clone(), arity));
                if contract.as_ref().is_some_and(|c| c.args.len() != arity) {
                    return Err(format!("contract arity mismatch for {name}/{arity}"));
                }
                functions.push(Function {
                    key: Key(module.name.clone(), name.clone(), arity),
                    line: node_line(form),
                    clauses,
                    contract,
                });
            }
        }
        if functions.is_empty() {
            return Err("no runtime functions to analyze".into());
        }
        let index: BTreeMap<_, _> = functions
            .iter()
            .enumerate()
            .map(|(i, f)| (f.key.clone(), i))
            .collect();
        if index.len() != functions.len() {
            return Err("duplicate function identity".into());
        }
        Ok(Self { functions, index })
    }
}

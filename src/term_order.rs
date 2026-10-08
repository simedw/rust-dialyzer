use crate::core::{ErlangTerm, ErlangTerms, TermId};
use num_bigint::BigInt;
use num_traits::FromPrimitive;
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureId(pub usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RuntimeKind {
    Reference,
    Function,
    Port,
    Pid,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RuntimeIdentity {
    pub scope: String,
    pub object: String,
}
#[derive(Clone, Debug)]
pub enum CapturedFunction {
    Local {
        module: String,
        new_index: u32,
        old_uniq: u32,
        arity: usize,
        name: String,
        environment: Vec<CaptureId>,
    },
    External {
        module: String,
        function: String,
        arity: usize,
    },
}
#[derive(Clone, Debug)]
pub enum CaptureTerm {
    Integer(String),
    Float(u64),
    Atom(String),
    Tuple(Vec<CaptureId>),
    Map(Vec<(CaptureId, CaptureId)>),
    Nil,
    List {
        items: Vec<CaptureId>,
        tail: CaptureId,
    },
    Binary(Vec<u8>),
    Bitstring {
        bytes: Vec<u8>,
        bits: u64,
    },
    Function(CapturedFunction),
    Runtime {
        kind: RuntimeKind,
        identity: RuntimeIdentity,
    },
}
#[derive(Clone, Debug)]
pub struct CaptureEnvironment {
    arena: std::sync::Arc<TermOrderArena>,
    root: CaptureId,
    count: usize,
}
impl CaptureEnvironment {
    pub fn from_native_values(values: &[crate::lattice::NativeValue]) -> Result<Self, String> {
        use crate::lattice::NativeValue;
        enum Frame<'a> {
            Visit(&'a NativeValue),
            Tuple(usize),
            List(usize),
        }
        let mut arena = TermOrderArena {
            nodes: Vec::new(),
            source_ids: Vec::new(),
            canonical_ids: Vec::new(),
            intern: HashMap::new(),
        };
        let nil = arena.append_capture(Node::Nil);
        arena.source_ids.push(nil);
        let mut atoms = HashMap::new();
        let mut integers = HashMap::new();
        let mut built = Vec::new();
        for value in values {
            let mut todo = vec![Frame::Visit(value)];
            while let Some(frame) = todo.pop() {
                let id = match frame {
                    Frame::Visit(NativeValue::Tuple(items)) => {
                        todo.push(Frame::Tuple(items.len()));
                        todo.extend(items.iter().rev().map(Frame::Visit));
                        continue;
                    }
                    Frame::Visit(NativeValue::List(items)) => {
                        todo.push(Frame::List(items.len()));
                        todo.extend(items.iter().rev().map(Frame::Visit));
                        continue;
                    }
                    Frame::Visit(NativeValue::Integer(value)) => {
                        if let Some(id) = integers.get(value) {
                            *id
                        } else {
                            let id = arena.append_capture(Node::Integer(value.clone()));
                            integers.insert(value, id);
                            id
                        }
                    }
                    Frame::Visit(NativeValue::Atom(value)) => {
                        if let Some(id) = atoms.get(value.as_str()) {
                            *id
                        } else {
                            let id = arena.append_capture(Node::Atom(value.clone()));
                            atoms.insert(value.as_str(), id);
                            id
                        }
                    }
                    Frame::Visit(NativeValue::Nil) => nil,
                    Frame::Tuple(count) => {
                        let items = built.split_off(built.len() - count);
                        arena.append_capture(Node::Tuple(items))
                    }
                    Frame::List(count) => {
                        let items = built.split_off(built.len() - count);
                        items.into_iter().rev().fold(nil, |tail, head| {
                            arena.append_capture(Node::Cons(head, tail))
                        })
                    }
                };
                arena.source_ids.push(id);
                built.push(id);
            }
        }
        let id = built.into_iter().rev().fold(nil, |tail, head| {
            arena.append_capture(Node::Cons(head, tail))
        });
        let root = CaptureId(arena.source_ids.len());
        arena.source_ids.push(id);
        Self::new(std::sync::Arc::new(arena), root)
    }

    pub fn append_to(&self, terms: &mut Vec<CaptureTerm>) -> CaptureId {
        let offset = terms.len();
        for node in &self.arena.nodes {
            let id = |n: Id| CaptureId(offset + n);
            terms.push(match node {
                Node::Integer(n) => CaptureTerm::Integer(n.to_string()),
                Node::Float(n) => CaptureTerm::Float(n.to_bits()),
                Node::Atom(a) => CaptureTerm::Atom(a.clone()),
                Node::Nil => CaptureTerm::Nil,
                Node::Tuple(xs) => CaptureTerm::Tuple(xs.iter().map(|n| id(*n)).collect()),
                Node::Map(pairs) | Node::DeferredMap(pairs) => {
                    CaptureTerm::Map(pairs.iter().map(|(k, v)| (id(*k), id(*v))).collect())
                }
                Node::Cons(head, tail) => CaptureTerm::List {
                    items: vec![id(*head)],
                    tail: id(*tail),
                },
                Node::Bits(bytes, bits) => CaptureTerm::Bitstring {
                    bytes: bytes.clone(),
                    bits: *bits,
                },
                Node::Runtime(kind, identity) => CaptureTerm::Runtime {
                    kind: *kind,
                    identity: identity.clone(),
                },
                Node::Function(CapturedFunction::Local {
                    module,
                    new_index,
                    old_uniq,
                    arity,
                    name,
                    environment,
                }) => CaptureTerm::Function(CapturedFunction::Local {
                    module: module.clone(),
                    new_index: *new_index,
                    old_uniq: *old_uniq,
                    arity: *arity,
                    name: name.clone(),
                    environment: environment.iter().map(|n| id(n.0)).collect(),
                }),
                Node::Function(CapturedFunction::External {
                    module,
                    function,
                    arity,
                }) => CaptureTerm::Function(CapturedFunction::External {
                    module: module.clone(),
                    function: function.clone(),
                    arity: *arity,
                }),
            });
        }
        CaptureId(offset + self.arena.source_ids[self.root.0])
    }
    pub fn append_values_to(&self, terms: &mut Vec<CaptureTerm>) -> Vec<CaptureId> {
        let mut root = self.append_to(terms);
        let mut values = Vec::with_capacity(self.count);
        loop {
            match &terms[root.0] {
                CaptureTerm::Nil => return values,
                CaptureTerm::List { items, tail } => {
                    values.extend(items.iter().copied());
                    root = *tail;
                }
                _ => unreachable!("validated proper capture environment"),
            }
        }
    }
    pub fn to_value(&self) -> serde_json::Value {
        use serde_json::json;
        let mut terms = Vec::new();
        let root = self.append_to(&mut terms);
        let ids = |xs: &[CaptureId]| xs.iter().map(|id| id.0).collect::<Vec<_>>();
        let terms = terms.iter().map(|term| match term {
            CaptureTerm::Integer(v) => json!({"kind":"integer","value":v}),
            CaptureTerm::Float(v) => json!({"kind":"float","value":format!("{v:016X}")}),
            CaptureTerm::Atom(v) => json!({"kind":"atom","value":v}),
            CaptureTerm::Nil => json!({"kind":"nil"}),
            CaptureTerm::Tuple(xs) => json!({"kind":"tuple","items":ids(xs)}),
            CaptureTerm::List{items,tail} => json!({"kind":"list","items":ids(items),"tail":tail.0}),
            CaptureTerm::Map(xs) => json!({"kind":"map","entries":xs.iter().map(|(k,v)|[k.0,v.0]).collect::<Vec<_>>()}),
            CaptureTerm::Binary(v) => json!({"kind":"binary","value":crate::type_exports::encode_base64(v)}),
            CaptureTerm::Bitstring{bytes,bits} => json!({"kind":"bitstring","bytes":crate::type_exports::encode_base64(bytes),"bits":bits}),
            CaptureTerm::Function(CapturedFunction::Local{module,new_index,old_uniq,arity,name,environment}) => json!({"kind":"function","flavor":"local","module":module,"new_index":new_index,"uniq":old_uniq,"arity":arity,"name":name,"environment":ids(environment)}),
            CaptureTerm::Function(CapturedFunction::External{module,function,arity}) => json!({"kind":"function","flavor":"external","module":module,"name":function,"arity":arity}),
            CaptureTerm::Runtime{kind,identity} => json!({"kind":"runtime","runtime_kind":match kind { RuntimeKind::Reference=>"reference",RuntimeKind::Function=>"function",RuntimeKind::Port=>"port",RuntimeKind::Pid=>"pid"},"scope":identity.scope,"object":identity.object}),
        }).collect::<Vec<_>>();
        json!({"root":root.0,"terms":terms})
    }
    pub fn len(&self) -> usize {
        self.count
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    pub fn new(arena: std::sync::Arc<TermOrderArena>, root: CaptureId) -> Result<Self, String> {
        let count = arena.captured_environment_size(root)?;
        Ok(Self { arena, root, count })
    }
    pub fn literal_roots(source: &ErlangTerms, roots: &[TermId]) -> Result<Self, String> {
        let mut arena = TermOrderArena::new(source)?;
        let values = roots
            .iter()
            .map(|root| {
                arena
                    .source_ids
                    .get(root.0)
                    .copied()
                    .ok_or_else(|| "literal capture root outside source arena".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut tail = arena.nodes.len();
        arena.nodes.push(Node::Nil);
        for head in values.into_iter().rev() {
            let node = arena.nodes.len();
            arena.nodes.push(Node::Cons(head, tail));
            tail = node;
        }
        let root = CaptureId(arena.source_ids.len());
        arena.source_ids.push(tail);
        Self::new(std::sync::Arc::new(arena), root)
    }
    pub fn literals(source: &ErlangTerms) -> Result<Self, String> {
        Self::new(
            std::sync::Arc::new(TermOrderArena::new(source)?),
            CaptureId(source.root.0),
        )
    }
    pub fn compare(&self, other: &Self) -> Result<Ordering, String> {
        self.arena
            .compare_captures(self.root, &other.arena, other.root)
    }
    pub fn parse(value: &serde_json::Value) -> Result<Self, String> {
        let (arena, root) = parse_captures(value)?;
        Self::new(std::sync::Arc::new(arena), root)
    }
}
pub fn parse_captures(value: &serde_json::Value) -> Result<(TermOrderArena, CaptureId), String> {
    use serde_json::json;
    let source = value["terms"].as_array().ok_or("capture terms expected")?;
    let id = |v: &serde_json::Value| {
        v.as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .map(CaptureId)
            .ok_or_else(|| "capture ID expected".to_owned())
    };
    let mut terms = Vec::new();
    for v in source {
        let ids = |name: &str| {
            v[name]
                .as_array()
                .ok_or_else(|| format!("capture {name}: array expected"))?
                .iter()
                .map(id)
                .collect::<Result<Vec<_>, _>>()
        };
        let text = |name: &str| {
            v[name]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("capture {name}: atom expected"))
        };
        let arity = || {
            v["arity"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| "capture function arity expected".to_owned())
        };
        terms.push(match v["kind"].as_str().ok_or("capture kind expected")? {
            "tuple" => CaptureTerm::Tuple(ids("items")?),
            "map" => CaptureTerm::Map(
                v["entries"]
                    .as_array()
                    .ok_or("capture map entries expected")?
                    .iter()
                    .map(|p| {
                        let p = p
                            .as_array()
                            .filter(|p| p.len() == 2)
                            .ok_or("capture map pair expected")?;
                        Ok((id(&p[0])?, id(&p[1])?))
                    })
                    .collect::<Result<_, String>>()?,
            ),
            "list" => CaptureTerm::List {
                items: ids("items")?,
                tail: id(&v["tail"])?,
            },
            "function" => CaptureTerm::Function(match v["flavor"].as_str() {
                Some("local") => CapturedFunction::Local {
                    module: text("module")?,
                    new_index: v["new_index"]
                        .as_u64()
                        .and_then(|n| u32::try_from(n).ok())
                        .ok_or("capture new_index expected")?,
                    old_uniq: v["uniq"]
                        .as_u64()
                        .and_then(|n| u32::try_from(n).ok())
                        .ok_or("capture uniq expected")?,
                    arity: arity()?,
                    name: text("name")?,
                    environment: ids("environment")?,
                },
                Some("external") => CapturedFunction::External {
                    module: text("module")?,
                    function: text("name")?,
                    arity: arity()?,
                },
                _ => return Err("capture function flavor expected".into()),
            }),
            "runtime" => CaptureTerm::Runtime {
                kind: match v["runtime_kind"].as_str() {
                    Some("reference") => RuntimeKind::Reference,
                    Some("function") => RuntimeKind::Function,
                    Some("port") => RuntimeKind::Port,
                    Some("pid") => RuntimeKind::Pid,
                    _ => return Err("runtime kind expected".into()),
                },
                identity: RuntimeIdentity {
                    scope: text("scope")?,
                    object: text("object")?,
                },
            },
            _ => {
                let leaf = ErlangTerms::parse(&json!({"terms":[v],"root":0}))?;
                match &leaf.terms[0] {
                    ErlangTerm::Atom(n) => CaptureTerm::Atom(n.clone()),
                    ErlangTerm::Integer(n) => CaptureTerm::Integer(n.clone()),
                    ErlangTerm::Float(n) => CaptureTerm::Float(*n),
                    ErlangTerm::Nil => CaptureTerm::Nil,
                    ErlangTerm::Binary(bytes) => CaptureTerm::Binary(bytes.clone()),
                    ErlangTerm::Bitstring { bytes, bits } => CaptureTerm::Bitstring {
                        bytes: bytes.clone(),
                        bits: *bits,
                    },
                    _ => return Err("capture leaf expected".into()),
                }
            }
        });
    }
    let root = id(&value["root"])?;
    if root.0 >= terms.len() {
        return Err("capture root outside table".into());
    }
    Ok((TermOrderArena::from_captures(&terms)?, root))
}
type Id = usize;
#[derive(Clone, Debug)]
enum Node {
    Function(CapturedFunction),
    Runtime(RuntimeKind, RuntimeIdentity),
    Integer(BigInt),
    Float(f64),
    Atom(String),
    Tuple(Vec<Id>),
    Map(Vec<(Id, Id)>),
    DeferredMap(Vec<(Id, Id)>),
    Nil,
    Cons(Id, Id),
    Bits(Vec<u8>, u64),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum CanonicalKey {
    Integer(BigInt),
    Float(u64),
    Atom(String),
    Tuple(Vec<Id>),
    Map(Vec<(Id, Id)>),
    Nil,
    Cons(Id, Id),
    Bits(Vec<u8>, u64),
    LocalFunction(String, u32, u32, Vec<Id>),
    ExternalFunction(String, String, usize),
    Runtime(RuntimeKind, RuntimeIdentity),
}
/// A flat immutable comparison arena. Child IDs retain DAG sharing; lists use flat cons nodes.
#[derive(Clone, Debug)]
pub struct TermOrderArena {
    nodes: Vec<Node>,
    source_ids: Vec<Id>,
    canonical_ids: Vec<Id>,
    intern: HashMap<CanonicalKey, Id>,
}
impl TermOrderArena {
    fn append_capture(&mut self, node: Node) -> Id {
        let key = match &node {
            Node::Integer(n) => CanonicalKey::Integer(n.clone()),
            Node::Float(n) => CanonicalKey::Float(n.to_bits()),
            Node::Atom(n) => CanonicalKey::Atom(n.clone()),
            Node::Tuple(xs) => {
                CanonicalKey::Tuple(xs.iter().map(|x| self.canonical_ids[*x]).collect())
            }
            Node::Map(xs) | Node::DeferredMap(xs) => {
                let mut pairs = xs
                    .iter()
                    .map(|(k, v)| (self.canonical_ids[*k], self.canonical_ids[*v]))
                    .collect::<Vec<_>>();
                pairs.sort_unstable();
                CanonicalKey::Map(pairs)
            }
            Node::Nil => CanonicalKey::Nil,
            Node::Cons(a, b) => CanonicalKey::Cons(self.canonical_ids[*a], self.canonical_ids[*b]),
            Node::Bits(bytes, bits) => {
                let mut normalized = bytes[..(*bits / 8) as usize].to_vec();
                if bits % 8 != 0 {
                    normalized.push(bytes[(*bits / 8) as usize] & (!0u8 << (8 - bits % 8)));
                }
                CanonicalKey::Bits(normalized, *bits)
            }
            Node::Runtime(k, id) => CanonicalKey::Runtime(*k, id.clone()),
            Node::Function(CapturedFunction::Local {
                module,
                new_index,
                old_uniq,
                environment,
                ..
            }) => CanonicalKey::LocalFunction(
                module.clone(),
                *new_index,
                *old_uniq,
                environment
                    .iter()
                    .map(|x| self.canonical_ids[x.0])
                    .collect(),
            ),
            Node::Function(CapturedFunction::External {
                module,
                function,
                arity,
            }) => CanonicalKey::ExternalFunction(module.clone(), function.clone(), *arity),
        };
        if let Some(id) = self.intern.get(&key) {
            return *id;
        }
        let id = self.nodes.len();
        self.intern.insert(key, id);
        self.canonical_ids.push(id);
        self.nodes.push(node);
        id
    }
    pub fn from_captures(source: &[CaptureTerm]) -> Result<Self, String> {
        let mut out = Self {
            nodes: Vec::new(),
            source_ids: Vec::with_capacity(source.len()),
            canonical_ids: Vec::new(),
            intern: HashMap::new(),
        };
        let mut atom_ids: HashMap<&str, Id> = HashMap::new();
        let mut integer_ids: HashMap<&str, Id> = HashMap::new();
        let mut nil_id = None;
        for term in source {
            let existing = match term {
                CaptureTerm::Atom(value) => atom_ids.get(value.as_str()).copied(),
                CaptureTerm::Integer(value) => integer_ids.get(value.as_str()).copied(),
                CaptureTerm::Nil => nil_id,
                _ => None,
            };
            if let Some(id) = existing {
                out.source_ids.push(id);
                continue;
            }
            let at = |id: &CaptureId| {
                out.source_ids
                    .get(id.0)
                    .copied()
                    .ok_or_else(|| "capture children must precede parents".to_owned())
            };
            let node = match term {
                CaptureTerm::Integer(n) => {
                    Node::Integer(n.parse().map_err(|_| "invalid capture integer")?)
                }
                CaptureTerm::Float(bits) => {
                    let n = f64::from_bits(*bits);
                    if !n.is_finite() {
                        return Err("nonfinite capture float".into());
                    }
                    Node::Float(n)
                }
                CaptureTerm::Atom(n) => Node::Atom(n.clone()),
                CaptureTerm::Tuple(xs) => Node::Tuple(xs.iter().map(at).collect::<Result<_, _>>()?),
                CaptureTerm::Map(xs) => {
                    let mut unique: Vec<(Id, Id)> = Vec::new();
                    let mut seen = HashMap::new();
                    for (key, value) in xs {
                        let (key, value) = (at(key)?, at(value)?);
                        let canonical = out.canonical_ids[key];
                        if let Some(index) = seen.get(&canonical) {
                            unique[*index] = (key, value);
                        } else {
                            seen.insert(canonical, unique.len());
                            unique.push((key, value));
                        }
                    }
                    Node::DeferredMap(unique)
                }
                CaptureTerm::Nil => Node::Nil,
                CaptureTerm::List { items, tail } => {
                    let mut tail = at(tail)?;
                    let items = items.iter().map(at).collect::<Result<Vec<_>, _>>()?;
                    for head in items.into_iter().rev() {
                        tail = out.append_capture(Node::Cons(head, tail));
                    }
                    out.source_ids.push(tail);
                    continue;
                }
                CaptureTerm::Binary(bytes) => Node::Bits(bytes.clone(), bytes.len() as u64 * 8),
                CaptureTerm::Bitstring { bytes, bits } => {
                    if *bits > bytes.len() as u64 * 8 {
                        return Err("capture bitstring outside bytes".into());
                    }
                    Node::Bits(bytes.clone(), *bits)
                }
                CaptureTerm::Function(fun) => {
                    let fun = match fun {
                        CapturedFunction::Local {
                            module,
                            new_index,
                            old_uniq,
                            arity,
                            name,
                            environment,
                        } => {
                            if *arity > 255 {
                                return Err(
                                    "capture local function arity outside native range".into()
                                );
                            }
                            CapturedFunction::Local {
                                module: module.clone(),
                                new_index: *new_index,
                                old_uniq: *old_uniq,
                                arity: *arity,
                                name: name.clone(),
                                environment: environment
                                    .iter()
                                    .map(|id| at(id).map(CaptureId))
                                    .collect::<Result<_, _>>()?,
                            }
                        }
                        CapturedFunction::External {
                            module,
                            function,
                            arity,
                        } => {
                            if *arity > 255 {
                                return Err(
                                    "capture external function arity outside native range".into()
                                );
                            }
                            CapturedFunction::External {
                                module: module.clone(),
                                function: function.clone(),
                                arity: *arity,
                            }
                        }
                    };
                    Node::Function(fun)
                }
                CaptureTerm::Runtime { kind, identity } => {
                    if identity.scope.is_empty() || identity.object.is_empty() {
                        return Err("opaque runtime identities require a scope and object".into());
                    }
                    Node::Runtime(*kind, identity.clone())
                }
            };
            let id = out.append_capture(node);
            match term {
                CaptureTerm::Atom(value) => { atom_ids.insert(value.as_str(), id); }
                CaptureTerm::Integer(value) => { integer_ids.insert(value.as_str(), id); }
                CaptureTerm::Nil => nil_id = Some(id),
                _ => {}
            }
            out.source_ids.push(id);
        }
        Ok(out)
    }
    pub fn compare_captures(
        &self,
        a: CaptureId,
        other: &Self,
        b: CaptureId,
    ) -> Result<Ordering, String> {
        self.compare_with(TermId(a.0), other, TermId(b.0), false)
    }
    pub fn compare_capture_keys(
        &self,
        a: CaptureId,
        other: &Self,
        b: CaptureId,
    ) -> Result<Ordering, String> {
        self.compare_with(TermId(a.0), other, TermId(b.0), true)
    }
    pub fn captured_environment_size(&self, root: CaptureId) -> Result<usize, String> {
        let mut id = *self
            .source_ids
            .get(root.0)
            .ok_or("capture root outside arena")?;
        let mut count = 0;
        loop {
            match self.nodes.get(id).ok_or("capture node outside arena")? {
                Node::Nil => return Ok(count),
                Node::Cons(_, tail) => {
                    count += 1;
                    id = *tail
                }
                _ => return Err("captured environment must be a proper list".into()),
            }
        }
    }
    pub fn new(source: &ErlangTerms) -> Result<Self, String> {
        let mut out = Self {
            nodes: Vec::new(),
            source_ids: Vec::with_capacity(source.terms.len()),
            canonical_ids: Vec::new(),
            intern: HashMap::new(),
        };
        for term in &source.terms {
            let id = out.append_core_term(term)?;
            out.source_ids.push(id);
        }
        Ok(out)
    }
    fn append_core_term(&mut self, term: &ErlangTerm) -> Result<Id, String> {
        let at = |id: &TermId| {
            self.source_ids
                .get(id.0)
                .copied()
                .ok_or_else(|| "term children must precede their parents".to_owned())
        };
        let node = match term {
            ErlangTerm::Integer(n) => Node::Integer(n.parse().map_err(|_| "invalid integer")?),
            ErlangTerm::Float(bits) => {
                let n = f64::from_bits(*bits);
                if !n.is_finite() {
                    return Err("nonfinite float is not an Erlang term".into());
                }
                Node::Float(n)
            }
            ErlangTerm::ExternalFun {
                module,
                function,
                arity,
            } => Node::Function(CapturedFunction::External {
                module: module.clone(),
                function: function.clone(),
                arity: usize::from(*arity),
            }),
            ErlangTerm::Atom(n) => Node::Atom(n.clone()),
            ErlangTerm::Tuple(xs) => Node::Tuple(xs.iter().map(at).collect::<Result<_, _>>()?),
            ErlangTerm::Map(xs) => Node::Map(
                self.canonical_map_pairs(xs)?
                    .iter()
                    .map(|(key, value)| Ok((at(key)?, at(value)?)))
                    .collect::<Result<_, String>>()?,
            ),
            ErlangTerm::Nil => Node::Nil,
            ErlangTerm::List { items, tail } => {
                let mut tail = at(tail)?;
                let items = items.iter().map(at).collect::<Result<Vec<_>, _>>()?;
                for head in items.into_iter().rev() {
                    let id = self.nodes.len();
                    self.nodes.push(Node::Cons(head, tail));
                    tail = id;
                }
                return Ok(tail);
            }
            ErlangTerm::Binary(bytes) => Node::Bits(bytes.clone(), bytes.len() as u64 * 8),
            ErlangTerm::Bitstring { bytes, bits } => {
                if *bits > bytes.len() as u64 * 8 {
                    return Err("bitstring length outside bytes".into());
                }
                Node::Bits(bytes.clone(), *bits)
            }
        };
        let id = self.nodes.len();
        self.nodes.push(node);
        Ok(id)
    }
    pub fn compare(&self, a: TermId, other: &Self, b: TermId) -> Result<Ordering, String> {
        self.compare_with(a, other, b, false)
    }
    pub fn compare_map_keys(&self, a: TermId, other: &Self, b: TermId) -> Result<Ordering, String> {
        self.compare_with(a, other, b, true)
    }
    pub fn canonical_map_pairs(
        &self,
        pairs: &[(TermId, TermId)],
    ) -> Result<Vec<(TermId, TermId)>, String> {
        let mut unique: Vec<(TermId, TermId)> = Vec::with_capacity(pairs.len());
        for (key, value) in pairs {
            if self.source_ids.get(value.0).is_none() {
                return Err("map value outside term arena".into());
            }
            let mut lo = 0;
            let mut hi = unique.len();
            while lo < hi {
                let mid = (lo + hi) / 2;
                if self.compare_map_keys(unique[mid].0, self, *key)?.is_lt() {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            if lo < unique.len() && self.compare_map_keys(unique[lo].0, self, *key)?.is_eq() {
                unique[lo] = (*key, *value);
            } else {
                // Validate the first key too, when no comparison was necessary.
                self.compare_map_keys(*key, self, *key)?;
                unique.insert(lo, (*key, *value));
            }
        }
        Ok(unique)
    }
    fn compare_with(
        &self,
        a: TermId,
        other: &Self,
        b: TermId,
        exact: bool,
    ) -> Result<Ordering, String> {
        let a = *self.source_ids.get(a.0).ok_or("term ID outside arena")?;
        let b = *other.source_ids.get(b.0).ok_or("term ID outside arena")?;
        compare(self, a, other, b, exact)
    }
}
pub(crate) struct RootTermOrder {
    order: TermOrderArena,
    source_ids: std::collections::BTreeMap<TermId, TermId>,
}
impl RootTermOrder {
    pub(crate) fn new(source: &ErlangTerms, root: TermId) -> Result<Self, String> {
        let mut reachable = std::collections::BTreeSet::new();
        let mut work = vec![root];
        while let Some(id) = work.pop() {
            if !reachable.insert(id) {
                continue;
            }
            match source.terms.get(id.0).ok_or("term ID outside arena")? {
                ErlangTerm::Tuple(items) => work.extend(items),
                ErlangTerm::List { items, tail } => {
                    work.extend(items);
                    work.push(*tail);
                }
                ErlangTerm::Map(pairs) => {
                    for (key, value) in pairs {
                        work.extend([*key, *value]);
                    }
                }
                _ => {}
            }
        }
        let mut order = TermOrderArena {
            nodes: Vec::new(),
            source_ids: Vec::new(),
            canonical_ids: Vec::new(),
            intern: HashMap::new(),
        };
        let mut source_ids = std::collections::BTreeMap::new();
        for id in reachable {
            let at = |id: &TermId| {
                source_ids
                    .get(id)
                    .copied()
                    .ok_or("term children must precede their parents".to_owned())
            };
            let term = match &source.terms[id.0] {
                ErlangTerm::Tuple(items) => {
                    ErlangTerm::Tuple(items.iter().map(at).collect::<Result<_, _>>()?)
                }
                ErlangTerm::List { items, tail } => ErlangTerm::List {
                    items: items.iter().map(at).collect::<Result<_, _>>()?,
                    tail: at(tail)?,
                },
                ErlangTerm::Map(pairs) => ErlangTerm::Map(
                    pairs
                        .iter()
                        .map(|(k, v)| Ok((at(k)?, at(v)?)))
                        .collect::<Result<_, String>>()?,
                ),
                term => term.clone(),
            };
            let node = order.append_core_term(&term)?;
            let mapped = TermId(order.source_ids.len());
            order.source_ids.push(node);
            source_ids.insert(id, mapped);
        }
        Ok(Self { order, source_ids })
    }
    /// Whether `id` is reachable from the root this order was built for.
    pub(crate) fn contains(&self, id: TermId) -> bool {
        self.source_ids.contains_key(&id)
    }
    pub(crate) fn compare(&self, a: TermId, other: &Self, b: TermId) -> Result<Ordering, String> {
        self.order.compare(
            *self.source_ids.get(&a).ok_or("term ID outside arena")?,
            &other.order,
            *other.source_ids.get(&b).ok_or("term ID outside arena")?,
        )
    }
}
/// Erlang ordering for finite Core literal families; process IDs, refs, ports and funs are not Core literals.
pub fn compare_terms(
    a: &ErlangTerms,
    ai: TermId,
    b: &ErlangTerms,
    bi: TermId,
) -> Result<Ordering, String> {
    let a = TermOrderArena::new(a)?;
    let b = TermOrderArena::new(b)?;
    a.compare(ai, &b, bi)
}
pub fn compare_map_keys(
    a: &ErlangTerms,
    ai: TermId,
    b: &ErlangTerms,
    bi: TermId,
) -> Result<Ordering, String> {
    let a = TermOrderArena::new(a)?;
    let b = TermOrderArena::new(b)?;
    a.compare_map_keys(ai, &b, bi)
}
fn rank(n: &Node, exact: bool) -> u8 {
    match n {
        Node::Integer(_) => 0,
        Node::Float(_) => u8::from(exact),
        Node::Atom(_) => 2,
        Node::Runtime(RuntimeKind::Reference, _) => 3,
        Node::Function(_) | Node::Runtime(RuntimeKind::Function, _) => 4,
        Node::Runtime(RuntimeKind::Port, _) => 5,
        Node::Runtime(RuntimeKind::Pid, _) => 6,
        Node::Tuple(_) => 7,
        Node::Map(_) | Node::DeferredMap(_) => 8,
        Node::Nil => 9,
        Node::Cons(..) => 10,
        Node::Bits(..) => 11,
    }
}
fn integer_float(n: &BigInt, f: f64) -> Ordering {
    let integer = BigInt::from_f64(f).expect("validated finite float");
    n.cmp(&integer).then_with(|| {
        if f.fract() > 0.0 {
            Ordering::Less
        } else if f.fract() < 0.0 {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Location {
    side: usize,
    node: Id,
}
struct Sequence {
    owner: (Location, Location, bool),
    pairs: Vec<(Location, Location, bool)>,
    next: usize,
}
struct SortState {
    side: usize,
    pairs: Vec<(Id, Id)>,
    sorted: Vec<(Id, Id)>,
    at: usize,
    lo: usize,
    hi: usize,
}
enum Frame {
    Eval(Location, Location, bool),
    Sequence(Sequence),
    AfterSequence(Sequence),
    Sort(Location),
    SortStep(SortState),
    AfterSort(SortState, usize),
    AfterMapA(Location, Location, bool),
    AfterMapB(Location, Location, bool, Vec<(Id, Id)>),
}
enum Value {
    Order(Ordering),
    Pairs(Vec<(Id, Id)>),
}
fn map_entries(node: &Node) -> &[(Id, Id)] {
    match node {
        Node::Map(xs) | Node::DeferredMap(xs) => xs,
        _ => unreachable!("map frame"),
    }
}
fn compare(
    a: &TermOrderArena,
    ai: Id,
    b: &TermOrderArena,
    bi: Id,
    exact: bool,
) -> Result<Ordering, String> {
    let arenas = [a, b];
    let mut frames = vec![Frame::Eval(
        Location { side: 0, node: ai },
        Location { side: 1, node: bi },
        exact,
    )];
    let mut result = None;
    let mut equal = HashSet::new();
    let mut sorted_maps: HashMap<Location, Vec<(Id, Id)>> = HashMap::new();
    let take_order = |value: Option<Value>| match value {
        Some(Value::Order(x)) => x,
        _ => unreachable!("comparison continuation"),
    };
    let take_pairs = |value: Option<Value>| match value {
        Some(Value::Pairs(x)) => x,
        _ => unreachable!("map continuation"),
    };
    while let Some(frame) = frames.pop() {
        match frame {
            Frame::Eval(al, bl, exact) => {
                let (a, b) = (arenas[al.side], arenas[bl.side]);
                let (an, bn) = (&a.nodes[al.node], &b.nodes[bl.node]);
                let same_arena = std::ptr::eq(a, b);
                let same_semantics = same_arena
                    && a.canonical_ids
                        .get(al.node)
                        .zip(a.canonical_ids.get(bl.node))
                        .is_some_and(|(a, b)| a == b);
                if (same_arena && al.node == bl.node)
                    || same_semantics
                    || equal.contains(&(al, bl, exact))
                {
                    result = Some(Value::Order(Ordering::Equal));
                    continue;
                }
                let mut c = rank(an, exact).cmp(&rank(bn, exact));
                if c != Ordering::Equal {
                    result = Some(Value::Order(c));
                    continue;
                }
                let child = |side, node| Location { side, node };
                let mut pairs = Vec::new();
                match (an, bn) {
                    (Node::Integer(a), Node::Integer(b)) => c = a.cmp(b),
                    (Node::Float(a), Node::Float(b)) => {
                        c = a.partial_cmp(b).expect("finite float").then_with(|| {
                            if exact && *a == 0.0 && *b == 0.0 {
                                b.is_sign_negative().cmp(&a.is_sign_negative())
                            } else {
                                Ordering::Equal
                            }
                        })
                    }
                    (Node::Integer(a), Node::Float(b)) => c = integer_float(a, *b),
                    (Node::Float(a), Node::Integer(b)) => c = integer_float(b, *a).reverse(),
                    (Node::Atom(a), Node::Atom(b)) => c = a.cmp(b),
                    (Node::Runtime(ak, a), Node::Runtime(bk, b)) => {
                        if ak != bk || a != b {
                            return Err("comparison reached distinct opaque runtime handles".into());
                        }
                    }
                    (Node::Runtime(RuntimeKind::Function, _), Node::Function(_))
                    | (Node::Function(_), Node::Runtime(RuntimeKind::Function, _)) => {
                        return Err("comparison reached opaque and known functions".into());
                    }
                    (Node::Function(a), Node::Function(b)) => match (a, b) {
                        (
                            CapturedFunction::Local {
                                module: am,
                                new_index: ai,
                                old_uniq: au,
                                environment: ae,
                                ..
                            },
                            CapturedFunction::Local {
                                module: bm,
                                new_index: bi,
                                old_uniq: bu,
                                environment: be,
                                ..
                            },
                        ) => {
                            c = am
                                .cmp(bm)
                                .then(ai.cmp(bi))
                                .then(au.cmp(bu))
                                .then(ae.len().cmp(&be.len()));
                            if c == Ordering::Equal {
                                pairs.extend(ae.iter().zip(be).map(|(a, b)| {
                                    (child(al.side, a.0), child(bl.side, b.0), exact)
                                }));
                            }
                        }
                        (
                            CapturedFunction::External {
                                module: am,
                                function: af,
                                arity: aa,
                            },
                            CapturedFunction::External {
                                module: bm,
                                function: bf,
                                arity: ba,
                            },
                        ) => c = am.cmp(bm).then(af.cmp(bf)).then(aa.cmp(ba)),
                        (CapturedFunction::Local { .. }, CapturedFunction::External { .. }) => {
                            c = Ordering::Less
                        }
                        (CapturedFunction::External { .. }, CapturedFunction::Local { .. }) => {
                            c = Ordering::Greater
                        }
                    },
                    (Node::Tuple(a), Node::Tuple(b)) => {
                        c = a.len().cmp(&b.len());
                        if c == Ordering::Equal {
                            pairs.extend(
                                a.iter()
                                    .zip(b)
                                    .map(|(a, b)| (child(al.side, *a), child(bl.side, *b), exact)),
                            );
                        }
                    }
                    (Node::Map(_) | Node::DeferredMap(_), Node::Map(_) | Node::DeferredMap(_)) => {
                        c = map_entries(an).len().cmp(&map_entries(bn).len());
                        if c == Ordering::Equal {
                            frames.push(Frame::AfterMapA(al, bl, exact));
                            frames.push(Frame::Sort(al));
                            continue;
                        }
                    }
                    (Node::Nil, Node::Nil) => {}
                    (Node::Cons(ah, at), Node::Cons(bh, bt)) => {
                        pairs.push((child(al.side, *ah), child(bl.side, *bh), exact));
                        pairs.push((child(al.side, *at), child(bl.side, *bt), exact));
                    }
                    (Node::Bits(a, ab), Node::Bits(b, bb)) => {
                        let n = ab.min(bb);
                        let full = (n / 8) as usize;
                        c = a[..full].cmp(&b[..full]);
                        if c == Ordering::Equal && n % 8 != 0 {
                            c = (a[full] >> (8 - n % 8)).cmp(&(b[full] >> (8 - n % 8)));
                        }
                        if c == Ordering::Equal {
                            c = ab.cmp(bb);
                        }
                    }
                    _ => unreachable!("same-ranked validated terms"),
                }
                if c != Ordering::Equal {
                    result = Some(Value::Order(c));
                } else {
                    frames.push(Frame::Sequence(Sequence {
                        owner: (al, bl, exact),
                        pairs,
                        next: 0,
                    }));
                }
            }
            Frame::Sequence(mut state) => {
                if state.next == state.pairs.len() {
                    equal.insert(state.owner);
                    result = Some(Value::Order(Ordering::Equal));
                } else {
                    let pair = state.pairs[state.next];
                    state.next += 1;
                    frames.push(Frame::AfterSequence(state));
                    frames.push(Frame::Eval(pair.0, pair.1, pair.2));
                }
            }
            Frame::AfterSequence(state) => {
                let order = take_order(result.take());
                if order == Ordering::Equal {
                    frames.push(Frame::Sequence(state));
                } else {
                    result = Some(Value::Order(order));
                }
            }
            Frame::Sort(loc) => {
                let node = &arenas[loc.side].nodes[loc.node];
                if let Some(sorted) = sorted_maps.get(&loc) {
                    result = Some(Value::Pairs(sorted.clone()));
                    continue;
                }
                if let Node::Map(pairs) = node {
                    result = Some(Value::Pairs(pairs.clone()));
                    continue;
                }
                frames.push(Frame::SortStep(SortState {
                    side: loc.side,
                    pairs: map_entries(node).to_vec(),
                    sorted: Vec::new(),
                    at: 0,
                    lo: 0,
                    hi: 0,
                }));
            }
            Frame::SortStep(mut state) => {
                if state.at == state.pairs.len() {
                    result = Some(Value::Pairs(state.sorted));
                    continue;
                }
                if state.lo == state.hi {
                    state.sorted.insert(state.lo, state.pairs[state.at]);
                    state.at += 1;
                    state.lo = 0;
                    state.hi = state.sorted.len();
                    frames.push(Frame::SortStep(state));
                } else {
                    let mid = (state.lo + state.hi) / 2;
                    let (a, b) = (state.sorted[mid].0, state.pairs[state.at].0);
                    let side = state.side;
                    frames.push(Frame::AfterSort(state, mid));
                    frames.push(Frame::Eval(
                        Location { side, node: a },
                        Location { side, node: b },
                        true,
                    ));
                }
            }
            Frame::AfterSort(mut state, mid) => {
                if take_order(result.take()).is_lt() {
                    state.lo = mid + 1;
                } else {
                    state.hi = mid;
                }
                frames.push(Frame::SortStep(state));
            }
            Frame::AfterMapA(al, bl, exact) => {
                let a = take_pairs(result.take());
                sorted_maps.insert(al, a.clone());
                frames.push(Frame::AfterMapB(al, bl, exact, a));
                frames.push(Frame::Sort(bl));
            }
            Frame::AfterMapB(al, bl, exact, a) => {
                let b = take_pairs(result.take());
                sorted_maps.insert(bl, b.clone());
                let mut pairs = Vec::with_capacity(a.len() * 2);
                pairs.extend(a.iter().zip(&b).map(|((a, _), (b, _))| {
                    (
                        Location {
                            side: al.side,
                            node: *a,
                        },
                        Location {
                            side: bl.side,
                            node: *b,
                        },
                        true,
                    )
                }));
                pairs.extend(a.iter().zip(&b).map(|((_, a), (_, b))| {
                    (
                        Location {
                            side: al.side,
                            node: *a,
                        },
                        Location {
                            side: bl.side,
                            node: *b,
                        },
                        exact,
                    )
                }));
                frames.push(Frame::Sequence(Sequence {
                    owner: (al, bl, exact),
                    pairs,
                    next: 0,
                }));
            }
        }
    }
    Ok(take_order(result))
}
#[cfg(test)]
mod tests {
    use super::*;

    fn handle(object: &str) -> CaptureTerm {
        CaptureTerm::Runtime {
            kind: RuntimeKind::Reference,
            identity: RuntimeIdentity {
                scope: "phase".into(),
                object: object.into(),
            },
        }
    }
    fn closure(index: u32, env: Vec<CaptureId>) -> CaptureTerm {
        CaptureTerm::Function(CapturedFunction::Local {
            module: "source".into(),
            new_index: index,
            old_uniq: 42,
            arity: 1,
            name: "site".into(),
            environment: env,
        })
    }

    #[test]
    fn scalar_capture_memo_preserves_source_ids_and_first_errors() {
        let source = vec![
            CaptureTerm::Atom("x".into()),
            CaptureTerm::Integer("1".into()),
            CaptureTerm::Atom("x".into()),
            CaptureTerm::Integer("01".into()),
            CaptureTerm::Nil,
            CaptureTerm::Nil,
            CaptureTerm::Tuple((0..6).map(CaptureId).collect()),
        ];
        let arena = TermOrderArena::from_captures(&source).unwrap();
        assert_eq!(arena.source_ids, vec![0, 1, 0, 1, 2, 2, 3]);
        assert_eq!(arena.canonical_ids, vec![0, 1, 2, 3]);
        assert!(matches!(&arena.nodes[3], Node::Tuple(ids) if ids == &[0, 1, 0, 1, 2, 2]));
        for (bad, later, error) in [
            (CaptureTerm::Integer("bad".into()), CaptureTerm::Float(f64::NAN.to_bits()), "invalid capture integer"),
            (CaptureTerm::Float(f64::NAN.to_bits()), CaptureTerm::Integer("bad".into()), "nonfinite capture float"),
        ] {
            let mut source = source.clone();
            source.extend([bad, later]);
            assert_eq!(TermOrderArena::from_captures(&source).unwrap_err(), error);
        }
    }
    #[test]
    fn repeated_state_captures_preserve_shared_subgraphs() {
        let mut environment = CaptureEnvironment::new(
            std::sync::Arc::new(TermOrderArena::from_captures(&[CaptureTerm::Nil]).unwrap()),
            CaptureId(0),
        )
        .unwrap();
        for depth in 1..=16 {
            let mut terms = Vec::new();
            let first = environment.append_to(&mut terms);
            let second = environment.append_to(&mut terms);
            let pair = CaptureId(terms.len());
            terms.push(CaptureTerm::Tuple(vec![first, second]));
            let nil = CaptureId(terms.len());
            terms.push(CaptureTerm::Nil);
            let root = CaptureId(terms.len());
            terms.push(CaptureTerm::List {
                items: vec![pair],
                tail: nil,
            });
            let arena = TermOrderArena::from_captures(&terms).unwrap();
            assert_eq!(
                arena.compare_captures(first, &arena, second).unwrap(),
                Ordering::Equal
            );
            assert!(arena.nodes.len() <= 2 * depth + 1);
            environment = CaptureEnvironment::new(std::sync::Arc::new(arena), root).unwrap();
        }
    }
    #[test]
    fn importing_captured_environment_preserves_nested_closures_and_shared_handles() {
        let terms = vec![
            CaptureTerm::Nil,
            handle("same"),
            CaptureTerm::Integer("123456789012345678901234567890".into()),
            CaptureTerm::Bitstring {
                bytes: vec![160],
                bits: 3,
            },
            closure(7, vec![CaptureId(1), CaptureId(2)]),
            CaptureTerm::Tuple(vec![CaptureId(3), CaptureId(4)]),
            CaptureTerm::Map(vec![(CaptureId(1), CaptureId(5))]),
            CaptureTerm::List {
                items: vec![CaptureId(6), CaptureId(4)],
                tail: CaptureId(0),
            },
        ];
        let source = CaptureEnvironment::new(
            std::sync::Arc::new(TermOrderArena::from_captures(&terms).unwrap()),
            CaptureId(7),
        )
        .unwrap();
        let mut imported = vec![CaptureTerm::Atom("existing".into())];
        let root = source.append_to(&mut imported);
        let copy = CaptureEnvironment::new(
            std::sync::Arc::new(TermOrderArena::from_captures(&imported).unwrap()),
            root,
        )
        .unwrap();
        assert_eq!(source.len(), copy.len());
        assert_eq!(source.compare(&copy).unwrap(), Ordering::Equal);
    }
    #[test]
    fn captured_boundary_rejects_forward_refs_and_improper_environments() {
        use serde_json::json;
        assert!(parse_captures(&json!({"root":0,"terms":[{"kind":"tuple","items":[0]}]})).is_err());
        assert!(parse_captures(&json!({"root":0,"terms":[{"kind":"function","flavor":"external","module":"","name":"","arity":256}]})).is_err());
        let arena = std::sync::Arc::new(
            TermOrderArena::from_captures(&[
                CaptureTerm::Atom("tail".into()),
                CaptureTerm::List {
                    items: vec![CaptureId(0)],
                    tail: CaptureId(0),
                },
            ])
            .unwrap(),
        );
        assert!(CaptureEnvironment::new(arena, CaptureId(1)).is_err());
    }
    #[test]
    fn deferred_key_sorting_uses_the_same_iterative_comparison_machine() {
        let mut terms = vec![CaptureTerm::Atom("a".into()), CaptureTerm::Atom("z".into())];
        let mut id = CaptureId(0);
        for _ in 0..10000 {
            let next = CaptureId(terms.len());
            terms.push(CaptureTerm::Map(vec![
                (id, CaptureId(0)),
                (CaptureId(1), CaptureId(1)),
            ]));
            id = next;
        }
        let a = TermOrderArena::from_captures(&terms).unwrap();
        let b = TermOrderArena::from_captures(&terms).unwrap();
        assert_eq!(a.compare_capture_keys(id, &b, id).unwrap(), Ordering::Equal);
    }
    #[test]
    fn captured_handles_are_rejected_only_when_comparison_reaches_them() {
        let a = TermOrderArena::from_captures(&[
            CaptureTerm::Atom("a".into()),
            handle("first"),
            CaptureTerm::Tuple(vec![CaptureId(0), CaptureId(1)]),
        ])
        .unwrap();
        let b = TermOrderArena::from_captures(&[
            CaptureTerm::Atom("b".into()),
            handle("second"),
            CaptureTerm::Tuple(vec![CaptureId(0), CaptureId(1)]),
        ])
        .unwrap();
        assert_eq!(
            a.compare_captures(CaptureId(2), &b, CaptureId(2)).unwrap(),
            Ordering::Less
        );
        assert!(a.compare_captures(CaptureId(1), &b, CaptureId(1)).is_err());
        let c = TermOrderArena::from_captures(&[handle("first")]).unwrap();
        assert_eq!(
            a.compare_capture_keys(CaptureId(1), &c, CaptureId(0))
                .unwrap(),
            Ordering::Equal
        );
        let f = TermOrderArena::from_captures(&[handle("first"), closure(0, vec![CaptureId(0)])])
            .unwrap();
        let g = TermOrderArena::from_captures(&[handle("second"), closure(1, vec![CaptureId(0)])])
            .unwrap();
        assert_eq!(
            f.compare_captures(CaptureId(1), &g, CaptureId(1)).unwrap(),
            Ordering::Less
        );
    }
    #[test]
    fn captured_maps_normalize_identity_keys_and_defer_opaque_sorting() {
        let a = TermOrderArena::from_captures(&[
            handle("one"),
            handle("two"),
            CaptureTerm::Atom("v".into()),
            CaptureTerm::Map(vec![
                (CaptureId(0), CaptureId(2)),
                (CaptureId(1), CaptureId(2)),
            ]),
        ])
        .unwrap();
        let b = TermOrderArena::from_captures(&[
            handle("one"),
            CaptureTerm::Atom("v".into()),
            CaptureTerm::Map(vec![
                (CaptureId(0), CaptureId(1)),
                (CaptureId(0), CaptureId(1)),
            ]),
        ])
        .unwrap();
        assert_eq!(
            a.compare_captures(CaptureId(3), &b, CaptureId(2)).unwrap(),
            Ordering::Greater
        );
        assert!(a.compare_captures(CaptureId(3), &a, CaptureId(3)).is_ok());
        let c = TermOrderArena::from_captures(&[
            handle("one"),
            handle("two"),
            CaptureTerm::Atom("v".into()),
            CaptureTerm::Map(vec![
                (CaptureId(0), CaptureId(2)),
                (CaptureId(1), CaptureId(2)),
            ]),
        ])
        .unwrap();
        assert!(a.compare_captures(CaptureId(3), &c, CaptureId(3)).is_err());
    }
    #[test]
    fn deeply_nested_captured_closures_and_map_keys_compare_without_recursion() {
        for map in [false, true] {
            let mut terms = vec![
                CaptureTerm::Atom("leaf".into()),
                CaptureTerm::Atom("value".into()),
            ];
            let mut id = CaptureId(0);
            for _ in 0..20000 {
                let next = CaptureId(terms.len());
                terms.push(if map {
                    CaptureTerm::Map(vec![(id, CaptureId(1))])
                } else {
                    closure(0, vec![id])
                });
                id = next;
            }
            let a = TermOrderArena::from_captures(&terms).unwrap();
            let b = TermOrderArena::from_captures(&terms).unwrap();
            assert_eq!(a.compare_capture_keys(id, &b, id).unwrap(), Ordering::Equal);
        }
    }
    fn deep(kind: u8, n: usize, leaf: &str, shared: bool) -> ErlangTerms {
        let mut terms = vec![ErlangTerm::Atom(leaf.into()), ErlangTerm::Nil];
        let mut root = TermId(0);
        for _ in 0..n {
            let term = match kind {
                0 => ErlangTerm::Tuple(if shared { vec![root, root] } else { vec![root] }),
                1 => ErlangTerm::List {
                    items: vec![root],
                    tail: TermId(1),
                },
                _ => ErlangTerm::Map(vec![(TermId(0), root)]),
            };
            root = TermId(terms.len());
            terms.push(term);
        }
        ErlangTerms { terms, root }
    }
    #[test]
    fn deep_arenas_and_shared_dags_compare_and_drop_iteratively() {
        for (kind, n, shared) in [
            (0, 50000, false),
            (1, 50000, false),
            (2, 50000, false),
            (0, 10000, true),
        ] {
            let a = deep(kind, n, "a", shared);
            let b = deep(kind, n, "a", shared);
            let c = deep(kind, n, "b", shared);
            assert_eq!(
                compare_terms(&a, a.root, &b, b.root).unwrap(),
                Ordering::Equal
            );
            assert_eq!(
                compare_terms(&a, a.root, &c, c.root).unwrap(),
                Ordering::Less
            );
        }
    }
}

#[cfg(test)]
mod root_order_tests {
    use super::*;
    #[test]
    fn root_qualified_order_matches_original_compacted_arenas() {
        let mut source=ErlangTerms {root:TermId(0),terms:vec![
            ErlangTerm::Nil, ErlangTerm::Integer("1".into()), ErlangTerm::Float(1f64.to_bits()),
            ErlangTerm::Atom("a".into()), ErlangTerm::Atom("b".into()), ErlangTerm::Integer("-123456789012345678901234567890".into()),
            ErlangTerm::Binary(vec![1,2]), ErlangTerm::Bitstring{bytes:vec![128],bits:1},
            ErlangTerm::Tuple(vec![TermId(1),TermId(3)]),
            ErlangTerm::List{items:vec![TermId(8),TermId(5)],tail:TermId(0)},
            ErlangTerm::List{items:vec![TermId(1)],tail:TermId(4)},
            ErlangTerm::Map(vec![(TermId(1),TermId(3)),(TermId(2),TermId(4)),(TermId(1),TermId(5))]),
            ErlangTerm::ExternalFun{module:"m".into(),function:"f".into(),arity:2},
        ]};
        let roots=source.terms.len();source.terms.push(ErlangTerm::Float(f64::NAN.to_bits()));
        let mut old=Vec::new();let mut new=Vec::new();
        for i in 0..roots {
            let mut terms=Vec::new();let root=crate::form_cache_wire::copy_arena(&mut terms,&source.terms,TermId(i));
            old.push((TermOrderArena::new(&ErlangTerms{root,terms}).unwrap(),root));
            new.push(RootTermOrder::new(&source,TermId(i)).unwrap());
        }
        for i in 0..roots {for j in 0..roots {
            assert_eq!(new[i].compare(TermId(i),&new[j],TermId(j)).unwrap(),old[i].0.compare(old[i].1,&old[j].0,old[j].1).unwrap());
        }}
        assert_eq!(RootTermOrder::new(&source,TermId(roots)).err().unwrap(),"nonfinite float is not an Erlang term");
    }
}

use crate::core::{CoreModule, CoreProgram, ErlangTerm, TermId};
use crate::lattice::VariableId;
use crate::program::{MetadataKind, ModuleId, Program, TypeName};
use num_bigint::BigInt;
use serde::Serialize;
use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};

type Result<T> = std::result::Result<T, String>;

pub(crate) trait FormTerms {
    fn name(&self) -> &str;
    fn terms(&self) -> &crate::shared_vec::SharedVec<ErlangTerm>;
}
impl FormTerms for CoreModule {
    fn name(&self) -> &str {
        &self.name
    }
    fn terms(&self) -> &crate::shared_vec::SharedVec<ErlangTerm> {
        &self.terms
    }
}
pub(crate) struct FormSource<'a> {
    pub name: &'a str,
    pub terms: &'a crate::shared_vec::SharedVec<ErlangTerm>,
}
impl FormTerms for FormSource<'_> {
    fn name(&self) -> &str {
        self.name
    }
    fn terms(&self) -> &crate::shared_vec::SharedVec<ErlangTerm> {
        self.terms
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FormId(pub usize);

#[derive(Clone, Debug)]
pub enum TypeForm {
    /// Uninspected native source form. Only the native resolver materializes it.
    DeferredRaw,
    /// A native generic type form understood by record validation, not resolution.
    UnrecognizedBuiltin {
        name: String,
        arguments: Option<Vec<FormId>>,
    },
    Variable(VariableId),
    Atom(String),
    Integer(BigInt),
    Operator {
        name: String,
        arguments: Vec<FormId>,
    },
    Annotated {
        variable: FormId,
        body: FormId,
    },
    Parenthesized(FormId),
    Builtin {
        name: Builtin,
        arguments: Option<Vec<FormId>>,
    },
    Named {
        name: TypeName,
        remote: bool,
        arguments: Vec<FormId>,
    },
    BoundedFunction {
        function: FormId,
        constraints: Vec<FormId>,
    },
    Constraint {
        name: String,
        arguments: Vec<FormId>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Builtin {
    Any,
    Arity,
    Atom,
    Binary,
    Bitstring,
    Bool,
    Boolean,
    Byte,
    Char,
    Dynamic,
    Float,
    Function,
    Fun,
    Identifier,
    Integer,
    Iodata,
    Iolist,
    List,
    Map,
    Mfa,
    Module,
    Nil,
    NegInteger,
    NonNegInteger,
    NoReturn,
    Node,
    None,
    NonemptyBinary,
    NonemptyBitstring,
    NonemptyList,
    NonemptyImproperList,
    NonemptyMaybeImproperList,
    NonemptyString,
    Number,
    Pid,
    Port,
    PosInteger,
    MaybeImproperList,
    Product,
    Range,
    Record,
    Reference,
    String,
    Term,
    Timeout,
    Tuple,
    Union,
    FieldType,
    MapFieldAssoc,
    MapFieldExact,
}

impl Builtin {
    pub fn name(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Arity => "arity",
            Self::Atom => "atom",
            Self::Binary => "binary",
            Self::Bitstring => "bitstring",
            Self::Bool => "bool",
            Self::Boolean => "boolean",
            Self::Byte => "byte",
            Self::Char => "char",
            Self::Dynamic => "dynamic",
            Self::Float => "float",
            Self::Function => "function",
            Self::Fun => "fun",
            Self::Identifier => "identifier",
            Self::Integer => "integer",
            Self::Iodata => "iodata",
            Self::Iolist => "iolist",
            Self::List => "list",
            Self::Map => "map",
            Self::Mfa => "mfa",
            Self::Module => "module",
            Self::Nil => "nil",
            Self::NegInteger => "neg_integer",
            Self::NonNegInteger => "non_neg_integer",
            Self::NoReturn => "no_return",
            Self::Node => "node",
            Self::None => "none",
            Self::NonemptyBinary => "nonempty_binary",
            Self::NonemptyBitstring => "nonempty_bitstring",
            Self::NonemptyList => "nonempty_list",
            Self::NonemptyImproperList => "nonempty_improper_list",
            Self::NonemptyMaybeImproperList => "nonempty_maybe_improper_list",
            Self::NonemptyString => "nonempty_string",
            Self::Number => "number",
            Self::Pid => "pid",
            Self::Port => "port",
            Self::PosInteger => "pos_integer",
            Self::MaybeImproperList => "maybe_improper_list",
            Self::Product => "product",
            Self::Range => "range",
            Self::Record => "record",
            Self::Reference => "reference",
            Self::String => "string",
            Self::Term => "term",
            Self::Timeout => "timeout",
            Self::Tuple => "tuple",
            Self::Union => "union",
            Self::FieldType => "field_type",
            Self::MapFieldAssoc => "map_field_assoc",
            Self::MapFieldExact => "map_field_exact",
        }
    }
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "any" => Self::Any,
            "arity" => Self::Arity,
            "atom" => Self::Atom,
            "binary" => Self::Binary,
            "bitstring" => Self::Bitstring,
            "bool" => Self::Bool,
            "boolean" => Self::Boolean,
            "byte" => Self::Byte,
            "char" => Self::Char,
            "dynamic" => Self::Dynamic,
            "float" => Self::Float,
            "function" => Self::Function,
            "fun" => Self::Fun,
            "identifier" => Self::Identifier,
            "integer" => Self::Integer,
            "iodata" => Self::Iodata,
            "iolist" => Self::Iolist,
            "list" => Self::List,
            "map" => Self::Map,
            "mfa" => Self::Mfa,
            "module" => Self::Module,
            "nil" => Self::Nil,
            "neg_integer" => Self::NegInteger,
            "non_neg_integer" => Self::NonNegInteger,
            "no_return" => Self::NoReturn,
            "node" => Self::Node,
            "none" => Self::None,
            "nonempty_binary" => Self::NonemptyBinary,
            "nonempty_bitstring" => Self::NonemptyBitstring,
            "nonempty_list" => Self::NonemptyList,
            "nonempty_improper_list" => Self::NonemptyImproperList,
            "nonempty_maybe_improper_list" => Self::NonemptyMaybeImproperList,
            "nonempty_string" => Self::NonemptyString,
            "number" => Self::Number,
            "pid" => Self::Pid,
            "port" => Self::Port,
            "pos_integer" => Self::PosInteger,
            "maybe_improper_list" => Self::MaybeImproperList,
            "product" => Self::Product,
            "range" => Self::Range,
            "record" => Self::Record,
            "reference" => Self::Reference,
            "string" => Self::String,
            "term" => Self::Term,
            "timeout" => Self::Timeout,
            "tuple" => Self::Tuple,
            "union" => Self::Union,
            "field_type" => Self::FieldType,
            "map_field_assoc" => Self::MapFieldAssoc,
            "map_field_exact" => Self::MapFieldExact,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Form {
    pub module: ModuleId,
    pub raw: TermId,
    pub value: TypeForm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclarationKind {
    Alias,
    Opaque,
    Nominal,
}

#[derive(Clone, Debug)]
pub struct Declaration {
    pub name: TypeName,
    pub kind: DeclarationKind,
    pub parameters: Vec<VariableId>,
    pub body: FormId,
    pub exported: bool,
    pub metadata: usize,
}

#[derive(Clone, Debug)]
pub struct Contract {
    pub file_location: crate::native_form_environment::RawFormTerm,
    pub name: crate::declaration_key::DeclarationKey,
    pub callback: bool,
    pub optional: bool,
    pub alternatives: Vec<FormId>,
    pub metadata: usize,
}

#[derive(Clone, Debug)]
pub struct RecordField {
    pub name: String,
    pub declared_type: Option<FormId>,
    pub default_expression: Option<TermId>,
}

#[derive(Clone, Debug)]
pub struct Record {
    pub module: ModuleId,
    pub name: String,
    pub fields: Vec<RecordField>,
    pub metadata: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct NativeFormSourceId(usize);

#[derive(Clone, Debug, Default)]
pub struct Forms {
    pub nodes: Vec<Form>,
    pub declarations: BTreeMap<TypeName, Declaration>,
    pub contracts: Vec<Contract>,
    pub records: BTreeMap<(ModuleId, String), Record>,
    parsed: BTreeMap<(ModuleId, TermId), FormId>,
    eager_parsed: BTreeSet<(ModuleId, TermId)>,
    native_sources: Vec<Arc<crate::core::ErlangTerms>>,
    native_parsed: BTreeMap<(ModuleId, NativeFormSourceId, TermId), FormId>,
    native_provenance: BTreeMap<FormId, NativeFormSourceId>,
}

#[derive(Serialize)]
pub struct Inspection {
    pub forms: usize,
    pub declarations: usize,
    pub exported_types: usize,
    pub aliases: usize,
    pub opaque: usize,
    pub nominal: usize,
    pub specs: usize,
    pub callbacks: usize,
    pub optional_callbacks: usize,
    pub alternatives: usize,
    pub records: usize,
    pub record_fields: usize,
    pub bounded_functions: usize,
    pub constraints: usize,
}

impl Forms {
    pub fn build(core: &CoreProgram, program: &Program) -> Result<Self> {
        let locations =
            crate::temporary_contracts::locations(core, program).map_err(|e| e.to_string())?;
        Self::build_mode(core, program, false, &locations).map_err(|e| e.to_string())
    }
    /// `locations` is `temporary_contracts::locations(core, program)`, which
    /// the caller computes first (it failed first here) and reuses.
    pub(crate) fn build_deferred(
        core: &CoreProgram,
        program: &Program,
        locations: &BTreeMap<usize, crate::native_form_environment::RawFormTerm>,
    ) -> std::result::Result<Self, crate::form_error::FormError> {
        Self::build_mode(core, program, true, locations)
    }
    fn build_mode(
        core: &CoreProgram,
        program: &Program,
        deferred: bool,
        locations: &BTreeMap<usize, crate::native_form_environment::RawFormTerm>,
    ) -> std::result::Result<Self, crate::form_error::FormError> {
        let mut result = Self::default();
        let exports: BTreeSet<_> = program
            .metadata
            .iter()
            .filter(|m| matches!(m.kind, MetadataKind::ExportType))
            .flat_map(|m| m.types.iter().cloned())
            .collect();
        let mut optional = Vec::new();
        // Term arenas of the modules whose raw forms are read here; copying
        // every module's terms up front cost more than the reads.
        let mut arenas: Vec<Option<std::sync::Arc<crate::core::ErlangTerms>>> =
            vec![None; core.modules.len()];
        let arena = |arenas: &mut Vec<Option<std::sync::Arc<crate::core::ErlangTerms>>>,
                     module: usize| {
            arenas[module]
                .get_or_insert_with(|| {
                    std::sync::Arc::new(crate::core::ErlangTerms {
                        root: TermId(0),
                        terms: core.modules[module].terms.clone(),
                    })
                })
                .clone()
        };
        for meta in program
            .metadata
            .iter()
            .filter(|m| matches!(m.kind, MetadataKind::OptionalCallbacks))
        {
            let raw = crate::native_form_environment::RawFormTerm::new(
                arena(&mut arenas, meta.module.0),
                meta.raw,
            )?;
            optional.extend(crate::declaration_key::optional_callbacks(
                &core.modules[meta.module.0].name,
                &raw,
            )?);
        }
        for (metadata, meta) in program.metadata.iter().enumerate() {
            let module = &core.modules[meta.module.0];
            match meta.kind {
                MetadataKind::Type | MetadataKind::Opaque | MetadataKind::Nominal => {
                    for raw in list(module, meta.raw)? {
                        let [name, body, parameters] = exact_tuple::<3>(module, raw)?;
                        let name = TypeName {
                            module: module.name().to_owned(),
                            name: atom(module, name)?.into(),
                            arity: list(module, parameters)?.len(),
                        };
                        let parameters = list(module, parameters)?
                            .into_iter()
                            .map(|p| {
                                let fields = exact_tuple::<3>(module, p)?;
                                if atom(module, fields[0])? != "var" {
                                    return Err(format!(
                                        "{} term{}: expected type parameter variable",
                                        module.name(),
                                        p.0
                                    ));
                                }
                                match &module.terms()[fields[2].0] {
                                    ErlangTerm::Atom(v) => Ok(VariableId::Atom(v.clone())),
                                    ErlangTerm::Integer(v) => Ok(VariableId::Integer(
                                        v.parse().map_err(|_| "invalid variable integer")?,
                                    )),
                                    _ => Err("type parameter must be atom or integer".into()),
                                }
                            })
                            .collect::<Result<Vec<_>>>()?;
                        let body = if deferred {
                            result.defer(meta.module, body)
                        } else {
                            result.parse(module, meta.module, body)?
                        };
                        let declaration = Declaration {
                            exported: exports.contains(&name),
                            name: name.clone(),
                            kind: match meta.kind {
                                MetadataKind::Opaque => DeclarationKind::Opaque,
                                MetadataKind::Nominal => DeclarationKind::Nominal,
                                _ => DeclarationKind::Alias,
                            },
                            parameters,
                            body,
                            metadata,
                        };
                        if result
                            .declarations
                            .insert(name.clone(), declaration)
                            .is_some()
                        {
                            return Err(format!(
                                "duplicate type {}:{}/{}",
                                name.module, name.name, name.arity
                            )
                            .into());
                        }
                    }
                }
                MetadataKind::Spec | MetadataKind::Callback if !deferred => {
                    let raw = crate::native_form_environment::RawFormTerm::new(
                        arena(&mut arenas, meta.module.0),
                        meta.raw,
                    )?;
                    for (name, alternatives) in
                        crate::declaration_key::declarations(&module.name, &raw)?
                    {
                        let alternatives = alternatives
                            .into_iter()
                            .map(|id| {
                                if deferred {
                                    Ok(result.defer(meta.module, id))
                                } else {
                                    result.parse(module, meta.module, id)
                                }
                            })
                            .collect::<Result<Vec<_>>>()?;
                        let mut is_optional = false;
                        for key in &optional {
                            if key.optional_membership_equals(&name)? {
                                is_optional = true;
                                break;
                            }
                        }
                        result.contracts.push(Contract {
                            file_location: locations[&metadata].clone(),
                            name,
                            callback: matches!(meta.kind, MetadataKind::Callback),
                            optional: is_optional,
                            alternatives,
                            metadata,
                        });
                    }
                }
                MetadataKind::Record => {
                    for raw in list(module, meta.raw)? {
                        let [name, fields] = exact_tuple::<2>(module, raw)?;
                        let name = atom(module, name)?.to_owned();
                        let fields = list(module, fields)?
                            .into_iter()
                            .map(|raw| {
                                let parts = tuple(module, raw)?;
                                let (field, declared_type) = if parts.first().is_some_and(|id| {
                                    atom(module, *id).ok() == Some("typed_record_field")
                                }) {
                                    if parts.len() != 3 {
                                        return Err(format!(
                                            "{} term{}: invalid typed record field",
                                            module.name(),
                                            raw.0
                                        ));
                                    }
                                    (
                                        parts[1],
                                        Some(if deferred {
                                            result.defer(meta.module, parts[2])
                                        } else {
                                            result.parse(module, meta.module, parts[2])?
                                        }),
                                    )
                                } else {
                                    (raw, None)
                                };
                                let parts = tuple(module, field)?;
                                if parts
                                    .first()
                                    .is_none_or(|id| atom(module, *id).ok() != Some("record_field"))
                                    || !(3..=4).contains(&parts.len())
                                {
                                    return Err(format!(
                                        "{} term{}: invalid record field",
                                        module.name(),
                                        field.0
                                    ));
                                }
                                let literal = exact_tuple::<3>(module, parts[2])?;
                                if atom(module, literal[0])? != "atom" {
                                    return Err(format!(
                                        "{} term{}: invalid record field name",
                                        module.name(),
                                        field.0
                                    ));
                                }
                                Ok(RecordField {
                                    name: atom(module, literal[2])?.into(),
                                    declared_type,
                                    default_expression: parts.get(3).copied(),
                                })
                            })
                            .collect::<Result<Vec<_>>>()?;
                        let record = Record {
                            module: meta.module,
                            name: name.clone(),
                            fields,
                            metadata,
                        };
                        if result
                            .records
                            .insert((meta.module, name.clone()), record)
                            .is_some()
                        {
                            return Err(
                                format!("{}: duplicate record {name}", module.name()).into()
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(result)
    }

    pub fn inspect(&self) -> Inspection {
        Inspection {
            forms: self.nodes.len(),
            declarations: self.declarations.len(),
            exported_types: self.declarations.values().filter(|d| d.exported).count(),
            aliases: self
                .declarations
                .values()
                .filter(|d| d.kind == DeclarationKind::Alias)
                .count(),
            opaque: self
                .declarations
                .values()
                .filter(|d| d.kind == DeclarationKind::Opaque)
                .count(),
            nominal: self
                .declarations
                .values()
                .filter(|d| d.kind == DeclarationKind::Nominal)
                .count(),
            specs: self.contracts.iter().filter(|c| !c.callback).count(),
            callbacks: self.contracts.iter().filter(|c| c.callback).count(),
            optional_callbacks: self
                .contracts
                .iter()
                .filter(|c| c.callback && c.optional)
                .count(),
            alternatives: self.contracts.iter().map(|c| c.alternatives.len()).sum(),
            records: self.records.len(),
            record_fields: self.records.values().map(|r| r.fields.len()).sum(),
            bounded_functions: self
                .nodes
                .iter()
                .filter(|f| matches!(f.value, TypeForm::BoundedFunction { .. }))
                .count(),
            constraints: self
                .nodes
                .iter()
                .filter(|f| matches!(f.value, TypeForm::Constraint { .. }))
                .count(),
        }
    }

    pub(crate) fn defer_raw(
        &mut self,
        module: ModuleId,
        raw: &crate::native_form_environment::RawFormTerm,
    ) -> FormId {
        let source = if let Some(index) = self
            .native_sources
            .iter()
            .position(|arena| Arc::ptr_eq(arena, raw.arena()))
        {
            NativeFormSourceId(index)
        } else {
            let source = NativeFormSourceId(self.native_sources.len());
            self.native_sources.push(raw.arena().clone());
            source
        };
        self.defer_source(module, source, raw.id())
    }
    fn defer_source(
        &mut self,
        module: ModuleId,
        source: NativeFormSourceId,
        raw: TermId,
    ) -> FormId {
        let key = (module, source, raw);
        if let Some(id) = self.native_parsed.get(&key) {
            return *id;
        }
        let id = FormId(self.nodes.len());
        self.nodes.push(Form {
            module,
            raw,
            value: TypeForm::DeferredRaw,
        });
        self.native_parsed.insert(key, id);
        self.native_provenance.insert(id, source);
        id
    }
    pub(crate) fn source_arena(&self, id: FormId) -> Option<Arc<crate::core::ErlangTerms>> {
        self.native_provenance
            .get(&id)
            .map(|source| self.native_sources[source.0].clone())
    }
    pub(crate) fn defer(&mut self, module: ModuleId, raw: TermId) -> FormId {
        if let Some(id) = self.parsed.get(&(module, raw)) {
            return *id;
        }
        let id = FormId(self.nodes.len());
        self.nodes.push(Form {
            module,
            raw,
            value: TypeForm::DeferredRaw,
        });
        self.parsed.insert((module, raw), id);
        id
    }
    pub(crate) fn materialize(&mut self, source: &impl FormTerms, id: FormId) -> Result<()> {
        if !matches!(self.nodes[id.0].value, TypeForm::DeferredRaw) {
            return Ok(());
        }
        let module = self.nodes[id.0].module;
        let shape = Shape::read(source, self.nodes[id.0].raw, true)?;
        let provenance = self.native_provenance.get(&id).copied();
        let children = shape
            .children
            .iter()
            .map(|raw| match provenance {
                Some(source) => self.defer_source(module, source, *raw),
                None => self.defer(module, *raw),
            })
            .collect();
        self.nodes[id.0].value = shape.into_value(source.name(), children);
        Ok(())
    }
    pub(crate) fn parse(
        &mut self,
        module: &impl FormTerms,
        mid: ModuleId,
        root: TermId,
    ) -> Result<FormId> {
        let mut work = vec![(root, false)];
        while let Some((raw, ready)) = work.pop() {
            if self.eager_parsed.contains(&(mid, raw)) {
                continue;
            }
            let shape = Shape::read(module, raw, false)?;
            if !ready {
                work.push((raw, true));
                for child in shape.children.iter().rev() {
                    if !self.eager_parsed.contains(&(mid, *child)) {
                        work.push((*child, false));
                    }
                }
                continue;
            }
            let ids = shape
                .children
                .iter()
                .map(|id| self.parsed[&(mid, *id)])
                .collect::<Vec<_>>();
            let value = shape.into_value(module.name(), ids);
            let id = if let Some(id) = self.parsed.get(&(mid, raw)).copied() {
                self.nodes[id.0].value = value;
                id
            } else {
                let id = FormId(self.nodes.len());
                self.nodes.push(Form {
                    module: mid,
                    raw,
                    value,
                });
                id
            };
            self.parsed.insert((mid, raw), id);
            self.eager_parsed.insert((mid, raw));
        }
        Ok(self.parsed[&(mid, root)])
    }
}

enum ShapeKind {
    Variable(VariableId),
    Atom(String),
    Integer(BigInt),
    Operator(String),
    Annotated,
    Parenthesized,
    Builtin(Builtin, bool),
    UnrecognizedBuiltin(String, bool),
    Named((Option<String>, String), bool),
    BoundedFunction,
    Constraint(String),
}
struct Shape {
    kind: ShapeKind,
    children: Vec<TermId>,
}
impl Shape {
    fn into_value(self, module: &str, ids: Vec<FormId>) -> TypeForm {
        match self.kind {
            ShapeKind::Variable(name) => TypeForm::Variable(name),
            ShapeKind::Atom(name) => TypeForm::Atom(name),
            ShapeKind::Integer(n) => TypeForm::Integer(n),
            ShapeKind::Operator(name) => TypeForm::Operator {
                name,
                arguments: ids,
            },
            ShapeKind::Annotated => TypeForm::Annotated {
                variable: ids[0],
                body: ids[1],
            },
            ShapeKind::Parenthesized => TypeForm::Parenthesized(ids[0]),
            ShapeKind::Builtin(name, any) => TypeForm::Builtin {
                name,
                arguments: (!any).then_some(ids),
            },
            ShapeKind::UnrecognizedBuiltin(name, any) => TypeForm::UnrecognizedBuiltin {
                name,
                arguments: (!any).then_some(ids),
            },
            ShapeKind::Named(name, remote) => TypeForm::Named {
                name: TypeName {
                    module: name.0.unwrap_or_else(|| module.to_owned()),
                    name: name.1,
                    arity: ids.len(),
                },
                remote,
                arguments: ids,
            },
            ShapeKind::BoundedFunction => TypeForm::BoundedFunction {
                function: ids[0],
                constraints: ids[1..].to_vec(),
            },
            ShapeKind::Constraint(name) => TypeForm::Constraint {
                name,
                arguments: ids,
            },
        }
    }
    fn read(module: &impl FormTerms, raw: TermId, native: bool) -> Result<Self> {
        let parts = tuple(module, raw)?;
        let tag = parts
            .first()
            .map(|p| atom(module, *p))
            .transpose()?
            .ok_or_else(|| format!("{} term{}: empty type AST", module.name(), raw.0))?;
        let invalid = || format!("{} term{}: malformed {tag} type form", module.name(), raw.0);
        let (kind, children) = match (tag, parts) {
            ("var", [_, _, name]) => (
                ShapeKind::Variable(match &module.terms()[name.0] {
                    ErlangTerm::Atom(v) => VariableId::Atom(v.clone()),
                    ErlangTerm::Integer(v) => {
                        VariableId::Integer(v.parse().map_err(|_| invalid())?)
                    }
                    _ => return Err(invalid()),
                }),
                vec![],
            ),
            ("atom", [_, _, name]) => (ShapeKind::Atom(atom(module, *name)?.into()), vec![]),
            ("integer" | "char", [_, _, value]) => {
                let ErlangTerm::Integer(n) = &module.terms()[value.0] else {
                    return Err(invalid());
                };
                (
                    ShapeKind::Integer(n.parse().map_err(|_| invalid())?),
                    vec![],
                )
            }
            ("op", [_, _, name, args @ ..]) if (1..=2).contains(&args.len()) => (
                ShapeKind::Operator(atom(module, *name)?.into()),
                args.to_vec(),
            ),
            ("ann_type", [_, _, args]) => {
                let args = list(module, *args)?;
                if args.len() != 2 {
                    return Err(invalid());
                }
                (ShapeKind::Annotated, args)
            }
            ("paren_type", [_, _, args]) => {
                let args = list(module, *args)?;
                if args.len() != 1 {
                    return Err(invalid());
                }
                (ShapeKind::Parenthesized, args)
            }
            ("user_type", [_, _, name, args]) => (
                ShapeKind::Named((None, atom(module, *name)?.into()), false),
                list(module, *args)?,
            ),
            ("remote_type", [_, _, args]) => {
                let args = list(module, *args)?;
                let [m, n, a] = args.as_slice() else {
                    return Err(invalid());
                };
                (
                    ShapeKind::Named(
                        (Some(literal_atom(module, *m)?), literal_atom(module, *n)?),
                        true,
                    ),
                    list(module, *a)?,
                )
            }
            ("type", [_, _, name, args]) => {
                let name = atom(module, *name)?;
                if name == "bounded_fun" {
                    let args = list(module, *args)?;
                    let [fun, constraints] = args.as_slice() else {
                        return Err(invalid());
                    };
                    let mut children = vec![*fun];
                    children.extend(list(module, *constraints)?);
                    (ShapeKind::BoundedFunction, children)
                } else if name == "constraint" {
                    let args = list(module, *args)?;
                    let [name, args] = args.as_slice() else {
                        return Err(invalid());
                    };
                    (
                        ShapeKind::Constraint(literal_atom(module, *name)?),
                        list(module, *args)?,
                    )
                } else {
                    let any = matches!(&module.terms()[args.0],ErlangTerm::Atom(s) if s=="any");
                    let children = if any {
                        Vec::new()
                    } else {
                        list(module, *args)?
                    };
                    let kind = match Builtin::parse(name) {
                        Some(builtin) => ShapeKind::Builtin(builtin, any),
                        None if native => ShapeKind::UnrecognizedBuiltin(name.into(), any),
                        None => {
                            return Err(format!(
                                "{} term{}: unsupported builtin type {name}",
                                module.name(),
                                raw.0
                            ));
                        }
                    };
                    (kind, children)
                }
            }
            ("type", [_, _, name]) if atom(module, *name)? == "any" => {
                (ShapeKind::Builtin(Builtin::Any, true), vec![])
            }
            _ => return Err(invalid()),
        };
        Ok(Self { kind, children })
    }
}

fn atom(module: &impl FormTerms, id: TermId) -> Result<&str> {
    if let ErlangTerm::Atom(s) = &module.terms()[id.0] {
        Ok(s)
    } else {
        Err(format!("{} term{}: expected atom", module.name(), id.0))
    }
}
fn tuple(module: &impl FormTerms, id: TermId) -> Result<&[TermId]> {
    if let ErlangTerm::Tuple(xs) = &module.terms()[id.0] {
        Ok(xs)
    } else {
        Err(format!("{} term{}: expected tuple", module.name(), id.0))
    }
}
fn exact_tuple<const N: usize>(module: &impl FormTerms, id: TermId) -> Result<[TermId; N]> {
    tuple(module, id)?
        .try_into()
        .map_err(|_| format!("{} term{}: expected {N}-tuple", module.name(), id.0))
}
fn list(module: &impl FormTerms, mut id: TermId) -> Result<Vec<TermId>> {
    let mut result = vec![];
    loop {
        match &module.terms()[id.0] {
            ErlangTerm::Nil => return Ok(result),
            ErlangTerm::List { items, tail } => {
                result.extend(items);
                id = *tail
            }
            _ => {
                return Err(format!(
                    "{} term{}: expected proper list",
                    module.name(),
                    id.0
                ));
            }
        }
    }
}
fn literal_atom(module: &impl FormTerms, id: TermId) -> Result<String> {
    let [tag, _, name] = exact_tuple::<3>(module, id)?;
    if atom(module, tag)? != "atom" {
        return Err(format!("{} term{}: expected atom AST", module.name(), id.0));
    }
    Ok(atom(module, name)?.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::NodeId;
    #[test]
    fn deferred_forms_skip_descendants_and_promote_without_changing_ids() {
        let source = FormSource {
            name: "m",
            terms: &crate::shared_vec::SharedVec::from(vec![
                ErlangTerm::Nil,
                ErlangTerm::Integer("0".into()),
                ErlangTerm::Atom("type".into()),
                ErlangTerm::Atom("list".into()),
                ErlangTerm::Atom("bad_child".into()),
                ErlangTerm::List {
                    items: vec![TermId(4)],
                    tail: TermId(0),
                },
                ErlangTerm::Tuple(vec![TermId(2), TermId(1), TermId(3), TermId(5)]),
                ErlangTerm::Atom("integer".into()),
                ErlangTerm::Integer("1".into()),
                ErlangTerm::Tuple(vec![TermId(7), TermId(1), TermId(8)]),
                ErlangTerm::List {
                    items: vec![TermId(9)],
                    tail: TermId(0),
                },
                ErlangTerm::Tuple(vec![TermId(2), TermId(1), TermId(3), TermId(10)]),
                ErlangTerm::Atom("unknown_builtin".into()),
                ErlangTerm::Tuple(vec![TermId(2), TermId(1), TermId(12), TermId(0)]),
            ]),
        };
        let mut forms = Forms::default();
        let bad = forms.defer(ModuleId(0), TermId(6));
        forms.materialize(&source, bad).unwrap();
        let TypeForm::Builtin {
            arguments: Some(ids),
            ..
        } = &forms.nodes[bad.0].value
        else {
            panic!("list");
        };
        assert!(matches!(forms.nodes[ids[0].0].value, TypeForm::DeferredRaw));
        assert!(forms.parse(&source, ModuleId(0), TermId(6)).is_err());
        let good = forms.defer(ModuleId(0), TermId(11));
        forms.materialize(&source, good).unwrap();
        let count = forms.nodes.len();
        assert_eq!(forms.parse(&source, ModuleId(0), TermId(11)).unwrap(), good);
        assert_eq!(forms.nodes.len(), count);
        let unknown = forms.defer(ModuleId(0), TermId(13));
        forms.materialize(&source, unknown).unwrap();
        assert!(matches!(
            forms.nodes[unknown.0].value,
            TypeForm::UnrecognizedBuiltin { .. }
        ));
        assert!(forms.parse(&source, ModuleId(0), TermId(13)).is_err());
    }
    struct Builder(CoreModule);
    impl Builder {
        fn new() -> Self {
            Self(CoreModule {
                name: "m".into(),
                source_forms: TermId(0),
                core: NodeId(0),
                terms: vec![ErlangTerm::Nil, ErlangTerm::Integer("1".into())].into(),
                nodes: crate::shared_vec::SharedVec::new(),
                map_iterator_orders: BTreeMap::new(),
                contract_variable_maps: BTreeMap::new(),
            })
        }
        fn term(&mut self, term: ErlangTerm) -> TermId {
            let id = TermId(self.0.terms.len());
            self.0.terms.push(term);
            id
        }
        fn atom(&mut self, name: &str) -> TermId {
            self.term(ErlangTerm::Atom(name.into()))
        }
        fn list(&mut self, items: Vec<TermId>) -> TermId {
            self.term(ErlangTerm::List {
                items,
                tail: TermId(0),
            })
        }
        fn ast(&mut self, tag: &str, payload: Vec<TermId>) -> TermId {
            let tag = self.atom(tag);
            let mut items = vec![tag, TermId(1)];
            items.extend(payload);
            self.term(ErlangTerm::Tuple(items))
        }
        fn builtin(&mut self, name: &str, args: Vec<TermId>) -> TermId {
            let name = self.atom(name);
            let args = self.list(args);
            self.ast("type", vec![name, args])
        }
        fn variable(&mut self, name: &str) -> TermId {
            let name = self.atom(name);
            self.ast("var", vec![name])
        }
        fn literal_atom(&mut self, name: &str) -> TermId {
            let name = self.atom(name);
            self.ast("atom", vec![name])
        }
    }
    #[test]
    fn bounded_constraints_keep_variable_identity_and_source_terms() {
        let mut b = Builder::new();
        let x = b.variable("X");
        let integer = b.builtin("integer", vec![]);
        let domain = b.builtin("product", vec![x]);
        let fun = b.builtin("fun", vec![domain, x]);
        let subtype = b.literal_atom("is_subtype");
        let args = b.list(vec![x, integer]);
        let constraint = b.builtin("constraint", vec![subtype, args]);
        let constraints = b.list(vec![constraint]);
        let root = b.builtin("bounded_fun", vec![fun, constraints]);
        let mut forms = Forms::default();
        let id = forms.parse(&b.0, ModuleId(0), root).unwrap();
        let TypeForm::BoundedFunction {
            function,
            constraints,
        } = &forms.nodes[id.0].value
        else {
            panic!()
        };
        assert_eq!(forms.nodes[function.0].raw, fun);
        assert_eq!(constraints.len(), 1);
        let TypeForm::Constraint { name, arguments } = &forms.nodes[constraints[0].0].value else {
            panic!()
        };
        assert_eq!(name, "is_subtype");
        assert_eq!(arguments.len(), 2);
        assert!(
            matches!(&forms.nodes[arguments[0].0].value,TypeForm::Variable(name) if name==&VariableId::Atom("X".into()))
        );
        assert_eq!(forms.nodes[arguments[1].0].raw, integer);
    }
    #[test]
    fn local_remote_and_generic_domains_remain_distinct() {
        let mut b = Builder::new();
        let any = b.atom("any");
        let tuple = b.atom("tuple");
        let tuple = b.ast("type", vec![tuple, any]);
        let name = b.atom("t");
        let args = b.list(vec![tuple]);
        let local = b.ast("user_type", vec![name, args]);
        let module = b.literal_atom("other");
        let name = b.literal_atom("t");
        let fields = b.list(vec![module, name, args]);
        let remote = b.ast("remote_type", vec![fields]);
        let mut forms = Forms::default();
        let local = forms.parse(&b.0, ModuleId(0), local).unwrap();
        let remote = forms.parse(&b.0, ModuleId(0), remote).unwrap();
        assert!(
            matches!(&forms.nodes[local.0].value,TypeForm::Named{name,remote:false,..} if name.module=="m"&&name.arity==1)
        );
        assert!(
            matches!(&forms.nodes[remote.0].value,TypeForm::Named{name,remote:true,..} if name.module=="other"&&name.arity==1)
        );
        assert_eq!(forms.nodes.iter().filter(|f| f.raw == tuple).count(), 1);
    }
    #[test]
    fn deep_forms_use_an_arena_and_reject_unknown_grammar() {
        let mut b = Builder::new();
        let mut root = b.builtin("integer", vec![]);
        for _ in 0..10000 {
            root = b.builtin("list", vec![root]);
        }
        let mut forms = Forms::default();
        forms.parse(&b.0, ModuleId(0), root).unwrap();
        assert_eq!(forms.nodes.len(), 10001);
        let unknown = b.builtin("made_up", vec![]);
        assert!(
            forms
                .parse(&b.0, ModuleId(0), unknown)
                .unwrap_err()
                .contains("unsupported builtin type made_up")
        );
        let malformed = b.term(ErlangTerm::Tuple(vec![]));
        assert!(
            forms
                .parse(&b.0, ModuleId(0), malformed)
                .unwrap_err()
                .contains("empty type AST")
        );
    }
}

#[cfg(test)]
mod native_source_tests {
    use super::*;
    use crate::{core::ErlangTerms, native_form_environment::RawFormTerm};
    fn source(value: &str) -> RawFormTerm {
        let terms = vec![
            ErlangTerm::Atom("type".into()), ErlangTerm::Integer("0".into()),
            ErlangTerm::Atom("tuple".into()), ErlangTerm::Atom("atom".into()),
            ErlangTerm::Atom(value.into()), ErlangTerm::Tuple(vec![TermId(3),TermId(1),TermId(4)]),
            ErlangTerm::Nil, ErlangTerm::List { items: vec![TermId(5)], tail: TermId(6) },
            ErlangTerm::Tuple(vec![TermId(0),TermId(1),TermId(2),TermId(7)]),
        ];
        RawFormTerm::new(Arc::new(ErlangTerms { root: TermId(8), terms: terms.into() }), TermId(8)).unwrap()
    }
    fn materialize(forms: &mut Forms, id: FormId) {
        let arena = forms.source_arena(id).unwrap();
        forms.materialize(&FormSource { name: "m", terms: &arena.terms }, id).unwrap();
    }
    fn child(forms: &Forms, id: FormId) -> FormId {
        let TypeForm::Builtin { arguments: Some(children), .. } = &forms.nodes[id.0].value else { panic!() };
        children[0]
    }
    #[test]
    fn equal_term_ids_from_distinct_arenas_keep_nested_provenance() {
        let first = source("first"); let second = source("second");
        let mut forms = Forms::default();
        let a = forms.defer_raw(ModuleId(0), &first);
        assert_eq!(forms.defer_raw(ModuleId(0), &first), a);
        let b = forms.defer_raw(ModuleId(0), &second); assert_ne!(a,b);
        materialize(&mut forms,a); materialize(&mut forms,b);
        let ac = child(&forms,a); let bc = child(&forms,b); assert_ne!(ac,bc);
        assert!(Arc::ptr_eq(&forms.source_arena(ac).unwrap(),first.arena()));
        assert!(Arc::ptr_eq(&forms.source_arena(bc).unwrap(),second.arena()));
        materialize(&mut forms,ac); materialize(&mut forms,bc);
        assert!(matches!(&forms.nodes[ac.0].value,TypeForm::Atom(v) if v=="first"));
        assert!(matches!(&forms.nodes[bc.0].value,TypeForm::Atom(v) if v=="second"));
    }
    #[test]
    fn cloned_forms_retain_existing_sources_and_isolate_new_forms() {
        let first=source("first"); let second=source("second");
        let mut original=Forms::default(); let a=original.defer_raw(ModuleId(0),&first);
        let mut clone=original.clone(); assert_eq!(clone.defer_raw(ModuleId(0),&first),a);
        clone.defer_raw(ModuleId(0),&second); assert_eq!(original.nodes.len(),1);
        assert_eq!(original.native_sources.len(),1); assert_eq!(clone.native_sources.len(),2);
        materialize(&mut clone,a); assert!(matches!(original.nodes[a.0].value,TypeForm::DeferredRaw));
        assert!(Arc::ptr_eq(&clone.source_arena(a).unwrap(),first.arena()));
    }
}

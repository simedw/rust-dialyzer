use std::{sync::OnceLock, collections::BTreeMap};

use crate::{
    atom::Atom,
    bifs::registry::Mfa,
    core::{CoreExpr, CoreProgram, ErlangTerm, ErlangTerms, NodeId},
    dataflow_map::DataflowMap,
    fast_hash::FastHashMap,
    labels::{CoreLabel, Labels},
    lattice::{Known, LatticeError, Type, TypeGetter, TypePredicate, TypeValue, VariableId},
    literal_types::LiteralTypes,
    program::{ModuleId, NodeRef},
};

pub struct DataflowNodes<'a> {
    core: &'a CoreProgram,
    labels: &'a Labels,
    literals: Vec<LiteralTypes>,
    contains_map: Vec<Vec<bool>>,
    named_variables: OnceLock<Result<BTreeMap<VariableId, Mfa>, String>>,
    definitions: OnceLock<FastHashMap<Atom, ModuleDefinitions>>,
}
/// Top-level definitions of the first module bearing a name, in the form a
/// linear scan with `named_mfa` would see them: the first definition per
/// name/arity, up to the first binder `named_mfa` rejects.
enum ModuleDefinitions {
    NotModule,
    Indexed {
        module: ModuleId,
        functions: FastHashMap<(Atom, usize), (NodeId, NodeId)>,
        error: Option<String>,
    },
}
pub enum DefinitionLookupError {
    ModuleAbsent,
    NotModule,
    Malformed(String),
    FunctionAbsent,
}
impl<'a> DataflowNodes<'a> {
    pub fn core(&self) -> &'a CoreProgram {
        self.core
    }
    pub fn labels(&self) -> &'a Labels {
        self.labels
    }
    pub fn named_variables(&self) -> Result<&BTreeMap<VariableId, Mfa>, String> {
        self.named_variables
            .get_or_init(|| {
                let mut named_variables = BTreeMap::new();
                for (module, m) in self.core.modules.iter().enumerate() {
                    if let CoreExpr::Module { definitions, .. } = &m.nodes[m.core.0].expr {
                        for (variable, _) in definitions {
                            let mfa = m.named_mfa(*variable)?;
                            let label = self
                                .labels
                                .label(NodeRef {
                                    module: ModuleId(module),
                                    node: variable.0,
                                })
                                .ok_or_else(|| "named variable label missing".to_owned())?;
                            named_variables.insert(VariableId::Integer(label.0.into()), mfa);
                        }
                    }
                }
                Ok(named_variables)
            })
            .as_ref()
            .map_err(Clone::clone)
    }
    /// The module and `(binder, fun)` definition named by `mfa`; equivalent
    /// to scanning the first module named `mfa.module` with `named_mfa`.
    pub fn definition(
        &self,
        mfa: &Mfa,
    ) -> Result<(ModuleId, NodeId, NodeId), DefinitionLookupError> {
        let definitions = self.definitions.get_or_init(|| {
            let mut index = FastHashMap::default();
            for (module, m) in self.core.modules.iter().enumerate() {
                index
                    .entry(Atom::new(&m.name))
                    .or_insert_with(|| Self::index_module(ModuleId(module), m));
            }
            index
        });
        match definitions.get(&mfa.module) {
            None => Err(DefinitionLookupError::ModuleAbsent),
            Some(ModuleDefinitions::NotModule) => Err(DefinitionLookupError::NotModule),
            Some(ModuleDefinitions::Indexed {
                module,
                functions,
                error,
            }) => match (functions.get(&(mfa.function, mfa.arity)), error) {
                (Some((variable, function)), _) => Ok((*module, *variable, *function)),
                (None, Some(error)) => Err(DefinitionLookupError::Malformed(error.clone())),
                (None, None) => Err(DefinitionLookupError::FunctionAbsent),
            },
        }
    }
    fn index_module(module: ModuleId, m: &crate::core::CoreModule) -> ModuleDefinitions {
        let CoreExpr::Module { definitions, .. } = &m.nodes[m.core.0].expr else {
            return ModuleDefinitions::NotModule;
        };
        let mut functions = FastHashMap::default();
        let mut error = None;
        for (variable, function) in definitions {
            match m.named_mfa(*variable) {
                Ok(mfa) => {
                    functions
                        .entry((mfa.function, mfa.arity))
                        .or_insert((*variable, *function));
                }
                Err(failure) => {
                    error = Some(failure);
                    break;
                }
            }
        }
        ModuleDefinitions::Indexed {
            module,
            functions,
            error,
        }
    }
    pub fn literal_types(&self) -> &[LiteralTypes] {
        &self.literals
    }
    pub fn build(core: &'a CoreProgram, labels: &'a Labels) -> Result<Self, String> {
        let modules = crate::ordered_parallel::setup_map(&core.modules, |module| {
            let mut maps: Vec<bool> = Vec::with_capacity(module.terms.len());
            for term in &module.terms {
                let value = match term {
                    ErlangTerm::Map(_) => true,
                    ErlangTerm::Tuple(items) => items.iter().any(|id| maps[id.0]),
                    ErlangTerm::List { items, tail } => {
                        maps[tail.0] || items.iter().any(|id| maps[id.0])
                    }
                    _ => false,
                };
                maps.push(value);
            }
            let roots = module
                .nodes
                .iter()
                .filter_map(|node| match node.expr {
                    CoreExpr::Literal(id) => Some(id),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let literals = LiteralTypes::build_roots(
                &ErlangTerms {
                    root: module.source_forms,
                    terms: module.terms.clone(),
                },
                &roots,
            )
            .map_err(|error| format!("module {}: {error}", module.name))?;
            Ok::<_, String>((literals, maps))
        });
        let mut literals = Vec::with_capacity(modules.len());
        let mut contains_map = Vec::with_capacity(modules.len());
        for module in modules {
            let (module_literals, maps) = module?;
            literals.push(module_literals);
            contains_map.push(maps);
        }
        Ok(Self {
            core,
            labels,
            literals,
            contains_map,
            named_variables: OnceLock::new(),
            definitions: OnceLock::new(),
        })
    }
    pub fn module(&self, id: ModuleId) -> Result<&crate::core::CoreModule, String> {
        self.core
            .modules
            .get(id.0)
            .ok_or("module outside Core arena".into())
    }
    pub fn contains_map(&self, module: ModuleId, id: crate::core::TermId) -> Result<bool, String> {
        self.contains_map
            .get(module.0)
            .and_then(|m| m.get(id.0))
            .copied()
            .ok_or("term outside literal arena".into())
    }
    pub fn literal_type(&self, module: ModuleId, id: crate::core::TermId) -> Result<Type, String> {
        self.literals
            .get(module.0)
            .and_then(|m| m.get(id))
            .cloned()
            .ok_or("literal outside typed arena".into())
    }
    pub fn expression(&self, node: NodeRef) -> Result<&CoreExpr, String> {
        self.core
            .modules
            .get(node.module.0)
            .and_then(|module| module.nodes.get(node.node))
            .map(|node| &node.expr)
            .ok_or_else(|| "Core node outside module arena".into())
    }
    pub fn optional_label(&self, node: NodeRef) -> Result<Option<CoreLabel>, String> {
        self.expression(node)?;
        self.labels
            .nodes
            .get(node.module.0)
            .and_then(|nodes| nodes.get(node.node))
            .copied()
            .ok_or("node outside label arena".into())
    }
    pub fn label(&self, node: NodeRef) -> Result<CoreLabel, String> {
        self.expression(node)?;
        self.labels
            .nodes
            .get(node.module.0)
            .and_then(|nodes| nodes.get(node.node))
            .copied()
            .flatten()
            .ok_or_else(|| "Core node has no native label".into())
    }
    pub fn lookup(&self, node: NodeRef, map: &DataflowMap) -> Result<Type, String> {
        match self.expression(node)? {
            CoreExpr::Literal(id) => self.literals[node.module.0]
                .get(*id)
                .cloned()
                .ok_or_else(|| "literal outside term arena".into()),
            _ => Ok(map.lookup(self.label(node)?)),
        }
    }
    pub fn enter_type(
        &self,
        node: NodeRef,
        typ: Type,
        map: &mut DataflowMap,
    ) -> Result<(), String> {
        let mut pending = vec![(node, typ)];
        while let Some((node, typ)) = pending.pop() {
            match self.expression(node)? {
                CoreExpr::Literal(_) => {}
                CoreExpr::Values(nodes) => {
                    let types = if typ.predicate(TypePredicate::Any)?
                        || typ.predicate(TypePredicate::None)?
                    {
                        vec![typ; nodes.len()]
                    } else {
                        let TypeValue::Types(Known::Known(types)) =
                            typ.get(TypeGetter::ToTypeList)?
                        else {
                            return Err("type-list getter returned an invalid shape".into());
                        };
                        types
                    };
                    if nodes.len() != types.len() {
                        return Err(LatticeError::FunctionClause.into());
                    }
                    pending.extend(nodes.iter().zip(types).rev().map(|(id, typ)| {
                        (
                            NodeRef {
                                module: node.module,
                                node: id.0,
                            },
                            typ,
                        )
                    }));
                }
                _ => map.enter_type(self.label(node)?, typ),
            }
        }
        Ok(())
    }
    pub fn enter_substitution(
        &self,
        key: NodeRef,
        value: NodeRef,
        map: &mut DataflowMap,
    ) -> Result<(), String> {
        let key = self.label(key)?;
        match self.expression(value)? {
            CoreExpr::Literal(id) if !self.contains_map[value.module.0][id.0] => {
                map.enter_literal_substitution(key, self.lookup(value, map)?)
            }
            CoreExpr::Var(_) => map.enter_variable_substitution(key, self.label(value)?),
            _ => {}
        }
        Ok(())
    }
    pub fn native_subtrees(&self, node: NodeRef) -> Result<Vec<NodeRef>, String> {
        let expr = self.expression(node)?;
        let ids = expr.native_children();
        Ok(ids
            .into_iter()
            .map(|id| Self::node(node.module, id))
            .collect())
    }
    pub fn mark_fresh(&self, nodes: &[NodeRef], map: &mut DataflowMap) -> Result<(), String> {
        let mut pending: Vec<_> = nodes.iter().rev().copied().collect();
        while let Some(node) = pending.pop() {
            let expr = self.expression(node)?;
            if matches!(expr, CoreExpr::Var(_)) {
                map.enter_type(self.label(node)?, Type::any());
            }
            pending.extend(fresh_children(expr).into_iter().rev().map(|id| NodeRef {
                module: node.module,
                node: id.0,
            }));
        }
        Ok(())
    }
    pub fn node(module: ModuleId, node: NodeId) -> NodeRef {
        NodeRef {
            module,
            node: node.0,
        }
    }
}

fn fresh_children(expr: &CoreExpr) -> Vec<NodeId> {
    match expr {
        CoreExpr::Literal(_) | CoreExpr::Var(_) | CoreExpr::Opaque(_) => vec![],
        CoreExpr::Values(nodes) | CoreExpr::Binary(nodes) | CoreExpr::Tuple(nodes) => nodes.clone(),
        CoreExpr::Bitstr {
            value,
            unit,
            typ,
            flags,
            ..
        } => vec![*value, *unit, *typ, *flags],
        CoreExpr::MapPair {
            operation, value, ..
        } => vec![*operation, *value],
        CoreExpr::Cons { head, tail } => vec![*head, *tail],
        CoreExpr::Map { pairs, .. } => pairs.clone(),
        CoreExpr::Alias { variable, pattern } => vec![*variable, *pattern],
        CoreExpr::Let {
            vars,
            argument,
            body,
        } => vars.iter().copied().chain([*argument, *body]).collect(),
        CoreExpr::Seq { argument, body } => vec![*argument, *body],
        CoreExpr::Apply { operator, args } => std::iter::once(*operator)
            .chain(args.iter().copied())
            .collect(),
        CoreExpr::Call { module, name, args } => [*module, *name]
            .into_iter()
            .chain(args.iter().copied())
            .collect(),
        CoreExpr::Primop { name, args } => {
            std::iter::once(*name).chain(args.iter().copied()).collect()
        }
        CoreExpr::Case { argument, clauses } => std::iter::once(*argument)
            .chain(clauses.iter().copied())
            .collect(),
        CoreExpr::Clause {
            patterns,
            guard,
            body,
        } => patterns.iter().copied().chain([*guard, *body]).collect(),
        CoreExpr::Fun { params, body } => params.iter().copied().chain([*body]).collect(),
        CoreExpr::Receive {
            clauses,
            timeout,
            action,
        } => clauses.iter().copied().chain([*timeout, *action]).collect(),
        CoreExpr::Try {
            argument,
            vars,
            body,
            exception_vars,
            handler,
        } => std::iter::once(*argument)
            .chain(vars.iter().copied())
            .chain([*body])
            .chain(exception_vars.iter().copied())
            .chain([*handler])
            .collect(),
        CoreExpr::Catch { body } => vec![*body],
        CoreExpr::Letrec { definitions, body } => definitions
            .iter()
            .flat_map(|(var, fun)| [*var, *fun])
            .chain([*body])
            .collect(),
        CoreExpr::Module {
            name,
            exports,
            attributes,
            definitions,
        } => std::iter::once(*name)
            .chain(exports.iter().copied())
            .chain(attributes.iter().flat_map(|(key, value)| [*key, *value]))
            .chain(definitions.iter().flat_map(|(var, fun)| [*var, *fun]))
            .collect(),
    }
}

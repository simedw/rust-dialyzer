use crate::{
    bifs::registry::{Mfa, Registry},
    constraints::atomic::Relation,
    constraints::closure_order::ClosureCodeProfile,
    constraints::derived::{BifResultProvider, DerivedOp, EvalError},
    constraints::normalize::{
        ClosureOrigin, GroupKind, NativeConstraintArena, NativeConstraintId, NativeOperand,
        NormalizationError,
    },
    constraints::preparation::{
        DependencyRetention, FunctionBody, FunctionParameters, PreparedConstruction,
    },
    constraints::tree::{FunctionVariable, PreparedTree},
    contract_runtime::ContractSummary,
    core::{CoreExpr, CoreModule, CoreProgram, ErlangTerm, NodeId},
    dataflow_nodes::DataflowNodes,
    labels::{CoreLabel, Labels},
    lattice::{LatticeError, NativeValue, Type, VariableId},
    literal_types::LiteralTypes,
    patterns::Patterns,
    program::{ModuleId, NodeRef},
    term_order::{CaptureEnvironment, CaptureId, CaptureTerm, TermOrderArena},
};
use std::{collections::BTreeMap, sync::Arc};
#[derive(Debug)]
pub enum GenerationError {
    ConstructionFailure,
    Native(LatticeError),
    BitstringDomain(crate::lattice::BitstringDomainError),
    Evaluation(EvalError),
    Boundary(String),
    Unsupported { node: NodeRef, kind: &'static str },
    UnsupportedPrimop { node: NodeRef, name: ErlangTerm },
    Normalization(NormalizationError),
    Underapprox(crate::underapprox::UnderapproxError),
}
impl From<LatticeError> for GenerationError {
    fn from(v: LatticeError) -> Self {
        Self::Native(v)
    }
}
impl From<NormalizationError> for GenerationError {
    fn from(v: NormalizationError) -> Self {
        match v {
            NormalizationError::Unsatisfiable => Self::ConstructionFailure,
            _ => Self::Normalization(v),
        }
    }
}
type Result<T> = std::result::Result<T, GenerationError>;
type Defined = im::OrdSet<VariableId>;
#[derive(Clone, Debug)]
pub struct BifConstruction {
    pub known: bool,
    pub generic_return: Type,
    pub arguments: Vec<Type>,
}
pub trait SourceBifProvider: BifResultProvider {
    fn construction(&self, mfa: &Mfa) -> std::result::Result<BifConstruction, EvalError>;
}
impl SourceBifProvider for Registry {
    fn construction(&self, mfa: &Mfa) -> std::result::Result<BifConstruction, EvalError> {
        let known = self.is_known(mfa);
        let arguments = if known {
            self.argument_domains(mfa)?
        } else {
            vec![Type::any(); mfa.arity]
        };
        let generic_return = if known || self.is_runtime_builtin(mfa) {
            self.result(mfa, &vec![Type::any(); mfa.arity])?
        } else {
            Type::any()
        };
        Ok(BifConstruction {
            known,
            generic_return,
            arguments,
        })
    }
}
impl SourceBifProvider for crate::constraints::derived::NoBifProvider {
    fn construction(&self, mfa: &Mfa) -> std::result::Result<BifConstruction, EvalError> {
        self.result(mfa, &[]).map(|generic_return| BifConstruction {
            known: false,
            generic_return,
            arguments: vec![],
        })
    }
}

#[derive(Clone, Debug)]
pub struct CallSignature {
    pub arguments: Vec<Type>,
    pub returns: Type,
}
pub struct ActiveContract {
    summary: Arc<ContractSummary>,
    captured_record: CaptureEnvironment,
}
impl ActiveContract {
    pub fn new(
        summary: Arc<ContractSummary>,
        captured_record: CaptureEnvironment,
    ) -> std::result::Result<Self, String> {
        if captured_record.len() != 1 {
            return Err("contract capture must contain one source contract record".into());
        }
        Ok(Self {
            summary,
            captured_record,
        })
    }
}
pub trait LibraryTypes {
    fn signature(&self, mfa: &Mfa) -> std::result::Result<Option<CallSignature>, String>;
    fn active_contract(&self, mfa: &Mfa) -> std::result::Result<Option<ActiveContract>, String>;
}
pub struct EmptyLibrary;
impl LibraryTypes for EmptyLibrary {
    fn active_contract(&self, _: &Mfa) -> std::result::Result<Option<ActiveContract>, String> {
        Ok(None)
    }
    fn signature(&self, _: &Mfa) -> std::result::Result<Option<CallSignature>, String> {
        Ok(None)
    }
}
/// Inputs arise from the preceding native success-typing phase, never from own specifications.
#[derive(Default)]
pub struct PropagatedTypes(pub BTreeMap<CoreLabel, Type>);
#[derive(Clone, Default)]
struct State {
    context: Vec<NativeConstraintId>,
    bodies: im::OrdMap<FunctionVariable, NativeConstraintId>,
    parameters: im::OrdMap<FunctionVariable, FunctionParameters>,
    recursive_variables: im::OrdMap<VariableId, VariableId>,
    next_label: u64,
    module: Option<ModuleId>,
    record_cache: Vec<ModuleId>,
    scc: im::OrdSet<FunctionVariable>,
    in_guard: bool,
    in_match: bool,
    missing_library_summaries: std::collections::BTreeSet<Mfa>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenerationOutput {
    BindingsOnly,
    WithSourceState,
}
pub struct GeneratedConstraints {
    pub dependency_retention: DependencyRetention,
    pub source_state: Option<CaptureEnvironment>,
    pub arena: NativeConstraintArena,
    pub functions: Vec<FunctionBody>,
    pub parameters: BTreeMap<FunctionVariable, FunctionParameters>,
    pub entries: Vec<FunctionVariable>,
    pub next_label: CoreLabel,
    pub refold_allocations: u64,
    pub missing_library_summaries: std::collections::BTreeSet<Mfa>,
}
impl GeneratedConstraints {
    pub fn prepare(mut self) -> Result<PreparedTree> {
        let prepared = PreparedConstruction::build(
            &mut self.arena,
            &self.functions,
            self.dependency_retention,
        )
        .map_err(GenerationError::Boundary)?;
        prepared
            .into_tree(self.arena, self.parameters, self.entries)
            .map_err(GenerationError::Boundary)
    }
}
#[derive(Clone, Copy)]
struct Site {
    module: ModuleId,
    node: NodeId,
}
impl Site {
    fn reference(&self) -> NodeRef {
        NodeRef {
            module: self.module,
            node: self.node.0,
        }
    }
}
enum Finish {
    Tuple,
    Values,
    Cons,
    Alias(NodeId),
    DiscardFirst,
    Call(Mfa),
    DynamicCall,
    Apply,
    ApplyNamed(Type),
    Binary,
    Timeout,
    Bitstr {
        size: NodeId,
        unit: NodeId,
        typ: NodeId,
        flags: NodeId,
    },
}
enum PatternFinish {
    Tuple,
    Cons,
    Values,
}
struct MapContext {
    pattern: crate::patterns::PatternRef,
    origin: NodeRef,
    entries: Vec<crate::patterns::PatternRef>,
    pairs: Vec<crate::constraints::derived::MapAssociation>,
    next: usize,
    defined: Defined,
    in_match: bool,
    base: usize,
}
enum Frame {
    MapKey {
        context: MapContext,
        value: crate::patterns::PatternRef,
        operation: crate::constraints::derived::MapOperation,
    },
    MapValue {
        context: MapContext,
        key: Type,
        operation: crate::constraints::derived::MapOperation,
    },
    MapArgument {
        context: MapContext,
    },
    CaseArgument {
        site: Site,
        clauses: Vec<NodeId>,
        defined: Defined,
        base: usize,
    },
    ClausePatterns {
        context: CaseContext,
        guard: NodeId,
        body: NodeId,
        defined: Defined,
        count: usize,
    },
    ClauseGuard {
        context: CaseContext,
        body: NodeId,
        defined: Defined,
        pattern_state: State,
    },
    ClauseBody {
        context: CaseContext,
    },
    Visit(Site, Defined),
    PatternVisit {
        pattern: crate::patterns::PatternRef,
        origin: NodeRef,
        defined: Defined,
    },
    PatternFinish {
        pattern: crate::patterns::PatternRef,
        origin: NodeRef,
        kind: PatternFinish,
        count: usize,
    },
    Finish(Site, Finish, usize),
    Let {
        site: Site,
        vars: Vec<NodeId>,
        body: NodeId,
        defined: Defined,
    },
    LetrecBody {
        site: Site,
        body: NodeId,
        defined: Defined,
        count: usize,
    },
    TryArgument {
        context: TryContext,
    },
    TryBody {
        context: TryContext,
        argument: Type,
    },
    TryHandler {
        site: Site,
        body_state: State,
        body_result: Type,
        outer: State,
        base: usize,
    },
    Function {
        site: Site,
        params: Vec<NodeId>,
        saved: State,
        base: usize,
    },
}
struct TryContext {
    site: Site,
    vars: Vec<NodeId>,
    body: NodeId,
    exception_vars: Vec<NodeId>,
    handler: NodeId,
    defined: Defined,
    saved: State,
    outer: State,
    base: usize,
}
#[derive(Clone)]
struct CaseContext {
    site: Site,
    clauses: Vec<NodeId>,
    next: usize,
    argument: Type,
    defined: Defined,
    previous: State,
    outer_context: Vec<NativeConstraintId>,
    subtractions: Option<Vec<Type>>,
    next_subtractions: Option<Vec<Type>>,
    branches: Vec<NativeConstraintId>,
    base: usize,
}
/// Source-derived iterative traversal. Unsupported source branches remain explicit until ported.
pub struct Generator<'a> {
    dependency_retention: DependencyRetention,
    core: &'a CoreProgram,
    labels: &'a Labels,
    profile: &'a ClosureCodeProfile,
    propagated: &'a PropagatedTypes,
    provider: &'a dyn SourceBifProvider,
    library: &'a dyn LibraryTypes,
    callgraph: GenerationCallGraph<'a>,
    capture_inputs: Option<&'a GenerationCaptureInputs<'a>>,
    phase_entries: Vec<(ModuleId, NodeId, NodeId)>,
    scc_variables: BTreeMap<Mfa, Type>,
    named_variables: &'a BTreeMap<VariableId, Mfa>,
    self_recursive: Option<CoreLabel>,
    records: &'a BTreeMap<(ModuleId, String, usize), Type>,
    literal_types: &'a [LiteralTypes],
    patterns: Patterns<'a, 'a>,
    core_captures: BTreeMap<ModuleId, crate::core_capture::CoreCaptures>,
    arena: NativeConstraintArena,
    state: State,
}
#[derive(Clone, Copy)]
pub enum GenerationCallGraph<'a> {
    Absent,
    Indexed {
        program: &'a crate::program::Program,
        dependencies: &'a crate::closure_dependencies::ClosureDependencies,
    },
}
pub trait RecordTableCaptures {
    /// Singleton environment containing the original prepared module record table.
    fn module_table(&self, module: ModuleId) -> std::result::Result<&CaptureEnvironment, String>;
}
/// Lazy labeled Core records scoped to one original program and label allocation.
pub struct CoreCaptureStore<'a> {
    core: &'a CoreProgram,
    labels: &'a Labels,
    modules: Vec<std::sync::OnceLock<std::result::Result<crate::core_capture::CoreCaptures, String>>>,
}
impl<'a> CoreCaptureStore<'a> {
    pub fn new(core: &'a CoreProgram, labels: &'a Labels) -> Self {
        Self {
            core,
            labels,
            modules: (0..core.modules.len()).map(|_| std::sync::OnceLock::new()).collect(),
        }
    }
    fn matches(&self, core: &CoreProgram, labels: &Labels) -> bool {
        std::ptr::eq(self.core, core) && std::ptr::eq(self.labels, labels)
    }
    fn get(
        &self,
        module: ModuleId,
    ) -> std::result::Result<&crate::core_capture::CoreCaptures, String> {
        self.modules
            .get(module.0)
            .ok_or("capture module outside Core arena")?
            .get_or_init(|| {
                crate::core_capture::CoreCaptures::build(self.core, self.labels, module)
            })
            .as_ref()
            .map_err(Clone::clone)
    }
}
pub struct GenerationCaptureInputs<'a> {
    pub core_captures: Option<&'a CoreCaptureStore<'a>>,
    pub runtime: &'a crate::constraints::generation_capture_headers::GenerationRuntimeCaptures,
    pub records: &'a dyn RecordTableCaptures,
    pub initial_record_cache: &'a [ModuleId],
    pub solvers: &'a [String],
}
pub struct GenerationPhase<'a> {
    pub captures: Option<&'a GenerationCaptureInputs<'a>>,
    pub callgraph: GenerationCallGraph<'a>,
    pub propagated: &'a PropagatedTypes,
    pub library: &'a dyn LibraryTypes,
    pub records: &'a BTreeMap<(ModuleId, String, usize), Type>,
    pub self_recursive: Option<CoreLabel>,
}
impl<'a> Generator<'a> {
    pub fn new(
        nodes: &'a DataflowNodes<'a>,
        profile: &'a ClosureCodeProfile,
        provider: &'a dyn SourceBifProvider,
        phase: GenerationPhase<'a>,
    ) -> Result<Self> {
        let GenerationPhase {
            captures,
            callgraph,
            propagated,
            library,
            records,
            self_recursive,
        } = phase;
        let core = nodes.core();
        let labels = nodes.labels();
        if let Some(store) = captures.and_then(|inputs| inputs.core_captures) {
            if !store.matches(core, labels) {
                return Err(GenerationError::Boundary(
                    "Core capture storage belongs to a different program or labels".into(),
                ));
            }
        }
        let literal_types = nodes.literal_types();
        let named_variables = nodes.named_variables().map_err(GenerationError::Boundary)?;
        Ok(Self {
            dependency_retention: DependencyRetention::Clear,
            core,
            labels,
            profile,
            propagated,
            provider,
            library,
            callgraph,
            capture_inputs: captures,
            phase_entries: Vec::new(),
            scc_variables: BTreeMap::new(),
            named_variables,
            self_recursive,
            records,
            literal_types,
            patterns: Patterns::new(nodes),
            core_captures: BTreeMap::new(),
            arena: NativeConstraintArena::default(),
            state: State {
                next_label: labels.next_label.0,
                record_cache: captures
                    .map(|c| c.initial_record_cache.to_vec())
                    .unwrap_or_default(),
                ..State::default()
            },
        })
    }
    pub fn with_dependency_retention(mut self, dependency_retention: DependencyRetention) -> Self {
        self.dependency_retention = dependency_retention;
        self
    }
    pub fn with_next_label(mut self, next_label: CoreLabel) -> Self {
        self.state.next_label = next_label.0;
        self
    }
    fn module(&self, s: &Site) -> &CoreModule {
        &self.core.modules[s.module.0]
    }
    fn variable_id(&self, s: &Site) -> Result<VariableId> {
        self.labels
            .label(s.reference())
            .map(|l| VariableId::Integer(l.0.into()))
            .ok_or_else(|| {
                GenerationError::Boundary(format!(
                    "missing shared source label: {:?}",
                    s.reference()
                ))
            })
    }
    fn var(&self, s: &Site) -> Result<Type> {
        Ok(Type::variable_id(self.variable_id(s)?))
    }
    fn fresh(&mut self) -> Result<Type> {
        let n = self.state.next_label;
        self.state.next_label = n
            .checked_add(1)
            .ok_or_else(|| GenerationError::Boundary("fresh variable label overflow".into()))?;
        Ok(Type::variable_id(VariableId::Integer(n.into())))
    }
    fn atomic(&mut self, left: Type, relation: Relation, right: NativeOperand) -> Result<()> {
        let c = self.arena.atomic(left, relation, right, self.provider)?;
        self.state.context.push(c);
        Ok(())
    }
    fn store_constraint_lists(
        &mut self,
        left: Vec<Type>,
        relation: Relation,
        right: Vec<NativeOperand>,
    ) -> Result<()> {
        if left.len() != right.len() {
            return Err(GenerationError::Boundary(
                "constraint list arities differ".into(),
            ));
        }
        for (left, right) in left.into_iter().zip(right).rev() {
            let right_any = match &right {
                NativeOperand::Type(typ) => typ.predicate(crate::lattice::TypePredicate::Any)?,
                NativeOperand::Derived(_) => false,
            };
            if !left.predicate(crate::lattice::TypePredicate::Any)? && !right_any {
                self.atomic(left, relation, right)?;
            }
        }
        Ok(())
    }
    fn equal(&mut self, left: Type, right: Type) -> Result<()> {
        self.atomic(left, Relation::Equal, NativeOperand::Type(right))
    }
    fn context(&mut self) -> Result<NativeConstraintId> {
        Ok(self.arena.group(
            GroupKind::Conjunction,
            self.state.context.iter().rev().copied().collect(),
        )?)
    }
    fn defined(&self, module: ModuleId, vars: &[NodeId], mut d: Defined) -> Result<Defined> {
        for node in vars {
            d.insert(self.variable_id(&Site {
                module,
                node: *node,
            })?);
        }
        Ok(d)
    }
    fn foldable(&self, s: &Site) -> bool {
        let m = self.module(s);
        let mut todo = vec![s.node];
        while let Some(n) = todo.pop() {
            match &m.nodes[n.0].expr {
                CoreExpr::Literal(term) => {
                    if self.state.in_match
                        && self
                            .patterns
                            .nodes
                            .contains_map(s.module, *term)
                            .expect("validated source term")
                    {
                        return false;
                    }
                }
                CoreExpr::Cons { head, tail } => todo.extend([*head, *tail]),
                CoreExpr::Tuple(xs) => todo.extend(xs),
                _ => return false,
            }
        }
        true
    }
    fn has_annotation(&self, site: &Site, atom: &str) -> bool {
        let module = self.module(site);
        let mut current = module.nodes[site.node.0].annotations;
        loop {
            match &module.terms[current.0] {
                ErlangTerm::Nil => return false,
                ErlangTerm::List { items, tail } => {
                    if items
                        .iter()
                        .any(|id| matches!(&module.terms[id.0],ErlangTerm::Atom(a)if a==atom))
                    {
                        return true;
                    }
                    current = *tail
                }
                _ => return false,
            }
        }
    }
    fn derived(
        &mut self,
        op: DerivedOp,
        site: &str,
        env: Vec<NativeValue>,
    ) -> Result<NativeOperand> {
        let environment = capture_values(&env).map_err(GenerationError::Boundary)?;
        let key = self
            .profile
            .instantiate(site, environment)
            .map_err(GenerationError::Boundary)?;
        Ok(NativeOperand::Derived(self.arena.derived(
            op,
            key,
            ClosureOrigin::Undefined,
        )))
    }
    fn finish_function(
        &mut self,
        site: Site,
        params: Vec<NodeId>,
        saved: State,
        body: Type,
    ) -> Result<Type> {
        let funvar = self.var(&site)?;
        let domain = params
            .iter()
            .map(|n| {
                self.var(&Site {
                    module: site.module,
                    node: *n,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        self.equal(funvar.clone(), Type::function(Some(domain), body))?;
        let body = self.context()?;
        let f =
            FunctionVariable::new(self.variable_id(&site)?).map_err(GenerationError::Boundary)?;
        self.state.bodies.insert(f.clone(), body);
        self.state.scc.insert(f.clone());
        let propagated_domain = match self
            .propagated
            .0
            .get(&self.labels.label(site.reference()).unwrap())
            .map(Type::view)
        {
            Some(crate::lattice::TypeView::Families(slots)) => slots
                .functions
                .and_then(|fun| fun.domain.map(<[Type]>::to_vec)),
            _ => None,
        };
        self.state.parameters.insert(
            f.clone(),
            FunctionParameters {
                arity: params.len(),
                recursive_variable: self.state.recursive_variables.get(f.variable()).cloned(),
                propagated_domain,
                self_recursive: self.labels.label(site.reference()) == self.self_recursive,
            },
        );
        let reference = self
            .arena
            .reference(f, self.arena.dependencies(body)?.clone());
        let old_context = self.arena.group(
            GroupKind::Conjunction,
            saved.context.iter().rev().copied().collect(),
        )?;
        self.state.context = vec![old_context, reference];
        Ok(funvar)
    }
    pub fn generate(
        mut self,
        entries: &[(ModuleId, NodeId, NodeId)],
        output: GenerationOutput,
    ) -> Result<GeneratedConstraints> {
        self.phase_entries = entries.to_vec();
        let mut defined = Defined::new();
        for (module, variable, function) in entries {
            let r = self.variable_id(&Site {
                module: *module,
                node: *variable,
            })?;
            let f = self.variable_id(&Site {
                module: *module,
                node: *function,
            })?;
            let mfa = self.core.modules[module.0]
                .named_mfa(*variable)
                .map_err(GenerationError::Boundary)?;
            self.scc_variables.insert(
                mfa,
                Type::variable_id(r.clone()),
            );
            self.state
                .scc
                .insert(FunctionVariable::new(f.clone()).map_err(GenerationError::Boundary)?);
            defined.insert(r.clone());
        }
        for (module, variable, function) in entries {
            let function_id = self.variable_id(&Site {
                module: *module,
                node: *function,
            })?;
            let recursive_id = self.variable_id(&Site {
                module: *module,
                node: *variable,
            })?;
            self.state
                .recursive_variables
                .insert(function_id, recursive_id);
            self.state.module = Some(*module);
            self.state.context.clear();
            self.traverse(
                Site {
                    module: *module,
                    node: *function,
                },
                defined.clone(),
            )?;
        }
        let functions = self
            .state
            .bodies
            .iter()
            .map(|(function, body)| FunctionBody {
                function: function.clone(),
                body: *body,
            })
            .collect();
        let source_state = if self.capture_inputs.is_some() {
            match output {
                GenerationOutput::WithSourceState => Some(self.capture_state()?),
                GenerationOutput::BindingsOnly => {
                    self.validate_final_capture_metadata()?;
                    None
                }
            }
        } else {
            None
        };
        Ok(GeneratedConstraints {
            dependency_retention: self.dependency_retention,
            source_state,
            arena: self.arena,
            functions,
            parameters: self.state.parameters.into_iter().collect(),
            entries: self.state.bodies.keys().cloned().collect(),
            next_label: CoreLabel(self.state.next_label),
            refold_allocations: self.patterns.allocation_count(),
            missing_library_summaries: self.state.missing_library_summaries,
        })
    }
    fn traverse(&mut self, root: Site, defined: Defined) -> Result<Type> {
        let mut todo = vec![Frame::Visit(root, defined)];
        let mut values = Vec::new();
        while let Some(frame) = todo.pop() {
            let clause_recovery = match &frame {
                Frame::ClausePatterns { context, .. } => {
                    let mut context = context.clone();
                    context.subtractions = context.next_subtractions.take();
                    Some(context)
                }
                Frame::ClauseGuard { context, .. } | Frame::ClauseBody { context } => {
                    Some(context.clone())
                }
                _ => None,
            };
            let mut result = (|| -> Result<()> {
                match frame {
                    Frame::Visit(site, defined) => {
                        let expr = self.module(&site).nodes[site.node.0].expr.clone();
                        let mut schedule = |children: Vec<NodeId>, finish: Finish| {
                            let count = children.len();
                            let module = site.module;
                            todo.push(Frame::Finish(site, finish, count));
                            for node in children.into_iter().rev() {
                                todo.push(Frame::Visit(Site { module, node }, defined.clone()));
                            }
                        };
                        match expr {
                            CoreExpr::Literal(id) => {
                                if self.state.in_match {
                                    let pattern = self
                                        .patterns
                                        .refold(crate::patterns::PatternRef::Core(site.reference()))
                                        .map_err(|e| {
                                            GenerationError::Boundary(format!(
                                                "pattern refold: {e:?}"
                                            ))
                                        })?;
                                    if !matches!(pattern, crate::patterns::PatternRef::Core(_)) {
                                        todo.push(Frame::PatternVisit {
                                            pattern,
                                            origin: site.reference(),
                                            defined,
                                        });
                                        return Ok(());
                                    }
                                }
                                values.push(
                                    self.literal_types[site.module.0]
                                        .get(id)
                                        .ok_or_else(|| {
                                            GenerationError::Boundary(
                                                "literal type not materialized".into(),
                                            )
                                        })?
                                        .clone(),
                                );
                            }
                            CoreExpr::Map { pairs, .. } => {
                                let pattern = crate::patterns::PatternRef::Core(site.reference());
                                let entries = pairs
                                    .into_iter()
                                    .map(|node| {
                                        crate::patterns::PatternRef::Core(NodeRef {
                                            module: site.module,
                                            node: node.0,
                                        })
                                    })
                                    .collect();
                                let context = MapContext {
                                    pattern,
                                    origin: site.reference(),
                                    entries,
                                    pairs: vec![],
                                    next: 0,
                                    defined,
                                    in_match: self.state.in_match,
                                    base: values.len(),
                                };
                                self.next_map_pair(context, &mut todo, &mut values)?;
                            }
                            CoreExpr::Var(_) => {
                                let label = self.variable_id(&site)?;
                                if defined.contains(&label) {
                                    values.push(self.var(&site)?);
                                } else {
                                    let mfa =
                                        self.named_variables.get(&label).ok_or_else(|| {
                                            GenerationError::Boundary(format!(
                                                "undefined Core variable: {:?}",
                                                site.reference()
                                            ))
                                        })?;
                                    let sig=self.library.signature(mfa).map_err(GenerationError::Boundary)?.ok_or_else(||GenerationError::Boundary(format!("undefined named Core variable without PLT signature: {:?}",site.reference())))?;
                                    let fresh = self.fresh()?;
                                    self.atomic(
                                        fresh.clone(),
                                        Relation::Subtype,
                                        NativeOperand::Type(Type::function(
                                            Some(sig.arguments),
                                            sig.returns,
                                        )),
                                    )?;
                                    values.push(fresh);
                                }
                            }
                            CoreExpr::Call { module, name, args } => {
                                let atom =
                                    |node: NodeId| match self.module(&site).nodes[node.0].expr {
                                        CoreExpr::Literal(id) => {
                                            match &self.module(&site).terms[id.0] {
                                                ErlangTerm::Atom(a) => Some(a.clone()),
                                                _ => None,
                                            }
                                        }
                                        _ => None,
                                    };
                                if let (Some(module), Some(function)) = (atom(module), atom(name)) {
                                    let mfa = Mfa {
                                        module: module.into(),
                                        function: function.into(),
                                        arity: args.len(),
                                    };
                                    let finish = if let Some(op) = self.scc_variables.get(&mfa) {
                                        Finish::ApplyNamed(op.clone())
                                    } else {
                                        Finish::Call(mfa)
                                    };
                                    schedule(args, finish);
                                } else {
                                    schedule(vec![module, name], Finish::DynamicCall);
                                }
                            }
                            CoreExpr::Apply { operator, args } => {
                                let mut nodes = args;
                                nodes.push(operator);
                                schedule(nodes, Finish::Apply)
                            }
                            CoreExpr::Binary(xs) => schedule(xs, Finish::Binary),
                            CoreExpr::Bitstr {
                                value,
                                size,
                                unit,
                                typ,
                                flags,
                            } => schedule(
                                vec![size, value],
                                Finish::Bitstr {
                                    size,
                                    unit,
                                    typ,
                                    flags,
                                },
                            ),
                            CoreExpr::Case { argument, clauses } => {
                                todo.push(Frame::CaseArgument {
                                    site,
                                    clauses,
                                    defined: defined.clone(),
                                    base: values.len(),
                                });
                                todo.push(Frame::Visit(
                                    Site {
                                        module: site.module,
                                        node: argument,
                                    },
                                    defined,
                                ));
                            }
                            CoreExpr::Try {
                                argument,
                                vars,
                                body,
                                exception_vars,
                                handler,
                            } => {
                                let outer = self.state.clone();
                                self.state.context.clear();
                                let context = TryContext {
                                    site,
                                    vars,
                                    body,
                                    exception_vars,
                                    handler,
                                    defined: defined.clone(),
                                    saved: self.state.clone(),
                                    outer,
                                    base: values.len(),
                                };
                                todo.push(Frame::TryArgument { context });
                                todo.push(Frame::Visit(
                                    Site {
                                        module: site.module,
                                        node: argument,
                                    },
                                    defined,
                                ));
                            }
                            CoreExpr::Tuple(xs) => schedule(xs, Finish::Tuple),
                            CoreExpr::Values(xs) => schedule(xs, Finish::Values),
                            CoreExpr::Cons { head, tail } => {
                                schedule(vec![head, tail], Finish::Cons)
                            }
                            CoreExpr::Alias { variable, pattern } => {
                                let d = self.defined(site.module, &[variable], defined)?;
                                todo.push(Frame::Finish(
                                    Site {
                                        module: site.module,
                                        node: site.node,
                                    },
                                    Finish::Alias(variable),
                                    1,
                                ));
                                todo.push(Frame::Visit(
                                    Site {
                                        module: site.module,
                                        node: pattern,
                                    },
                                    d,
                                ));
                            }
                            CoreExpr::Catch { .. } => values.push(self.var(&site)?),
                            CoreExpr::Seq { argument, body } => {
                                schedule(vec![argument, body], Finish::DiscardFirst)
                            }
                            CoreExpr::Let {
                                vars,
                                argument,
                                body,
                            } => {
                                let module = site.module;
                                todo.push(Frame::Let {
                                    site,
                                    vars,
                                    body,
                                    defined: defined.clone(),
                                });
                                todo.push(Frame::Visit(
                                    Site {
                                        module,
                                        node: argument,
                                    },
                                    defined,
                                ));
                            }
                            CoreExpr::Letrec { definitions, body } => {
                                let vars = definitions.iter().map(|p| p.0).collect::<Vec<_>>();
                                let d = self.defined(site.module, &vars, defined)?;
                                for (v, f) in &definitions {
                                    self.state.recursive_variables.insert(
                                        self.variable_id(&Site {
                                            module: site.module,
                                            node: *f,
                                        })?,
                                        self.variable_id(&Site {
                                            module: site.module,
                                            node: *v,
                                        })?,
                                    );
                                }
                                let module = site.module;
                                todo.push(Frame::LetrecBody {
                                    site,
                                    body,
                                    defined: d.clone(),
                                    count: definitions.len(),
                                });
                                for (_, f) in definitions.into_iter().rev() {
                                    todo.push(Frame::Visit(Site { module, node: f }, d.clone()));
                                }
                            }
                            CoreExpr::Fun { params, body } => {
                                let d = self.defined(site.module, &params, defined)?;
                                let saved = self.state.clone();
                                self.state.context.clear();
                                let label = self.labels.label(site.reference()).unwrap();
                                if let Some(propagated) = self.propagated.0.get(&label)
                                    && let crate::lattice::TypeView::Families(slots) =
                                        propagated.view()
                                    && let Some(domain) = slots.functions.and_then(|fun| fun.domain)
                                    && !domain
                                        .iter()
                                        .any(|t| matches!(t.view(), crate::lattice::TypeView::None))
                                {
                                    self.atomic(
                                        self.var(&site)?,
                                        Relation::Subtype,
                                        NativeOperand::Type(propagated.clone()),
                                    )?;
                                }
                                let module = site.module;
                                todo.push(Frame::Function {
                                    site,
                                    params,
                                    saved,
                                    base: values.len(),
                                });
                                todo.push(Frame::Visit(Site { module, node: body }, d));
                            }
                            CoreExpr::Primop { name, args } => {
                                let CoreExpr::Literal(id) = self.module(&site).nodes[name.0].expr
                                else {
                                    return Err(GenerationError::Boundary(
                                        "primop name is not literal".into(),
                                    ));
                                };
                                match &self.module(&site).terms[id.0] {
                                    ErlangTerm::Atom(s) if s == "raise" || s == "match_fail" => {
                                        return Err(GenerationError::ConstructionFailure);
                                    }
                                    ErlangTerm::Atom(s) if s == "bs_init_writable" => values.push(
                                        Type::bitstring(0.into(), 0.into())
                                            .map_err(GenerationError::BitstringDomain)?,
                                    ),
                                    ErlangTerm::Atom(s)
                                        if [
                                            "remove_message",
                                            "nif_start",
                                            "debug_line",
                                            "executable_line",
                                        ]
                                        .contains(&s.as_str()) =>
                                    {
                                        values.push(Type::any())
                                    }
                                    ErlangTerm::Atom(s) if s == "build_stacktrace" => {
                                        let value = self.var(&site)?;
                                        let info = self
                                            .provider
                                            .construction(&Mfa {
                                                module: "erlang".into(),
                                                function: "build_stacktrace".into(),
                                                arity: 0,
                                            })
                                            .map_err(GenerationError::Evaluation)?;
                                        self.atomic(
                                            value.clone(),
                                            Relation::Subtype,
                                            NativeOperand::Type(info.generic_return),
                                        )?;
                                        values.push(value);
                                    }
                                    ErlangTerm::Atom(s) if s == "recv_peek_message" => {
                                        values.push(Type::product(vec![
                                            self.fresh()?,
                                            self.fresh()?,
                                        ]));
                                    }
                                    ErlangTerm::Atom(s) if s == "recv_wait_timeout" => {
                                        let [timeout]: [NodeId; 1] =
                                            args.try_into().map_err(|_| {
                                                GenerationError::Boundary(
                                                    "recv_wait_timeout expects one argument".into(),
                                                )
                                            })?;
                                        let infinite = matches!(self.literal_term(&site,timeout),Some(ErlangTerm::Atom(a))if a=="infinity");
                                        if infinite {
                                            values.push(Type::none());
                                        } else {
                                            todo.push(Frame::Finish(site, Finish::Timeout, 1));
                                            todo.push(Frame::Visit(
                                                Site {
                                                    module: site.module,
                                                    node: timeout,
                                                },
                                                defined,
                                            ));
                                        }
                                    }
                                    ErlangTerm::Atom(s) if s == "dialyzer_unknown" => {
                                        values.push(self.var(&site)?)
                                    }
                                    _ => {
                                        return Err(GenerationError::UnsupportedPrimop {
                                            node: site.reference(),
                                            name: self.module(&site).terms[id.0].clone(),
                                        });
                                    }
                                }
                            }
                            _ => {
                                return Err(GenerationError::Unsupported {
                                    node: site.reference(),
                                    kind: expr.kind(),
                                });
                            }
                        }
                    }
                    Frame::Finish(site, finish, count) => {
                        let args =
                            values.split_off(values.len().checked_sub(count).ok_or_else(|| {
                                GenerationError::Boundary("Core result stack underflow".into())
                            })?);
                        let value = match finish {
                            Finish::Apply => {
                                let mut args = args;
                                let operator = args.pop().ok_or_else(|| {
                                    GenerationError::Boundary("apply operator absent".into())
                                })?;
                                self.apply_constraints(&site, operator, &args)?
                            }
                            Finish::ApplyNamed(operator) => {
                                self.apply_constraints(&site, operator, &args)?
                            }
                            Finish::Call(mfa) => {
                                let dst = self.var(&site)?;
                                if let Some(constraint) = self.bif_constraints(&mfa, &dst, &args)? {
                                    self.state.context.push(constraint);
                                } else {
                                    self.plt_constraints(&mfa, &dst, &args)?;
                                }
                                dst
                            }
                            Finish::DynamicCall => {
                                self.store_constraint_lists(
                                    args,
                                    Relation::Subtype,
                                    vec![
                                        NativeOperand::Type(Type::builtin(
                                            crate::lattice::BuiltinType::Module,
                                        )?),
                                        NativeOperand::Type(Type::builtin(
                                            crate::lattice::BuiltinType::Atom,
                                        )?),
                                    ],
                                )?;
                                self.var(&site)?
                            }
                            Finish::DiscardFirst => args[1].clone(),
                            Finish::Alias(variable) => {
                                self.equal(
                                    self.var(&Site {
                                        module: site.module,
                                        node: variable,
                                    })?,
                                    args[0].clone(),
                                )?;
                                args[0].clone()
                            }
                            Finish::Values => self.product_constraints(args)?,
                            Finish::Tuple => {
                                let foldable = self.foldable(&site);
                                let record = self.source_record(&site)?;
                                self.tuple_constraints(args, foldable, record)?
                            }
                            Finish::Timeout => {
                                let [timeout]: [Type; 1] = args.try_into().map_err(|_| {
                                    GenerationError::Boundary(
                                        "timeout traversal result count differs".into(),
                                    )
                                })?;
                                self.atomic(
                                    timeout,
                                    Relation::Subtype,
                                    NativeOperand::Type(Type::builtin(
                                        crate::lattice::BuiltinType::Timeout,
                                    )?),
                                )?;
                                self.var(&site)?
                            }
                            Finish::Binary => {
                                let dst = self.var(&site)?;
                                let rhs = self.derived(
                                    DerivedOp::BinarySegments {
                                        segments: args.clone(),
                                    },
                                    "-traverse/3-fun-0-",
                                    vec![NativeValue::List(
                                        args.iter().map(Type::to_native_value).collect(),
                                    )],
                                )?;
                                self.atomic(dst.clone(), Relation::Subtype, rhs)?;
                                dst
                            }
                            Finish::Bitstr {
                                size,
                                unit,
                                typ,
                                flags,
                            } => self.bitstr_constraints(&site, size, unit, typ, flags, args)?,
                            Finish::Cons => {
                                self.cons_constraints(self.var(&site)?, args, self.foldable(&site))?
                            }
                        };
                        values.push(value);
                    }
                    Frame::Let {
                        site,
                        vars,
                        body,
                        defined,
                    } => {
                        let arg = values.pop().ok_or_else(|| {
                            GenerationError::Boundary("let argument missing".into())
                        })?;
                        let vars_t = vars
                            .iter()
                            .map(|v| {
                                self.var(&Site {
                                    module: site.module,
                                    node: *v,
                                })
                            })
                            .collect::<Result<Vec<_>>>()?;
                        self.equal(Type::product(vars_t), arg)?;
                        todo.push(Frame::Visit(
                            Site {
                                module: site.module,
                                node: body,
                            },
                            self.defined(site.module, &vars, defined)?,
                        ));
                    }
                    Frame::LetrecBody {
                        site,
                        body,
                        defined,
                        count,
                    } => {
                        values.truncate(values.len().checked_sub(count).ok_or_else(|| {
                            GenerationError::Boundary("letrec function results missing".into())
                        })?);
                        todo.push(Frame::Visit(
                            Site {
                                module: site.module,
                                node: body,
                            },
                            defined,
                        ));
                    }
                    Frame::TryArgument { context } => {
                        let argument = values.pop().ok_or_else(|| {
                            GenerationError::Boundary("try argument result absent".into())
                        })?;
                        let defined = self.defined(
                            context.site.module,
                            &context.vars,
                            context.defined.clone(),
                        )?;
                        let body = context.body;
                        let module = context.site.module;
                        todo.push(Frame::TryBody { context, argument });
                        todo.push(Frame::Visit(Site { module, node: body }, defined));
                    }
                    Frame::TryBody { context, argument } => {
                        let result = values.pop().ok_or_else(|| {
                            GenerationError::Boundary("try body result absent".into())
                        })?;
                        let vars = context
                            .vars
                            .iter()
                            .map(|v| {
                                self.var(&Site {
                                    module: context.site.module,
                                    node: *v,
                                })
                            })
                            .collect::<Result<Vec<_>>>()?;
                        match self.equal(Type::product(vars), argument) {
                            Ok(()) => {
                                self.begin_handler(context, result, &mut todo, &mut values)?
                            }
                            Err(GenerationError::ConstructionFailure) => {
                                self.state = context.saved.clone();
                                self.begin_handler(context, Type::none(), &mut todo, &mut values)?;
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    Frame::TryHandler {
                        site,
                        body_state,
                        body_result,
                        outer,
                        base,
                    } => {
                        let handler = values.pop().ok_or_else(|| {
                            GenerationError::Boundary("try handler result absent".into())
                        })?;
                        values.truncate(base);
                        values.push(self.finish_try(
                            site,
                            body_state,
                            body_result,
                            outer,
                            handler,
                        )?);
                    }
                    Frame::MapKey {
                        context,
                        value,
                        operation,
                    } => {
                        let key = values.pop().ok_or_else(|| {
                            GenerationError::Boundary("map key result absent".into())
                        })?;
                        self.state.in_match = context.in_match;
                        let origin = context.origin;
                        let defined = context.defined.clone();
                        todo.push(Frame::MapValue {
                            context,
                            key,
                            operation,
                        });
                        todo.push(Frame::PatternVisit {
                            pattern: value,
                            origin,
                            defined,
                        });
                    }
                    Frame::MapValue {
                        mut context,
                        key,
                        operation,
                    } => {
                        let value = values.pop().ok_or_else(|| {
                            GenerationError::Boundary("map value result absent".into())
                        })?;
                        context
                            .pairs
                            .push(crate::constraints::derived::MapAssociation {
                                key,
                                value,
                                operation,
                            });
                        self.next_map_pair(context, &mut todo, &mut values)?;
                    }
                    Frame::MapArgument { context } => {
                        let argument = values.pop().ok_or_else(|| {
                            GenerationError::Boundary("map argument result absent".into())
                        })?;
                        let result = self.finish_map(context, argument)?;
                        values.push(result);
                    }
                    Frame::PatternVisit {
                        pattern,
                        origin,
                        defined,
                    } => {
                        self.visit_pattern(pattern, origin, defined, &mut todo, &mut values)?;
                    }
                    Frame::PatternFinish {
                        pattern,
                        origin,
                        kind,
                        count,
                    } => {
                        let args = values.split_off(values.len() - count);
                        let result = match kind {
                            PatternFinish::Values => self.product_constraints(args)?,
                            PatternFinish::Cons => self.cons_constraints(
                                self.pattern_variable(pattern)?,
                                args,
                                self.pattern_foldable(pattern)?,
                            )?,
                            PatternFinish::Tuple => {
                                let record = self.pattern_record(pattern, origin.module)?;
                                self.tuple_constraints(
                                    args,
                                    self.pattern_foldable(pattern)?,
                                    record,
                                )?
                            }
                        };
                        values.push(result);
                    }
                    Frame::CaseArgument {
                        site,
                        clauses,
                        defined,
                        base,
                    } => {
                        let argument = values.pop().ok_or_else(|| {
                            GenerationError::Boundary("case argument result missing".into())
                        })?;
                        let context = CaseContext {
                            site,
                            subtractions: if clauses.len() > 15 {
                                None
                            } else {
                                Some(vec![])
                            },
                            clauses,
                            next: 0,
                            argument,
                            defined,
                            next_subtractions: None,
                            previous: self.state.clone(),
                            outer_context: self.state.context.clone(),
                            branches: vec![],
                            base,
                        };
                        self.next_clause(context, &mut todo, &mut values)?;
                    }
                    Frame::ClausePatterns {
                        mut context,
                        guard,
                        body,
                        defined,
                        count,
                    } => {
                        let patterns = values.split_off(values.len() - count);
                        self.equal(context.argument.clone(), Type::product(patterns))?;
                        if let Some(subtractions) = &context.subtractions {
                            let residual = self.derived(
                                DerivedOp::ClauseResidual {
                                    argument: context.argument.clone(),
                                    subtract: subtractions.clone(),
                                },
                                "-handle_clauses_1/7-fun-0-",
                                vec![
                                    context.argument.to_native_value(),
                                    NativeValue::List(
                                        subtractions.iter().map(Type::to_native_value).collect(),
                                    ),
                                ],
                            )?;
                            self.atomic(context.argument.clone(), Relation::Subtype, residual)?;
                        }
                        context.subtractions = context.next_subtractions.take();
                        let pattern_state = self.state.clone();
                        self.state.context.clear();
                        self.state.in_guard = true;
                        let module = context.site.module;
                        todo.push(Frame::ClauseGuard {
                            context,
                            body,
                            defined: defined.clone(),
                            pattern_state,
                        });
                        todo.push(Frame::Visit(
                            Site {
                                module,
                                node: guard,
                            },
                            defined,
                        ));
                    }
                    Frame::ClauseGuard {
                        context,
                        body,
                        defined,
                        pattern_state,
                    } => {
                        let result = values.pop().ok_or_else(|| {
                            GenerationError::Boundary("guard result missing".into())
                        })?;
                        self.equal(result, Type::atom("true"))?;
                        let guard = self.context()?;
                        let guard = self.arena.disjunctive_normal_form(guard)?;
                        let pattern = self.arena.group(
                            GroupKind::Conjunction,
                            pattern_state.context.iter().rev().copied().collect(),
                        )?;
                        let joined = self
                            .arena
                            .group(GroupKind::Conjunction, vec![pattern, guard])?;
                        self.state.context = vec![joined];
                        self.state.in_guard = pattern_state.in_guard;
                        self.state.in_match = false;
                        let module = context.site.module;
                        todo.push(Frame::ClauseBody { context });
                        todo.push(Frame::Visit(Site { module, node: body }, defined));
                    }
                    Frame::ClauseBody { mut context } => {
                        let result = values.pop().ok_or_else(|| {
                            GenerationError::Boundary("clause body result missing".into())
                        })?;
                        self.equal(self.var(&context.site)?, result)?;
                        context.branches.push(self.context()?);
                        context.previous = self.state.clone();
                        self.next_clause(context, &mut todo, &mut values)?;
                    }
                    Frame::Function {
                        site,
                        params,
                        saved,
                        base,
                    } => {
                        let body = values.pop().ok_or_else(|| {
                            GenerationError::Boundary("function result missing".into())
                        })?;
                        values.truncate(base);
                        values.push(self.finish_function(site, params, saved, body)?);
                    }
                }
                Ok(())
            })();
            if matches!(result, Err(GenerationError::ConstructionFailure))
                && let Some(context) = clause_recovery
            {
                self.state = context.previous.clone();
                result = self.next_clause(context, &mut todo, &mut values);
            }
            if let Err(e) = result {
                if !matches!(e, GenerationError::ConstructionFailure) {
                    return Err(e);
                }
                let mut caught = None;
                while let Some(frame) = todo.pop() {
                    let frame = match frame {
                        Frame::ClausePatterns { mut context, .. } => {
                            context.subtractions = context.next_subtractions.take();
                            self.state = context.previous.clone();
                            match self.next_clause(context, &mut todo, &mut values) {
                                Ok(()) => {
                                    caught = Some(None);
                                    break;
                                }
                                Err(GenerationError::ConstructionFailure) => continue,
                                Err(error) => return Err(error),
                            }
                        }
                        Frame::ClauseGuard { context, .. } | Frame::ClauseBody { context } => {
                            self.state = context.previous.clone();
                            match self.next_clause(context, &mut todo, &mut values) {
                                Ok(()) => {
                                    caught = Some(None);
                                    break;
                                }
                                Err(GenerationError::ConstructionFailure) => continue,
                                Err(error) => return Err(error),
                            }
                        }
                        Frame::TryArgument { context } | Frame::TryBody { context, .. } => {
                            self.state = context.saved.clone();
                            self.begin_handler(context, Type::none(), &mut todo, &mut values)?;
                            caught = Some(None);
                            break;
                        }
                        Frame::TryHandler {
                            site,
                            body_state,
                            body_result,
                            outer,
                            base,
                        } => {
                            self.state = body_state.clone();
                            self.state.context.clear();
                            values.truncate(base);
                            match self.finish_try(
                                site,
                                body_state,
                                body_result,
                                outer,
                                Type::none(),
                            ) {
                                Ok(value) => {
                                    values.push(value);
                                    caught = Some(None);
                                    break;
                                }
                                Err(GenerationError::ConstructionFailure) => continue,
                                Err(error) => return Err(error),
                            }
                        }
                        other => other,
                    };
                    if let Frame::Function {
                        site,
                        params,
                        saved,
                        base,
                    } = frame
                    {
                        caught = Some(Some((site, params, saved, base)));
                        break;
                    }
                }
                let Some(caught) = caught else {
                    return Err(e);
                };
                let Some((site, params, saved, base)) = caught else {
                    continue;
                };
                self.state = saved.clone();
                self.state.context.clear();
                values.truncate(base);
                // The source failure constraint uses propagated domain if available, otherwise Any arguments.
                let domain = match self
                    .propagated
                    .0
                    .get(&self.labels.label(site.reference()).unwrap())
                    .map(Type::view)
                {
                    Some(crate::lattice::TypeView::Families(slots)) => {
                        slots.functions.and_then(|f| f.domain.map(<[Type]>::to_vec))
                    }
                    _ => None,
                }
                .unwrap_or_else(|| vec![Type::any(); params.len()]);
                let ftype = Type::function(Some(domain), Type::none());
                let funvar = self.var(&site)?;
                self.equal(funvar.clone(), ftype)?;
                let body = self.context()?;
                let f = FunctionVariable::new(self.variable_id(&site)?)
                    .map_err(GenerationError::Boundary)?;
                self.state.bodies.insert(f.clone(), body);
                self.state.scc.insert(f.clone());
                self.state.parameters.insert(
                    f.clone(),
                    FunctionParameters {
                        arity: params.len(),
                        recursive_variable: self
                            .state
                            .recursive_variables
                            .get(f.variable())
                            .cloned(),
                        propagated_domain: None,
                        self_recursive: self.labels.label(site.reference()) == self.self_recursive,
                    },
                );
                let reference = self
                    .arena
                    .reference(f, self.arena.dependencies(body)?.clone());
                let old_context = self.arena.group(
                    GroupKind::Conjunction,
                    saved.context.iter().rev().copied().collect(),
                )?;
                self.state.context = vec![old_context, reference];
                values.push(funvar);
            }
        }
        if values.len() != 1 {
            return Err(GenerationError::Boundary(
                "Core traversal result arity differs from one".into(),
            ));
        }
        Ok(values.pop().unwrap())
    }
}
fn capture_values(values: &[NativeValue]) -> std::result::Result<CaptureEnvironment, String> {
    CaptureEnvironment::from_native_values(values)
}

#[path = "generation_bifs.rs"]
mod bifs;

impl Generator<'_> {
    fn plt_constraints(&mut self, mfa: &Mfa, dst: &Type, args: &[Type]) -> Result<()> {
        let signature = self
            .library
            .signature(mfa)
            .map_err(GenerationError::Boundary)?;
        let contract = if self.scc_variables.contains_key(mfa) {
            None
        } else {
            self.library
                .active_contract(mfa)
                .map_err(GenerationError::Boundary)?
        };
        if let Some(contract) = contract {
            let mut domain = contract.summary.arguments.clone();
            if domain.len() != args.len() {
                return Err(GenerationError::Boundary("contract arity differs".into()));
            }
            if let Some(signature) = &signature {
                if signature.arguments.len() != args.len() {
                    return Err(GenerationError::Boundary("signature arity differs".into()));
                }
                domain = Type::meet_lists(&domain, &signature.arguments)?;
            }
            let captures = contract_capture(
                &contract.captured_record,
                args,
                signature.as_ref().map(|s| &s.returns),
            )
            .map_err(GenerationError::Boundary)?;
            let key = self
                .profile
                .instantiate(
                    if signature.is_some() {
                        "-get_plt_constr/4-fun-2-"
                    } else {
                        "-get_plt_constr/4-fun-0-"
                    },
                    captures,
                )
                .map_err(GenerationError::Boundary)?;
            let ret = NativeOperand::Derived(self.arena.derived(
                DerivedOp::ContractReturn {
                    summary: contract.summary,
                    arguments: args.to_vec(),
                    signature_return: signature.as_ref().map(|s| s.returns.clone()),
                },
                key,
                ClosureOrigin::Undefined,
            ));
            let left = std::iter::once(dst.clone())
                .chain(args.iter().cloned())
                .collect();
            let right = std::iter::once(ret)
                .chain(domain.into_iter().map(NativeOperand::Type))
                .collect();
            self.store_constraint_lists(left, Relation::Subtype, right)?;
        } else if let Some(signature) = signature {
            if signature.arguments.len() != args.len() {
                return Err(GenerationError::Boundary("signature arity differs".into()));
            }
            let left = std::iter::once(dst.clone())
                .chain(args.iter().cloned())
                .collect();
            let right = std::iter::once(signature.returns)
                .chain(signature.arguments)
                .map(NativeOperand::Type)
                .collect();
            self.store_constraint_lists(left, Relation::Subtype, right)?;
        } else {
            self.state.missing_library_summaries.insert(mfa.clone());
        }
        Ok(())
    }
}
fn contract_capture(
    record: &CaptureEnvironment,
    args: &[Type],
    range: Option<&Type>,
) -> std::result::Result<CaptureEnvironment, String> {
    let mut terms = Vec::new();
    let record_list = record.append_to(&mut terms);
    let CaptureTerm::List { items, .. } = &terms[record_list.0] else {
        return Err("source contract capture list expected".into());
    };
    let record_id = items[0];
    let args_env = capture_values(&[NativeValue::List(
        args.iter().map(Type::to_native_value).collect(),
    )])?;
    let args_root = args_env.append_to(&mut terms);
    let CaptureTerm::List { items, .. } = &terms[args_root.0] else {
        return Err("argument capture list expected".into());
    };
    let args_id = items[0];
    let mut env = vec![args_id, record_id];
    if let Some(range) = range {
        let range_root = capture_values(&[range.to_native_value()])?.append_to(&mut terms);
        let CaptureTerm::List { items, .. } = &terms[range_root.0] else {
            return Err("range capture list expected".into());
        };
        env.push(items[0]);
    }
    let nil = CaptureId(terms.len());
    terms.push(CaptureTerm::Nil);
    let root = CaptureId(terms.len());
    terms.push(CaptureTerm::List {
        items: env,
        tail: nil,
    });
    CaptureEnvironment::new(Arc::new(TermOrderArena::from_captures(&terms)?), root)
}

impl Generator<'_> {
    fn apply_constraints(&mut self, site: &Site, operator: Type, args: &[Type]) -> Result<Type> {
        use crate::lattice::{Known, TypeGetter, TypePredicate, TypeValue};
        let prototype = if operator.predicate(TypePredicate::Function)? {
            operator.clone()
        } else {
            let ret = self.fresh()?;
            let domain = (0..args.len())
                .map(|_| self.fresh())
                .collect::<Result<Vec<_>>>()?;
            Type::function(Some(domain), ret)
        };
        self.equal(prototype.clone(), operator)?;
        let TypeValue::Type(range) = prototype.get(TypeGetter::FunctionRange)? else {
            unreachable!("typed range getter")
        };
        let dst = self.var(site)?;
        self.atomic(dst.clone(), Relation::Subtype, NativeOperand::Type(range))?;
        let TypeValue::Types(Known::Known(domain)) = prototype.get(TypeGetter::FunctionArgs)?
        else {
            return Err(LatticeError::FunctionClause.into());
        };
        if domain.len() != args.len() {
            return Err(LatticeError::FunctionClause.into());
        }
        self.store_constraint_lists(
            args.to_vec(),
            Relation::Subtype,
            domain.into_iter().map(NativeOperand::Type).collect(),
        )?;
        self.apply_target_constraints(site, &dst, args)?;
        Ok(dst)
    }
}

impl Generator<'_> {
    fn literal_term<'a>(&'a self, site: &Site, node: NodeId) -> Option<&'a ErlangTerm> {
        let CoreExpr::Literal(id) = self.module(site).nodes[node.0].expr else {
            return None;
        };
        Some(&self.module(site).terms[id.0])
    }
    fn segment_unit(
        &self,
        site: &Site,
        unit_node: NodeId,
    ) -> Result<crate::constraints::derived::SegmentUnit> {
        use crate::constraints::derived::SegmentUnit;
        let Some(ErlangTerm::Integer(unit)) = self.literal_term(site, unit_node) else {
            return Err(GenerationError::Boundary(
                "bitstr unit is not literal integer".into(),
            ));
        };
        SegmentUnit::new(unit.parse::<u16>().map_err(|_| {
            GenerationError::Boundary("bitstr unit exceeds compiled segment domain".into())
        })?)
        .map_err(GenerationError::Boundary)
    }
    fn bitstr_constraints(
        &mut self,
        site: &Site,
        size_node: NodeId,
        unit_node: NodeId,
        typ_node: NodeId,
        flags_node: NodeId,
        args: Vec<Type>,
    ) -> Result<Type> {
        use crate::constraints::derived::BinaryMode;
        use crate::lattice::BuiltinType;
        let [size, value]: [Type; 2] = args
            .try_into()
            .map_err(|_| GenerationError::Boundary("bitstr results differ from two".into()))?;
        if self.literal_term(site, unit_node).is_none() {
            return Err(GenerationError::Boundary(
                "bitstr unit is not literal".into(),
            ));
        }
        let literal = self.literal_term(site, size_node).cloned();
        let (constructed, matched) = match literal {
            Some(ErlangTerm::Atom(a)) if a == "all" => {
                let unit = self.segment_unit(site, unit_node)?;
                let ty = Type::bitstring(unit.get().into(), 0.into())
                    .map_err(GenerationError::BitstringDomain)?;
                (NativeOperand::Type(ty.clone()), NativeOperand::Type(ty))
            }
            Some(ErlangTerm::Atom(a)) if a == "undefined" => {
                let ty = Type::builtin(BuiltinType::Binary)?;
                (NativeOperand::Type(ty.clone()), NativeOperand::Type(ty))
            }
            Some(ErlangTerm::Integer(n)) => {
                let unit = self.segment_unit(site, unit_node)?;
                let bits = n
                    .parse::<num_bigint::BigInt>()
                    .map_err(|e| GenerationError::Boundary(format!("invalid integer term: {e}")))?
                    * num_bigint::BigInt::from(unit.get());
                (
                    NativeOperand::Type(
                        Type::bitstring(0.into(), bits.clone())
                            .map_err(GenerationError::BitstringDomain)?,
                    ),
                    NativeOperand::Type(
                        Type::bitstring(1.into(), bits)
                            .map_err(GenerationError::BitstringDomain)?,
                    ),
                )
            }
            Some(_) => (
                NativeOperand::Type(Type::none()),
                NativeOperand::Type(Type::none()),
            ),
            None => {
                let unit = self.segment_unit(site, unit_node)?;
                let constructed = self.derived(
                    DerivedOp::BinaryLength {
                        size: size.clone(),
                        unit,
                        mode: BinaryMode::Construct,
                    },
                    "-bitstr_constr/3-fun-0-",
                    vec![
                        size.to_native_value(),
                        NativeValue::Integer(unit.get().into()),
                        NativeValue::Integer(0.into()),
                    ],
                )?;
                let matched = self.derived(
                    DerivedOp::BinaryLength {
                        size: size.clone(),
                        unit,
                        mode: BinaryMode::Match,
                    },
                    "-bitstr_constr/3-fun-0-",
                    vec![
                        size.to_native_value(),
                        NativeValue::Integer(unit.get().into()),
                        NativeValue::Integer(1.into()),
                    ],
                )?;
                self.atomic(
                    size.clone(),
                    Relation::Subtype,
                    NativeOperand::Type(Type::builtin(BuiltinType::NonNegativeInteger)?),
                )?;
                (constructed, matched)
            }
        };
        let Some(ErlangTerm::Atom(typ)) = self.literal_term(site, typ_node) else {
            return Err(GenerationError::Boundary(
                "bitstr type is not literal atom".into(),
            ));
        };
        let value_constraint = match typ.as_str() {
            "binary" => matched,
            "float" => NativeOperand::Type(Type::builtin(if self.state.in_match {
                BuiltinType::Float
            } else {
                BuiltinType::Number
            })?),
            "integer" if self.state.in_match => {
                let unit = self.segment_unit(site, unit_node)?;
                let Some(ErlangTerm::List { items, tail }) = self.literal_term(site, flags_node)
                else {
                    return Err(GenerationError::Boundary(
                        "compiled binary flags must be a literal list".into(),
                    ));
                };
                let module = self.module(site);
                if !matches!(module.terms[tail.0], ErlangTerm::Nil) {
                    return Err(GenerationError::Boundary(
                        "compiled binary flags must be proper".into(),
                    ));
                }
                let flags = items
                    .iter()
                    .map(|id| match &module.terms[id.0] {
                        ErlangTerm::Atom(a) => Ok(NativeValue::Atom(a.clone())),
                        _ => Err(GenerationError::Boundary(
                            "compiled binary flag is not an atom".into(),
                        )),
                    })
                    .collect::<Result<Vec<_>>>()?;
                let signed = flags
                    .iter()
                    .any(|flag| matches!(flag, NativeValue::Atom(a) if a == "signed"));
                self.derived(
                    DerivedOp::BinaryValue {
                        size: size.clone(),
                        unit,
                        signed,
                    },
                    "-bitstr_val_constr/3-fun-0-",
                    vec![
                        size.to_native_value(),
                        NativeValue::Integer(unit.get().into()),
                        NativeValue::List(flags),
                    ],
                )?
            }
            "integer" | "utf8" | "utf16" | "utf32" => {
                NativeOperand::Type(Type::builtin(BuiltinType::Integer)?)
            }
            _ => return Err(LatticeError::CaseClause(Box::new(Type::atom(typ))).into()),
        };
        self.atomic(value, Relation::Subtype, value_constraint)?;
        let dst = self.var(site)?;
        self.atomic(dst.clone(), Relation::Subtype, constructed)?;
        Ok(dst)
    }
}

#[path = "generation_try.rs"]
mod try_constraints;

#[path = "generation_clauses.rs"]
mod clauses;

#[path = "generation_structural.rs"]
mod structural;

#[path = "generation_patterns.rs"]
mod pattern_constraints;

#[path = "generation_maps.rs"]
mod maps;

#[path = "generation_apply_targets.rs"]
mod apply_targets;

#[path = "generation_state.rs"]
mod state_capture;

#[cfg(test)]
mod capture_store_tests {
    use super::*;
    use crate::core::{CompilerProfile, CoreNode, CoreVersion, TermId, TermMapOrder};

    fn program() -> CoreProgram {
        let terms = vec![
            ErlangTerm::Nil,
            ErlangTerm::Atom("probe".into()),
            ErlangTerm::Atom("f".into()),
            ErlangTerm::Integer("0".into()),
            ErlangTerm::Tuple(vec![TermId(2), TermId(3)]),
            ErlangTerm::Atom("ok".into()),
        ];
        let expressions = vec![
            CoreExpr::Literal(TermId(1)),
            CoreExpr::Var(TermId(4)),
            CoreExpr::Literal(TermId(5)),
            CoreExpr::Fun {
                params: vec![],
                body: NodeId(2),
            },
            CoreExpr::Module {
                name: NodeId(0),
                exports: vec![NodeId(1)],
                attributes: vec![],
                definitions: vec![(NodeId(1), NodeId(3))],
            },
        ];
        let nodes = expressions
            .into_iter()
            .enumerate()
            .map(|(id, expr)| CoreNode {
                id: NodeId(id),
                annotations: TermId(0),
                expr,
            })
            .collect();
        CoreProgram {
            version: CoreVersion::V6,
            profile: CompilerProfile {
                otp: "28".into(),
                erts: "16".into(),
                elixir: "1.20.4".into(),
                dialyzer: "5".into(),
                core_options: vec![],
                term_map_order: TermMapOrder::NativeToListWithIterator,
            },
            atom_layout: None,
            contract_closure_profile: None,
            modules: vec![CoreModule {
                name: "probe".into(),
                source_forms: TermId(0),
                core: NodeId(4),
                terms,
                nodes,
                map_iterator_orders: BTreeMap::new(),
                contract_variable_maps: BTreeMap::new(),
            }],
            node_count: 5,
            term_count: 6,
            kinds: BTreeMap::new(),
        }
    }

    #[test]
    fn lazy_tables_match_all_records_selected_order_and_analysis_identity() {
        let core = program();
        let labels = Labels::build(&core, CoreLabel(0)).unwrap();
        let store = CoreCaptureStore::new(&core, &labels);
        assert!(store.modules.iter().all(|cell| cell.get().is_none()));
        let fresh = crate::core_capture::CoreCaptures::build(&core, &labels, ModuleId(0)).unwrap();
        let cached = store.get(ModuleId(0)).unwrap();
        assert!(std::ptr::eq(cached, store.get(ModuleId(0)).unwrap()));
        for selected in [
            vec![NodeId(0), NodeId(1), NodeId(2), NodeId(3), NodeId(4)],
            vec![NodeId(4), NodeId(1), NodeId(4)],
            vec![],
        ] {
            let mut old = vec![CaptureTerm::Atom("existing".into())];
            let mut new = old.clone();
            assert_eq!(
                fresh.append_selected(&selected, &mut old).unwrap(),
                cached.append_selected(&selected, &mut new).unwrap()
            );
            assert_eq!(format!("{old:?}"), format!("{new:?}"));
            assert!(
                fresh
                    .environment(&selected)
                    .unwrap()
                    .compare(&cached.environment(&selected).unwrap())
                    .unwrap()
                    .is_eq()
            );
        }
        let another = CoreCaptureStore::new(&core, &labels);
        assert!(another.modules[0].get().is_none());
        assert!(!std::ptr::eq(cached, another.get(ModuleId(0)).unwrap()));
        let other_core = program();
        let other_labels = Labels::build(&core, CoreLabel(0)).unwrap();
        assert!(!store.matches(&other_core, &labels));
        assert!(!store.matches(&core, &other_labels));
        assert!(store.get(ModuleId(1)).is_err());
    }

    struct NoRecords;
    impl RecordTableCaptures for NoRecords {
        fn module_table(&self, _: ModuleId) -> std::result::Result<&CaptureEnvironment, String> {
            Err("unexpected record table request".into())
        }
    }

    #[test]
    fn shared_tables_preserve_fresh_evolving_generator_state() {
        let core = program();
        let labels = Labels::build(&core, CoreLabel(0)).unwrap();
        let nodes = DataflowNodes::build(&core, &labels).unwrap();
        let program = crate::program::Program::build(&core).unwrap();
        let runtime = crate::constraints::generation_capture_headers::GenerationRuntimeCaptures::for_analysis("capture-test".into(), labels.next_label, &program).unwrap();
        let profile = ClosureCodeProfile::parse(&serde_json::json!({"module":"dialyzer_typesig","original_sha256":"0000000000000000000000000000000000000000000000000000000000000000","fun_table":[]})).unwrap();
        let registry = Registry::load(include_str!("../../corpus/bifs.json")).unwrap();
        let propagated = PropagatedTypes::default();
        let records = BTreeMap::new();
        let store = CoreCaptureStore::new(&core, &labels);
        let plain_inputs = GenerationCaptureInputs {
            core_captures: None,
            runtime: &runtime,
            records: &NoRecords,
            initial_record_cache: &[],
            solvers: &[],
        };
        let cached_inputs = GenerationCaptureInputs {
            core_captures: Some(&store),
            runtime: &runtime,
            records: &NoRecords,
            initial_record_cache: &[],
            solvers: &[],
        };
        let make = |captures| {
            Generator::new(
                &nodes,
                &profile,
                &registry,
                GenerationPhase {
                    captures: Some(captures),
                    callgraph: GenerationCallGraph::Absent,
                    propagated: &propagated,
                    library: &EmptyLibrary,
                    records: &records,
                    self_recursive: None,
                },
            )
            .unwrap()
        };
        let mut old = make(&plain_inputs);
        let mut new = make(&cached_inputs);
        old.phase_entries = vec![(ModuleId(0), NodeId(1), NodeId(3))];
        new.phase_entries = old.phase_entries.clone();
        let initial = new.capture_state_exact().unwrap();
        assert!(
            initial
                .compare(&old.capture_state_exact().unwrap())
                .unwrap()
                .is_eq()
        );
        for (flag, label) in [(true, 100), (false, 200)] {
            old.state.in_guard = flag;
            new.state.in_guard = flag;
            old.state.next_label = label;
            new.state.next_label = label;
            let current = new.capture_state_exact().unwrap();
            assert!(
                current
                    .compare(&old.capture_state_exact().unwrap())
                    .unwrap()
                    .is_eq()
            );
            assert!(!current.compare(&initial).unwrap().is_eq());
        }
        let old_constraint = old.arena.group(GroupKind::Conjunction, vec![]).unwrap();
        let new_constraint = new.arena.group(GroupKind::Conjunction, vec![]).unwrap();
        old.state.context.push(old_constraint);
        new.state.context.push(new_constraint);
        let changed_constraints = new.capture_state_exact().unwrap();
        assert!(
            changed_constraints
                .compare(&old.capture_state_exact().unwrap())
                .unwrap()
                .is_eq()
        );
        new.state.context.clear();
        assert!(
            !changed_constraints
                .compare(&new.capture_state_exact().unwrap())
                .unwrap()
                .is_eq()
        );
        let other_runtime = crate::constraints::generation_capture_headers::GenerationRuntimeCaptures::for_analysis("new-runtime-scope".into(), labels.next_label, &program).unwrap();
        let other_inputs = GenerationCaptureInputs {
            core_captures: Some(&store),
            runtime: &other_runtime,
            records: &NoRecords,
            initial_record_cache: &[],
            solvers: &[],
        };
        let mut other = make(&other_inputs);
        other.phase_entries = old.phase_entries.clone();
        let other_state = other.capture_state_exact().unwrap();
        assert_ne!(other_state.to_value(), initial.to_value());
        assert_eq!(
            other_state.compare(&initial).unwrap_err(),
            "comparison reached distinct opaque runtime handles"
        );
        let foreign_core = super::capture_store_tests::program();
        let foreign_store = CoreCaptureStore::new(&foreign_core, &labels);
        let foreign_inputs = GenerationCaptureInputs {
            core_captures: Some(&foreign_store),
            runtime: &runtime,
            records: &NoRecords,
            initial_record_cache: &[],
            solvers: &[],
        };
        assert!(matches!(
            Generator::new(
                &nodes,
                &profile,
                &registry,
                GenerationPhase {
                    captures: Some(&foreign_inputs),
                    callgraph: GenerationCallGraph::Absent,
                    propagated: &propagated,
                    library: &EmptyLibrary,
                    records: &records,
                    self_recursive: None
                }
            ),
            Err(GenerationError::Boundary(_))
        ));
        assert!(std::ptr::eq(
            store.get(ModuleId(0)).unwrap(),
            store.get(ModuleId(0)).unwrap()
        ));
    }

    #[test]
    fn actual_compiled_module_capture_equality_when_requested() {
        let Ok(paths) = std::env::var("CORE_CAPTURE_PROOF_MODULES") else {
            return;
        };
        for path in paths.split(':') {
            let module: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            let wire = serde_json::json!({"version":5,"profile":{"term_map_order":"native_to_list_with_iterator","otp":"28","erts":"16","elixir":"1.20.4","dialyzer":"5","core_options":[]},"modules":[module]});
            let core = CoreProgram::parse(&wire.to_string()).unwrap();
            let labels = Labels::build(&core, CoreLabel(0)).unwrap();
            let store = CoreCaptureStore::new(&core, &labels);
            let fresh =
                crate::core_capture::CoreCaptures::build(&core, &labels, ModuleId(0)).unwrap();
            let cached = store.get(ModuleId(0)).unwrap();
            let all: Vec<_> = (0..core.modules[0].nodes.len()).map(NodeId).collect();
            for selected in [
                all.clone(),
                vec![core.modules[0].core],
                vec![core.modules[0].core, NodeId(0), core.modules[0].core],
                vec![],
            ] {
                let mut before = vec![CaptureTerm::Atom("prefix".into())];
                let mut after = before.clone();
                assert_eq!(
                    fresh.append_selected(&selected, &mut before).unwrap(),
                    cached.append_selected(&selected, &mut after).unwrap()
                );
                assert_eq!(format!("{before:?}"), format!("{after:?}"));
                assert!(
                    fresh
                        .environment(&selected)
                        .unwrap()
                        .compare(&cached.environment(&selected).unwrap())
                        .unwrap()
                        .is_eq()
                );
            }
            eprintln!(
                "actual captures exact: {} / {} Cerl nodes / four selection shapes",
                core.modules[0].name,
                all.len()
            );
        }
    }

    #[test]
    fn malformed_table_error_stays_at_first_use() {
        let mut core = program();
        let labels = Labels::build(&core, CoreLabel(0)).unwrap();
        core.modules[0].nodes[1].annotations = TermId(1);
        let store = CoreCaptureStore::new(&core, &labels);
        assert!(store.modules[0].get().is_none());
        let expected = crate::core_capture::CoreCaptures::build(&core, &labels, ModuleId(0))
            .err()
            .unwrap();
        assert_eq!(store.get(ModuleId(0)).err().unwrap(), expected);
        assert_eq!(store.get(ModuleId(0)).err().unwrap(), expected);
        assert!(store.modules[0].get().is_some());
    }
}

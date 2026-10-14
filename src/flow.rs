use crate::core::{CoreExpr, CoreProgram, ErlangTerm, NodeId, TermId};
use crate::lattice::{AtomsView, IntegerView, ListView, NumberView, TuplesView, Type, TypeView};
use crate::program::{BindingId, FunctionId, FunctionName, ModuleId, NodeRef, Program};
use crate::schedule::{ComponentId, Plan};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Environment {
    pub bindings: im::OrdMap<BindingId, Type>,
    pub expressions: im::OrdMap<NodeRef, Type>,
}
impl Environment {
    fn new() -> Self {
        Self::default()
    }
    fn is_empty(&self) -> bool {
        self.bindings.is_empty() && self.expressions.is_empty()
    }
}
impl FromIterator<(BindingId, Type)> for Environment {
    fn from_iter<I: IntoIterator<Item = (BindingId, Type)>>(iter: I) -> Self {
        Self {
            bindings: iter.into_iter().collect(),
            expressions: im::OrdMap::new(),
        }
    }
}
pub type Result<T> = std::result::Result<T, FlowError>;
#[derive(Clone, Debug)]
pub struct FlowError {
    pub origin: Option<NodeRef>,
    pub message: String,
}
impl std::fmt::Display for FlowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.origin, self.message)
    }
}
impl std::error::Error for FlowError {}
impl From<crate::lattice::LatticeError> for FlowError {
    fn from(e: crate::lattice::LatticeError) -> Self {
        Self {
            origin: None,
            message: e.to_string(),
        }
    }
}
fn any_impossible_intersection(a: &[Type], b: &[Type]) -> Result<bool> {
    for (x, y) in a.iter().zip(b) {
        if x.meet(y)?.is_none() {
            return Ok(true);
        }
    }
    Ok(false)
}
pub use crate::call_transfer::Signature;
#[derive(Clone, Debug)]
pub struct Transfer {
    pub accepted: Vec<Type>,
    pub result: Type,
    pub can_raise: bool,
}
#[derive(Clone, Debug)]
pub struct GuardTransfer {
    pub arguments: Vec<Type>,
    pub result: Type,
}
pub trait BifProvider {
    fn admissible_domains(
        &self,
        name: &FunctionName,
    ) -> std::result::Result<Option<Vec<Type>>, String>;
    /// None means a non-BIF call, to be resolved by the external store.
    fn transfer(
        &self,
        name: &FunctionName,
        args: &[Type],
    ) -> std::result::Result<Option<Transfer>, String>;
    /// Refine inputs under a guard truth assumption, including comparison domains.
    fn guard(
        &self,
        name: &FunctionName,
        args: &[Type],
        truth: bool,
    ) -> std::result::Result<Option<GuardTransfer>, String>;
}
pub trait ConstraintProvider {
    /// Existing success summary in the input PLT; absence is the native unconstrained function type.
    fn initial_summary(
        &self,
        _name: &FunctionName,
    ) -> std::result::Result<Option<Signature>, String> {
        Ok(None)
    }
    /// Newly installed validated contracts also trigger the native module refinement phase.
    fn new_contracts(&self, _module: &str) -> bool {
        false
    }
    fn external_summary(&self, name: &FunctionName) -> std::result::Result<Signature, String>;
    /// Raw validated contract domains. This must not evaluate the dependent return.
    fn contract_domains(
        &self,
        name: &FunctionName,
    ) -> std::result::Result<Option<Vec<Type>>, String>;
    /// Only used outside the caller's active SCC; own specifications never constrain inference.
    fn contract_constraint(
        &self,
        name: &FunctionName,
        actual: &[Type],
    ) -> std::result::Result<Option<Signature>, String>;
}
#[derive(Clone, Debug)]
pub struct CallEvidence {
    pub success: Signature,
    pub contract: Option<Signature>,
    pub bif: Option<Signature>,
    pub narrowed_arguments: Vec<Type>,
    pub failed_success: bool,
    pub failed_contract: bool,
    pub failed_bif: bool,
}
#[derive(Clone, Debug)]
pub enum WarningCause {
    CallArguments {
        target: FunctionName,
        actual: Vec<Type>,
        expected: Vec<Type>,
        evidence: Option<Box<CallEvidence>>,
    },
    ContractRange {
        target: FunctionName,
        actual: Vec<Type>,
        evidence: Box<crate::call_transfer::CallTransfer>,
    },
    OpaqueCall {
        target: FunctionName,
        actual: Vec<Type>,
        evidence: Box<crate::call_transfer::CallTransfer>,
    },
    OpaqueApply {
        target: FunctionId,
        actual: Vec<Type>,
        evidence: Box<crate::call_transfer::CallTransfer>,
    },
    RemoteTarget {
        field: &'static str,
        module: NodeId,
        name: NodeId,
        arguments: Vec<Type>,
        actual: Type,
        expected: Type,
    },
    ApplyCallArguments {
        target: FunctionId,
        actual: Vec<Type>,
        success: Signature,
    },
    ApplyArguments {
        actual: Vec<Type>,
        expected: Vec<Type>,
    },
    ApplyNonFunction {
        actual: Type,
        arity: usize,
    },
    PatternImpossible {
        pattern: NodeId,
        actual: Type,
    },
    PatternCovered {
        pattern: NodeId,
        original: Type,
    },
    GuardImpossible {
        actual: Type,
        operation: Option<FunctionName>,
        arguments: Vec<Type>,
    },
    NoReturn {
        result: Type,
    },
    MapUpdate {
        map: Type,
        key: Type,
    },
    BinaryConstruction {
        segment: NodeId,
        field: &'static str,
        actual: Type,
    },
}
#[derive(Clone, Debug)]
pub struct Warning {
    pub function: FunctionId,
    pub origin: NodeRef,
    pub provenance: NodeRef,
    pub forced: bool,
    pub cause: WarningCause,
}
#[derive(Clone, Debug)]
pub struct Outcome {
    pub value: Type,
    pub environment: Environment,
    pub can_raise: bool,
}
#[derive(Clone, Debug)]
pub struct Analysis {
    pub signatures: Vec<Signature>,
    pub success_signatures: Vec<Signature>,
    pub warnings: Vec<Warning>,
    pub evaluations: usize,
    pub contextual_inputs: Vec<Vec<Type>>,
}
pub struct Engine<'a, B, C> {
    pub core: &'a CoreProgram,
    pub program: &'a Program,
    pub plan: &'a Plan,
    pub bifs: &'a B,
    pub constraints: &'a C,
    pub depth: i64,
    pub iteration_limit: usize,
}
struct Evaluation {
    summary: Signature,
    warnings: Vec<Warning>,
    captures: BTreeMap<FunctionId, Environment>,
    forwarded: Vec<(FunctionId, Vec<Type>)>,
}
struct Context<'a, B, C> {
    engine: &'a Engine<'a, B, C>,
    signatures: &'a [Signature],
    function: FunctionId,
    component: ComponentId,
    warnings: Vec<Warning>,
    captures: BTreeMap<FunctionId, Environment>,
    forwarded: Vec<(FunctionId, Vec<Type>)>,
    call_details: BTreeMap<NodeRef, (FunctionName, Vec<Type>)>,
}
fn err(at: NodeRef, msg: impl Into<String>) -> FlowError {
    FlowError {
        origin: Some(at),
        message: msg.into(),
    }
}
fn values(t: &Type) -> Vec<Type> {
    match t.view() {
        TypeView::Product(xs) => xs.to_vec(),
        _ => vec![t.clone()],
    }
}
fn result(xs: Vec<Type>) -> Type {
    if xs.len() == 1 {
        xs.into_iter().next().unwrap()
    } else {
        Type::product(xs)
    }
}
fn join_env(a: &Environment, b: &Environment) -> Result<Environment> {
    let mut out = a.clone();
    for (key, value) in &b.bindings {
        if let Some(old) = out.bindings.get_mut(key) {
            *old = old.join(value)?;
        } else {
            out.bindings.insert(*key, value.clone());
        }
    }
    for (key, value) in &b.expressions {
        if let Some(old) = out.expressions.get_mut(key) {
            *old = old.join(value)?;
        } else {
            out.expressions.insert(*key, value.clone());
        }
    }
    Ok(out)
}

impl<B: BifProvider, C: ConstraintProvider> Engine<'_, B, C> {
    pub fn infer(&self) -> Result<Analysis> {
        let mut signatures: Vec<_> = self
            .program
            .functions
            .iter()
            .map(|f| Signature {
                arguments: vec![Type::any(); f.arity],
                result: Type::none(),
            })
            .collect();
        let mut capture_envs: BTreeMap<FunctionId, Environment> = BTreeMap::new();
        let mut evaluations = 0;
        let mut tried_unit = false;
        let mut solving_domains = false;
        // Stable signatures are computed before emitting diagnostics.
        for _round in 0..self.iteration_limit {
            let mut changed = false;
            for component in &self.plan.functions.callee_first {
                let comp = self
                    .plan
                    .functions
                    .components
                    .iter()
                    .find(|c| c.id == *component)
                    .unwrap();
                for vertex in &comp.members {
                    let function = self.plan.functions.vertices[vertex.0];
                    let Evaluation {
                        summary, captures, ..
                    } = self.evaluate(
                        function,
                        *component,
                        &signatures,
                        &signatures[function.0].arguments,
                        capture_envs.get(&function).cloned().unwrap_or_default(),
                    )?;
                    evaluations += 1;
                    let old = &signatures[function.0];
                    let next = Signature {
                        arguments: if solving_domains {
                            old.arguments
                                .iter()
                                .zip(&summary.arguments)
                                .map(|(a, b)| Ok(a.meet(b)?.limit(self.depth)?))
                                .collect::<Result<Vec<_>>>()?
                        } else {
                            old.arguments.clone()
                        },
                        result: summary.result.limit(self.depth)?,
                    };
                    if *old != next {
                        signatures[function.0] = next;
                        changed = true;
                    }
                    for (target, env) in captures {
                        let old = capture_envs.entry(target).or_default();
                        let new = join_env(old, &env)?;
                        if *old != new {
                            *old = new;
                            changed = true;
                        }
                    }
                }
            }
            if !changed {
                if !tried_unit {
                    tried_unit = true;
                    let mut retry = false;
                    for comp in &self.plan.functions.components {
                        if comp.recursive {
                            for vertex in &comp.members {
                                let id = self.plan.functions.vertices[vertex.0];
                                if signatures[id.0].result.is_none() {
                                    signatures[id.0].result = Type::unit();
                                    retry = true;
                                }
                            }
                        }
                    }
                    if retry {
                        continue;
                    }
                }
                if !solving_domains {
                    solving_domains = true;
                    continue;
                }
                return self.contextual(signatures, capture_envs, evaluations);
            }
        }
        Err(FlowError {
            origin: None,
            message: format!(
                "success typing failed to converge within {} rounds",
                self.iteration_limit
            ),
        })
    }
    fn contextual(
        &self,
        success: Vec<Signature>,
        mut capture_envs: BTreeMap<FunctionId, Environment>,
        mut evaluations: usize,
    ) -> Result<Analysis> {
        let mut inputs: Vec<Vec<Type>> = self
            .program
            .functions
            .iter()
            .map(|f| {
                vec![
                    if self.plan.closures.escaping.local.contains(&f.id) {
                        Type::any()
                    } else {
                        Type::none()
                    };
                    f.arity
                ]
            })
            .collect();
        let mut refinement_modules: Vec<bool> = self
            .program
            .modules
            .iter()
            .map(|m| self.constraints.new_contracts(&m.name))
            .collect();
        for f in &self.program.functions {
            let new = &success[f.id.0];
            if new.result.is_none() || new.arguments.iter().any(Type::is_none) {
                continue;
            }
            let old = if let crate::program::FunctionIdentity::Named(name) = &f.identity {
                self.constraints.initial_summary(name).map_err(|e| {
                    err(
                        NodeRef {
                            module: f.module,
                            node: f.node.0,
                        },
                        e,
                    )
                })?
            } else {
                None
            };
            let old = old.unwrap_or(Signature {
                arguments: vec![Type::any(); f.arity],
                result: Type::any(),
            });
            if old != *new {
                refinement_modules[f.module.0] = true;
            }
        }
        let mut current = success.clone();
        let mut reachable: Vec<bool> = self
            .program
            .functions
            .iter()
            .map(|f| self.plan.closures.escaping.local.contains(&f.id))
            .collect();
        for _ in 0..self.iteration_limit {
            let mut changed = false;
            for component in &self.plan.functions.callee_first {
                let comp = self
                    .plan
                    .functions
                    .components
                    .iter()
                    .find(|c| c.id == *component)
                    .unwrap();
                for vertex in &comp.members {
                    let id = self.plan.functions.vertices[vertex.0];
                    if !reachable[id.0] || inputs[id.0].iter().any(Type::is_none) {
                        continue;
                    }
                    let Evaluation {
                        summary: out,
                        captures,
                        forwarded,
                        ..
                    } = self.evaluate(
                        id,
                        *component,
                        &current,
                        &inputs[id.0],
                        capture_envs.get(&id).cloned().unwrap_or_default(),
                    )?;
                    evaluations += 1;
                    let narrowed = out.result.limit(self.depth)?;
                    if current[id.0].result != narrowed {
                        current[id.0].result = narrowed;
                        changed = true;
                    }
                    for (target, args) in forwarded {
                        if !reachable[target.0] {
                            reachable[target.0] = true;
                            changed = true;
                        }
                        for (old, new) in inputs[target.0].iter_mut().zip(args) {
                            let joined = old.join(&new)?.limit(self.depth)?;
                            if *old != joined {
                                *old = joined;
                                changed = true;
                            }
                        }
                    }
                    for (target, env) in captures {
                        let old = capture_envs.entry(target).or_default();
                        let new = join_env(old, &env)?;
                        if *old != new {
                            *old = new;
                            changed = true;
                        }
                    }
                }
            }
            if !changed {
                let mut warnings = Vec::new();
                for f in &self.program.functions {
                    if !reachable[f.id.0] || inputs[f.id.0].iter().any(Type::is_none) {
                        continue;
                    }
                    let component = self.plan.functions.vertex_component[f.id.0];
                    let Evaluation {
                        summary: out,
                        warnings: w,
                        ..
                    } = self.evaluate(
                        f.id,
                        component,
                        &current,
                        &inputs[f.id.0],
                        capture_envs.get(&f.id).cloned().unwrap_or_default(),
                    )?;
                    warnings.extend(w);
                    evaluations += 1;
                    if out.result.is_impossible() {
                        warnings.push(Warning {
                            function: f.id,
                            origin: NodeRef {
                                module: f.module,
                                node: f.node.0,
                            },
                            provenance: NodeRef {
                                module: f.module,
                                node: f.node.0,
                            },
                            forced: false,
                            cause: WarningCause::NoReturn { result: out.result },
                        });
                    }
                }
                let mut persisted = current;
                for f in &self.program.functions {
                    if !refinement_modules[f.module.0] {
                        persisted[f.id.0] = success[f.id.0].clone();
                    }
                }
                return Ok(Analysis {
                    signatures: persisted,
                    success_signatures: success,
                    warnings,
                    evaluations,
                    contextual_inputs: inputs,
                });
            }
        }
        Err(FlowError {
            origin: None,
            message: "contextual dataflow did not converge".into(),
        })
    }
    fn evaluate(
        &self,
        function: FunctionId,
        component: ComponentId,
        signatures: &[Signature],
        inputs: &[Type],
        mut env: Environment,
    ) -> Result<Evaluation> {
        let f = &self.program.functions[function.0];
        for capture in &f.captures {
            if !env.bindings.contains_key(capture) {
                env.bindings.insert(*capture, Type::any());
            }
        }
        let module = &self.core.modules[f.module.0];
        let CoreExpr::Fun { params, body } = &module.nodes[f.node.0].expr else {
            return Err(err(
                NodeRef {
                    module: f.module,
                    node: f.node.0,
                },
                "indexed function is not Core fun",
            ));
        };
        for (p, t) in params.iter().zip(inputs) {
            let at = NodeRef {
                module: f.module,
                node: p.0,
            };
            let id = *self
                .program
                .binder_nodes
                .get(&at)
                .ok_or_else(|| err(at, "missing parameter binding"))?;
            env.bindings.insert(id, t.clone());
        }
        let mut context = Context {
            engine: self,
            signatures,
            function,
            component,
            warnings: Vec::new(),
            captures: BTreeMap::new(),
            forwarded: Vec::new(),
            call_details: BTreeMap::new(),
        };
        let out = context.eval(f.module, *body, env)?;
        let arguments = params
            .iter()
            .map(|p| {
                let id = self.program.binder_nodes[&NodeRef {
                    module: f.module,
                    node: p.0,
                }];
                out.environment
                    .bindings
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(Type::none)
            })
            .collect();
        Ok(Evaluation {
            summary: Signature {
                arguments,
                result: out.value,
            },
            warnings: context.warnings,
            captures: context.captures,
            forwarded: context.forwarded,
        })
    }
}
// The evaluator uses an explicit continuation stack. Core depth never consumes Rust call stack.
enum Frame {
    Record(NodeId),
    MapBase {
        base: NodeId,
        nodes: Vec<NodeId>,
        operations: Vec<(bool, NodeId)>,
    },
    Eval(NodeId, Environment),
    Many {
        nodes: Vec<NodeId>,
        next: usize,
        results: Vec<Type>,
        env: Environment,
        raises: bool,
        finish: Finish,
    },
    Let {
        vars: Vec<NodeId>,
        argument: NodeId,
        body: NodeId,
    },
    LetBody {
        vars: Vec<NodeId>,
        argument: NodeId,
        raises: bool,
    },
    Seq {
        body: NodeId,
    },
    Case {
        argument: NodeId,
        clauses: Vec<NodeId>,
        original: Type,
        residual: Type,
        index: usize,
        env: Environment,
        total: Type,
        joined: Environment,
        raises: bool,
    },
    ClauseBody {
        state: Box<Frame>,
        covered: Type,
        patterns: Vec<NodeId>,
        matched: Type,
    },
    Guard {
        state: Box<Frame>,
        body: NodeId,
        matched: Type,
        guard: NodeId,
        patterns: Vec<NodeId>,
    },
    Try {
        argument: NodeId,
        vars: Vec<NodeId>,
        body: NodeId,
        exception_vars: Vec<NodeId>,
        handler: NodeId,
        env: Environment,
    },
    TryHandler {
        exception_vars: Vec<NodeId>,
        handler: NodeId,
        original: Environment,
        baseline: Environment,
    },
    TryJoin {
        success: Outcome,
        baseline: Environment,
    },
    Catch(Environment),
}
enum Finish {
    Letrec(NodeId),
    Binary,
    Bitstr {
        at: NodeId,
        value: NodeId,
        size: NodeId,
        unit: BigInt,
        typ: String,
        all: bool,
        utf: bool,
    },
    PrimopBoolean,
    Map {
        base: NodeId,
        operations: Vec<(bool, NodeId)>,
    },
    Values,
    Tuple,
    Cons,
    Call {
        at: NodeId,
        module: NodeId,
        name: NodeId,
    },
    Apply {
        at: NodeId,
        operator: NodeId,
    },
}
impl<B: BifProvider, C: ConstraintProvider> Context<'_, B, C> {
    fn at(&self, module: ModuleId, node: NodeId) -> NodeRef {
        NodeRef {
            module,
            node: node.0,
        }
    }
    fn warn(&mut self, module: ModuleId, node: NodeId, cause: WarningCause) -> Result<()> {
        let core = &self.engine.core.modules[module.0];
        let selected = match (&cause, &core.nodes[node.0].expr) {
            (
                WarningCause::CallArguments {
                    actual, expected, ..
                }
                | WarningCause::ApplyArguments { actual, expected },
                CoreExpr::Call { args, .. } | CoreExpr::Apply { args, .. },
            ) => {
                let mut selected = node;
                for (i, (a, b)) in actual.iter().zip(expected).enumerate() {
                    if a.meet(b)?.is_none() {
                        if let Some(arg) = args.get(i) {
                            selected = *arg;
                        }
                        break;
                    }
                }
                selected
            }
            (
                WarningCause::ApplyCallArguments {
                    actual, success, ..
                },
                CoreExpr::Apply { args, .. },
            ) => {
                let mut selected = node;
                for (i, (a, b)) in actual.iter().zip(&success.arguments).enumerate() {
                    if a.meet(b)?.is_none() {
                        if let Some(arg) = args.get(i) {
                            selected = *arg;
                        }
                        break;
                    }
                }
                selected
            }
            (WarningCause::GuardImpossible { .. }, CoreExpr::Call { args, .. }) => args
                .iter()
                .copied()
                .find(|id| matches!(core.nodes[id.0].expr, CoreExpr::Var(_)))
                .or_else(|| args.first().copied())
                .unwrap_or(node),
            _ => node,
        };
        self.warnings.push(Warning {
            function: self.function,
            origin: self.at(module, selected),
            provenance: self.at(module, node),
            forced: matches!(&cause, WarningCause::PatternImpossible { .. })
                && !self.is_comprehension_default(module, node),
            cause,
        });
        Ok(())
    }
    fn is_comprehension_default(&self, module: ModuleId, node: NodeId) -> bool {
        if !self.engine.program.origins[module.0].node(node.0).generated {
            return false;
        }
        let m = &self.engine.core.modules[module.0];
        let CoreExpr::Clause {
            patterns, guard, ..
        } = &m.nodes[node.0].expr
        else {
            return false;
        };
        if self.atom(module, *guard).as_deref() != Some("true") {
            return false;
        }
        if patterns.len() != 1 && patterns.len() != 2 {
            return false;
        }
        if patterns.len() == 2 && !matches!(m.nodes[patterns[1].0].expr, CoreExpr::Var(_)) {
            return false;
        }
        let CoreExpr::Cons { head, tail } = &m.nodes[patterns[0].0].expr else {
            return false;
        };
        matches!(m.nodes[head.0].expr, CoreExpr::Var(_))
            && matches!(m.nodes[tail.0].expr, CoreExpr::Var(_))
    }
    fn bind_value(
        &self,
        module: ModuleId,
        node: NodeId,
        t: Type,
        env: &mut Environment,
    ) -> Result<()> {
        let at = self.at(module, node);
        let id = *self
            .engine
            .program
            .binder_nodes
            .get(&at)
            .or_else(|| self.engine.program.variable_uses.get(&at))
            .ok_or_else(|| err(at, "missing variable binding"))?;
        env.bindings.insert(id, t);
        Ok(())
    }
    fn variable_type(&self, module: ModuleId, node: NodeId, env: &Environment) -> Result<Type> {
        let at = self.at(module, node);
        let id = self
            .engine
            .program
            .variable_uses
            .get(&at)
            .ok_or_else(|| err(at, "missing variable-use binding"))?;
        if let Some(t) = env.bindings.get(id) {
            return Ok(t.clone());
        }
        if let Some(target) = self.engine.program.bindings[id.0].declared_function {
            let sig = &self.signatures[target.0];
            return Ok(Type::function(
                Some(sig.arguments.clone()),
                sig.result.clone(),
            ));
        }
        Err(err(at, "uninitialized lexical variable"))
    }
    fn refine(
        &self,
        module: ModuleId,
        node: NodeId,
        t: &Type,
        env: &mut Environment,
    ) -> Result<()> {
        let m = &self.engine.core.modules[module.0];
        let mut tasks = vec![(node, t.clone())];
        while let Some((node, t)) = tasks.pop() {
            let at = self.at(module, node);
            if matches!(m.nodes[node.0].expr, CoreExpr::Literal(_)) {
                continue;
            }
            let narrowed = match env.expressions.get(&at) {
                Some(old) => old.meet(&t)?,
                None => t,
            };
            env.expressions.insert(at, narrowed.clone());
            match &m.nodes[node.0].expr {
                CoreExpr::Var(_) => {
                    if let Some(id) = self.engine.program.variable_uses.get(&at) {
                        let old = self.variable_type(module, node, env)?;
                        env.bindings.insert(*id, old.meet(&narrowed)?);
                    }
                }
                CoreExpr::Values(xs) => {
                    let ts = if narrowed.is_any() || narrowed.is_none() {
                        vec![narrowed; xs.len()]
                    } else {
                        values(&narrowed)
                    };
                    if ts.len() != xs.len() {
                        return Err(err(at, "value refinement product arity mismatch"));
                    }
                    tasks.extend(xs.iter().copied().zip(ts));
                }
                CoreExpr::Tuple(xs) => {
                    let structural = narrowed.shallow_structural()?;
                    let mut fields = vec![Type::none(); xs.len()];
                    if structural.is_any() {
                        fields.fill(Type::any());
                    } else if !structural.is_none() {
                        match structural.view() {
                            TypeView::Families(s) => match s.tuples {
                                Some(TuplesView::Any) => fields.fill(Type::any()),
                                Some(TuplesView::Alternatives(alts)) => {
                                    for alt in alts.iter() {
                                        if alt.elements.len() == xs.len() {
                                            for (i, t) in alt.elements.iter().enumerate() {
                                                fields[i] = fields[i].join(t)?;
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            },
                            _ => {
                                return Err(err(
                                    at,
                                    "tuple expression refinement requires tuple family",
                                ));
                            }
                        }
                    }
                    tasks.extend(xs.iter().copied().zip(fields));
                }
                CoreExpr::Cons { head, tail } => {
                    let structural = narrowed.shallow_structural()?;
                    let (h, t) = if structural.is_any() {
                        (Type::any(), Type::any())
                    } else if structural.is_none() {
                        (Type::none(), Type::none())
                    } else {
                        match structural.view() {
                            TypeView::Families(s) => match s.list {
                                Some(ListView::List { elements, tail, .. }) => (
                                    elements.clone(),
                                    Type::list(elements.clone(), tail.clone(), false),
                                ),
                                _ => (Type::none(), Type::none()),
                            },
                            _ => {
                                return Err(err(
                                    at,
                                    "cons expression refinement requires list family",
                                ));
                            }
                        }
                    };
                    tasks.extend([(*head, h), (*tail, t)]);
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn atom(&self, module: ModuleId, node: NodeId) -> Option<String> {
        let m = &self.engine.core.modules[module.0];
        match &m.nodes[node.0].expr {
            CoreExpr::Literal(t) => match &m.terms[t.0] {
                ErlangTerm::Atom(a) => Some(a.clone()),
                _ => None,
            },
            _ => None,
        }
    }
    fn literal(&self, module: ModuleId, root: TermId, at: NodeId) -> Result<Type> {
        let m = &self.engine.core.modules[module.0];
        let mut built: BTreeMap<usize, Type> = BTreeMap::new();
        let mut tasks = vec![(root, false)];
        while let Some((id, ready)) = tasks.pop() {
            if built.contains_key(&id.0) {
                continue;
            }
            let term = &m.terms[id.0];
            if !ready {
                tasks.push((id, true));
                match term {
                    ErlangTerm::Tuple(xs) => tasks.extend(xs.iter().map(|x| (*x, false))),
                    ErlangTerm::List { items, tail } => {
                        tasks.push((*tail, false));
                        tasks.extend(items.iter().map(|x| (*x, false)))
                    }
                    ErlangTerm::Map(entries) => {
                        tasks.extend(entries.iter().flat_map(|(k, v)| [(*k, false), (*v, false)]))
                    }
                    _ => {}
                }
                continue;
            }
            let t = match term {
                ErlangTerm::ExternalFun(external_fun_payload) => {
                    let crate::core::ExternalFun { arity, .. } = external_fun_payload.as_ref();

                    Type::function(Some(vec![Type::any(); usize::from(*arity)]), Type::any())
                }
                ErlangTerm::Atom(a) => Type::atom(a),
                ErlangTerm::Integer(i) => Type::integer(
                    i.parse::<BigInt>()
                        .map_err(|e| err(self.at(module, at), e.to_string()))?,
                ),
                ErlangTerm::Float(_) => Type::float(),
                ErlangTerm::Binary(b) => Type::bitstring(0.into(), (b.len() * 8).into())
                    .map_err(|e| err(self.at(module, at), e))?,
                ErlangTerm::Bitstring { bits, .. } => Type::bitstring(0.into(), (*bits).into())
                    .map_err(|e| err(self.at(module, at), e))?,
                ErlangTerm::Nil => Type::nil(),
                ErlangTerm::Tuple(xs) => {
                    Type::tuple(xs.iter().map(|x| built[&x.0].clone()).collect())?
                }
                ErlangTerm::List { items, tail } => items
                    .iter()
                    .rev()
                    .try_fold(built[&tail.0].clone(), |t, x| {
                        Type::cons(built[&x.0].clone(), t)
                    })?,
                ErlangTerm::Map(entries) => {
                    let mut map = Type::map(Vec::new(), Type::none(), Type::none())?;
                    for (k, v) in entries {
                        map = map.map_put(built[&k.0].clone(), built[&v.0].clone())?;
                    }
                    map
                }
            };
            built.insert(id.0, t);
        }
        Ok(built.remove(&root.0).unwrap())
    }
    fn eval(
        &mut self,
        module: ModuleId,
        root: NodeId,
        environment: Environment,
    ) -> Result<Outcome> {
        let m = &self.engine.core.modules[module.0];
        let mut frames = vec![Frame::Eval(root, environment)];
        let mut last: Option<Outcome> = None;
        while let Some(frame) = frames.pop() {
            match frame {
                Frame::Eval(node, mut env) => {
                    frames.push(Frame::Record(node));
                    let at = self.at(module, node);
                    match &m.nodes[node.0].expr {
                        CoreExpr::Literal(t) => {
                            last = Some(Outcome {
                                value: self.literal(module, *t, node)?,
                                environment: env,
                                can_raise: false,
                            })
                        }
                        CoreExpr::Var(_) => {
                            let value = self.variable_type(module, node, &env)?;
                            last = Some(Outcome {
                                value,
                                environment: env,
                                can_raise: false,
                            });
                        }
                        CoreExpr::Fun { .. } => {
                            let target = self.engine.program.function_nodes[module.0][&node.0];
                            let f = &self.engine.program.functions[target.0];
                            let captures = f
                                .captures
                                .iter()
                                .filter_map(|id| env.bindings.get(id).map(|t| (*id, t.clone())))
                                .collect();

                            let old = self.captures.entry(target).or_default();
                            *old = join_env(old, &captures)?;
                            let s = &self.signatures[target.0];
                            last = Some(Outcome {
                                value: Type::function(Some(s.arguments.clone()), s.result.clone()),
                                environment: env,
                                can_raise: false,
                            });
                        }
                        CoreExpr::Binary(segments) => frames.push(Frame::Many {
                            nodes: segments.clone(),
                            next: 0,
                            results: Vec::new(),
                            env,
                            raises: false,
                            finish: Finish::Binary,
                        }),
                        CoreExpr::Bitstr {
                            value,
                            size,
                            unit,
                            typ,
                            ..
                        } => {
                            let unit = match &m.nodes[unit.0].expr {
                                CoreExpr::Literal(t) => match &m.terms[t.0] {
                                    ErlangTerm::Integer(v) => {
                                        v.parse::<BigInt>().map_err(|e| err(at, e.to_string()))?
                                    }
                                    ErlangTerm::Atom(a)
                                        if a == "undefined"
                                            && matches!(
                                                self.atom(module, *typ).as_deref(),
                                                Some("utf8" | "utf16" | "utf32")
                                            ) =>
                                    {
                                        1.into()
                                    }
                                    _ => return Err(err(at, "bitstring unit must be integer")),
                                },
                                _ => return Err(err(at, "bitstring unit must be literal")),
                            };
                            let typ = self
                                .atom(module, *typ)
                                .ok_or_else(|| err(at, "bitstring type must be literal atom"))?;
                            let all = self.atom(module, *size).as_deref() == Some("all");
                            let utf = self.atom(module, *size).as_deref() == Some("undefined")
                                && matches!(typ.as_str(), "utf8" | "utf16" | "utf32");
                            frames.push(Frame::Many {
                                nodes: vec![*size, *value],
                                next: 0,
                                results: Vec::new(),
                                env,
                                raises: false,
                                finish: Finish::Bitstr {
                                    at: node,
                                    value: *value,
                                    size: *size,
                                    unit,
                                    typ,
                                    all,
                                    utf,
                                },
                            });
                        }
                        CoreExpr::Map {
                            base,
                            pairs,
                            pattern: false,
                        } => {
                            let mut nodes = Vec::new();
                            let mut operations = Vec::new();
                            for pair in pairs {
                                let CoreExpr::MapPair {
                                    operation,
                                    key,
                                    value,
                                } = &m.nodes[pair.0].expr
                                else {
                                    return Err(err(
                                        self.at(module, *pair),
                                        "map member is not map pair",
                                    ));
                                };
                                operations.push((
                                    match self.atom(module, *operation).as_deref() {
                                        Some("assoc") => false,
                                        Some("exact") => true,
                                        _ => {
                                            return Err(err(
                                                self.at(module, *pair),
                                                "invalid Core map operation",
                                            ));
                                        }
                                    },
                                    *pair,
                                ));
                                nodes.extend([*key, *value]);
                            }
                            frames.push(Frame::MapBase {
                                base: *base,
                                nodes,
                                operations,
                            });
                            frames.push(Frame::Eval(*base, env));
                        }
                        CoreExpr::Values(xs) | CoreExpr::Tuple(xs) => frames.push(Frame::Many {
                            nodes: xs.clone(),
                            next: 0,
                            results: Vec::new(),
                            env,
                            raises: false,
                            finish: if matches!(m.nodes[node.0].expr, CoreExpr::Tuple(_)) {
                                Finish::Tuple
                            } else {
                                Finish::Values
                            },
                        }),
                        CoreExpr::Cons { head, tail } => frames.push(Frame::Many {
                            nodes: vec![*head, *tail],
                            next: 0,
                            results: Vec::new(),
                            env,
                            raises: false,
                            finish: Finish::Cons,
                        }),
                        CoreExpr::Let {
                            vars,
                            argument,
                            body,
                        } => {
                            frames.push(Frame::Let {
                                vars: vars.clone(),
                                argument: *argument,
                                body: *body,
                            });
                            frames.push(Frame::Eval(*argument, env));
                        }
                        CoreExpr::Seq { argument, body } => {
                            frames.push(Frame::Seq { body: *body });
                            frames.push(Frame::Eval(*argument, env));
                        }
                        CoreExpr::Letrec { definitions, body } => {
                            for (name, function) in definitions {
                                let target =
                                    self.engine.program.function_nodes[module.0][&function.0];
                                let sig = &self.signatures[target.0];
                                self.bind_value(
                                    module,
                                    *name,
                                    Type::function(Some(sig.arguments.clone()), sig.result.clone()),
                                    &mut env,
                                )?;
                            }
                            frames.push(Frame::Many {
                                nodes: definitions.iter().map(|(_, f)| *f).collect(),
                                next: 0,
                                results: Vec::new(),
                                env,
                                raises: false,
                                finish: Finish::Letrec(*body),
                            });
                        }
                        CoreExpr::Call {
                            module: modnode,
                            name,
                            args,
                        } => {
                            let mut nodes = vec![*modnode, *name];
                            nodes.extend(args);
                            frames.push(Frame::Many {
                                nodes,
                                next: 0,
                                results: Vec::new(),
                                env,
                                raises: false,
                                finish: Finish::Call {
                                    at: node,
                                    module: *modnode,
                                    name: *name,
                                },
                            });
                        }
                        CoreExpr::Apply { operator, args } => {
                            let mut nodes = args.clone();
                            nodes.push(*operator);
                            frames.push(Frame::Many {
                                nodes,
                                next: 0,
                                results: Vec::new(),
                                env,
                                raises: false,
                                finish: Finish::Apply {
                                    at: node,
                                    operator: *operator,
                                },
                            });
                        }
                        CoreExpr::Case { argument, clauses } => {
                            frames.push(Frame::Case {
                                argument: *argument,
                                clauses: clauses.clone(),
                                original: Type::unit(),
                                residual: Type::unit(),
                                index: 0,
                                env: Environment::new(),
                                total: Type::none(),
                                joined: Environment::new(),
                                raises: false,
                            });
                            frames.push(Frame::Eval(*argument, env));
                        }
                        CoreExpr::Try(try_payload) => {
                            let crate::core::TryExpr {
                                argument,
                                vars,
                                body,
                                exception_vars,
                                handler,
                            } = try_payload.as_ref();

                            frames.push(Frame::Try {
                                argument: *argument,
                                vars: vars.clone(),
                                body: *body,
                                exception_vars: exception_vars.clone(),
                                handler: *handler,
                                env: env.clone(),
                            });
                            frames.push(Frame::Eval(*argument, env));
                        }
                        CoreExpr::Primop { name, args } => {
                            let opcode = self
                                .atom(module, *name)
                                .ok_or_else(|| err(at, "primop name must be literal atom"))?;
                            let value = match opcode.as_str() {
                                "match_fail" | "raise" => Type::none(),
                                "bs_init_writable" => {
                                    Type::bitstring(0.into(), 0.into()).map_err(|e| err(at, e))?
                                }
                                "build_stacktrace" | "dialyzer_unknown" | "remove_message"
                                | "nif_start" | "debug_line" | "executable_line" => Type::any(),
                                "recv_peek_message" => Type::product(vec![
                                    Type::atoms(["true", "false"]),
                                    Type::any(),
                                ]),
                                "recv_wait_timeout" => {
                                    if args.len() != 1 {
                                        return Err(err(at, "recv_wait_timeout arity must be 1"));
                                    }
                                    frames.push(Frame::Many {
                                        nodes: args.clone(),
                                        next: 0,
                                        results: Vec::new(),
                                        env,
                                        raises: false,
                                        finish: Finish::PrimopBoolean,
                                    });
                                    continue;
                                }
                                _ => {
                                    return Err(err(
                                        at,
                                        format!("unsupported native primop {opcode}"),
                                    ));
                                }
                            };
                            last = Some(Outcome {
                                value,
                                environment: env,
                                can_raise: false,
                            });
                        }
                        CoreExpr::Catch { body } => {
                            frames.push(Frame::Catch(env.clone()));
                            frames.push(Frame::Eval(*body, env));
                        }
                        _ => {
                            return Err(err(
                                at,
                                format!(
                                    "unsupported Core semantics: {}",
                                    m.nodes[node.0].expr.kind()
                                ),
                            ));
                        }
                    }
                }
                Frame::Record(node) => {
                    let out = last.as_mut().ok_or_else(|| {
                        err(self.at(module, node), "expression record missing result")
                    })?;
                    let at = self.at(module, node);
                    if let Some(constraint) = out.environment.expressions.get(&at) {
                        out.value = out.value.meet(constraint)?;
                    }
                    out.environment.expressions.insert(at, out.value.clone());
                }
                Frame::MapBase {
                    base,
                    nodes,
                    operations,
                } => {
                    let mut out = last.take().unwrap();
                    let map = out.value.meet(&Type::map_any())?;
                    self.refine(module, base, &map, &mut out.environment)?;
                    if map.is_none() {
                        out.value = Type::none();
                        out.can_raise = true;
                        last = Some(out);
                    } else {
                        frames.push(Frame::Many {
                            nodes,
                            next: 0,
                            results: vec![map],
                            env: out.environment,
                            raises: out.can_raise,
                            finish: Finish::Map { base, operations },
                        });
                    }
                }
                Frame::Many {
                    nodes,
                    next,
                    mut results,
                    mut env,
                    mut raises,
                    finish,
                } => {
                    if next > 0 {
                        let out = last.take().unwrap();
                        env = out.environment;
                        raises |= out.can_raise;
                        results.push(out.value);
                    }
                    if next < nodes.len() {
                        let child = nodes[next];
                        frames.push(Frame::Many {
                            nodes,
                            next: next + 1,
                            results,
                            env: env.clone(),
                            raises,
                            finish,
                        });
                        frames.push(Frame::Eval(child, env));
                    } else {
                        let value = match finish {
                            Finish::Binary => {
                                let mut binary = Type::bitstring(0.into(), 0.into())
                                    .map_err(|e| err(self.at(module, root), e))?;
                                for segment in results {
                                    binary = binary.bitstr_concat(&segment)?;
                                }
                                binary
                            }
                            Finish::Bitstr {
                                at,
                                value,
                                size,
                                unit,
                                typ,
                                all,
                                utf,
                            } => {
                                let raw_size = &results[0];
                                let raw_value = &results[1];
                                let value_domain = match typ.as_str() {
                                    "binary" => Type::bitstring(1.into(), 0.into())
                                        .map_err(|e| err(self.at(module, at), e))?,
                                    "integer" | "utf8" | "utf16" | "utf32" => Type::all_integers(),
                                    "float" => Type::number(),
                                    _ => {
                                        return Err(err(
                                            self.at(module, at),
                                            "unsupported bitstring type",
                                        ));
                                    }
                                };
                                let val = raw_value.meet(&value_domain)?;
                                self.refine(module, value, &val, &mut env)?;
                                let sz = if all || utf {
                                    None
                                } else {
                                    Some(
                                        raw_size.meet(
                                            &Type::range_strings("0", "pos_inf")
                                                .map_err(|e| err(self.at(module, at), e))?,
                                        )?,
                                    )
                                };
                                if let Some(sz) = &sz {
                                    self.refine(module, size, sz, &mut env)?;
                                }
                                if sz.as_ref().is_some_and(Type::is_none) {
                                    self.warn(
                                        module,
                                        size,
                                        WarningCause::BinaryConstruction {
                                            segment: at,
                                            field: "size",
                                            actual: raw_size.clone(),
                                        },
                                    )?;
                                    raises = true;
                                    Type::none()
                                } else if val.is_none() {
                                    self.warn(
                                        module,
                                        value,
                                        WarningCause::BinaryConstruction {
                                            segment: at,
                                            field: "value",
                                            actual: raw_value.clone(),
                                        },
                                    )?;
                                    raises = true;
                                    Type::none()
                                } else if all || utf {
                                    Type::bitstring(1.into(), 0.into())
                                        .map_err(|e| err(self.at(module, at), e))?
                                } else {
                                    let size = sz.unwrap().shallow_structural()?;
                                    match size.view() {
                                        TypeView::Families(slots) => match slots.number {
                                            Some(NumberView::Integer(IntegerView::Set(s)))
                                                if s.len() == 1 =>
                                            {
                                                Type::bitstring(0.into(), s.first().unwrap() * unit)
                                                    .map_err(|e| err(self.at(module, at), e))?
                                            }
                                            Some(NumberView::Integer(IntegerView::Set(s))) => {
                                                Type::bitstring(
                                                    unit.clone(),
                                                    s.first().unwrap() * unit,
                                                )
                                                .map_err(|e| err(self.at(module, at), e))?
                                            }
                                            _ => Type::bitstring(1.into(), 0.into())
                                                .map_err(|e| err(self.at(module, at), e))?,
                                        },
                                        _ => {
                                            return Err(err(
                                                self.at(module, at),
                                                "bitstring size refinement invariant",
                                            ));
                                        }
                                    }
                                }
                            }
                            Finish::PrimopBoolean => Type::atoms(["true", "false"]),
                            Finish::Map { base, operations } => {
                                let mut map = results[0].meet(&Type::map_any())?;
                                self.refine(module, base, &map, &mut env)?;
                                if !map.is_none() {
                                    for (i, (exact, pair)) in operations.iter().enumerate() {
                                        let key = results[2 * i + 1].clone();
                                        let value = results[2 * i + 2].clone();
                                        let prior = map.clone();
                                        map = if *exact {
                                            map.map_update(key.clone(), value)?
                                        } else {
                                            map.map_put(key.clone(), value)?
                                        };
                                        if map.is_none() {
                                            self.warn(
                                                module,
                                                *pair,
                                                WarningCause::MapUpdate { map: prior, key },
                                            )?;
                                            raises = true;
                                            break;
                                        }
                                    }
                                }
                                map
                            }
                            Finish::Values => result(results),
                            Finish::Letrec(body) => {
                                frames.push(Frame::Eval(body, env));
                                continue;
                            }
                            Finish::Tuple => Type::tuple(results)?,
                            Finish::Cons => Type::cons(results[0].clone(), results[1].clone())?,
                            Finish::Call {
                                at,
                                module: modnode,
                                name,
                            } => {
                                let args = results.split_off(2);
                                let raw_module = &results[0];
                                let raw_name = &results[1];
                                let module_type = raw_module.meet(&Type::all_atoms())?;
                                let name_type = raw_name.meet(&Type::all_atoms())?;
                                self.refine(module, modnode, &module_type, &mut env)?;
                                self.refine(module, name, &name_type, &mut env)?;
                                if module_type.is_none()
                                    || name_type.is_none()
                                    || args.iter().any(Type::is_impossible)
                                {
                                    let failure = if module_type.is_none() && !raw_module.is_none()
                                    {
                                        Some((modnode, "module", raw_module))
                                    } else if name_type.is_none() && !raw_name.is_none() {
                                        Some((name, "name", raw_name))
                                    } else {
                                        None
                                    };
                                    if let Some((node, field, actual)) = failure {
                                        self.warn(
                                            module,
                                            node,
                                            WarningCause::RemoteTarget {
                                                field,
                                                module: modnode,
                                                name,
                                                arguments: args,
                                                actual: actual.clone(),
                                                expected: Type::all_atoms(),
                                            },
                                        )?;
                                    }
                                    Type::none()
                                } else {
                                    let single_atom = |t: &Type| -> Option<String> {
                                        match t.view() {
                                            TypeView::Families(slots) => match slots.atoms {
                                                Some(AtomsView::Set(xs)) if xs.len() == 1 => {
                                                    Some(xs.first().unwrap().to_string())
                                                }
                                                _ => None,
                                            },
                                            _ => None,
                                        }
                                    };
                                    if let (Some(m), Some(f)) =
                                        (single_atom(&module_type), single_atom(&name_type))
                                    {
                                        let target = FunctionName {
                                            module: m,
                                            name: f,
                                            arity: args.len(),
                                        };
                                        let t = self.call(module, at, &target, &args, &mut env)?;
                                        raises |= t.can_raise;
                                        t.result
                                    } else {
                                        Type::any()
                                    }
                                }
                            }
                            Finish::Apply { at, operator } => {
                                let operator_type = results.pop().unwrap();
                                self.apply(
                                    module,
                                    at,
                                    operator,
                                    &operator_type,
                                    &results,
                                    &mut env,
                                )?
                            }
                        };
                        last = Some(Outcome {
                            value,
                            environment: env,
                            can_raise: raises,
                        });
                    }
                }
                Frame::Let {
                    vars,
                    argument,
                    body,
                } => {
                    let mut out = last.take().unwrap();
                    if !out.value.is_impossible() {
                        let xs = values(&out.value);
                        if xs.len() != vars.len() {
                            return Err(err(
                                self.at(module, body),
                                "Core let product arity mismatch",
                            ));
                        }
                        for (v, t) in vars.iter().zip(xs) {
                            self.bind_value(module, *v, t, &mut out.environment)?;
                        }
                        frames.push(Frame::LetBody {
                            vars,
                            argument,
                            raises: out.can_raise,
                        });
                        frames.push(Frame::Eval(body, out.environment));
                    } else {
                        last = Some(out);
                    }
                }
                Frame::LetBody {
                    vars,
                    argument,
                    raises,
                } => {
                    let mut out = last.take().unwrap();
                    if !out.value.is_impossible() {
                        let types = vars
                            .iter()
                            .map(|v| {
                                let id = self.engine.program.binder_nodes[&self.at(module, *v)];
                                out.environment.bindings[&id].clone()
                            })
                            .collect();
                        self.refine(module, argument, &result(types), &mut out.environment)?;
                    }
                    out.can_raise |= raises;
                    last = Some(out);
                }
                Frame::Seq { body } => {
                    let out = last.take().unwrap();
                    if out.value.is_impossible() {
                        last = Some(out);
                    } else {
                        frames.push(Frame::Eval(body, out.environment));
                    }
                }
                Frame::Case {
                    argument,
                    clauses,
                    mut original,
                    mut residual,
                    index,
                    mut env,
                    total,
                    joined,
                    raises,
                } => {
                    if index == 0 && original.is_unit() {
                        let out = last.take().unwrap();
                        original = out.value.clone();
                        residual = out.value;
                        env = out.environment;
                        if residual.is_impossible() {
                            last = Some(Outcome {
                                value: residual,
                                environment: env,
                                can_raise: out.can_raise,
                            });
                            continue;
                        }
                    }
                    if index == clauses.len() {
                        last = Some(Outcome {
                            value: total,
                            environment: if joined.is_empty() { env } else { joined },
                            can_raise: raises || !residual.is_none(),
                        });
                        continue;
                    }
                    let clause = clauses[index];
                    let CoreExpr::Clause {
                        patterns,
                        guard,
                        body,
                    } = &m.nodes[clause.0].expr
                    else {
                        return Err(err(self.at(module, clause), "case child is not clause"));
                    };
                    let mut branch = env.clone();
                    let matched = self.patterns(module, patterns, &residual, &mut branch)?;
                    let state = Frame::Case {
                        argument,
                        clauses,
                        original: original.clone(),
                        residual,
                        index: index + 1,
                        env,
                        total,
                        joined,
                        raises,
                    };
                    if matched.is_none() {
                        let mut original_env = match &state {
                            Frame::Case { env, .. } => env.clone(),
                            _ => unreachable!(),
                        };
                        let original_match =
                            self.patterns(module, patterns, &original, &mut original_env)?;
                        self.warn(
                            module,
                            clause,
                            if original_match.is_none() {
                                WarningCause::PatternImpossible {
                                    pattern: clause,
                                    actual: original,
                                }
                            } else {
                                WarningCause::PatternCovered {
                                    pattern: clause,
                                    original,
                                }
                            },
                        )?;
                        frames.push(state);
                    } else {
                        frames.push(Frame::Guard {
                            state: Box::new(state),
                            body: *body,
                            matched,
                            guard: *guard,
                            patterns: patterns.clone(),
                        });
                        frames.push(Frame::Eval(*guard, branch));
                    }
                }
                Frame::Guard {
                    state,
                    body,
                    matched,
                    guard,
                    patterns,
                } => {
                    let mut out = last.take().unwrap();
                    let truth = out.value.meet(&Type::atom("true"))?;
                    if truth.is_none() {
                        self.warn(
                            module,
                            guard,
                            WarningCause::GuardImpossible {
                                actual: out.value,
                                operation: self
                                    .call_details
                                    .get(&self.at(module, guard))
                                    .map(|x| x.0.clone()),
                                arguments: self
                                    .call_details
                                    .get(&self.at(module, guard))
                                    .map(|x| x.1.clone())
                                    .unwrap_or_default(),
                            },
                        )?;
                        frames.push(*state);
                        continue;
                    }
                    self.refine_guard(module, guard, true, &mut out.environment)?;
                    let refined =
                        self.patterns(module, &patterns, &matched, &mut out.environment)?;
                    let Frame::Case { argument, .. } = &*state else {
                        unreachable!()
                    };
                    if let CoreExpr::Values(nodes) = &m.nodes[argument.0].expr {
                        for (n, t) in nodes.iter().zip(values(&refined)) {
                            self.refine(module, *n, &t, &mut out.environment)?;
                        }
                    } else {
                        self.refine(module, *argument, &refined, &mut out.environment)?;
                    }
                    let covered = if out.value.is_subtype(&Type::atom("true"))? {
                        matched
                    } else {
                        Type::none()
                    };
                    frames.push(Frame::ClauseBody {
                        state,
                        covered,
                        patterns,
                        matched: refined,
                    });
                    frames.push(Frame::Eval(body, out.environment));
                }
                Frame::ClauseBody {
                    state,
                    covered,
                    patterns,
                    matched,
                } => {
                    let mut out = last.take().unwrap();
                    let Frame::Case {
                        argument,
                        clauses,
                        original,
                        residual,
                        index,
                        env,
                        total,
                        joined,
                        raises,
                    } = *state
                    else {
                        unreachable!()
                    };
                    if !out.value.is_impossible() {
                        let refined =
                            self.patterns(module, &patterns, &matched, &mut out.environment)?;
                        if let CoreExpr::Values(nodes) = &m.nodes[argument.0].expr {
                            for (n, t) in nodes.iter().zip(values(&refined)) {
                                self.refine(module, *n, &t, &mut out.environment)?;
                            }
                        } else {
                            self.refine(module, argument, &refined, &mut out.environment)?;
                        }
                    }
                    let residual = residual
                        .subtract(&covered)
                        .map_err(|e| err(self.at(module, root), e.to_string()))?;
                    frames.push(Frame::Case {
                        argument,
                        clauses,
                        original,
                        residual,
                        index,
                        env,
                        total: total.join(&out.value)?,
                        joined: if out.value.is_impossible() {
                            joined
                        } else {
                            join_env(&joined, &out.environment)?
                        },
                        raises: raises || out.can_raise,
                    });
                }
                Frame::Try {
                    argument: argnode,
                    vars,
                    body,
                    exception_vars,
                    handler,
                    env,
                } => {
                    let mut argument = last.take().unwrap();
                    let xs = values(&argument.value);
                    if vars.len() != xs.len() {
                        last = Some(argument);
                        continue;
                    }
                    let baseline = argument.environment.clone();
                    frames.push(Frame::TryHandler {
                        exception_vars,
                        handler,
                        original: env,
                        baseline,
                    });
                    if argument.value.is_impossible() {
                        last = Some(Outcome {
                            value: Type::none(),
                            environment: Environment::new(),
                            can_raise: false,
                        });
                    } else {
                        for (v, t) in vars.iter().zip(xs) {
                            self.bind_value(module, *v, t, &mut argument.environment)?;
                        }
                        frames.push(Frame::LetBody {
                            vars,
                            argument: argnode,
                            raises: false,
                        });
                        frames.push(Frame::Eval(body, argument.environment));
                    }
                }
                Frame::TryHandler {
                    exception_vars,
                    handler,
                    mut original,
                    baseline,
                } => {
                    let success = last.take().unwrap();
                    for var in exception_vars {
                        if matches!(m.nodes[var.0].expr, CoreExpr::Var(_)) {
                            self.bind_value(module, var, Type::any(), &mut original)?;
                        }
                    }
                    frames.push(Frame::TryJoin { success, baseline });
                    frames.push(Frame::Eval(handler, original));
                }
                Frame::TryJoin { success, baseline } => {
                    let handler = last.take().unwrap();
                    let environment = if success.value.is_impossible() {
                        handler.environment
                    } else if handler.value.is_impossible() {
                        success.environment
                    } else {
                        join_env(&success.environment, &handler.environment)?
                    };
                    last = Some(Outcome {
                        value: success.value.join(&handler.value)?,
                        environment: if environment.is_empty() {
                            baseline
                        } else {
                            environment
                        },
                        can_raise: success.can_raise || handler.can_raise,
                    });
                }
                Frame::Catch(original) => {
                    let _ = last.take().unwrap();
                    last = Some(Outcome {
                        value: Type::any(),
                        environment: original,
                        can_raise: false,
                    });
                }
            }
        }
        last.ok_or_else(|| err(self.at(module, root), "empty evaluation result"))
    }
    fn call(
        &mut self,
        module: ModuleId,
        node: NodeId,
        name: &FunctionName,
        args: &[Type],
        env: &mut Environment,
    ) -> Result<Transfer> {
        let at = self.at(module, node);
        self.call_details.insert(at, (name.clone(), args.to_vec()));
        if args.iter().any(Type::is_impossible) {
            return Ok(Transfer {
                accepted: args.to_vec(),
                result: Type::none(),
                can_raise: false,
            });
        }
        let (success, same_component) = if let Some(target) = self.engine.program.named.get(name) {
            self.forwarded.push((*target, args.to_vec()));
            (
                self.signatures[target.0].clone(),
                self.engine.plan.functions.vertex_component[target.0] == self.component,
            )
        } else {
            (
                self.engine
                    .constraints
                    .external_summary(name)
                    .map_err(|e| err(at, e))?,
                false,
            )
        };
        if success.arguments.len() != args.len() {
            return Err(err(at, "success signature arity mismatch"));
        }
        let contract = if same_component {
            None
        } else {
            self.engine
                .constraints
                .contract_constraint(name, args)
                .map_err(|e| err(at, e))?
        };
        if contract
            .as_ref()
            .is_some_and(|c| c.arguments.len() != args.len())
        {
            return Err(err(at, "contract signature arity mismatch"));
        }
        let bif_domain = self
            .engine
            .bifs
            .admissible_domains(name)
            .map_err(|e| err(at, e))?;
        if bif_domain.as_ref().is_some_and(|d| d.len() != args.len()) {
            return Err(err(at, "BIF domain arity mismatch"));
        }
        let mut accepted = Vec::with_capacity(args.len());
        let mut narrowed = Vec::with_capacity(args.len());
        for (i, actual) in args.iter().enumerate() {
            let mut domain = success.arguments[i].clone();
            if let Some(contract) = &contract {
                domain = domain.meet(&contract.arguments[i])?;
            }
            if let Some(bif) = &bif_domain {
                domain = domain.meet(&bif[i])?;
            }
            narrowed.push(actual.meet(&domain)?);
            accepted.push(domain);
        }
        let dependent = if same_component {
            None
        } else {
            self.engine
                .constraints
                .contract_constraint(name, &narrowed)
                .map_err(|e| err(at, e))?
        };
        if dependent.is_some() != contract.is_some() {
            return Err(err(
                at,
                "contract presence changed with argument refinement",
            ));
        }
        let bif = if bif_domain.is_some() {
            Some(
                self.engine
                    .bifs
                    .transfer(name, &narrowed)
                    .map_err(|e| err(at, e))?
                    .ok_or_else(|| err(at, "BIF domain present but transfer missing"))?,
            )
        } else {
            None
        };
        let mut result = success.result.clone();
        if let Some(contract) = &dependent {
            result = result.meet(&contract.result)?;
        }
        if let Some(bif) = &bif {
            result = result.meet(&bif.result)?;
        }
        let evidence = CallEvidence {
            failed_success: any_impossible_intersection(args, &success.arguments)?,
            failed_contract: match &contract {
                Some(c) => any_impossible_intersection(args, &c.arguments)?,
                None => false,
            },
            failed_bif: match &bif_domain {
                Some(d) => any_impossible_intersection(args, d)?,
                None => false,
            },
            success,
            contract: dependent,
            bif: bif.as_ref().map(|t| Signature {
                arguments: t.accepted.clone(),
                result: t.result.clone(),
            }),
            narrowed_arguments: narrowed,
        };
        let mut transfer = Transfer {
            accepted,
            result,
            can_raise: bif.as_ref().is_some_and(|b| b.can_raise),
        };
        if transfer.accepted.len() != args.len() {
            return Err(err(at, "provider accepted argument arity mismatch"));
        }
        let impossible = any_impossible_intersection(args, &transfer.accepted)?;
        if impossible {
            self.warn(
                module,
                node,
                WarningCause::CallArguments {
                    target: name.clone(),
                    actual: args.to_vec(),
                    expected: transfer.accepted.clone(),
                    evidence: Some(Box::new(evidence)),
                },
            )?;
            transfer.result = Type::none();
            transfer.can_raise = true;
        }
        if let CoreExpr::Call { args: argnodes, .. } | CoreExpr::Apply { args: argnodes, .. } =
            &self.engine.core.modules[module.0].nodes[node.0].expr
        {
            for (n, t) in argnodes.iter().zip(&transfer.accepted) {
                self.refine(module, *n, t, env)?;
            }
        }
        Ok(transfer)
    }
    fn apply(
        &mut self,
        module: ModuleId,
        node: NodeId,
        operator: NodeId,
        operator_type: &Type,
        args: &[Type],
        env: &mut Environment,
    ) -> Result<Type> {
        let at = self.at(module, node);
        if args.iter().any(Type::is_impossible) {
            return Ok(Type::none());
        }
        let targets = self.engine.plan.closures.call_targets.get(&at).cloned();
        if let Some(targets) = targets {
            let mut accepted = vec![Type::none(); args.len()];
            let mut returned = Type::none();
            let mut warnings = Vec::new();
            let mut all_warned = !targets.external;
            for target in targets.local {
                let warning_start = self.warnings.len();
                let mut branch_env = env.clone();
                let transfer = if let crate::program::FunctionIdentity::Named(name) =
                    &self.engine.program.functions[target.0].identity
                {
                    let name = name.clone();
                    self.call(module, node, &name, args, &mut branch_env)?
                } else {
                    self.forwarded.push((target, args.to_vec()));
                    let sig = self.signatures[target.0].clone();
                    let narrowed = args
                        .iter()
                        .zip(&sig.arguments)
                        .map(|(a, b)| a.meet(b))
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    let bad = narrowed.iter().any(Type::is_none);
                    if bad {
                        self.warn(
                            module,
                            node,
                            WarningCause::ApplyCallArguments {
                                target,
                                actual: args.to_vec(),
                                success: sig.clone(),
                            },
                        )?;
                    }
                    Transfer {
                        accepted: narrowed,
                        result: if bad { Type::none() } else { sig.result },
                        can_raise: bad,
                    }
                };
                let candidate_warnings = self.warnings.split_off(warning_start);
                if candidate_warnings.is_empty() {
                    all_warned = false;
                }
                warnings.extend(candidate_warnings);
                if !transfer.accepted.iter().any(Type::is_none) {
                    for (old, new) in accepted.iter_mut().zip(&transfer.accepted) {
                        *old = old.join(new)?;
                    }
                }
                returned = returned.join(&transfer.result)?;
            }
            if all_warned {
                self.warnings.extend(warnings);
            }
            if !targets.external {
                if let CoreExpr::Apply { args: argnodes, .. } =
                    &self.engine.core.modules[module.0].nodes[node.0].expr
                {
                    for (n, t) in argnodes.iter().zip(&accepted) {
                        self.refine(module, *n, t, env)?;
                    }
                }
                return Ok(returned);
            }
        }
        let callable = operator_type.meet(&Type::function(
            Some(vec![Type::any(); args.len()]),
            Type::any(),
        ))?;
        if callable.is_none() {
            self.warn(
                module,
                node,
                WarningCause::ApplyNonFunction {
                    actual: operator_type.clone(),
                    arity: args.len(),
                },
            )?;
            return Ok(Type::none());
        }
        self.refine(module, operator, &callable, env)?;
        match callable.view() {
            TypeView::Families(s) => {
                let f = s
                    .functions
                    .ok_or_else(|| err(at, "callable family missing function component"))?;
                if let Some(domain) = f.domain
                    && any_impossible_intersection(args, domain)?
                {
                    self.warn(
                        module,
                        node,
                        WarningCause::ApplyArguments {
                            actual: args.to_vec(),
                            expected: domain.to_vec(),
                        },
                    )?;
                    return Ok(Type::none());
                }
                if let Some(domain) = f.domain
                    && let CoreExpr::Apply { args: argnodes, .. } =
                        &self.engine.core.modules[module.0].nodes[node.0].expr
                {
                    for (n, t) in argnodes.iter().zip(domain) {
                        self.refine(module, *n, t, env)?;
                    }
                }
                Ok(if f.range.is_unit() {
                    Type::none()
                } else {
                    f.range.clone()
                })
            }
            _ => Err(err(at, "dynamic apply did not resolve to callable family")),
        }
    }
    fn refine_guard(
        &self,
        module: ModuleId,
        node: NodeId,
        truth: bool,
        env: &mut Environment,
    ) -> Result<()> {
        let m = &self.engine.core.modules[module.0];
        if let CoreExpr::Call {
            module: modnode,
            name,
            args,
        } = &m.nodes[node.0].expr
        {
            let name = FunctionName {
                module: self
                    .atom(module, *modnode)
                    .ok_or_else(|| err(self.at(module, node), "dynamic guard module"))?,
                name: self
                    .atom(module, *name)
                    .ok_or_else(|| err(self.at(module, node), "dynamic guard name"))?,
                arity: args.len(),
            };
            let types = args
                .iter()
                .map(|n| match &m.nodes[n.0].expr {
                    CoreExpr::Var(_) => self.variable_type(module, *n, env),
                    CoreExpr::Literal(t) => self.literal(module, *t, *n),
                    _ => env
                        .expressions
                        .get(&self.at(module, *n))
                        .cloned()
                        .ok_or_else(|| {
                            err(
                                self.at(module, *n),
                                "guard argument missing scoped expression type",
                            )
                        }),
                })
                .collect::<Result<Vec<_>>>()?;
            if let Some(out) = self
                .engine
                .bifs
                .guard(&name, &types, truth)
                .map_err(|e| err(self.at(module, node), e))?
            {
                if out.arguments.len() != args.len() {
                    return Err(err(self.at(module, node), "guard provider arity mismatch"));
                }
                for (n, t) in args.iter().zip(out.arguments) {
                    self.refine(module, *n, &t, env)?;
                }
            }
        }
        Ok(())
    }
    fn patterns(
        &self,
        module: ModuleId,
        patterns: &[NodeId],
        input: &Type,
        env: &mut Environment,
    ) -> Result<Type> {
        if input.is_none() {
            return Ok(Type::none());
        }
        let xs = if input.is_any() {
            vec![Type::any(); patterns.len()]
        } else {
            values(input)
        };
        if xs.len() != patterns.len() {
            return Err(err(
                self.at(module, patterns.first().copied().unwrap_or(NodeId(0))),
                "clause pattern product arity mismatch",
            ));
        }
        let mut matched = Vec::new();
        for (p, t) in patterns.iter().zip(xs) {
            matched.push(self.pattern(module, *p, &t, env)?);
        }
        if matched.iter().any(Type::is_none) {
            Ok(Type::none())
        } else {
            Ok(result(matched))
        }
    }
    fn pattern_shape(&self, module: ModuleId, root: NodeId, env: &Environment) -> Result<Type> {
        let m = &self.engine.core.modules[module.0];
        let mut shapes: BTreeMap<usize, Type> = BTreeMap::new();
        let mut tasks = vec![(root, false)];
        while let Some((id, ready)) = tasks.pop() {
            if shapes.contains_key(&id.0) {
                continue;
            }
            let e = &m.nodes[id.0].expr;
            if !ready {
                tasks.push((id, true));
                match e {
                    CoreExpr::Tuple(xs) | CoreExpr::Values(xs) => {
                        tasks.extend(xs.iter().map(|x| (*x, false)))
                    }
                    CoreExpr::Cons { head, tail } => tasks.extend([(*head, false), (*tail, false)]),
                    CoreExpr::Alias { pattern, .. } => tasks.push((*pattern, false)),
                    _ => {}
                }
                continue;
            }
            let t = match e {
                CoreExpr::Var(_) => self
                    .engine
                    .program
                    .binder_nodes
                    .get(&self.at(module, id))
                    .or_else(|| self.engine.program.variable_uses.get(&self.at(module, id)))
                    .and_then(|b| env.bindings.get(b))
                    .cloned()
                    .unwrap_or_else(Type::any),
                CoreExpr::Literal(t) => self.literal(module, *t, id)?,
                CoreExpr::Tuple(xs) => {
                    Type::tuple(xs.iter().map(|x| shapes[&x.0].clone()).collect())?
                }
                CoreExpr::Values(xs) => result(xs.iter().map(|x| shapes[&x.0].clone()).collect()),
                CoreExpr::Cons { head, tail } => {
                    Type::cons(shapes[&head.0].clone(), shapes[&tail.0].clone())?
                }
                CoreExpr::Alias { pattern, .. } => shapes[&pattern.0].clone(),
                CoreExpr::Map { pattern: true, .. } => Type::map_any(),
                CoreExpr::Binary(_) => {
                    Type::bitstring(1.into(), 0.into()).map_err(|e| err(self.at(module, id), e))?
                }
                _ => return Err(err(self.at(module, id), "unsupported pattern skeleton")),
            };
            shapes.insert(id.0, t);
        }
        Ok(shapes.remove(&root.0).unwrap())
    }
    fn pattern(
        &self,
        module: ModuleId,
        root: NodeId,
        input: &Type,
        env: &mut Environment,
    ) -> Result<Type> {
        // Reconstruct typed patterns using postorder tasks; binding equality is applied at leaves.
        enum Task {
            Visit(NodeId, Type),
            Finish(NodeId, usize, bool),
            BinaryNext {
                node: NodeId,
                segments: Vec<NodeId>,
                index: usize,
                remaining: Type,
                accumulated: Type,
            },
            BinaryValue {
                node: NodeId,
                segments: Vec<NodeId>,
                index: usize,
                remaining: Type,
                accumulated: Type,
                segment: Type,
                all: bool,
            },
            MapNext {
                node: NodeId,
                pairs: Vec<NodeId>,
                index: usize,
                input: Type,
                refined: Type,
            },
            MapValue {
                node: NodeId,
                pairs: Vec<NodeId>,
                index: usize,
                input: Type,
                refined: Type,
                key: Type,
            },
        }
        let m = &self.engine.core.modules[module.0];
        let mut tasks = vec![Task::Visit(root, input.clone())];
        let mut results: Vec<Type> = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Visit(node, t) => {
                    let at = self.at(module, node);
                    match &m.nodes[node.0].expr {
                        CoreExpr::Var(_) => {
                            let id = *self
                                .engine
                                .program
                                .binder_nodes
                                .get(&at)
                                .or_else(|| self.engine.program.variable_uses.get(&at))
                                .ok_or_else(|| err(at, "missing pattern binding"))?;
                            let narrowed = match env.bindings.get(&id) {
                                Some(old) => old.meet(&t)?,
                                None => t,
                            };
                            env.bindings.insert(id, narrowed.clone());
                            results.push(narrowed);
                        }
                        CoreExpr::Literal(term) => {
                            results.push(t.meet(&self.literal(module, *term, node)?)?)
                        }
                        CoreExpr::Alias { variable, pattern } => {
                            tasks.push(Task::Finish(node, 2, false));
                            tasks.push(Task::Visit(*variable, t.clone()));
                            tasks.push(Task::Visit(*pattern, t));
                        }
                        CoreExpr::Tuple(children) => {
                            let narrowed = t.meet(&self.pattern_shape(module, node, env)?)?;
                            if narrowed.is_none() {
                                results.push(Type::none());
                                continue;
                            }
                            let mut fields = vec![Type::none(); children.len()];
                            match narrowed.view() {
                                TypeView::Families(s) => match s.tuples {
                                    Some(TuplesView::Alternatives(xs)) => {
                                        for alt in xs.iter() {
                                            if alt.elements.len() == children.len() {
                                                for (i, t) in alt.elements.iter().enumerate() {
                                                    fields[i] = fields[i].join(t)?;
                                                }
                                            }
                                        }
                                    }
                                    Some(TuplesView::Any) => fields.fill(Type::any()),
                                    _ => return Err(err(at, "tuple pattern missing tuple slot")),
                                },
                                _ => return Err(err(at, "tuple pattern domain invariant")),
                            }
                            tasks.push(Task::Finish(node, children.len(), false));
                            for (child, t) in children.iter().zip(fields).rev() {
                                tasks.push(Task::Visit(*child, t));
                            }
                        }
                        CoreExpr::Map {
                            pairs,
                            pattern: true,
                            ..
                        } => {
                            let input = t.meet(&Type::map_any())?;
                            if input.is_none() {
                                results.push(Type::none());
                            } else {
                                tasks.push(Task::MapNext {
                                    node,
                                    pairs: pairs.clone(),
                                    index: 0,
                                    input,
                                    refined: Type::map_any(),
                                });
                            }
                        }
                        CoreExpr::Binary(segments) => {
                            let remaining = t.meet(
                                &Type::bitstring(1.into(), 0.into()).map_err(|e| err(at, e))?,
                            )?;
                            if remaining.is_none() {
                                results.push(Type::none());
                            } else {
                                tasks.push(Task::BinaryNext {
                                    node,
                                    segments: segments.clone(),
                                    index: 0,
                                    remaining,
                                    accumulated: Type::bitstring(0.into(), 0.into())
                                        .map_err(|e| err(at, e))?,
                                });
                            }
                        }
                        CoreExpr::Cons { head, tail } => {
                            let narrowed = t.meet(&Type::list(Type::any(), Type::any(), true))?;
                            if narrowed.is_none() {
                                results.push(Type::none());
                                continue;
                            }
                            let (element, termination) = match narrowed.view() {
                                TypeView::Families(s) => match s.list {
                                    Some(ListView::List { elements, tail, .. }) => (
                                        elements.clone(),
                                        Type::list(elements.clone(), tail.clone(), false),
                                    ),
                                    _ => return Err(err(at, "cons pattern missing nonempty list")),
                                },
                                _ => return Err(err(at, "cons pattern domain invariant")),
                            };
                            tasks.push(Task::Finish(node, 2, true));
                            tasks.push(Task::Visit(*tail, termination));
                            tasks.push(Task::Visit(*head, element));
                        }
                        _ => {
                            return Err(err(
                                at,
                                format!(
                                    "unsupported pattern semantics {}",
                                    m.nodes[node.0].expr.kind()
                                ),
                            ));
                        }
                    }
                }
                Task::BinaryNext {
                    node,
                    segments,
                    index,
                    remaining,
                    accumulated,
                } => {
                    if index == segments.len() {
                        results.push(accumulated);
                        continue;
                    }
                    let id = segments[index];
                    let at = self.at(module, id);
                    let CoreExpr::Bitstr {
                        value,
                        size,
                        unit,
                        typ,
                        flags,
                    } = &m.nodes[id.0].expr
                    else {
                        return Err(err(at, "binary pattern segment must be bitstr"));
                    };
                    let unit = match &m.nodes[unit.0].expr {
                        CoreExpr::Literal(t) => match &m.terms[t.0] {
                            ErlangTerm::Integer(v) => {
                                v.parse::<BigInt>().map_err(|e| err(at, e.to_string()))?
                            }
                            ErlangTerm::Atom(a)
                                if a == "undefined"
                                    && matches!(
                                        self.atom(module, *typ).as_deref(),
                                        Some("utf8" | "utf16" | "utf32")
                                    ) =>
                            {
                                1.into()
                            }
                            _ => return Err(err(at, "binary pattern unit must be integer")),
                        },
                        _ => return Err(err(at, "binary pattern unit must be literal")),
                    };
                    let typ = self
                        .atom(module, *typ)
                        .ok_or_else(|| err(at, "binary pattern type must be atom"))?;
                    let all = self.atom(module, *size).as_deref() == Some("all");
                    let utf = self.atom(module, *size).as_deref() == Some("undefined")
                        && matches!(typ.as_str(), "utf8" | "utf16" | "utf32");
                    let (segment, value_domain) = if all {
                        if typ != "binary" || index + 1 != segments.len() {
                            return Err(err(at, "all binary pattern must be final binary segment"));
                        }
                        let segment = remaining
                            .meet(&Type::bitstring(unit, 0.into()).map_err(|e| err(at, e))?)?;
                        (segment.clone(), segment)
                    } else if utf {
                        (
                            Type::bitstring(8.into(), 0.into()).map_err(|e| err(at, e))?,
                            Type::all_integers(),
                        )
                    } else {
                        let nonnegative =
                            Type::range_strings("0", "pos_inf").map_err(|e| err(at, e))?;
                        let raw_size = match &m.nodes[size.0].expr {
                            CoreExpr::Literal(t) => self.literal(module, *t, *size)?,
                            CoreExpr::Var(_) => self.variable_type(module, *size, env)?,
                            _ => return Err(err(at, "unsupported binary pattern size expression")),
                        };
                        let size_type = raw_size.meet(&nonnegative)?.shallow_structural()?;
                        self.refine(module, *size, &size_type, env)?;
                        if size_type.is_none() {
                            results.push(Type::none());
                            continue;
                        }
                        let finite = match size_type.view() {
                            TypeView::Families(slots) => match slots.number {
                                Some(NumberView::Integer(IntegerView::Set(s))) => Some(s),
                                _ => None,
                            },
                            _ => None,
                        };
                        let segment = if let Some(s) = finite {
                            Type::bitstring(
                                if s.len() == 1 { 0.into() } else { unit.clone() },
                                s.first().unwrap() * &unit,
                            )
                            .map_err(|e| err(at, e))?
                        } else {
                            Type::bitstring(unit.clone(), 0.into()).map_err(|e| err(at, e))?
                        };
                        let value_domain = match typ.as_str() {
                            "binary" => segment.clone(),
                            "float" => Type::float(),
                            "integer" => {
                                let signed = match &m.nodes[flags.0].expr {
                                    CoreExpr::Literal(t) => match &m.terms[t.0] {
                                        ErlangTerm::Nil => false,
                                        ErlangTerm::List { items, tail } => {
                                            if !matches!(m.terms[tail.0], ErlangTerm::Nil) {
                                                return Err(err(
                                                    at,
                                                    "binary flags must be proper list",
                                                ));
                                            }
                                            let mut signed = false;
                                            for t in items {
                                                match &m.terms[t.0] {
                                                    ErlangTerm::Atom(a) => signed |= a == "signed",
                                                    _ => {
                                                        return Err(err(
                                                            at,
                                                            "binary flag must be atom",
                                                        ));
                                                    }
                                                }
                                            }
                                            signed
                                        }
                                        _ => return Err(err(at, "binary flags must be list")),
                                    },
                                    _ => return Err(err(at, "binary flags must be literal")),
                                };
                                if let Some(s) = finite {
                                    let n = s.last().unwrap() * &unit;
                                    if n >= BigInt::from(128) {
                                        if signed {
                                            Type::all_integers()
                                        } else {
                                            nonnegative
                                        }
                                    } else {
                                        let n = n
                                            .to_usize()
                                            .ok_or_else(|| err(at, "negative binary width"))?;
                                        let (low, high): (BigInt, BigInt) = if signed {
                                            let half = if n == 0 {
                                                BigInt::from(0)
                                            } else {
                                                BigInt::from(1) << (n - 1)
                                            };
                                            (-&half, half - 1)
                                        } else {
                                            (BigInt::from(0), (BigInt::from(1) << n) - 1)
                                        };
                                        Type::range_strings(&low.to_string(), &high.to_string())
                                            .map_err(|e| err(at, e))?
                                    }
                                } else {
                                    Type::all_integers()
                                }
                            }
                            _ => return Err(err(at, "unsupported binary pattern segment type")),
                        };
                        (segment, value_domain)
                    };
                    let suffix = if all {
                        Type::bitstring(0.into(), 0.into()).map_err(|e| err(at, e))?
                    } else {
                        segment.bitstr_match(&remaining)?
                    };
                    if suffix.is_none() || segment.is_none() {
                        results.push(Type::none());
                        continue;
                    }
                    tasks.push(Task::BinaryValue {
                        node,
                        segments,
                        index,
                        remaining: suffix,
                        accumulated,
                        segment,
                        all,
                    });
                    tasks.push(Task::Visit(*value, value_domain));
                }
                Task::BinaryValue {
                    node,
                    segments,
                    index,
                    remaining,
                    accumulated,
                    segment,
                    all,
                } => {
                    let value = results.pop().ok_or_else(|| {
                        err(self.at(module, node), "binary pattern child missing")
                    })?;
                    if value.is_none() {
                        results.push(Type::none());
                    } else {
                        tasks.push(Task::BinaryNext {
                            node,
                            segments,
                            index: index + 1,
                            remaining,
                            accumulated: accumulated.bitstr_concat(if all {
                                &value
                            } else {
                                &segment
                            })?,
                        });
                    }
                }
                Task::MapNext {
                    node,
                    pairs,
                    index,
                    input,
                    refined,
                } => {
                    if index == pairs.len() {
                        results.push(input.meet(&refined)?);
                        continue;
                    }
                    let pair = pairs[index];
                    let CoreExpr::MapPair {
                        operation,
                        key,
                        value,
                    } = &m.nodes[pair.0].expr
                    else {
                        return Err(err(self.at(module, pair), "map pattern member is not pair"));
                    };
                    if self.atom(module, *operation).as_deref() != Some("exact") {
                        return Err(err(
                            self.at(module, pair),
                            "map pattern operation must be exact",
                        ));
                    }
                    let key = match &m.nodes[key.0].expr {
                        CoreExpr::Literal(t) => self.literal(module, *t, *key)?,
                        CoreExpr::Var(_) => self.variable_type(module, *key, env)?,
                        _ => {
                            return Err(err(
                                self.at(module, *key),
                                "unsupported map pattern key expression",
                            ));
                        }
                    };
                    let value_type = input.map_get(&key)?;
                    tasks.push(Task::MapValue {
                        node,
                        pairs,
                        index,
                        input,
                        refined,
                        key,
                    });
                    tasks.push(Task::Visit(*value, value_type));
                }
                Task::MapValue {
                    node,
                    pairs,
                    index,
                    input,
                    refined,
                    key,
                } => {
                    let value = results
                        .pop()
                        .ok_or_else(|| err(self.at(module, node), "map pattern child missing"))?;
                    if value.is_none() {
                        results.push(Type::none());
                    } else {
                        let refined = refined.map_put(key, value)?;
                        tasks.push(Task::MapNext {
                            node,
                            pairs,
                            index: index + 1,
                            input,
                            refined,
                        });
                    }
                }
                Task::Finish(node, count, cons) => {
                    let start = results.len().checked_sub(count).ok_or_else(|| {
                        err(self.at(module, node), "pattern evaluation underflow")
                    })?;
                    let xs = results.split_off(start);
                    let ty = if xs.iter().any(Type::is_none) {
                        Type::none()
                    } else if matches!(m.nodes[node.0].expr, CoreExpr::Alias { .. }) {
                        xs[0].meet(&xs[1])?
                    } else if cons {
                        Type::cons(xs[0].clone(), xs[1].clone())?
                    } else {
                        Type::tuple(xs)?
                    };
                    if let CoreExpr::Alias { variable, .. } = &m.nodes[node.0].expr {
                        self.bind_value(module, *variable, ty.clone(), env)?;
                    }
                    results.push(ty);
                }
            }
        }
        if results.len() != 1 {
            return Err(err(self.at(module, root), "pattern result arity mismatch"));
        }
        Ok(results.pop().unwrap())
    }
}
#[cfg(test)]
#[path = "flow_tests.rs"]
mod tests;

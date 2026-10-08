use crate::{
    bifs::registry::Mfa,
    constraints::atomic::{AtomicOutcome, BindingKey, BindingMap, SolveError},
    constraints::closure_order::ClosureCodeProfile,
    constraints::generation::{
        GenerationCaptureInputs, GenerationError, GenerationOutput, GenerationPhase, Generator, SourceBifProvider,
    },
    constraints::tree::FunctionVariable,
    core::CoreExpr,
    dataflow_nodes::DataflowNodes,
    labels::CoreLabel,
    lattice::Type,
    native_error::NativeTransferError,
    program::{ModuleId, NodeRef},
    source_callgraph::{GraphVertex, SourceCallGraph},
    term_order::CaptureEnvironment,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupportedSolver {
    V1,
    V2,
}
#[derive(Clone, Debug, Default)]
pub enum SolverSelection {
    #[default]
    Default,
    Explicit(Vec<SupportedSolver>),
}
impl SolverSelection {
    pub fn parse(requested: &[String]) -> Result<Self, AnalyzeSccError> {
        if requested.is_empty() {
            return Ok(Self::Default);
        }
        requested
            .iter()
            .map(|name| match name.as_str() {
                "v1" => Ok(SupportedSolver::V1),
                "v2" => Ok(SupportedSolver::V2),
                _ => Err(AnalyzeSccError::UnsupportedSolver(name.clone())),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Self::Explicit)
    }
    fn dependency_retention(&self) -> crate::constraints::preparation::DependencyRetention {
        match self {
            Self::Explicit(solvers) if solvers.contains(&SupportedSolver::V1) => {
                crate::constraints::preparation::DependencyRetention::Retain
            }
            _ => crate::constraints::preparation::DependencyRetention::Clear,
        }
    }
    fn source_atoms(&self) -> Vec<String> {
        match self {
            Self::Default => vec!["v2".into()],
            Self::Explicit(solvers) if solvers.is_empty() => vec!["v2".into()],
            Self::Explicit(solvers) => solvers
                .iter()
                .map(|solver| match solver {
                    SupportedSolver::V1 => "v1".into(),
                    SupportedSolver::V2 => "v2".into(),
                })
                .collect(),
        }
    }
}
#[derive(Debug)]
pub enum AnalyzeSccError {
    Boundary(String),
    Native(NativeTransferError),
    UnsupportedSolver(String),
    Generation(GenerationError),
    Solve(SolveError),
}
pub struct SccAnalysis {
    pub bindings: BindingMap,
    pub reached_functions: Vec<FunctionVariable>,
    pub next_label: CoreLabel,
    pub refold_allocations: u64,
    pub source_state: Option<CaptureEnvironment>,
}
pub struct SccAnalyzer<'a> {
    pub nodes: &'a DataflowNodes<'a>,
    pub profile: &'a ClosureCodeProfile,
    pub provider: &'a dyn SourceBifProvider,
    pub graph: &'a SourceCallGraph,
    pub phase: GenerationPhase<'a>,
}
impl SccAnalyzer<'_> {
    pub fn analyze(
        self,
        ordered_mfas: &[Mfa],
        next_label: CoreLabel,
        solvers: &SolverSelection,
        output: GenerationOutput,
    ) -> Result<SccAnalysis, AnalyzeSccError> {
        let mut timings = crate::phase_timings::PhaseTimings::detail("scc_components");
        let core = self.nodes.core();
        let mut entries = Vec::with_capacity(ordered_mfas.len());
        for mfa in ordered_mfas {
            let module = core
                .modules
                .iter()
                .position(|module| module.name == mfa.module)
                .ok_or_else(|| {
                    AnalyzeSccError::Boundary(format!("SCC module {} absent", mfa.module))
                })?;
            let CoreExpr::Module { definitions, .. } =
                &core.modules[module].nodes[core.modules[module].core.0].expr
            else {
                return Err(AnalyzeSccError::Boundary(
                    "SCC Core root is not a module".into(),
                ));
            };
            let mut found = None;
            for (variable, function) in definitions {
                if core.modules[module].named_mfa(*variable)
                    .map_err(|error| AnalyzeSccError::Generation(GenerationError::Boundary(error)))?
                    == *mfa
                {
                    found = Some((ModuleId(module), *variable, *function));
                    break;
                }
            }
            entries.push(found.ok_or_else(|| {
                AnalyzeSccError::Boundary(format!(
                    "SCC function {}:{}/{} absent",
                    mfa.module, mfa.function, mfa.arity
                ))
            })?);
        }
        if entries.is_empty() {
            return Err(AnalyzeSccError::Native(NativeTransferError::FunctionClause));
        }
        let self_recursive = match entries.as_slice() {
            [(module, _, function)] => {
                let label = self
                    .nodes
                    .labels()
                    .label(NodeRef {
                        module: *module,
                        node: function.0,
                    })
                    .ok_or_else(|| AnalyzeSccError::Boundary("SCC function label absent".into()))?;
                self.graph
                    .modules
                    .get(module.0)
                    .filter(|facts| facts.self_recursive.contains(&GraphVertex::Label(label)))
                    .map(|_| label)
            }
            _ => None,
        };
        let solver_atoms = solvers.source_atoms();
        let captures = self.phase.captures.map(|inputs| GenerationCaptureInputs {
            core_captures: inputs.core_captures,
            runtime: inputs.runtime,
            records: inputs.records,
            initial_record_cache: &[],
            solvers: &solver_atoms,
        });
        timings.mark("resolve_entries");
        let generated = Generator::new(
            self.nodes,
            self.profile,
            self.provider,
            GenerationPhase {
                captures: captures.as_ref(),
                callgraph: self.phase.callgraph,
                propagated: self.phase.propagated,
                library: self.phase.library,
                records: self.phase.records,
                self_recursive,
            },
        )
        .map_err(AnalyzeSccError::Generation)?
        .with_next_label(next_label)
        .with_dependency_retention(solvers.dependency_retention())
        .generate(&entries, output)
        .map_err(AnalyzeSccError::Generation)?;
        timings.mark("generate_constraints");
        let reached_functions = generated.entries.clone();
        let next_label = generated.next_label;
        let refold_allocations = generated.refold_allocations;
        let source_state = generated.source_state.clone();
        let parameters = generated.parameters.clone();
        let tree = generated.prepare().map_err(AnalyzeSccError::Generation)?;
        timings.mark("prepare_constraints");
        let bindings = if reached_functions.len() > 1 {
            tree.prepare_scc(reached_functions.clone())
                .map_err(AnalyzeSccError::Boundary)?
                .solve(BindingMap::new(), self.provider)
                .map_err(AnalyzeSccError::Solve)?
                .bindings
        } else {
            let result = tree
                .solve(BindingMap::new(), self.provider)
                .map_err(AnalyzeSccError::Solve)?;
            let AtomicOutcome::Solved { bindings, .. } = result.outcome else {
                return Err(AnalyzeSccError::Boundary(
                    "source function reference failed".into(),
                ));
            };
            let function = &reached_functions[0];
            let typ = bindings
                .get(&BindingKey::Label(function.variable().clone()))
                .cloned()
                .unwrap_or_else(Type::any);
            let mut ordinary = BindingMap::new();
            if let Some(variable) = &parameters[function].recursive_variable
                && typ != Type::any()
            {
                ordinary.insert(
                    BindingKey::Label(variable.clone()),
                    typ.limit(5).map_err(|error| {
                        AnalyzeSccError::Generation(GenerationError::Native(error))
                    })?,
                );
            }
            if typ != Type::any() {
                ordinary.insert(
                    BindingKey::Label(function.variable().clone()),
                    typ.limit(5).map_err(|error| {
                        AnalyzeSccError::Generation(GenerationError::Native(error))
                    })?,
                );
            }
            ordinary
        };
        timings.mark("solve_and_limit");
        Ok(SccAnalysis {
            bindings,
            reached_functions,
            next_label,
            refold_allocations,
            source_state,
        })
    }
}

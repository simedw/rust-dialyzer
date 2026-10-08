use rust_dialyzer_poc::{
    bifs::registry::{Mfa, Registry},
    call_transfer::Signature,
    closure_dependencies::{ClosureDependencies, DependencyPolicy},
    constraints::generation_capture_headers::GenerationRuntimeCaptures,
    contract_book::ContractBook,
    contract_runtime::ContractSummary,
    core::CoreProgram,
    core_analysis::{
        AnalysisPhases, AnalysisState, CoreAnalyzer, ModuleRequest, OrderedScc, PhaseSchedule,
        SccRequest,
    },
    dataflow_nodes::DataflowNodes,
    labels::{CoreLabel, Labels},
    lattice::Type,
    program::{ModuleId, Program},
    source_callgraph::{GraphVertex, SourceCallGraph},
    success_phase::FunctionTypes,
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    io::{self, BufRead},
    rc::Rc,
};
fn sigmap(types: &FunctionTypes) -> Value {
    json!(types.iter().map(|(label,sig)|(label.0.to_string(),json!({"arguments":sig.arguments.iter().map(Type::to_value).collect::<Vec<_>>(),"result":sig.result.to_value()}))).collect::<serde_json::Map<_,_>>())
}
struct Phases {
    events: Rc<RefCell<Vec<Value>>>,
    scc: Vec<Type>,
    modules: Vec<Type>,
    scc_index: usize,
    module_index: usize,
    name: Mfa,
    has_contract: bool,
    named_only: bool,
    fail_args: bool,
    named_label: CoreLabel,
    published: FunctionTypes,
    published_contracts: Vec<Mfa>,
}
impl Phases {
    fn verify_publication(&self, state: &AnalysisState) -> Result<(), String> {
        if sigmap(&self.published) != sigmap(&state.signatures) || self.published_contracts != state.contracts.active_names().cloned().collect::<Vec<_>>() {
            return Err("published lookup state differs from full analysis state".into());
        }
        Ok(())
    }
    fn active(&self, state: &AnalysisState) -> Value {
        if self.has_contract && state.contracts.active(&self.name).is_some() {
            json!([self.name])
        } else {
            json!([])
        }
    }
    fn output(previous: &FunctionTypes, ret: &Type, fail_args: bool) -> FunctionTypes {
        previous
            .iter()
            .map(|(label, sig)| {
                (
                    *label,
                    Signature {
                        arguments: vec![
                            if fail_args { Type::none() } else { Type::any() };
                            sig.arguments.len()
                        ],
                        result: ret.clone(),
                    },
                )
            })
            .collect()
    }
}
impl AnalysisPhases for Phases {
    type Error = String;
    fn publish<'a>(&mut self, signatures: impl Iterator<Item = (CoreLabel, &'a Signature)>, contracts: impl Iterator<Item = &'a Mfa>) {
        self.published.extend(signatures.map(|(label, signature)| (label, signature.clone())));
        self.published_contracts.extend(contracts.cloned());
        self.published_contracts.sort();
        self.published_contracts.dedup();
    }
    fn analyze_scc(&mut self, r: SccRequest<'_>) -> Result<FunctionTypes, String> {
        self.verify_publication(r.state)?;
        self.events.borrow_mut().push(json!({"kind":"scc","mfas":r.ordered_mfas,"next_label":r.next_core_label.0,"previous":sigmap(r.previous),"active":self.active(r.state)}));
        let ret = self
            .scc
            .get(self.scc_index)
            .or(self.scc.last())
            .ok_or("empty SCC phases")?;
        self.scc_index += 1;
        let mut out = Self::output(r.previous, ret, self.fail_args);
        if self.named_only {
            out.retain(|label, _| *label == self.named_label);
        }
        out.insert(
            CoreLabel(999999),
            Signature {
                arguments: vec![],
                result: Type::from_value(&json!({"kind":"integer"}))?,
            },
        );
        Ok(out)
    }
    fn refine_module(&mut self, r: ModuleRequest<'_>) -> Result<FunctionTypes, String> {
        self.verify_publication(r.state)?;
        self.events.borrow_mut().push(json!({"kind":"module","module":"fixture","previous":sigmap(r.previous),"active":self.active(r.state)}));
        let ret = self
            .modules
            .get(self.module_index)
            .or(self.modules.last())
            .cloned()
            .unwrap_or_else(Type::any);
        self.module_index += 1;
        Ok(Self::output(r.previous, &ret, self.fail_args))
    }
}
struct Schedule {
    events: Rc<RefCell<Vec<Value>>>,
    runtime: GenerationRuntimeCaptures,
    name: Mfa,
}
impl Schedule {
    fn work(&self) -> Vec<OrderedScc> {
        vec![OrderedScc {
            identity: 1,
            members: vec![GraphVertex::Named {
                module: self.name.module.clone(),
                function: self.name.function.clone(),
                arity: self.name.arity.into(),
            }],
        }]
    }
}
impl PhaseSchedule for Schedule {
    type Error = String;
    fn runtime(&self) -> &GenerationRuntimeCaptures {
        &self.runtime
    }
    fn finalize(&mut self, _: &SourceCallGraph) -> Result<Vec<OrderedScc>, String> {
        Ok(self.work())
    }
    fn modules_from_functions(
        &mut self,
        _: &SourceCallGraph,
        c: &[CoreLabel],
    ) -> Result<Vec<ModuleId>, String> {
        self.events
            .borrow_mut()
            .push(json!({"kind":"modules","changed":c.iter().map(|n|n.0).collect::<Vec<_>>()}));
        Ok(vec![ModuleId(0)])
    }
    fn reset_from_functions(
        &mut self,
        _: &SourceCallGraph,
        c: &[CoreLabel],
    ) -> Result<Vec<OrderedScc>, String> {
        self.events
            .borrow_mut()
            .push(json!({"kind":"reset","changed":c.iter().map(|n|n.0).collect::<Vec<_>>()}));
        Ok(self.work())
    }
}
fn run(r: &Value) -> Result<Value, String> {
    let core = CoreProgram::parse(&r["program"].to_string())?;
    let labels = Labels::build(&core, CoreLabel(0))?;
    let nodes = DataflowNodes::build(&core, &labels)?;
    let program = Program::build(&core)?;
    let bifs = Registry::load(include_str!("../../corpus/bifs.json"))?;
    let deps = ClosureDependencies::analyze(
        &core,
        &program,
        &DependencyPolicy::parse(include_str!("../../corpus/dependency_policy.json"))?,
    )?;
    let graph = SourceCallGraph::build(&nodes, &program, &deps, &bifs)
        .map_err(|e| e.to_string())?;
    let name = Mfa {
        module: "fixture".into(),
        function: "entry".into(),
        arity: 1,
    };
    let mut candidates = BTreeMap::new();
    let has_contract = !r["contract_return"].is_null();
    if has_contract {
        let ret = Type::from_value(&r["contract_return"])?;
        candidates.insert(
            name.clone(),
            ContractSummary::from_parts(vec![(
                Type::function(Some(vec![Type::any()]), ret),
                vec![],
            )])
            .map_err(|e| e.to_string())?,
        );
    }
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut phases = Phases {
        events: events.clone(),
        scc: r["scc_returns"]
            .as_array()
            .ok_or("scc phases")?
            .iter()
            .map(Type::from_value)
            .collect::<Result<_, _>>()?,
        modules: r["module_returns"]
            .as_array()
            .ok_or("module phases")?
            .iter()
            .map(Type::from_value)
            .collect::<Result<_, _>>()?,
        scc_index: 0,
        module_index: 0,
        name: name.clone(),
        has_contract,
        published: FunctionTypes::new(),
        published_contracts: Vec::new(),
        named_only: r["named_only"] == true,
        fail_args: r["fail_args"] == true,
        named_label: *graph
            .named_labels
            .get(&GraphVertex::Named {
                module: name.module.clone(),
                function: name.function.clone(),
                arity: name.arity.into(),
            })
            .ok_or("named graph label")?,
    };
    let mut schedule = Schedule {
        events: events.clone(),
        runtime: GenerationRuntimeCaptures::default(),
        name: name.clone(),
    };
    let analyzer = CoreAnalyzer {
        nodes: &nodes,
        program: &program,
        graph: &graph,
        bifs: &bifs,
        next_core_label: CoreLabel(r["next_label"].as_u64().ok_or("next label")?),
    };
    let state = analyzer
        .run(
            AnalysisState {
                signatures: FunctionTypes::new(),
                contracts: ContractBook::new(candidates),
            },
            &mut phases,
            &mut schedule,
        )
        .map_err(|e| format!("{e:?}"))?;
    phases.verify_publication(&state)?;
    let active = if has_contract && state.contracts.active(&name).is_some() {
        json!([name])
    } else {
        json!([])
    };
    let output = json!({"id":r["id"],"native":{"status":"ok","signatures":sigmap(&state.signatures),"active":active},"events":*events.borrow()});
    Ok(output)
}
fn main() {
    for line in io::stdin().lock().lines() {
        let out = line
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str::<Value>(&s).map_err(|e| e.to_string()))
            .and_then(|r| run(&r));
        println!("{}", out.unwrap_or_else(|e| json!({"error":e})));
    }
}

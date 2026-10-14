//! Whole-read equality and a coarse value-erased shape classifier.
//! Neither proves cache eligibility. Shape ignores values, closure captures,
//! constructor attempts and failure paths. Only sequential entries are covered.
use crate::{core::{NodeId, CoreProgram}, labels::CoreLabel, lattice::Type, program::ModuleId};
use std::{collections::BTreeMap, io::Write, sync::{Mutex, OnceLock}, time::Instant};

fn output() -> Option<&'static Mutex<std::io::BufWriter<std::fs::File>>> {
    static OUTPUT: OnceLock<Option<Mutex<std::io::BufWriter<std::fs::File>>>> = OnceLock::new();
    OUTPUT.get_or_init(|| {
        let path = std::env::var_os("RUST_DIALYZER_GENERATION_PROBE")?;
        Some(Mutex::new(std::io::BufWriter::with_capacity(1024 * 1024,
            std::fs::File::create(path).expect("create generation probe"))))
    }).as_ref()
}
pub(super) fn enabled() -> bool { output().is_some() }
pub(super) fn flush() {
    if let Some(out) = output() { let _ = out.lock().unwrap().flush(); }
}

pub(super) struct EntryProbe {
    started: Instant,
    identity: serde_json::Value,
    constraints: usize,
    pub library_queries: usize,
    pub reads: BTreeMap<CoreLabel, Option<Type>>,
}
impl EntryProbe {
    pub fn new(core: &CoreProgram, entries: &[(ModuleId, NodeId, NodeId)],
        entry: &(ModuleId, NodeId, NodeId), fresh: u64, synthetic: &crate::dataflow_key::SyntheticLabels,
        constraints: usize) -> Self {
        Self { started: Instant::now(), constraints, library_queries: 0, reads: BTreeMap::new(), identity: serde_json::json!({
            "program": std::ptr::from_ref(core) as usize,
            "scc": entries.iter().map(|(m,v,f)| (m.0,v.0,f.0)).collect::<Vec<_>>(),
            "entry": (entry.0.0,entry.1.0,entry.2.0), "fresh": fresh,
            "synthetic": [synthetic.profile().first(), synthetic.profile().stride(), synthetic.allocations()],
        }) }
    }
    pub fn finish(self, arena: &crate::constraints::normalize::NativeConstraintArena, succeeded: bool) {
        let elapsed_ms = self.started.elapsed().as_secs_f64() * 1000.;
        let reads: Vec<_> = self.reads.iter().map(|(label, value)| {
            (label.0, value.map(|typ| std::ptr::from_ref(&*typ) as usize))
        }).collect();
        let domains: Vec<_> = self.reads.iter().map(|(label, value)| {
            let domain = value.as_ref().and_then(|typ| match typ.view() {
                crate::lattice::TypeView::Families(slots) => slots.functions.and_then(|fun| fun.domain),
                _ => None,
            });
            (label.0, domain.map(|values| values.iter().map(|typ|
                std::ptr::from_ref(&**typ) as usize).collect::<Vec<_>>()))
        }).collect();
        let record = serde_json::json!({"identity":self.identity,"reads":reads,
            "domains":domains,"shape":arena.probe_shape(self.constraints),
            "prefix_constraints":self.constraints,"library_queries":self.library_queries,
            "shape_classifier_only":true,
            "elapsed_ms":elapsed_ms,"constraints":arena.len()-self.constraints,"succeeded":succeeded,
            "pass":crate::job_trace::pass(),"bound_only":true});
        if let Some(out) = output() { let _ = writeln!(out.lock().unwrap(), "{record}"); }
    }
}

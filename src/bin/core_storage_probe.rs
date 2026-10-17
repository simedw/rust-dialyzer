//! Retained Core storage census. This diagnostic is excluded from speed benchmarks.
use rust_dialyzer_poc::{
    beam_frontend::pipeline::{ContextSource, Emission, load_emission},
    core::{CoreExpr, CoreNode, ErlangTerm, NodeId, TermId},
};
use serde_json::{Value, json};
use std::{collections::HashSet, mem::size_of};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

unsafe extern "C" {
    fn mi_usable_size(pointer: *const std::ffi::c_void) -> usize;
}
fn usable_bytes<T>(items: &[T], capacity: usize) -> usize {
    if capacity == 0 || size_of::<T>() == 0 {
        return 0;
    }
    // Callers pass a whole Vec backing block or a slice of one whole boxed
    // payload. This binary installs mimalloc globally, so the pointer is a
    // live mimalloc allocation base, not an interior pointer.
    unsafe { mi_usable_size(items.as_ptr().cast()) }
}

#[derive(Default)]
struct Text<'a> {
    count: usize,
    len: usize,
    capacity: usize,
    unique: HashSet<&'a str>,
}
impl<'a> Text<'a> {
    fn add(&mut self, text: &'a String) {
        self.count += 1;
        self.len += text.len();
        self.capacity += text.capacity();
        self.unique.insert(text);
    }
    fn summary(&self) -> Value {
        json!({"count":self.count,"len_bytes":self.len,"capacity_bytes":self.capacity,
            "unique_count":self.unique.len(),"unique_len_bytes":self.unique.iter().map(|s|s.len()).sum::<usize>()})
    }
}
fn node_children_bytes(expr: &CoreExpr) -> usize {
    let ids = |v: &Vec<NodeId>| v.capacity() * size_of::<NodeId>();
    let pairs = |v: &Vec<(NodeId, NodeId)>| v.capacity() * size_of::<(NodeId, NodeId)>();
    match expr {
        CoreExpr::Values(v) | CoreExpr::Binary(v) | CoreExpr::Tuple(v) => ids(v),
        CoreExpr::Map { pairs: v, .. } => ids(v),
        #[cfg(otp_domain = "29")]
        CoreExpr::Record { pairs: v, .. } => ids(v),
        #[cfg(otp_domain = "29")]
        CoreExpr::RecordPair { .. } => 0,
        CoreExpr::Let { vars, .. } => ids(vars),
        CoreExpr::Apply { args, .. }
        | CoreExpr::Call { args, .. }
        | CoreExpr::Primop { args, .. } => ids(args),
        CoreExpr::Case { clauses, .. } | CoreExpr::Receive { clauses, .. } => ids(clauses),
        CoreExpr::Clause { patterns, .. } => ids(patterns),
        CoreExpr::Fun { params, .. } => ids(params),
        CoreExpr::Try(try_payload) => { let rust_dialyzer_poc::core::TryExpr {
            vars,
            exception_vars,
            ..
        } = try_payload.as_ref(); ids(vars) + ids(exception_vars) },
        CoreExpr::Letrec { definitions, .. } => pairs(definitions),
        CoreExpr::Module(module_payload) => { let rust_dialyzer_poc::core::ModuleExpr {
            exports,
            attributes,
            definitions,
            ..
        } = module_payload.as_ref(); ids(exports) + pairs(attributes) + pairs(definitions) },
        CoreExpr::Literal(_)
        | CoreExpr::Var(_)
        | CoreExpr::Opaque(_)
        | CoreExpr::Bitstr { .. }
        | CoreExpr::Cons { .. }
        | CoreExpr::MapPair { .. }
        | CoreExpr::Seq { .. }
        | CoreExpr::Alias { .. }
        | CoreExpr::Catch { .. } => 0,
    }
}
fn run() -> Result<(), String> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: core_storage_probe MANIFEST.json")?;
    let manifest: Value = serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let paths = manifest["beam_files"]
        .as_array()
        .ok_or("beam_files missing")?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or("BEAM path must be string")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let emission = match manifest["core_emission"].as_str() {
        Some("reference") => Emission::Reference,
        Some("direct") => Emission::Direct,
        None | Some("fused") => Emission::Fused,
        _ => return Err("unknown core_emission".into()),
    };
    let loaded = match manifest["runtime_context"].as_str() {
        Some("imported") => load_emission(
            &paths,
            ContextSource::Imported {
                context: &manifest["context"],
                closure_profile: &manifest["closure_profile"],
            },
            emission,
        )?,
        None | Some("native") => {
            let read = |key: &str| {
                std::fs::read(
                    manifest[key]
                        .as_str()
                        .ok_or_else(|| format!("{key} missing"))?,
                )
                .map_err(|e| e.to_string())
            };
            let typesig = read("dialyzer_typesig_beam")?;
            let contracts = read("dialyzer_contracts_beam")?;
            load_emission(
                &paths,
                ContextSource::Native {
                    typesig_beam: &typesig,
                    contracts_beam: &contracts,
                },
                emission,
            )?
        }
        _ => return Err("unknown runtime_context".into()),
    };
    let mut atoms = Text::default();
    let mut integers = Text::default();
    let mut external = Text::default();
    let (mut terms, mut nodes, mut term_children, mut node_children, mut binary_bytes) =
        (0, 0, 0, 0, 0);
    let (mut term_capacity, mut node_capacity) = (0, 0);
    let (mut term_usable, mut node_usable) = (0, 0);
    let (mut term_payload_usable, mut node_payload_usable) = (0, 0);
    for module in &loaded.core.modules {
        terms += module.terms.len();
        nodes += module.nodes.len();
        for (items, capacity) in module.terms.backing_blocks() {
            term_capacity += capacity;
            term_usable += usable_bytes(items, capacity);
        }
        for (items, capacity) in module.nodes.backing_blocks() {
            node_capacity += capacity;
            node_usable += usable_bytes(items, capacity);
        }
        for term in module.terms.iter() {
            match term {
                ErlangTerm::Atom(s) => atoms.add(s),
                ErlangTerm::Integer(s) => integers.add(s),
                ErlangTerm::ExternalFun(external_fun_payload) => {
                    term_payload_usable += usable_bytes(std::slice::from_ref(external_fun_payload.as_ref()), 1);
                    let rust_dialyzer_poc::core::ExternalFun {
                        module, function, ..
                    } = external_fun_payload.as_ref();

                    external.add(module);
                    external.add(function);
                }
                ErlangTerm::Tuple(v) | ErlangTerm::List { items: v, .. } => {
                    term_children += v.capacity() * size_of::<TermId>()
                }
                ErlangTerm::Map(v) => term_children += v.capacity() * size_of::<(TermId, TermId)>(),
                ErlangTerm::Binary(v) | ErlangTerm::Bitstring { bytes: v, .. } => {
                    binary_bytes += v.capacity()
                }
                ErlangTerm::Nil | ErlangTerm::Float(_) => {}
            }
        }
        for node in module.nodes.iter() {
            node_children += node_children_bytes(&node.expr);
            match &node.expr {
                CoreExpr::Try(payload) => node_payload_usable += usable_bytes(std::slice::from_ref(payload.as_ref()), 1),
                CoreExpr::Module(payload) => node_payload_usable += usable_bytes(std::slice::from_ref(payload.as_ref()), 1),
                _ => {},
            }
        }
    }
    println!(
        "{}",
        json!({"modules":loaded.core.modules.len(),"frontend":loaded.timings,
        "terms":terms,"nodes":nodes,"node_kinds":loaded.core.kinds,"term_record_size":size_of::<ErlangTerm>(),"node_record_size":size_of::<CoreNode>(),
        "term_records_len_bytes":terms * size_of::<ErlangTerm>(),"node_records_len_bytes":nodes * size_of::<CoreNode>(),
        "term_record_capacity_bytes":term_capacity * size_of::<ErlangTerm>(),"node_record_capacity_bytes":node_capacity * size_of::<CoreNode>(),
        "term_capacity":term_capacity,"node_capacity":node_capacity,
        "term_record_usable_bytes":term_usable,"node_record_usable_bytes":node_usable,
        "term_payload_usable_bytes":term_payload_usable,"node_payload_usable_bytes":node_payload_usable,
        "term_children_capacity_bytes":term_children,"node_children_capacity_bytes":node_children,"binary_capacity_bytes":binary_bytes,
        "atoms":atoms.summary(),"integers":integers.summary(),"external_fun_text":external.summary(),
        "limits":"Counts final Core modules including backing record capacity, excludes source arenas, inference and allocator overhead. Shared backing blocks must not be summed across cloned views. Unique text sets are probe overhead; unique bytes exclude interner metadata."})
    );
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

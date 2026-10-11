//! Separate the runtime identity gate from the native lowering gate.
use rust_dialyzer_poc::{
    beam_frontend::{
        debug, forms,
        pipeline::{assemble, ContextSource},
        Compilation,
    },
    external_term::{decode_interned, Limits},
};
use serde_json::{json, Value};
use std::fs;
#[test]
#[ignore = "requires complete independently captured compiler Core and analyzer inputs"]
fn complete_native_runtime_scope_analysis() {
    std::thread::Builder::new().stack_size(4 << 30).spawn(|| {
        let plan: Value = serde_json::from_slice(&fs::read(std::env::var("BEAM_CORE_SCOPE_PLAN").unwrap()).unwrap()).unwrap();
        let compiled = rust_dialyzer_poc::ordered_parallel::setup_map(plan["modules"].as_array().unwrap(), |row| {
            let bytes=fs::read(row["beam"].as_str().unwrap()).unwrap();
            let debug=debug::read_beam(&bytes,Limits::default()).unwrap();
            let cleaned=fs::read(row["cleaned_core"].as_str().unwrap()).unwrap();
            Compilation { module: debug.module.clone(), source_forms: forms(&debug).unwrap(), cleaned_core:decode_interned(&cleaned,Limits {bytes:256*1024*1024,nodes:100_000_000,expanded:100_000_000}).unwrap() }
        });
        let typesig=fs::read(plan["dialyzer_typesig_beam"].as_str().unwrap()).unwrap();
        let contracts=fs::read(plan["dialyzer_contracts_beam"].as_str().unwrap()).unwrap();
        let loaded=assemble(compiled,ContextSource::Native { typesig_beam:&typesig,contracts_beam:&contracts }).unwrap();
        let options:Value=serde_json::from_slice(&fs::read(plan["options"].as_str().unwrap()).unwrap()).unwrap();
        let library=fs::read_to_string(plan["prior_library"].as_str().unwrap()).unwrap();
        let mut result=rust_dialyzer_poc::compiled_probe::analyze(&loaded.core,&options,&library,&loaded.closure_profile).unwrap();
        let mut output=json!({"status":"ok","modules":loaded.core.modules.len(),"identity_origin":"deterministic_native_scope","core_origin":"independent_compiler_oracle"});
        for key in ["public_warning_count","warnings","signatures"] { output[key]=result[key].take(); }
        serde_json::to_writer(fs::File::create(plan["output"].as_str().unwrap()).unwrap(),&output).unwrap();
    }).unwrap().join().unwrap();
}

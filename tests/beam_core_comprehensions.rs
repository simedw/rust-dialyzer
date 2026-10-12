//! Complete modules from the independent OTP 28.3 compiler oracle.
use rust_dialyzer_poc::{beam_frontend,external_term};
use std::path::PathBuf;
#[path="support/beam_core_oracle.rs"] mod core_oracle;
fn oracle(name:&str){
 let dir=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/beam_core_port/comprehensions");
 let limits=external_term::Limits::default();
 let beam=std::fs::read(dir.join(format!("{name}.beam"))).unwrap();
 let module=beam_frontend::debug::read_beam(&beam,limits).unwrap();
 let forms=beam_frontend::forms(&module).unwrap();
 let native=beam_frontend::raw_core(&module,&forms).unwrap();
 let expected=external_term::decode(&std::fs::read(dir.join(format!("{name}.raw_core.etf"))).unwrap(),limits).unwrap();
 core_oracle::assert_exact(&native,&expected,&format!("{name} full raw Core oracle"));
}

#[test]
fn native_comprehension_matches_complete_otp_module(){oracle("native_comprehension");}

#[test]
fn native_comprehension_edges_matches_complete_otp_module(){oracle("native_comprehension_edges");}

#[test]
fn native_comprehension_more_matches_complete_otp_module(){oracle("native_comprehension_more");}

#[test]
fn native_comprehension_scope_matches_complete_otp_module(){oracle("native_comprehension_scope");}

#[test]
fn native_zip_patterns_matches_complete_otp_module(){oracle("native_zip_patterns");}

#[test]
fn native_generator_rhs_matches_complete_otp_module(){oracle("native_generator_rhs");}

#[test]
fn native_strict_wildcards_matches_complete_otp_module(){oracle("native_strict_wildcards");}

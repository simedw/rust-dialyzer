use rust_dialyzer_poc::{beam_frontend::{self, debug}, core::{ErlangTerm, ErlangTerms, TermId}, external_term::{decode,encode,Limits}};
use std::{fs,path::PathBuf};
fn fixture(name:&str,suffix:&str)->Vec<u8> {fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/beam_core_port/aliases").join(format!("{name}.{suffix}"))).unwrap()}
fn difference(a:&ErlangTerms,x:TermId,b:&ErlangTerms,y:TermId,path:&str)->Option<String> {
    match (&a.terms[x.0],&b.terms[y.0]) {
        (ErlangTerm::Tuple(xs),ErlangTerm::Tuple(ys)) if xs.len()==ys.len()=>xs.iter().zip(ys).enumerate().find_map(|(i,(&x,&y))|difference(a,x,b,y,&format!("{path}/tuple[{i}]"))),
        (ErlangTerm::List{items:xs,tail:xt},ErlangTerm::List{items:ys,tail:yt}) if xs.len()==ys.len()=>xs.iter().zip(ys).enumerate().find_map(|(i,(&x,&y))|difference(a,x,b,y,&format!("{path}/list[{i}]"))).or_else(||difference(a,*xt,b,*yt,&format!("{path}/tail"))),
        (x,y) if x==y=>None,
        (x,y)=>Some(format!("{path}: actual {x:?}, expected {y:?}")),
    }
}
fn check(name:&str) {
    let module=debug::read_beam(&fixture(name,"beam"),Limits::default()).unwrap();
    let forms=beam_frontend::forms(&module).unwrap();
    let raw=beam_frontend::raw_core(&module,&forms).unwrap();
    let cleaned=beam_frontend::clean::clean(&raw).unwrap();
    for (stage,actual) in [("raw_core",raw),("clean_core",cleaned)] {
        let expected=decode(&fixture(name,&format!("{stage}.etf")),Limits::default()).unwrap();
        if encode(&actual,Limits::default(),false).unwrap()!=encode(&expected,Limits::default(),false).unwrap() {
            panic!("{name} {stage}: {}",difference(&actual,actual.root,&expected,expected.root,"root").unwrap_or_else(||"encoding difference".into()));
        }
    }
}
#[test] fn core_lower_alias_patterns() {check("native_aliases");}

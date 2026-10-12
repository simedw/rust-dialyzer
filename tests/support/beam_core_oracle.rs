use rust_dialyzer_poc::{core::{ErlangTerm,ErlangTerms,TermId},external_term};
fn difference(a:&ErlangTerms,x:TermId,b:&ErlangTerms,y:TermId,path:&str)->Option<String>{
 match (&a.terms[x.0],&b.terms[y.0]) {
  (ErlangTerm::Tuple(xs),ErlangTerm::Tuple(ys)) if xs.len()==ys.len()=>xs.iter().zip(ys).enumerate().find_map(|(i,(&x,&y))|difference(a,x,b,y,&format!("{path}/tuple[{i}]"))),
  (ErlangTerm::List{items:xs,tail:xt},ErlangTerm::List{items:ys,tail:yt}) if xs.len()==ys.len()=>xs.iter().zip(ys).enumerate().find_map(|(i,(&x,&y))|difference(a,x,b,y,&format!("{path}/list[{i}]"))).or_else(||difference(a,*xt,b,*yt,&format!("{path}/tail"))),
  (x,y) if x==y=>None,
  (x,y)=>Some(format!("{path}: actual {x:?}, expected {y:?}")),
 }
}
pub fn assert_exact(actual:&ErlangTerms,expected:&ErlangTerms,label:&str){
 let limits=external_term::Limits::default();
 if external_term::encode(actual,limits,false).unwrap()!=external_term::encode(expected,limits,false).unwrap(){let detail=difference(actual,actual.root,expected,expected.root,"root").unwrap_or_else(||"encoding difference".into());let detail:String=detail.chars().take(1200).collect();panic!("{label}: {detail}");}
}

from pathlib import Path
import json
ROOT=Path(__file__).resolve().parents[2]
source=json.loads((ROOT/'.cache/warning_render/source-functions.json').read_text())
terms=[]
for term in source['terms']:
 k=term['kind']
 if k=='atom':v=term['value']
 elif k=='integer':v=int(term['value'])
 elif k=='nil':v=[]
 elif k=='tuple':v=tuple(terms[x] for x in term['items'])
 elif k=='list':v=[terms[x] for x in term['items']]+terms[term['tail']]
 else:raise ValueError(k)
 terms.append(v)
funcs=terms[source['root']]
FN={'a':'Arguments','c':'Code','call_or_apply_to_string':'CallReason','con':'Contract','erl_types:t_is_opaque':'IsOpaque','erl_types:t_to_string':'TypeString','field_diffs':'FieldDiffs','form_expected':'Expected','form_expected_without_opaque':'ExpectedWithoutOpaque','form_position_string':'Positions','form_positions':'OpaquePositions','format_invalid_contract_details':'InvalidDetails','io_lib:format':'Format','is_integer':'IsInteger','lists:flatten':'Flatten','lists:reverse':'Reverse','min':'Min','ordinal':'Ordinal','pos':'Position','ps':'PatternString','rec_type':'RecordString','sig':'Signature','string:split':'Split','t':'TypeLayout','message_to_string':'Message'}
def quote(s):return json.dumps(s,ensure_ascii=False)
def vec(values):return 'vec!['+','.join(values)+']'
def pattern(p):
 tag=p[0]
 if tag=='var':return 'Pattern::Var('+quote(p[2])+')'
 if tag=='atom':return 'Pattern::Atom('+quote(p[2])+')'
 if tag=='integer':return 'Pattern::Integer('+quote(str(p[2]))+')'
 if tag=='nil':return 'Pattern::Nil'
 if tag=='cons':return 'Pattern::Cons(Box::new('+pattern(p[2])+'),Box::new('+pattern(p[3])+'))'
 if tag=='tuple':return 'Pattern::Tuple('+vec([pattern(x) for x in p[2]])+')'
 if tag=='string':return 'Pattern::Text('+quote(''.join(map(chr,p[2])))+')'
 if tag=='match':return 'Pattern::All('+vec([pattern(p[2]),pattern(p[3])])+')'
 if tag=='op' and p[2]=='++':return 'Pattern::Prefix(Box::new('+pattern(p[3])+'),Box::new('+pattern(p[4])+'))'
 raise ValueError(('pattern',p))
def clause(c):
 _,_,patterns,guards,body=c
 return 'Clause {patterns:'+vec([pattern(x) for x in patterns])+',guards:'+vec([vec([expr(x) for x in g]) for g in guards])+',body:'+vec([expr(x) for x in body])+'}'
def expr(e):
 tag=e[0]
 if tag=='var':return 'Expr::Var('+quote(e[2])+')'
 if tag=='atom':return 'Expr::Atom('+quote(e[2])+')'
 if tag=='integer':return 'Expr::Integer('+quote(str(e[2]))+')'
 if tag=='string':return 'Expr::Text('+quote(''.join(map(chr,e[2])))+')'
 if tag=='nil':return 'Expr::Nil'
 if tag=='cons':return 'Expr::Cons(Box::new('+expr(e[2])+'),Box::new('+expr(e[3])+'))'
 if tag=='tuple':return 'Expr::Tuple('+vec([expr(x) for x in e[2]])+')'
 if tag=='match':return 'Expr::Match('+pattern(e[2])+',Box::new('+expr(e[3])+'))'
 if tag=='call':
  target=e[2]
  if target[0]=='atom':name=target[2]
  elif target[0]=='remote':name=target[2][2]+':'+target[3][2]
  else:raise ValueError(target)
  return 'Expr::Call(Function::'+FN[name]+','+vec([expr(x) for x in e[3]])+')'
 if tag=='op':return 'Expr::Op(Operator::'+{'++':'Append','=:=':'Equal','orelse':'Or','rem':'Remainder'}[e[2]]+','+vec([expr(x) for x in e[3:]])+')'
 if tag=='case':return 'Expr::Case(Box::new('+expr(e[2])+'),'+vec([clause(x) for x in e[3]])+')'
 if tag=='if':return 'Expr::If('+vec([clause(x) for x in e[2]])+')'
 if tag=='lc':
  quals=[]
  for q in e[3]:
   if q[0]=='generate':quals.append('Qualifier::Generate('+pattern(q[2])+','+expr(q[3])+')')
   else:quals.append('Qualifier::Filter('+expr(q)+')')
  return 'Expr::Comprehension(Box::new('+expr(e[2])+'),'+vec(quals)+')'
 raise ValueError(('expr',e))
lines=['use super::{Clause,Expr,Pattern,Qualifier,Function,Operator};','pub(super) fn source() -> Vec<(Function, usize, Vec<Clause>)> { vec![']
for f in funcs:
 _,_,name,arity,clauses=f
 lines.append('(Function::'+FN[name]+','+str(arity)+','+vec([clause(c) for c in clauses])+'),')
lines.append('] }')
(ROOT/'src/render_source.rs').write_text('\n'.join(lines)+'\n')
print('generated',len(funcs),'functions',sum(len(f[4]) for f in funcs),'clauses')

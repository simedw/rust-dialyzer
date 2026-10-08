from pathlib import Path
import hashlib,itertools,json,subprocess
ROOT=Path(__file__).resolve().parent.parent
D=ROOT/'.cache/warning_emission';D.mkdir(parents=True,exist_ok=True)
proof_files=[ROOT/'src/emission.rs',ROOT/'src/bin/emission_probe.rs',ROOT/'src/warning_policy.rs',ROOT/'src/lattice.rs',ROOT/'src/type_api.rs',ROOT/'src/native_order.rs',ROOT/'src/runtime_term.rs',ROOT/'src/presentation.rs',ROOT/'src/presentation_form.rs',ROOT/'src/core.rs',ROOT/'Cargo.toml',ROOT/'Cargo.lock',ROOT/'scripts/native_types.exs',ROOT/'scripts/core_export.exs',ROOT/'scripts/warning_emission_oracle.exs',Path(__file__).resolve()]
def hashes():return {str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in proof_files}
implementation=hashes()
subprocess.run(['cargo','build','--release','--offline','--bin','emission_probe'],cwd=ROOT,check=True)
def c(name,*args):return {'constructor':name,'arguments':list(args)}
def seq(values):return {'list':list(values)}
ANY,NONE,UNIT,INT=c('t_any'),c('t_none'),c('t_unit'),c('t_integer')
def fun(result):return c('t_fun',seq([]),result)
def nominal(result,opacity):return c('t_nominal',{'tuple':[{'atom':'owner'},{'atom':'x'},{'integer':'0'},{'atom':opacity}]},result)
contracts=[None,ANY,NONE,UNIT,INT,fun(ANY),fun(NONE),fun(UNIT),fun(INT),nominal(fun(UNIT),'opaque'),nominal(UNIT,'opaque')]
rows=[]
def emit(**row):row['id']=str(len(rows));rows.append(row)
for named,handled,reachable,escaping,parent_named,count,result,kind,own,parent in itertools.product([False,True],[False,True],[False,True],[False,True],[False,True],[0,1,2],[NONE,UNIT,ANY,INT],['no_match','only_explicit','only_normal','both'],contracts[:5],contracts):
 if not named and own is not None:continue
 emit(named=named,handled=handled,reachable=reachable,escaping=escaping,parent_named=parent_named,incoming_count=count,result=result,return_kind=kind,own_contract=own,parent_contract=parent,warning_mode=True)
for mode,forced,ann in itertools.product([False,True],[False,True],[[],[0],[-1],[1],[9],[['compiler_generated']],[1,'compiler_generated'],[[0,4]],[[2,0]],['unknown',3]]):
 if ann==[['compiler_generated']]:ann=['compiler_generated']
 emit(operation='add',named=True,handled=True,reachable=True,result=NONE,warning_mode=mode,forced=forced,annotations=ann)
compare_types=[ANY,NONE,UNIT,INT,c('t_float'),c('t_number'),c('t_atom'),c('t_atom',{'atom':'ok'}),c('t_binary'),c('t_tuple'),c('t_list',INT),nominal(INT,'opaque'),nominal(INT,'transparent'),nominal(c('t_atom',{'atom':'ok'}),'opaque')]
for left,right,op,module in itertools.product(compare_types,compare_types,['=:=','=/=','==','/='],['erlang','other']):
 emit(operation='compare',named=True,handled=True,reachable=True,result=left,right=right,operator=op,module=module,warning_mode=True)
tokens=[list(values) for length in range(4) for values in itertools.product(['a','b'],repeat=length)]
for old,new in itertools.product(tokens,tokens):emit(operation='collection',old=old,new=new,result=NONE,warning_mode=False)
for named,meta,tag,suppressed,ft,fs in itertools.product([False,True],[False,True],[False,True],[False,True],['inherit','enabled','disabled'],['inherit','enabled','disabled']):
 emit(operation='policy',named=named,module_metadata=meta,module_tag=tag,module_suppressed=suppressed,function_tag=ft,function_suppression=fs,result=NONE,warning_mode=False)
request=D/'warnings.requests.jsonl';request.write_text(''.join(json.dumps(r)+'\n' for r in rows))
with request.open() as i,(D/'warnings.native.jsonl').open('w') as o:subprocess.run(['elixir','scripts/warning_emission_oracle.exs'],stdin=i,stdout=o,cwd=ROOT,check=True)
with (D/'warnings.native.jsonl').open() as i,(D/'warnings.results.jsonl').open('w') as o:subprocess.run([ROOT/'target/release/emission_probe'],stdin=i,stdout=o,check=True)
results=[json.loads(r) for r in (D/'warnings.results.jsonl').read_text().splitlines()]
if len(rows)!=52665 or len(results)!=len(rows):raise RuntimeError('warning emission denominator changed')
if [r['id'] for r in results]!=[r['id'] for r in rows]:raise RuntimeError('warning emission identities changed')
summary={'requests':len(rows),'matched':sum(r['matched'] for r in results),'failed':sum(not r['matched'] for r in results)}
(D/'warnings.summary.json').write_text(json.dumps(summary)+'\n');print(summary)
if summary['failed']:raise SystemExit(1)

if hashes()!=implementation:raise RuntimeError('warning emission implementation changed during verification')
summary.update(complete_emission_rules=True,complete_core_warning_collection=False,implementation_sha256=implementation,inputs_sha256={str(request.relative_to(ROOT)):hashlib.sha256(request.read_bytes()).hexdigest()})
(D/'report.json').write_text(json.dumps(summary,indent=2)+'\n')

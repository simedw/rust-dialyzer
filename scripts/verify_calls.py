from pathlib import Path
import hashlib,json,subprocess,itertools
ROOT=Path(__file__).resolve().parent.parent
D=ROOT/'.cache/calls';D.mkdir(parents=True,exist_ok=True)
proof_files=[ROOT/'src/call_transfer.rs',ROOT/'src/bin/call_probe.rs',ROOT/'src/core.rs',ROOT/'src/contract_runtime.rs',ROOT/'src/lattice.rs',ROOT/'src/type_api.rs',ROOT/'src/native_order.rs',ROOT/'src/runtime_term.rs',ROOT/'src/presentation.rs',ROOT/'src/presentation_form.rs',ROOT/'src/native_error.rs',*sorted((ROOT/'src/bifs').glob('*.rs')),ROOT/'Cargo.toml',ROOT/'Cargo.lock',ROOT/'corpus/bifs.json',ROOT/'scripts/native_types.exs',ROOT/'scripts/core_export.exs',ROOT/'scripts/call_oracle.exs',Path(__file__).resolve()]
def hashes():return {str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in proof_files}
implementation=hashes()
subprocess.run(['cargo','build','--release','--offline','--bin','call_probe'],cwd=ROOT,check=True)
def c(name,*args):return {'constructor':name,'arguments':list(args)}
def a(name):return {'atom':name}
def i(value):return {'integer':str(value)}
def seq(values):return {'list':list(values)}
ANY,NONE,UNIT=c('t_any'),c('t_none'),c('t_unit')
INT,ATOM,BIN=c('t_integer'),c('t_atom'),c('t_binary')
NIL=c('t_nil');OK=c('t_atom',a('ok'));BAD=c('t_atom',a('bad'));ONE=c('t_integer',i(1));TWO=c('t_integer',i(2))
def fun(args,ret):return c('t_fun',seq(args),ret)
def nominal(value,opacity):return c('t_nominal',{'tuple':[a('owner'),a('value'),i(0),a(opacity)]},value)
pool=[ANY,NONE,UNIT,INT,c('t_float'),c('t_number'),ONE,TWO,c('t_integer',i(-1)),ATOM,OK,BAD,c('t_boolean'),BIN,c('t_bitstr',i(3),i(2)),NIL,c('t_list',INT),c('t_nonempty_list',ATOM),c('t_tuple'),c('t_tuple',seq([])),c('t_tuple',seq([OK,INT])),c('t_map'),c('t_map',seq([]),NONE,NONE),c('t_fun'),fun([INT],ATOM),c('t_pid'),c('t_var',a('A'))]
for opacity in ['transparent','opaque']:
 for value in [INT,OK,BIN,c('t_list',INT),c('t_tuple',seq([OK,INT])),c('t_map'),fun([INT],ATOM)]:pool.append(nominal(value,opacity))
entries=json.load(open(ROOT/'corpus/bifs.json'))['entries'];rows=[]
def emit(**row):row['id']=str(len(rows));rows.append(row)
for entry in entries:
 m=entry['mfa'];mfa=[m['module'],m['function'],m['arity']]
 emit(actual=[ANY]*m['arity'],mfa=mfa)
 for position in range(m['arity']):
  for value in pool:
   actual=[ANY]*m['arity'];actual[position]=value;emit(actual=actual,mfa=mfa)
# Strict generic source fallback remains separate from runtime BIF membership.
runtime=json.load(open(ROOT/'corpus/bifs.json'))['runtime_builtin_inventory']['mfas']
for entry in runtime:
 m=entry.get('mfa',entry);mfa=[m['module'],m['function'],m['arity']]
 if any(x['mfa']==m for x in entries):continue
 for value in [ANY,NONE,UNIT]:emit(actual=[value]*m['arity'],mfa=mfa)
# Scalar conjunctions exercise independently failed ranges and Unit/local-return handling.
for actual,sigarg,sigrange,local in itertools.product(pool,[ANY,INT,OK],[ANY,NONE,UNIT,INT,OK],[ANY,NONE,UNIT]):
 emit(actual=[actual],success={'arguments':[sigarg],'result':sigrange},local_return=local)
VAR=c('t_var',a('A'))
contracts=[{'alternatives':[{'signature':fun([INT],INT),'constraints':[]}]},
 {'alternatives':[{'signature':fun([ANY],OK),'constraints':[]}]},
 {'alternatives':[{'signature':fun([INT],OK),'constraints':[]},{'signature':fun([OK],INT),'constraints':[]}]},
 {'alternatives':[{'signature':fun([VAR],VAR),'constraints':[{'left':VAR,'right':INT}]}]},
 {'alternatives':[{'signature':fun([ANY],UNIT),'constraints':[]}]},
 {'alternatives':[{'signature':fun([ANY],NONE),'constraints':[]}]},
 {'alternatives':[{'signature':fun([nominal(INT,'opaque')],nominal(OK,'opaque')),'constraints':[]}]}]
for contract,actual,sigrange,sigarg,local in itertools.product(contracts,pool,[ANY,INT,OK,UNIT],[ANY,INT],[ANY,NONE]):
 emit(actual=[actual],success={'arguments':[sigarg],'result':sigrange},contract=contract,local_return=local)
# Conjunction with real dependent built-ins and contracts together.
for mfa,contract,actual in itertools.product([['erlang','byte_size',1],['erlang','tuple_size',1],['erlang','hd',1],['erlang','is_integer',1]],contracts,pool):
 emit(actual=[actual],mfa=mfa,contract=contract,success={'arguments':[ANY],'result':ATOM})
# Mixed opacity reasons must pick expected_opaque even when transparent conflict arrives first.
for given,expected,module in itertools.product([INT,nominal(INT,'opaque')],[INT,nominal(INT,'opaque')],['owner','caller']):
 emit(actual=[given,expected],success={'arguments':[expected,given],'result':ANY},module=module)
# The native failing-call branch has a broader is_record range that raises; success must leave it lazy.
for result in [ANY,c('t_atom',a('false'))]:
 emit(actual=[c('t_sup',seq([c('t_tuple',seq([])),c('t_tuple',seq([OK,INT]))])),ATOM],mfa=['erlang','is_record',2],success={'arguments':[c('t_tuple',seq([OK,INT])),OK],'result':result})
# Actual native warning consensus and return/argument accumulation across candidate sequences.
alternatives=[{'success':{'arguments':[INT],'result':INT},'warnings':['integer']},
 {'success':{'arguments':[ATOM],'result':ATOM},'warnings':['atom']},
 {'success':{'arguments':[INT],'result':INT},'warnings':[]},
 {'success':{'arguments':[ANY],'result':UNIT},'local_return':NONE,'warnings':['unit']},
 {'success':{'arguments':[OK],'result':ANY},'warnings':['tag']},
 {'external':True}]
for length in range(4):
 for candidates in itertools.product(alternatives,repeat=length):
  for actual in [ANY,OK,ONE]:emit(operation='accumulate',actual=[actual],candidates=list(candidates))
request=D/'calls.requests.jsonl';request.write_text(''.join(json.dumps(r)+'\n' for r in rows))
with request.open() as inp,(D/'calls.native.jsonl').open('w') as out:subprocess.run(['elixir','scripts/call_oracle.exs'],stdin=inp,stdout=out,cwd=ROOT,check=True)
with (D/'calls.native.jsonl').open() as inp,(D/'calls.results.jsonl').open('w') as out:subprocess.run([ROOT/'target/release/call_probe'],stdin=inp,stdout=out,check=True)
refs=[json.loads(x) for x in (D/'calls.native.jsonl').read_text().splitlines()];results=[json.loads(x) for x in (D/'calls.results.jsonl').read_text().splitlines()]
if len(results)!=len(refs) or len(refs)!=len(rows) or len(rows)!=20588:raise RuntimeError('native call result denominator changed')
if [r['id'] for r in results]!=[r['id'] for r in rows]:raise RuntimeError('native call result identities changed')
summary={'requests':len(rows),'matched':sum(x['matched'] for x in results),'failed':sum(not x['matched'] for x in results),'native_errors':sum(x.get('outcome',{}).get('status')=='error' for x in refs),'bif_mfas':len(entries),'alternative_cases':sum(x.get('operation')=='accumulate' for x in refs)}
(D/'calls.summary.json').write_text(json.dumps(summary)+'\n');print(summary)
if summary['failed']:raise SystemExit(1)

if hashes()!=implementation:raise RuntimeError('native call implementation changed during verification')
summary.update(complete_call_algebra=not summary['failed'],complete_call_diagnostics=False,implementation_sha256=implementation,inputs_sha256={str(request.relative_to(ROOT)):hashlib.sha256(request.read_bytes()).hexdigest()})
(D/'report.json').write_text(json.dumps(summary,indent=2)+'\n')

#!/usr/bin/env python3
"""Regenerate compiler-only preparation evidence, preserving same-VM provenance."""
import argparse, hashlib, json, os, pathlib, subprocess, time
BASE = pathlib.Path(__file__).resolve().parents[1]

def digest(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()

def run(command, stdin, stdout, env):
    started = time.monotonic()
    with pathlib.Path(stdin).open('rb') as src, pathlib.Path(stdout).open('wb') as dst:
        subprocess.run(command, cwd=BASE, stdin=src, stdout=dst, env=env, check=True)
    return time.monotonic() - started

def main():
    p=argparse.ArgumentParser()
    p.add_argument('--requests',required=True)
    p.add_argument('--probe',required=True)
    p.add_argument('--out',required=True)
    p.add_argument('--expected-cases',type=int,default=327)
    p.add_argument('--expected-records',type=int,default=1849)
    p.add_argument('--expected-contracts',type=int,default=1486)
    a=p.parse_args()
    out=pathlib.Path(a.out).resolve();out.mkdir(parents=True,exist_ok=True)
    inputs=[json.loads(line) for line in pathlib.Path(a.requests).read_text().splitlines()]
    if len(inputs)!=a.expected_cases:raise ValueError('unexpected request count')
    originals={}
    oracle=[];rust=[]
    for index,request in enumerate(inputs):
        for key in ('core','environment'):
            path=pathlib.Path(request[key]).resolve();originals[str(path)]=digest(path)
        proof=str(out / 'proof_core' / f'{index}.json')
        oracle.append({**request,'proof_core':proof})
        rust.append({**request,'core':proof,'proof_core':proof})
    def write(name,rows):
        path=out/name;path.write_text(''.join(json.dumps(r,separators=(',',':'))+'\n' for r in rows));return path
    oracle_requests=write('oracle.requests.jsonl',oracle)
    rust_requests=write('rust.requests.jsonl',rust)
    native=out/'native.jsonl';actual=out/'rust.jsonl';report=out/'comparison.json'
    env=os.environ.copy();env.update(ASDF_ERLANG_VERSION='28.3',ASDF_ELIXIR_VERSION='1.19.4-otp-28',ASDF_RUST_VERSION='1.92.0')
    sources={str(path.relative_to(BASE)):digest(path) for path in sorted((BASE/'src').rglob('*.rs'))}
    for name in ('scripts/preparation_probes/compiled_definitions_oracle.exs','scripts/preparation_probes/compiled_definitions_compare.exs','scripts/verify_compiled_definitions.py','scripts/core_export.exs','scripts/forms_runtime.exs','scripts/native_types.exs','Cargo.toml','Cargo.lock'):
        sources[name]=digest(BASE/name)
    beam_paths=subprocess.check_output(['elixir','-e','Enum.each([:dialyzer_codeserver,:dialyzer_utils,:dialyzer_contracts,:erl_types,:maps,:dict], fn m -> IO.puts(:code.which(m)) end)'],cwd=BASE,env=env,text=True).splitlines()
    for beam in beam_paths:
        sources[beam]=digest(beam)
    probe=pathlib.Path(a.probe).resolve();probe_hash=digest(probe)
    print(f'Native preparation: {len(inputs)} compiler cases',flush=True)
    native_seconds=run(['elixir',str(BASE/'scripts/preparation_probes/compiled_definitions_oracle.exs')],oracle_requests,native,env)
    proof_hashes={r['core']:digest(r['core']) for r in rust}
    print('Rust preparation: consuming exact same-VM proof Core snapshots',flush=True)
    rust_seconds=run([str(probe)],rust_requests,actual,env)
    subprocess.run(['elixir',str(BASE/'scripts/preparation_probes/compiled_definitions_compare.exs'),str(native),str(actual),str(report)],cwd=BASE,env=env,check=True)
    result=json.loads(report.read_text())
    for name,expected in [('cases',a.expected_cases),('matches',a.expected_cases),('records',a.expected_records),('contracts',a.expected_contracts)]:
        if result[name]!=expected:raise ValueError(f'{name}: expected {expected}, observed {result[name]}')
    for path,before in originals.items():
        if digest(path)!=before:raise ValueError(f'original input changed: {path}')
    for path,before in proof_hashes.items():
        if digest(path)!=before:raise ValueError(f'proof input changed: {path}')
    for path,before in sources.items():
        if digest(BASE/path)!=before:raise ValueError(f'source changed during proof: {path}')
    if digest(probe)!=probe_hash:raise ValueError('probe changed during proof')
    result.update(implementation_sha256=sources,inputs_sha256=originals|proof_hashes|{str(probe):probe_hash},original_inputs=originals,proof_inputs=proof_hashes,source_sha256=sources,probe_sha256=probe_hash,native_seconds=native_seconds,rust_seconds=rust_seconds,
        scope={'native_records':True,'candidate_contracts_and_callbacks':True,'exact_processed_contract_capture':True,'final_target_type_tables':True,'exact_reported_unknown_type_events':True,'target_inferred_facts':False,'arbitrary_malformed_contexts_complete':False})
    (out/'report.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({k:result[k] for k in ('cases','matches','records','contracts')},sort_keys=True),flush=True)
if __name__=='__main__':main()

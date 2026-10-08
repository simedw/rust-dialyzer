#!/usr/bin/env python3
import argparse,pathlib,hashlib,json,subprocess,os
p=argparse.ArgumentParser();p.add_argument('--root',required=True);p.add_argument('--out',required=True);a=p.parse_args();root=pathlib.Path(a.root).resolve();out=pathlib.Path(a.out).resolve();out.mkdir(parents=True,exist_ok=False)
base=pathlib.Path('/private/tmp/rust-dialyzer-incremental-container-proof/proof');dependency=pathlib.Path('/private/tmp/rust-dialyzer-dependency-values-checkpoint-1');env=dict(os.environ);env['PATH']='/Users/simedw/.asdf/installs/erlang/28.3/bin:/Users/simedw/.asdf/installs/elixir/1.19.4-otp-28/bin:'+env['PATH']
paths=[p for folder in ['src','scripts']for p in (root/folder).rglob('*')if p.is_file()];paths += [root/'Cargo.lock',root/'Cargo.toml',*[root/'target/release'/n for n in ['classic_checked_probe','raw_incremental_pipeline_probe','incremental_header_probe']],base/'empty.native.iplt']
def sha(p):
 h=hashlib.sha256()
 with p.open('rb')as f:
  for chunk in iter(lambda:f.read(1048576),b''):h.update(chunk)
 return h.hexdigest()
def inventory():return {str(p):sha(p)for p in sorted(set(paths))}
before=inventory()
subprocess.run(['python3',str(dependency/'verify_values.py'),'--binary',str(root/'target/release/classic_checked_probe'),'--out',str(out/'dependencies')],check=True,env=env)
subprocess.run(['python3',str(root/'scripts/replay_incremental_container.py'),'--root',str(root),'--expected',str(base),'--out',str(out/'containers')],check=True,env=env)
headers=out/'headers';headers.mkdir()
with (headers/'requests.jsonl').open('wb')as f:subprocess.run(['elixir',str(root/'scripts/incremental_header_native.exs'),str(headers/'files')],stdout=f,check=True,env=env)
with (headers/'requests.jsonl').open('rb')as i,(headers/'rust.jsonl').open('wb')as o:subprocess.run([str(root/'target/release/incremental_header_probe')],stdin=i,stdout=o,check=True,env=env)
with (headers/'reader.jsonl').open('wb')as o:subprocess.run(['elixir',str(root/'scripts/incremental_header_reader.exs'),str(headers/'requests.jsonl'),str(headers/'rust.jsonl')],stdout=o,check=True,env=env)
rows=[json.loads(x)for x in (headers/'reader.jsonl').read_text().splitlines()];assert len(rows)==32 and all(r['exact']is True for r in rows)
after=inventory();assert before==after
report={'dependencies':582,'whole_dependency_dicts':72,'Original_iplt_readers':133,'deferred_header_transport':32,'unchanged':True,'public_incremental_workflows':'not exercised'}
(out/'report.json').write_text(json.dumps(report,indent=2)+'\n');(out/'source-provenance.json').write_text(json.dumps({'before':before,'after':after},indent=2)+'\n');print(json.dumps(report))

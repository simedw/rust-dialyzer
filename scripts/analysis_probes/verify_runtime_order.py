#!/usr/bin/env python3
"""Replay original runtime primitives through the integrated ordering modules."""
import hashlib,json,os,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
HERE=Path(__file__).resolve().parent
OUT=ROOT/'.cache/runtime_order'
OUT.mkdir(parents=True,exist_ok=True)
ENV=dict(os.environ,ASDF_ERLANG_VERSION='28.3',ASDF_ELIXIR_VERSION='1.19.4-otp-28')
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
parts=[('primitives',1491),('ets',1120),('maps',156)]
files=set([Path(__file__).resolve(),ROOT/'scripts/compiler_atom_primitives.exs'])
for name,_ in parts:
 binary=ROOT/f'target/release/graph_{name}_probe'
 files.update(Path(x) for x in binary.with_suffix('.d').read_text().splitlines()[0].split(': ',1)[1].split())
 files.add(HERE/f'graph_{name}_oracle.exs')
 files.add(binary)
beams=subprocess.check_output(['elixir','-e','Enum.each([:erts_debug,:ets,:maps,:digraph],fn m->p=:code.which(m);if is_list(p),do: IO.puts(p)end)'],env=ENV,text=True).splitlines()
files.update(map(Path,beams))
before={str(p.resolve()):sha(p) for p in files}
summary={}
artifacts={}
for name,count in parts:
 native=OUT/f'{name}_native.jsonl';rust=OUT/f'{name}_rust.jsonl'
 with native.open('wb') as o:subprocess.run(['elixir',str(HERE/f'graph_{name}_oracle.exs')],stdout=o,env=ENV,check=True)
 native_hash=sha(native)
 with native.open('rb') as i,rust.open('wb') as o:subprocess.run([str(ROOT/f'target/release/graph_{name}_probe')],stdin=i,stdout=o,check=True)
 rows=[json.loads(x) for x in rust.read_text().splitlines()]
 assert len(rows)==count,(name,len(rows),count)
 assert all(r.get('matches') is True and r.get('key_roundtrip',True) is True for r in rows),(name,[r for r in rows if not r.get('matches')][:3])
 assert sha(native)==native_hash
 summary[name]={'cases':count,'matches':count}
 artifacts[str(native)]=native_hash;artifacts[str(rust)]=sha(rust)
assert before=={str(p.resolve()):sha(p) for p in files},'proof sources changed'
(OUT/'report.json').write_text(json.dumps({'scopes':summary,'source_sha256':before,'artifact_sha256':artifacts,'full_scheduler':False},indent=2)+'\n')
print(summary)

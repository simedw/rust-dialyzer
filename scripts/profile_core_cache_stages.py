#!/usr/bin/env python3
"""Observe cache stages on both inventories without editing production code.
Runs cold then warm cache passes under the shared lock and removes scratch caches.
Stage sums are worker elapsed time, not CPU time or additive process wall.
"""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import tempfile
import time
import sys
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'bench/phase0'))
from storage import require_disk_reserve
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'reports/analysis-cost-2026-10-10/core-cache-stages'
OUT.mkdir(parents=True,exist_ok=True)
original=(ROOT/'prototypes/core_cache/core_cache.exs').read_text()
helper='''defmodule CacheStageProbe do
  def time(name, fun) do
    start = System.monotonic_time(:nanosecond)
    result = fun.()
    elapsed = System.monotonic_time(:nanosecond) - start
    times = Process.get(:cache_stage_times, %{})
    Process.put(:cache_stage_times, Map.update(times, name, elapsed, &(&1 + elapsed)))
    result
  end
end
'''
s=helper+original
def replace(a,b):
 global s
 assert s.count(a)==1,(a,s.count(a))
 s=s.replace(a,b)
replace('    bytes = File.read!(path)','    Process.put(:cache_stage_times, %{})\n    bytes = CacheStageProbe.time(:read_beam, fn -> File.read!(path) end)')
replace('    key = :crypto.hash(:sha256, bytes) |> Base.encode16(case: :lower)','    key = CacheStageProbe.time(:hash_beam, fn -> :crypto.hash(:sha256, bytes) |> Base.encode16(case: :lower) end)')
replace('    if File.exists?(core_file) and File.exists?(atoms_file) do','    if CacheStageProbe.time(:lookup, fn -> File.exists?(core_file) and File.exists?(atoms_file) end) do')
replace('%{beam: path, core: core_file, atoms: read_atoms(atoms_file), compiled: false}','atoms = CacheStageProbe.time(:read_atom_sidecar, fn -> read_atoms(atoms_file) end)\n      %{beam: path, core: core_file, atoms: atoms, compiled: false, stage_times: Process.get(:cache_stage_times), beam_bytes: byte_size(bytes)}')
replace('      atoms = Enum.map(atoms, &Atom.to_string/1)','      atoms = CacheStageProbe.time(:atom_names, fn -> Enum.map(atoms, &Atom.to_string/1) end)')
replace('      write_atomically(core_file, :erlang.term_to_binary(exported))','      core_bytes = CacheStageProbe.time(:encode_core, fn -> :erlang.term_to_binary(exported) end)\n      CacheStageProbe.time(:write_core, fn -> write_atomically(core_file, core_bytes) end)')
replace('      write_atomically(atoms_file, :erlang.term_to_binary(atoms))','      atom_bytes = CacheStageProbe.time(:encode_atoms, fn -> :erlang.term_to_binary(atoms) end)\n      CacheStageProbe.time(:write_atoms, fn -> write_atomically(atoms_file, atom_bytes) end)')
replace('%{beam: path, core: core_file, atoms: atoms, compiled: true}','format = elem(exported, 0)\n      node_count = if format == :native_core_module_v1, do: elem(exported, 6), else: length(elem(exported, 6))\n      %{beam: path, core: core_file, atoms: atoms, compiled: true, stage_times: Process.get(:cache_stage_times), beam_bytes: byte_size(bytes), core_bytes: byte_size(core_bytes), format: format, core_nodes: node_count}')
replace('      :beam_lib.chunks(bytes, [:debug_info, :abstract_code])','      CacheStageProbe.time(:decode_debug_and_forms, fn -> :beam_lib.chunks(bytes, [:debug_info, :abstract_code]) end)')
replace('    {:ok, unclean} = backend.debug_info(:core_v1, name, metadata, @options)','    {:ok, unclean} = CacheStageProbe.time(:backend_to_core, fn -> backend.debug_info(:core_v1, name, metadata, @options) end)')
replace('    NativeCoreTransport.build(name, forms, :dialyzer_clean_core.clean(unclean))','    cleaned = CacheStageProbe.time(:clean_core, fn -> :dialyzer_clean_core.clean(unclean) end)\n    CacheStageProbe.time(:transport, fn -> NativeCoreTransport.build(name, forms, cleaned) end)')
s+='''
IO.puts(["CACHE_STAGE_PROBE ", :json.encode(%{entries: Enum.map(entries, &Map.drop(&1, [:atoms])), global_stage_ns: Process.get(:cache_stage_times, %{})})])
'''
# Time global atom/context work separately without changing the output expression.
replace('atoms = entries |> Enum.flat_map(& &1.atoms) |> Enum.map(&String.to_atom/1)','global_start = System.monotonic_time(:nanosecond)\natoms = entries |> Enum.flat_map(& &1.atoms) |> Enum.map(&String.to_atom/1)')
replace('compiled = Enum.count(entries, & &1.compiled)','Process.put(:cache_stage_times, %{atom_layout_profiles_and_manifest: System.monotonic_time(:nanosecond) - global_start})\ncompiled = Enum.count(entries, & &1.compiled)')
results=[]
with open('/private/tmp/rust-dialyzer-run.lock','a') as lock:
 fcntl.flock(lock,fcntl.LOCK_EX)
 for project in ['phoenix','elixir']:
  require_disk_reserve('/private/tmp')
  with tempfile.TemporaryDirectory(prefix='core-cache-stage-') as tmp:
   tmp=Path(tmp);script=tmp/'probe.exs';script.write_text(s)
   beams=ROOT/'reports/head-to-head-2026-10-09'/project/'beams.txt'
   env=dict(os.environ,ERL_FLAGS='+S 14:14')
   for mode in ['cold','warm']:
    cmd=[str(ROOT/'.cache/real-projects/elixir-v1.20.4/bin/elixir'),str(script),str(ROOT),str(beams),str(tmp/'cache'),str(tmp/'manifest.json'),'--jobs','14']
    started=time.perf_counter()
    run=subprocess.run(cmd,cwd=ROOT,env=env,capture_output=True,text=True)
    wall=time.perf_counter()-started
    if run.returncode:
     (OUT/f'{project}-{mode}-failure.stderr').write_text(run.stderr)
     print(run.stderr, file=sys.stderr)
     run.check_returncode()
    (OUT/f'{project}-{mode}.stderr').write_text(run.stderr)
    lines=run.stdout.splitlines()
    actual=json.loads(next(x[len('CACHE_STAGE_PROBE '):] for x in lines if x.startswith('CACHE_STAGE_PROBE ')))
    loop=json.loads(next(x for x in reversed(lines) if x.startswith('{')))
    entries=actual['entries']
    assert all(x['compiled']==(mode=='cold') for x in entries)
    totals={}
    for entry in entries:
     for stage,ns in entry['stage_times'].items():totals[stage]=totals.get(stage,0)+ns/1e6
    largest=sorted(entries,key=lambda e:sum(e['stage_times'].values()),reverse=True)[:12]
    result=dict(project=project,mode=mode,process_wall_s=wall,module_loop_s=loop['seconds'],outside_loop_s=wall-loop['seconds'],modules=len(entries),beam_bytes=sum(e['beam_bytes'] for e in entries),worker_elapsed_ms=totals,global_stage_ms={k:v/1e6 for k,v in actual['global_stage_ns'].items()},entries=entries,largest_modules=largest)
    if mode=='cold':result.update(core_bytes=sum(e['core_bytes'] for e in entries),core_nodes=sum(e['core_nodes'] for e in entries),fallback_modules=sum(e['format']!='native_core_module_v1' for e in entries))
    results.append(result)
    print(json.dumps({k:v for k,v in result.items() if k not in ['entries','largest_modules']}),flush=True)
(OUT/'measurements.json').write_text(json.dumps(results,indent=2)+'\n')
(OUT/'instrumented-cache.exs').write_text(s)
(OUT/'provenance.json').write_text(json.dumps(dict(source_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),source_sha256={str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [ROOT/'prototypes/core_cache/core_cache.exs',ROOT/'scripts/core_export.exs',ROOT/'scripts/core_native_export.exs',ROOT/'scripts/compiler_atom_primitives.exs']},instrumentation_sha256=hashlib.sha256(s.encode()).hexdigest(),selection='Both retained complete input inventories; one cold/warm pair each; 14 workers; filesystem caches not flushed',limits='Diagnostic instrumented runs, not repeated production benchmarks. Worker elapsed sums overlap and include scheduling/GC. No analyzer or semantic-output parity run.',scratch_caches_removed=True),indent=2)+'\n')

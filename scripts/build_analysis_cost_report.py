#!/usr/bin/env python3
"""Build an offline cost report from retained evidence. Does not run the analyzer.

Usage: python3 scripts/build_analysis_cost_report.py
Outputs: reports/analysis-cost-2026-10-10/{index.html,data.json,stages.csv}
"""
import collections
import csv
import gzip
import hashlib
import html
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'reports/analysis-cost-2026-10-10'
BASE = ROOT / 'reports/halving-rust-2026-10-09'
PROVENANCE = {}

def read(path):
    path = ROOT / path if not isinstance(path, Path) else path
    payload = path.read_bytes()
    PROVENANCE[str(path.relative_to(ROOT))] = hashlib.sha256(payload).hexdigest()
    if path.suffix == '.gz':
        payload = gzip.decompress(payload)
    return json.loads(payload)

def text(path):
    path = ROOT / path
    payload = path.read_bytes()
    PROVENANCE[str(path.relative_to(ROOT))] = hashlib.sha256(payload).hexdigest()
    return payload.decode()

def esc(value): return html.escape(str(value))
def num(value): return f'{value:,}' if isinstance(value, int) else f'{value:,.2f}'
def ms(value): return 'not captured' if value is None else f'{value:,.2f} ms'
def link(path, label=None):
    return f'<a href="../../{esc(path)}">{esc(label or path)}</a>'
def table(headers, rows):
    return '<div class="table-scroll"><table><thead><tr>'+''.join(f'<th>{esc(h)}</th>' for h in headers)+'</tr></thead><tbody>'+''.join('<tr>'+''.join(f'<td>{v}</td>' for v in row)+'</tr>' for row in rows)+'</tbody></table></div>'

def aggregate_body(rows, diagnostics, phase):
    totals = collections.Counter()
    for row in rows:
        if row['module_memo_shadow'] or row['diagnostics'] != diagnostics:
            continue
        for key, value in row[phase].items():
            if key == 'events':
                totals.update({'events.'+k: v for k, v in value.items()})
            elif isinstance(value, int): totals[key] += value
    return dict(totals)

PROJECTS = ['phoenix', 'elixir']
records = {p: read(BASE / f'focused-profiling/{p}-phase-records.json') for p in PROJECTS}
focus_meta = read(BASE / 'focused-profiling/metadata.json')
focus_summary = read(BASE / 'focused-profiling/summary.json')
focused = read(BASE / 'focused-profiling/focused-summary.json')
contracts = read(BASE / 'focused-profiling/contracts-component-records.json')
production = read(BASE / 'solver-reuse/production/otp/summary.json')
solver = read(BASE / 'solver-reuse/census-summary.json')
body_summary = read(BASE / 'body-read-census/summary.json')
body_meta = read(BASE / 'body-read-census/metadata.json')
registry_summary = read(BASE / 'registry-transfer-census/census/summary.json')
body = {}
registry = {}
for p in PROJECTS:
    rows = read(BASE / f'body-read-census/normal/{p}/census.json.gz')
    body[p] = {}
    for name, diagnostic, phase in [('refinement-inference', False, 'inference'), ('diagnostics-inference', True, 'inference'), ('diagnostics-warnings', True, 'warnings')]:
        body[p][name] = aggregate_body(rows, diagnostic, phase)
        assert body[p][name] == body_summary[p+'/normal'][name]['totals'], (p, name)
    report = read(BASE / f'registry-transfer-census/census/normal/{p}/transfer-census.json')
    summary = next(r for r in registry_summary['runs'] if r['mode']=='normal' and r['project']==p)
    registry[p] = summary['owners'][0]
    # Validate counters from the emitted report, not only prose tables.
    def find_dict(value, required):
        if isinstance(value, dict):
            if all(k in value for k in required): return value
            for child in value.values():
                found = find_dict(child, required)
                if found is not None: return found
        elif isinstance(value, list):
            for child in value:
                found = find_dict(child, required)
                if found is not None: return found
    raw = find_dict(report, ['attempts', 'complete_stable_repeats'])
    assert raw is not None
    assert raw['attempts'] == registry[p]['aggregate']['attempts']
    assert raw['complete_stable_repeats'] == registry[p]['aggregate']['stable_complete_repeats']
    raw_path = BASE / f'solver-reuse/census-run/{p}.stderr.gz'
    raw_bytes = raw_path.read_bytes()
    PROVENANCE[str(raw_path.relative_to(ROOT))] = hashlib.sha256(raw_bytes).hexdigest()
    raw_visits = [json.loads(line[len('SOLVER_WORK '):]) for line in gzip.decompress(raw_bytes).decode().splitlines() if line.startswith('SOLVER_WORK ')]
    visits = solver[p]['visits']
    assert len(raw_visits) == len(visits)
    for raw_visit, visit in zip(raw_visits, visits):
        for key in ['functions', 'nodes', 'rounds', 'tried_unit']:
            assert raw_visit[key] == visit[key], (p, key)
        assert len(raw_visit['roots']) == visit['root_calls']
        work = collections.Counter()
        for root in raw_visit['roots']: work.update(root['work'])
        assert dict(work) == visit['work'], p
        assert sum(root['certificate']['matched'] for root in raw_visit['roots']) == visit['matching_roots']
    assert sum(v['root_calls'] for v in visits) == solver[p]['total']['root_calls']
    assert sum(v['work']['atomic'] for v in visits) == solver[p]['total']['atomic']
    for r in records[p]:
        assert abs(sum(x['ms'] for x in r['phases'])-r['total_ms']) < 0.002
        assert abs(r['end_ms']-r['start_ms']-r['total_ms']) < 0.002

structural_note = text('reports/two-x-investigation-2026-10-09/rust-2x-cache-setup.md')
assert '4,276,031/4,460,441 Core nodes' in structural_note
structure = {'phoenix': {'core_nodes':4276031,'original_terms':7596094,'retained_terms':4445691}, 'elixir': {'core_nodes':4460441,'original_terms':8649728,'retained_terms':5133038}}
source_names = ['src/bin/core_cache_analyze.rs','src/compiled_probe.rs','src/compiled_analysis.rs','src/core_pipeline.rs','src/core_analysis.rs','src/core_phases.rs','src/constraints/analyze_scc.rs','src/constraints/tree.rs','src/body_flow.rs','src/function_state.rs','src/lattice.rs','src/warning_publication.rs','src/diagnostic_inputs.rs','src/source_graph_algorithms.rs','prototypes/core_cache/core_cache.exs']
source = {name:text(name) for name in source_names}
head = subprocess.check_output(['git','rev-parse','HEAD'], cwd=ROOT, text=True).strip()
tracked_hashes = focus_meta.get('source_sha256', {})
# Some evidence manifests use a different hash-map field.
if not tracked_hashes:
    for value in focus_meta.values():
        if isinstance(value, dict) and 'src/core_analysis.rs' in value:
            tracked_hashes=value; break
changed = [name for name in source_names if name in tracked_hashes and hashlib.sha256(source[name].encode()).hexdigest()!=tracked_hashes[name]]

def phase(project, scope, mark, label=None):
    hits=[]
    for r in records[project]:
        if r['scope']==scope and (label is None or r['label']==label):
            hits.extend(v['ms'] for v in r['phases'] if v['name']==mark)
    return sum(hits) if hits else None

def function_ref(path, needle):
    lines=source[path].splitlines()
    at=next((i for i,line in enumerate(lines,1) if needle in line),None)
    assert at is not None, (path,needle)
    return {'path':path,'line':at,'symbol':needle}

# Stage catalog keeps data boundaries distinct even when the timer combines them.
stages=[]
def stage(title, path, symbol, inputs, outputs, algorithm, scope=None, mark=None, label=None, work='', limit=''):
    ref=function_ref(path,symbol)
    stages.append(dict(id=len(stages)+1,title=title,source=ref,inputs=inputs,outputs=outputs,algorithm=algorithm,
        timing={p:phase(p,scope,mark,label) if scope else None for p in PROJECTS},scope=scope,mark=mark,label=label,work=work,limit=limit))
stage('Read BEAMs and compile cleaned Core','prototypes/core_cache/core_cache.exs','def entry(',
 '600 / 846 BEAM files; debug-info backend; compiler profile','One cached ETF record and atom sidecar per module; ordered manifest',
 'Content SHA-256 lookup, parallel compilation on cache misses, OTP Core lowering and cleaning, atomic file rename.',
 work='600 / 846 modules. Historical preserved Core counts: 4,276,031 / 4,460,441 nodes.',limit='Latest retained matched fresh medians put the whole Core-cache process at 2.14 / 2.06 s. Backend compilation, cleaning, export, hash lookup and context emission are not individually timed.')
stage('Decode module records','src/bin/core_cache_analyze.rs','fn load(',
 'Manifest module_files and context; cached ETF bytes','Validated CoreProgram with flat node and term tables',
 'Workers fetch module indexes, read and decode independently, restore input order, then assemble the CoreProgram.',
 work='600 / 846 module decodes. Module validation work and native-to-flat conversion counts were not captured.',limit='Load is outside probe_run. The old phase probe separately reports load_seconds; see run context below. The manifest/options/library reads precede that timer.')
stage('Load registries and configuration','src/compiled_probe.rs','let bifs = Registry::load',
 'Warning options, BIF registry, dependency policy, contract mode, prior JSON','AnalysisInput and transfer/policy registries',
 'Parse and validate immutable registry inputs and select the solver and reporting mode.', 'probe_run','registries_and_configuration',work='Registry census inventories 590 MFAs; the timed setup includes other registries as well.')
stage('Label Core nodes','src/compiled_analysis.rs','let labels = Labels::build',
 'All Core modules in input order','Labels mapping Core objects to stable analysis identities',
 'Traverse and allocate labels deterministically. Runs concurrently with folded Core, program and policy setup.', 'setup_branch','labels','labels',work='Historical Core input: 4.276 M / 4.460 M nodes. Label allocations are not individually counted.')
stage('Build folded Core','src/compiled_analysis.rs','let folded = FoldedCore::build',
 'Original CoreProgram','Folded pattern view preserving compiler labels',
 'Construct folded literal/pattern representation alongside the original Core view.', 'setup_branch','folded_core','folded_core',work='Folded node creations and fold hits are not captured.')
stage('Index functions and declarations','src/compiled_analysis.rs','let program = Program::build_with_contracts',
 'Core functions, module attributes, contract mode','Program identities and origins; candidate declaration metadata',
 'Index named and nested functions and retain declaration origins. Concurrent with other independent setup.', 'setup_branch','program','program',work='Named inferred output later has 31,050 / 39,126 signatures. That output count is not the exact setup function count.')
stage('Parse the prior store','src/compiled_analysis.rs','let prior = PriorPlt::parse',
 'Prior library JSON; import/purge plan','Imported prior facts and original membership',
 'Parse the explicit prior world and apply mode-specific invalidation before graph cleanup.', 'setup_branch','prior_parse','program',work='This fixture has a very small prior parse time. Imported declaration row counts are not captured.')
stage('Prepare policy and binders','src/compiled_analysis.rs','let policy = Policy::build',
 'Warning profiles, source suppression metadata, module declarations','Policy and per-module BinderState',
 'Build policy then construct module binder states in parallel.', 'setup_branch','policy','policy',work='600 / 846 per-module binder preparations.',limit='Binder-state time is a separate mark: 17.18 / 10.30 ms in this probe. Policy and binder times must be added only within their own branch.')
stage('Prepare remote definitions and module inputs','src/compiled_analysis.rs','PreparedPhaseInputs::build',
 'Prior facts, target records/types/specs, folded Core and labels','PreparedDefinitions and immutable refinement inputs',
 'Resolve reached declarations; preserve laziness for unused imported bodies; prepare per-module views.', 'setup_branch','refinement_inputs','refinement_inputs',work='600 / 846 module preparation inputs.',limit='The recorded refinement_inputs mark does not include the entire preceding prepare_definitions call. That preceding work has no isolated timer in this evidence.')
stage('Build dataflow and closure dependencies','src/compiled_analysis.rs','ClosureDependencies::analyze',
 'Core, labels, Program and closure policy','DataflowNodes and possible closure/call-target dependencies',
 'Build literal and map facts while discovering possible local call targets and escaping functions.', 'setup_branch','dataflow_and_dependencies','callgraph',work='One whole-program setup; call-target sets and closure edges not counted.',limit='Two parallel owners share this joined timer; their individual times are unknown.')
stage('Build the call graph','src/compiled_analysis.rs','SourceCallGraph::build_with_layout',
 'Program, call targets, BIF registry and runtime atom layout','Ordered target call graph with incoming/outgoing edges',
 'Register functions and edges while retaining native identity/order semantics.', 'setup_branch','callgraph','callgraph',work='First schedule has 27,242 / 35,180 SCCs. These are components, not node or edge counts.')
stage('Clean external and missing calls','src/compiled_analysis.rs','cleanup_callgraph(graph',
 'Raw graph, exports, binder, policy and original PLT membership','Cleaned graph; external/missing-call events; module dependencies',
 'Separate external summaries from target jobs and preserve missing-export origins.', 'setup_branch','cleanup','callgraph',work='Removed vertices, external edges and missing-call events are not counted in the selected timing probe.')
stage('Discover SCCs and prepare the schedule','src/compiled_analysis.rs','core_pipeline::prepare_schedule',
 'Cleaned graph and Program; reserved runtime scope','SCC condensation and dependency schedule',
 'Two-pass DFS SCC discovery, collapse recursive groups, schedule callees before callers. Includes native-order/runtime preparation.', 'setup_branch','schedule','callgraph',work='Initial 27,242 / 35,180 SCCs across 89 / 101 dependency waves.',limit='DFS traversal is O(V+E) in graph work; compatibility tables and ordering add costs. This timer is broader than SCC DFS alone.')
stage('Clean current prior keys and prepare callbacks','src/compiled_analysis.rs','prior.after_cleanup',
 'Prior store, cleaned graph and target callback definitions','Library view for inference and target callback state',
 'Delete current named keys from imported state, then prepare callbacks and persistence groups.', 'compiled_analysis','prior_cleanup',work='Callback groups are not counted.',limit='Callback preparation is separately 43.41 / 22.97 ms; no callback inference occurs here.')
stage('Run SCC inference and publish contracts','src/core_analysis.rs','pub fn run<',
 'SCC jobs, prior phase signatures, active contracts and immutable code facts','Updated target signatures; newly activated valid contracts',
 'Prepare function entries, generate and solve constraints, restrict results to owned labels, limit persisted types to depth five and commit validated contracts.', 'core_pipeline','inference_and_refinement',work='Historical scheduling visits total 53,807 / 70,876. Large-SCC calculation counters appear below.',limit='This is the combined inference/refinement parent time. It must not be added to its SCC or module child durations. Contract activation is a phase change even when signatures are equal.')
stage('Generate, prepare and solve constraints','src/constraints/analyze_scc.rs','pub fn analyze(',
 'One SCC, reached nested functions, records, BIFs, propagated types and active contracts','Prepared constraint graph and limited inferred bindings',
 'Generate atomic/AND/OR/reference nodes; prepare masks and dependencies; solve bindings with invalidation, branch joins and recursive fixpoints.',work='Selected solver census: 9 large-SCC visits per inventory, 21,501 root calls and 4,804,519 atomic evaluations. Details below.',limit='Only ordered SCCs of size >=64 have phase timings. Solver census selection differs; it is not a whole-program operation count. Current code additionally has bounded whole-root replay, which this earlier no-skip census predates.')
stage('Refine modules with a function worklist','src/core_phases.rs','fn refine_module_uncached',
 'Previous module types, fixed CallWorld, Core/pattern views and current active contracts','Refined function types and dependency read records',
 'Build mutable FunctionState, evaluate functions through a FIFO worklist, forward arguments and reschedule callers on return changes; memoize an eligible unchanged module invocation.',work='274,784 / 384,540 evaluated refinement bodies; 27,345,359 / 32,622,978 evaluator frames.',limit='Module memo pass durations are shown separately. Body census is a later ordinary release build; frames are interpreter actions, not unique Core nodes.')
stage('Collect final diagnostics','src/core_pipeline.rs','let diagnostics = DiagnosticInputs',
 'Converged signatures, declarations, selected modules, library and callbacks','Raw contract, body, unused/no-return and callback warnings; documentation signatures',
 'Prepare diagnostic sessions. Current code joins contract checks with per-module diagnostics. Each body session also performs its inference traversal before warning mode.', 'core_pipeline','diagnostics',work='Diagnostic inference bodies: 115,872 / 166,296; warning-mode bodies: 36,716 / 47,352.',limit='These timings precede the accepted contract/module overlap. They describe a historical serial boundary, not today\'s absolute diagnostic cost. Body counts are independently recomputed from later raw census records.')
stage('Publish dependencies, filter and order warnings','src/compiled_analysis.rs','let warnings = warning_publication::publish',
 'Raw analysis, cleanup and unknown warnings; contract origins; policy','Ordered public warning terms with requested line/column positions; module dependencies',
 'Rewrite contract origins, apply same-origin filtering and suppressions, combine warnings, sort/deduplicate and project public locations.', 'compiled_analysis','publication',work='Public output: 323 / 215 warnings. Raw-before-filter and suppression counts not captured.',limit='Dependency publication overlaps inference and has a separate 48.90 / 56.39 ms branch. Unknown-warning assembly is separately 2.79 / 0.55 ms.')
stage('Project signatures and serialize the result','src/compiled_probe.rs','let signatures = crate::ordered_parallel',
 'Converged state and public warnings','JSON signature values, warning payload and public counts',
 'Project selected named signatures in parallel; return the probe value; the executable writes the final JSON result.', 'probe_run','signatures_and_output_projection',work='31,050 / 39,126 named signature rows; 323 / 215 warnings.',limit='The projection mark excludes final stdout JSON writing. Output release is in the probe trailing scope; process wall also includes work outside the timing scopes.')

# The executable's load field is separate from nested phase records.
for p in PROJECTS:
    stages[1]['timing'][p] = focus_summary[p]['rows']['probe_on']['load_s'] * 1000
stages[1]['scope'] = 'executable output field'
stages[1]['mark'] = 'load_seconds'
# Correct explicitly stated diagnostic counts against actual raw census.
for s in stages:
    if s['title']=='Collect final diagnostics':
        s['work']=('Diagnostic inference bodies: '+ ' / '.join(num(body[p]['diagnostics-inference']['bodies']) for p in PROJECTS)+'; warning-mode bodies: '+' / '.join(num(body[p]['diagnostics-warnings']['bodies']) for p in PROJECTS)+'.')

iterations={}
for p in PROJECTS:
    iterations[p]=[]
    memo=iter(r for r in records[p] if r['scope']=='module_memo')
    for r in records[p]:
        if r['scope']!='scc_graph':continue
        m=re.fullmatch(r'iter(\d+) sccs (\d+) waves (\d+)',r['label']);assert m
        row={'iteration':int(m[1]),'sccs':int(m[2]),'waves':int(m[3]),'scc_ms':r['total_ms']}
        nxt=next(memo,None)
        if nxt:
            mm=re.fullmatch(r'modules (\d+) same_previous (\d+) hits (\d+)',nxt['label']);assert mm
            row.update(modules=int(mm[1]),same_previous=int(mm[2]),memo_hits=int(mm[3]),module_ms=nxt['total_ms'])
        iterations[p].append(row)
assert sum(r['sccs'] for r in iterations['phoenix'])==53807
assert sum(r['sccs'] for r in iterations['elixir'])==70876

# Revalidate the core numbers used in the prose against extracted values.
assert body['phoenix']['refinement-inference']['bodies']==274784
assert body['elixir']['refinement-inference']['events.frames']==32622978
assert registry['phoenix']['aggregate']['attempts']==1664186
assert len(solver['phoenix']['visits'])==len(solver['elixir']['visits'])==9

out_data={'report_date':'2026-10-10','source_head':head,'measurement_policy':'Archived measured evidence, not a fresh benchmark of this HEAD. Nested/parallel times cannot be added. Missing counts remain unknown.',
 'focused_source_commit':focus_meta.get('base_commit'),'focused_source_drift':changed,'projects':{},'stages':stages}
for p in PROJECTS:
    out_data['projects'][p]={'production_medians':production[p],'structure_historical':structure[p],'phase_records':records[p],'iterations':iterations[p],'body':body[p],'solver_selected':solver[p],'registry':registry[p]}

parts=[]
def add(value):parts.append(value)
add('''<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Where the analysis work goes · Rust Dialyzer</title><style>
:root{--bg:#f4f1e8;--ink:#202e2b;--muted:#5c665e;--green:#24634e;--orange:#b94829;--line:#cdd0c1;--panel:#e6e9dc;--gold:#e7be54}*{box-sizing:border-box}html{scroll-behavior:smooth;scroll-padding-top:25px}body{margin:0;background:var(--bg);color:var(--ink);font:16px/1.65 'Trebuchet MS',sans-serif}main{max-width:1250px;margin:auto;padding:55px 35px}h1{font:normal clamp(42px,6vw,76px)/1.04 Georgia,serif;letter-spacing:-.045em;margin:20px 0;max-width:900px}h1 em{color:var(--green)}h2{font:normal 36px/1.16 Georgia,serif;letter-spacing:-.02em;margin:0 0 20px}h3{font:normal 25px/1.2 Georgia,serif}p{max-width:940px}a{color:var(--green);text-underline-offset:4px}a:focus-visible,summary:focus-visible{outline:3px solid var(--orange);outline-offset:4px}code,pre,.mono{font-family:'Courier New',monospace}code{font-size:.86em;background:var(--panel);padding:2px 4px;overflow-wrap:anywhere}.eyebrow{font:12px/1.7 'Courier New',monospace;text-transform:uppercase;letter-spacing:.12em;color:var(--orange)}.intro{font-size:21px;max-width:890px;color:var(--muted)}.note{border-left:3px solid var(--orange);background:#eee5d8;padding:18px 22px;margin:25px 0}.note p{margin:5px 0;font-size:15px}.chips{display:flex;gap:10px;flex-wrap:wrap;margin:25px 0}.chips a{border:1px solid var(--line);padding:7px 13px;text-decoration:none;font-size:13px}.chips a:hover{border-color:var(--green)}section{padding:40px 0;border-top:1px solid var(--line)}.table-scroll{overflow:auto}table{border-collapse:collapse;width:100%;margin:20px 0;font-size:14px}th{text-align:left;color:var(--green);font-weight:normal;white-space:nowrap}th,td{padding:12px 14px 12px 0;border-bottom:1px solid var(--line);vertical-align:top}td.num{font-family:'Courier New',monospace;white-space:nowrap}.small{font-size:13px;color:var(--muted)}.grid{display:grid;grid-template-columns:1fr 1fr;gap:30px}.metric{border-top:3px solid var(--green);padding:15px 0}.metric strong{font:normal 42px Georgia,serif;display:block}.metric small{display:block;color:var(--muted)}details.stage{border:1px solid var(--line);margin:12px 0;background:#f8f6ee}summary{cursor:pointer;padding:18px 20px;font-size:17px}summary .number{color:var(--orange);font:13px 'Courier New',monospace;margin-right:15px}summary .times{float:right;color:var(--green);font:13px/1.8 'Courier New',monospace}details .inside{padding:0 22px 22px}details .inside p{margin:10px 0}.boundaries{display:grid;grid-template-columns:1fr 1fr;gap:20px;background:var(--panel);padding:15px 20px;margin:15px 0}.boundaries .label{font:11px 'Courier New',monospace;color:var(--green);letter-spacing:.08em}.boundary{font-size:14px}.source{font-size:12px;overflow-wrap:anywhere}.measure{font-size:14px;border-top:1px solid var(--line);padding-top:12px}svg{width:100%;height:auto;background:var(--panel);display:block}svg text{font-family:'Trebuchet MS',sans-serif;fill:var(--ink)}figure{margin:24px 0}figcaption{font:12px/1.7 'Courier New',monospace;color:var(--muted);margin:10px 0}pre{white-space:pre-wrap;background:var(--ink);color:var(--bg);padding:22px;font-size:13px;overflow-wrap:anywhere}ul li{margin:8px 0}.gap{color:var(--orange)}.split{font-family:'Courier New',monospace;white-space:nowrap}.foot{font-size:12px;color:var(--muted);padding:30px 0}@media(max-width:700px){main{padding:30px 18px}.grid,.boundaries{grid-template-columns:1fr}summary .times{float:none;display:block;margin:8px 0 0 30px}th,td{padding:10px 12px 10px 0}h2{font-size:29px}}@media(prefers-reduced-motion:reduce){html{scroll-behavior:auto}}@media print{main{padding:0}.chips{display:none}body{font-size:10pt;background:white}h1{font-size:40pt}details{break-inside:avoid}summary{font-size:12pt}section{padding:20px 0}svg,pre,.note{print-color-adjust:exact;-webkit-print-color-adjust:exact}}
</style></head><body><main><div class="eyebrow">Rust Dialyzer · optimisation evidence · 10 October 2026</div><h1>Where the analysis<br><em>work goes.</em></h1><p class="intro">A code-stage report for the complete Phoenix and Elixir inventories. Each stage identifies its input, output, algorithm, measured time and available calculation counts. The goal is to choose the next experiment with a clear account of the work it could remove.</p>''')
add(f'<p class="small">Code map checked against HEAD <code>{head}</code>. Measurements below are retained runs from several earlier builds. No new analyzer benchmark or production edit was performed to make this report.</p>')
add('''<nav class="chips" aria-label="Report contents"><a href="#context">Workloads & timing rules</a><a href="#stages">20 code stages</a><a href="#timeline">Timeline & iterations</a><a href="#scc">Constraint work</a><a href="#body">Body work</a><a href="#registry">Built-in work</a><a href="#next">Next experiments</a><a href="#gaps">Measurement gaps</a><a href="#evidence">Data & provenance</a></nav><div class="note"><p>Use the phase timeline to locate elapsed-time costs and the censuses to understand work volume. They are different experiments. A million repeated operations do not imply a million expensive operations, and worker durations do not add up to process wall time.</p><p>The focused timing build has known hot PGO profile mismatches and predates accepted diagnostic overlap and whole-root replay. Its durations are evidence about that instrumented build. They are not current production effect sizes.</p></div>''')
if (OUT/'core-cache-stages/README.md').exists():
    add('<div class="note"><p>A later isolated BEAM-to-Core probe now breaks down this first stage. It confirms 25.8 / 29.4 MB of BEAM input expands to 289.3 / 263.0 MB of Core ETF across 4.276 / 4.460 million nodes. Debug/form recovery leads summed worker time; Core lowering and transport are also substantial. These worker durations overlap.</p><p><a href="core-cache-stages/README.md">Read the stage investigation</a> · <a href="core-cache-stages/summary.json">Measured stage summary</a></p></div>')
add('<section id="context"><div class="eyebrow">01 · establish the denominator</div><h2>Two complete input worlds</h2>')
rows=[]
for label,key in [('BEAM modules','modules'),('Final public warnings','warnings'),('Fresh Core + Rust median, seconds','cold_rust_wall_s'),('Prepared-Core Rust process median, seconds','rust_wall_s'),('Cold Core-cache process median, seconds','cold_cache_wall_s'),('Peak Rust footprint, GB','rust_peak_GB'),('Fresh OTP PLT median, seconds','otp_wall_s')]:
    rows.append([esc(label)]+[num(production[p][key]) for p in PROJECTS])
rows.insert(2,['Final named signature rows','31,050','39,126'])
add(table(['Matched production window at accepted source 85188ed','Phoenix','Elixir'],rows))
add('<p class="small">Three alternating fresh pairs, source-matched PGO, all modules compiled on each fresh run, exact warning multiplicity and complete signatures. Filesystem caches were not flushed. Individual component medians need not sum exactly to a median total. '+link('reports/halving-rust-2026-10-09/solver-reuse/production/otp/REPORT.md','Matched production evidence')+'</p>')
add(table(['Historical structural inventory, before transport changes','Phoenix','Elixir'],[['Original Core nodes',num(structure['phoenix']['core_nodes']),num(structure['elixir']['core_nodes'])],['Original exported term entries',num(structure['phoenix']['original_terms']),num(structure['elixir']['original_terms'])],['Retained term descendants identified by structural probe',num(structure['phoenix']['retained_terms']),num(structure['elixir']['retained_terms'])]]))
add('<p class="small">Core node IDs/counts were preserved in the compaction investigation. Term counts above are historical and must not be described as the current native cache size. A current per-module arena inventory remains a measurement gap. '+link('reports/two-x-investigation-2026-10-09/rust-2x-cache-setup.md','Structural inventory evidence')+'</p>')
add('<h3>What the units mean</h3><ul><li>Elapsed milliseconds describe one timing scope, including waiting. Parallel branch times overlap.</li><li>CPU seconds add execution across threads. They can exceed wall seconds.</li><li>Core nodes are static syntax objects; constraint nodes are generated solver objects; evaluator frames are dynamic interpreter actions. Keep those counts separate.</li><li>SCC visits and body evaluations count repeated work across fixpoint passes, not unique functions.</li><li>Normal-mode census counts exclude module-memo reference invocations. VERIFY adds replay checks, so it is not the performance denominator.</li></ul></section>')
add('<section id="stages"><div class="eyebrow">02 · the code, in execution order</div><h2>Inputs, outputs and cost at each stage</h2><p>Open a row for the mechanism and work counts. Header times are Phoenix / Elixir milliseconds in the focused historical phase run. A missing isolated timer stays visible as “not captured”. Source links point to the current checkout; line numbers are recorded in data.json and stages.csv.</p>')
for s in stages:
    add(f'<details class="stage" id="stage-{s["id"]:02d}"><summary><span class="number">{s["id"]:02d}</span>{esc(s["title"])}<span class="times">{ms(s["timing"]["phoenix"])} / {ms(s["timing"]["elixir"])}</span></summary><div class="inside">')
    add(f'<div class="boundaries"><div><span class="label">INPUT</span><p class="boundary">{esc(s["inputs"])}</p></div><div><span class="label">OUTPUT</span><p class="boundary">{esc(s["outputs"])}</p></div></div><p>{esc(s["algorithm"])}</p><p class="measure">Work observed: {esc(s["work"])}</p>')
    if s['limit']: add(f'<p class="small">Measurement boundary: {esc(s["limit"])}</p>')
    r=s['source'];add(f'<p class="source">{link(r["path"])} · line {r["line"]} · <code>{esc(r["symbol"])}</code></p></div></details>')
add('</section>')
add('<section id="timeline"><div class="eyebrow">03 · elapsed time and dependency structure</div><h2>What happens on the critical path</h2><p>The bars below are sequential checkpoints in the same probe_run record, so they form a valid partition of that scope. They exclude Core compilation, loading before probe_run, and final stdout writing. Internal worker scopes overlap these parent bars.</p>')
for p in PROJECTS:
    parent=next(r for r in records[p] if r['scope']=='probe_run')
    compiled=next(r for r in records[p] if r['scope']=='compiled_analysis')
    core=next(r for r in records[p] if r['scope']=='core_pipeline')
    bands=[(x['name'],x['ms']) for x in compiled['phases']]
    # Replace one parent with its children, preserving each checkpoint duration exactly.
    expanded=[]
    for name,duration in bands:
        if name=='core_pipeline':
            expanded.extend((x['name'],x['ms']) for x in core['phases'])
            expanded.append(('pipeline wrapper gap',duration-core['total_ms']))
        else:expanded.append((name,duration))
    complete=[('registry/config',parent['phases'][0]['ms'])]+expanded+[('analysis wrapper gap',parent['phases'][1]['ms']-compiled['total_ms'])]+[(x['name'],x['ms']) for x in parent['phases'][2:]]
    assert abs(sum(v for _,v in complete)-parent['total_ms'])<0.01
    colors=['#a7b995','#7f9b79','#4c7963','#24634e','#b94829','#ca795b','#c4a648']
    add(f'<h3>{p.title()} · {parent["total_ms"]/1000:.3f} seconds in probe_run</h3><figure><svg viewBox="0 0 1080 110" role="img" aria-label="{p} sequential parent checkpoint durations"><text x="15" y="22" font-size="12">0 s</text><text x="980" y="22" font-size="12">{parent["total_ms"]/1000:.3f} s</text>')
    offset=15
    for i,(name,duration) in enumerate(complete):
        width=duration/parent['total_ms']*1040
        color=colors[i%len(colors)]
        if duration>0:add(f'<rect x="{offset:.3f}" y="35" width="{width:.3f}" height="48" fill="{color}"><title>{esc(name)}: {ms(duration)}</title></rect>')
        if width>95:add(f'<text x="{offset+8:.3f}" y="65" fill="#fff" style="fill:white" font-size="12">{esc(name.replace("_"," "))[:24]}</text>')
        offset+=width
    add('</svg><figcaption>Hover a bar for its checkpoint. Full durations and original timestamped records are in data.json.</figcaption></figure>')
add(table(['Exclusive parent checkpoint in compiled_analysis','Phoenix ms','Elixir ms'],[[esc(x['name'])]+[ms(phase(p,'compiled_analysis',x['name'])) for p in PROJECTS] for x in next(r for r in records['phoenix'] if r['scope']=='compiled_analysis')['phases']]))
add('<h3>How the fixpoint workload shrinks</h3><p>An SCC pass processes affected components, publishes types and contracts, then module refinement can schedule another pass. A module memo hit skips an entire eligible refinement invocation after checking its previous types and recorded dependency reads. “Same previous” alone is insufficient for a hit.</p>')
for p in PROJECTS:
    rows=[]
    for r in iterations[p]:
        rows.append([str(r['iteration']),num(r['sccs']),str(r['waves']),ms(r['scc_ms']),num(r.get('modules',0)) if 'modules' in r else 'no pass',num(r.get('same_previous',0)) if 'modules' in r else '—',num(r.get('memo_hits',0)) if 'modules' in r else '—',ms(r.get('module_ms'))])
    add(f'<h3>{p.title()}</h3>'+table(['Iteration','SCC jobs','Waves','SCC elapsed','Module requests','Same previous','Memo hits','Module elapsed'],rows))
add('<p>The smaller Elixir tail still takes nine SCC passes. Compare the last six passes before targeting the main parser job alone. Per-pass module durations include scheduling, memo guards and publication, not just interpreter work.</p><h3>The parser is near the measured tail</h3>')
rows=[]
for p in PROJECTS:
    matches=[(k,v) for k,v in focused[p]['selected_scc_aggregates'].items() if k.startswith('elixir_parser:')]
    assert len(matches)==1
    key,value=matches[0]
    rows.append([p.title(),str(value['visits']),ms(value['phases_ms']['generate_constraints']),ms(value['phases_ms']['prepare_constraints']),ms(value['phases_ms']['solve_and_limit']),ms(value['worker_elapsed_ms'])])
add(table(['Parser aggregate in focused build','Visits','Generation','Preparation','Solve / limit','Worker elapsed'],rows))
add('<p>These are sums of three sequential visits to one selected worker scope. The parser finishes roughly 9–24 ms before the enclosing scheduler completes in the first three passes. Other SCCs run concurrently; their durations cannot be added to estimate the critical path. The selection threshold omits smaller SCCs.</p></section>')
add('<section id="scc"><div class="eyebrow">04 · how much solving happens?</div><h2>Generated nodes and repeated calculations</h2><p>The earlier no-skip solver census covers nine large-SCC visits per inventory. It is a selected subset, not all 53,807 / 70,876 historical schedule visits. Each visit has a newly prepared graph, so summing graph nodes counts repeated generation too.</p>')
rows=[]
for p in PROJECTS:
    d=solver[p]
    rows.append([p.title(),str(len(d['visits'])),num(sum(v['nodes'] for v in d['visits'])),num(d['total']['root_calls']),num(d['total']['atomic']),num(sum(v['work']['restarts'] for v in d['visits'])),num(d['total']['matching_roots'])])
add(table(['Inventory','Selected visits','Constraint nodes, summed','Root calls','Atomic evaluations','Conjunction restarts','Observed matching roots'],rows))
add(table(['Selected solver operation count','Phoenix','Elixir'], [[esc(key)] + [num(sum(v['work'].get(key,0) for v in solver[p]['visits'])) for p in PROJECTS] for key in ['atomic','conjunctions','disjunctions','references','cold_scopes','active_scopes','restarts']]))
add('<p>A root call solves one function reference against current bindings. Atomic evaluations include repeated traversal and flat-atomic work; flat_atomic is a subset, so never add it again. Conjunction restarts reflect dependency-triggered reevaluation. Matching roots are an earlier observational recurrence measure; they are not the current cache hit count.</p>')
for p in PROJECTS:
    rows=[]
    for i,v in enumerate(solver[p]['visits'],1):
        rows.append([str(i),num(v['functions']),num(v['nodes']),str(v['rounds']),num(v['root_calls']),num(v['work']['atomic']),num(v['work']['restarts']),'yes' if v['tried_unit'] else 'no'])
    add(f'<h3>{p.title()} · selected solver visits</h3>'+table(['Visit','Reached functions','Constraint nodes','Rounds','Root calls','Atomic evaluations','Restarts','Unit retry'],rows))
add('<p>For the 645-function parser group, generation produces 217,213 constraint nodes per recorded visit and solving needs up to 13 rounds. The Unit retry remains part of the algorithm. The 643 ordered-MFA timing label and 645 reached-function census describe different counts, including nested functions. They are not joined here into a per-operation time estimate.</p><p>Current '+link('src/constraints/tree.rs','PreparedScc::solve')+' already implements bounded whole-root replay for eligible large solves with a pure provider. It checks a successful result’s observed mutable bindings, keeps coordinator updates in their original order and never caches failures. Limits are 4,096 entries and 8 MiB of retained guard buffers per cache; this is not a process-memory bound.</p><p class="small">A counter total from before replay cannot be advertised as the work remaining after replay. '+link('reports/halving-rust-2026-10-09/solver-reuse/production/REPORT.md','Adoption and correctness evidence')+'</p></section>')
add('<section id="body"><div class="eyebrow">05 · the dynamic interpreter workload</div><h2>Bodies, clauses, guards and forwarding</h2><p>These normal-mode counters are recomputed from compressed C4 raw records. They exclude module-memo shadow invocations. Refinement, diagnostic inference and diagnostic warning mode are shown separately so they cannot be accidentally counted twice.</p>')
body_keys=[('bodies','Body evaluations'),('events.frames','Evaluator frames'),('events.clause_visits','Clause visits'),('events.clause_resumptions','Clause continuation resumptions'),('events.binder_runs','Pattern binder runs'),('events.guard_runs','Guard runs'),('events.forward_noop','Unchanged argument forwards'),('events.forward_change','Changed argument forwards'),('events.capture_attempts','Capture attempts'),('events.local_read_events','Observed mutable local reads')]
for name,label in [('refinement-inference','Module refinement'),('diagnostics-inference','Diagnostic session inference'),('diagnostics-warnings','Diagnostic warning mode')]:
    add(f'<h3>{label}</h3>'+table(['Operation','Phoenix','Elixir'],[[label]+[num(body[p][name].get(key,0)) for p in PROJECTS] for key,label in body_keys]))
add('<h3>What one evaluation actually does</h3><div class="grid"><div><p>The function worklist chooses a called function, loads its environment and binds parameter types. BodyFlow evaluates Core through continuation frames. Patterns and guards narrow type maps. Successful branches join returns; calls consult local, imported, BIF and active-contract facts.</p><p>Forwarding can change callee arguments and enqueue work. A changed local return can enqueue callers. Captures and pattern allocations also mutate state, so an equal returned type does not prove an evaluation can be skipped.</p></div><div><p>The earlier exact-input recurrence covers about half the refinement frames, but it includes changed dependencies and effects. In the admitted preceding-silent subset, C4 finds only 271 / 637 equal-input, equal-read, equal-return bodies, covering 10,517 / 30,475 frames.</p><p>Those matches are not complete replay certificates. Read-overflow and unobserved mutable reads reject candidates. More general effect replay or subexpression caching needs additional design and proof.</p></div></div>')
add('<p>Refinement no-op forwarding is '+num(body['phoenix']['refinement-inference']['events.forward_noop'])+' / '+num(body['elixir']['refinement-inference']['events.forward_noop'])+' calls. That suggested an allocation-saving experiment, but its fresh PGO screen failed Phoenix. High count alone did not predict a useful wall-time gain.</p><p class="small">Definitions and quota limits: '+link('reports/halving-rust-2026-10-09/body-read-census/REPORT.md','C4 report')+' · '+link('reports/halving-rust-2026-10-09/body-read-differences/REPORT.md','C5 read differences')+'</p></section>')
add('<section id="registry"><div class="eyebrow">06 · repeated built-in transfers</div><h2>How much built-in work repeats?</h2><p>The no-skip Registry census observes complete transfer outcomes, using bounded actor-local history. Every original request still executes. Equal retained outcomes describe recurrence, not the hit rate of any proposed cache.</p>')
add(table(['Normal-mode complete Registry observations','Phoenix','Elixir'],[[label]+[num(registry[p]['aggregate'][key]) for p in PROJECTS] for key,label in [('attempts','Reached transfer requests'),('stable_complete_repeats','Stable complete retained repeats'),('stores','Stores / nonrepeat completions'),('errors','Returned errors'),('unwinds','Unwinds')]]))
origin_names=[x['name'] for x in registry['phoenix']['origins']]
rows=[]
for origin in origin_names:
    vals=[]
    for p in PROJECTS:
        r=next(x for x in registry[p]['origins'] if x['name']==origin)
        vals += [num(r['attempts']),num(r['stable_complete_repeats'])]
    rows.append([esc(origin)]+vals)
add(table(['Origin','Phoenix calls','Phoenix repeats','Elixir calls','Elixir repeats'],rows))
add('<p>The observed repeat shares are '+f'{100*registry["phoenix"]["aggregate"]["stable_share_of_all_attempts"]:.2f}% / {100*registry["elixir"]["aggregate"]["stable_share_of_all_attempts"]:.2f}%'+'. Some predicates and constant/Any results are cheap. Hashing, comparing keys, retaining owners and cloning accepted arguments can cost more than recomputing them.</p><p>Input to a transfer is the MFA, argument types and relevant Registry context. Output is a complete TransferOutcome, including result and accepted-domain information. A private result-only caller may need less, but changing its projection must preserve the generic provider’s behavior.</p><p class="small">'+link('reports/halving-rust-2026-10-09/registry-transfer-census/REPORT.md','Registry census evidence')+'</p></section>')
add('<section id="next"><div class="eyebrow">07 · choose experiments that remove real work</div><h2>Where to investigate next</h2><p>These are investigation priorities, not claimed speedups. Use the historical phase map to choose the region, then measure its current implementation before changing it.</p>')
add(table(['Priority','Why it earns a closer measurement','Concrete next measurement'],[
 ['Parser generation / solver','217,213 generated constraint nodes and repeated solving rounds; parser sits near the first three scheduling tails.','Current source-matched sparse timings plus root hit/miss, dirty child and atomic counts in each parser visit. Separate preparation substeps.'],
 ['Body evaluation / clause handling','Tens of millions of frames. Accepted production sampling also identifies body, solver and clause leaves.','Per-module/pass frame and clause histograms with current CallWorld and mutation counters; distinguish unique static syntax from repeated frames.'],
 ['Diagnostic work','Historical serial diagnostics exceeded 1 s; current code overlaps contract and module work.','Time each branch on the current binary and reconstruct the overlap. Count diagnostic inference and warning frames separately.'],
 ['Registry transfers','1.66 M / 2.02 M reached requests with substantial recurrence. Previous simple caches lost.','Measure lookup/key/owner costs versus the original transfer by operation class. Preserve nominal context and complete outcomes.'],
 ['Memory and type algebra','Retained production peaks around 8.2–8.5 GB; allocation and substitution appear in saved samples.','Count current interned types, slots, meet/join/substitute hits and bytes allocated. Pair normal runs with CPU/instruction and memory measurements.'],
 ['Setup graph and output','Graph/schedule setup and signature projection remain visible costs.','Current graph vertex/edge inventory, SCC DFS visits, graph allocation bytes, warning sort comparisons and serialized output bytes.']]))
add('<h3>Avoid repeating rejected experiments unchanged</h3>')
add(table(['Saved experiment','Recorded outcome','Evidence'],[
 ['Lazy joined forwarding vector','Ordinary screen won, fresh PGO lost Phoenix. Retained source unchanged.',link('reports/halving-rust-2026-10-09/later-rejects/lazy-forwarding/REPORT.md','Report')],
 ['Cache immutable function definitions','Saved experiment rejected. Recheck its exact failure and measurement boundary before another metadata cache.',link('reports/halving-rust-2026-10-09/later-rejects/function-definitions/REPORT.md','Report')],
 ['Solve-owned BIF result memo','Narrow repeated provider requests did not establish an accepted gain.',link('reports/halving-rust-2026-10-09/later-rejects/bif-results/REPORT.md','Report')],
 ['Registry result-only memo','Saved result replay experiment rejected; high recurrence did not establish useful speed.',link('reports/halving-rust-2026-10-09/later-rejects/registry-inline-results/REPORT.md','Report')]]))
add('<p>The historical contract warning split spends about 51.56 / 55.48 ms constructing ordered entries and 444.68 / 543.73 ms collecting warnings. Parallelizing only the small entry-preparation step has a low ceiling. The broader contract/module overlap has already been adopted, so measure the remaining branch lengths first.</p><p class="small">'+link('reports/halving-rust-2026-10-09/focused-profiling/CONTRACTS.md','Contract split evidence')+' · '+link('reports/halving-rust-2026-10-09/post-root-sampling/REPORT.md','Accepted production sampling')+'</p></section>')
add('<section id="gaps"><div class="eyebrow">08 · make the next run complete</div><h2>What we still cannot put a number on</h2><p>These values are unknown in the selected evidence. A blank is more useful than a borrowed count from a different workload or a fabricated zero.</p>')
add(table(['Boundary','Needed counters / timers','Current attachment point'],[
 ['BEAM → Core','Per-module BEAM bytes; cache hit/miss; backend, cleaning, transport and context time; ETF bytes.','CoreCache.entry / compile and manifest emission'],
 ['Core decode','Current Core nodes/term entries/maps; validations; native/flat fallback counts; decode allocations.','core_cache_analyze::load and module_from_etf'],
 ['Program / graph','Named/nested functions, closure targets, escaping labels, vertices, edges, cleaned external edges.','Program and ClosureDependencies; SourceCallGraph before/after cleanup'],
 ['SCC generation','Core visits, captures, refolds, constraints by node kind, preparation substeps and bytes.','SccAnalyzer::analyze; GenerationOutput preparation'],
 ['Solve','Normal root hit/miss/rejected guards; recursive rounds; atomic/AND/OR visits; dirty children; substitution and map operations.','PreparedScc::solve and ConstraintTree::solve'],
 ['Refinement','Worklist pop/push/no-op; body frames and unique nodes; binder/guard visits; return changes; module memo hit/miss reasons.','FunctionState; BodyFlow; refine_module_shared'],
 ['Diagnostics / publication','Per-branch intervals; raw warnings by category; suppression/dedup counts; sorting comparisons; final JSON bytes/time.','DiagnosticInputs; core_pipeline; warning_publication; executable writer']]))
add('<h3>A measurement contract for the next benchmark</h3><ol><li>Freeze the exact input manifests, options, prior store, source hashes, binary and compiler flags. Name whether Core is fresh or prepared.</li><li>Gather sparse stage intervals with a shared origin and stable scope IDs. Do not emit one stderr record per atomic operation.</li><li>Gather counters in a separate observational run unless measured overhead is small. Store definitions, selection filters and normal/VERIFY denominators with the counts.</li><li>If instrumentation changes hot PGO control-flow hashes, retrain that exact source or explicitly label the result as an instrumented-build measurement.</li><li>Run both inventories under the shared lock with alternating baseline/candidate pairs. Preserve full signatures and warning multiplicity before considering a speed change.</li></ol><p>Existing <code>RUST_DIALYZER_TIMINGS=1</code> records phase scopes. Level 2 also emits per-SCC/publication scopes and can distort totals. Current PhaseTimings has no shared-origin timestamps or general calculation counters; those are separate additions, not facts silently available from timing logs.</p></section>')
add('<section id="evidence"><div class="eyebrow">09 · reproducible evidence</div><h2>Regenerate or inspect the numbers</h2><p>'+link('scripts/build_analysis_cost_report.py','Report generator')+' reads retained records, recomputes body counters and solver operations from raw compressed census rows, validates timing partitions and selected solver totals, and checks Registry totals against emitted reports. It writes this HTML, a complete JSON data set and a stage CSV. It runs no analyzer and changes no production code.</p><pre>python3 scripts/build_analysis_cost_report.py</pre><p><a href="data.json">Complete data and input SHA-256 manifest</a> · <a href="stages.csv">Stage table as CSV</a> · '+link('docs/beam-to-type-checks.html','Earlier illustrated process guide')+'</p>')
add(f'<p class="small">Focused timing baseline: {esc(focus_meta["base_commit"])}. Current source differs from its hash manifest in {len(changed)} of the compared source files. This report deliberately retains those historical results with their build boundary.</p>')
add('<details><summary>Source files changed since the focused timing snapshot</summary><ul>'+''.join('<li>'+link(name)+'</li>' for name in changed)+'</ul></details>')
add('<details><summary>Evidence files and content hashes</summary><ul>'+''.join('<li class="source">'+link(name)+' · <code>'+digest+'</code></li>' for name,digest in sorted(PROVENANCE.items()))+'</ul></details></section><footer class="foot">Prepared 10 October 2026. Counts are operations or objects under the stated scope, never promised removable seconds. The HTML has no external assets and works offline.</footer></main></body></html>')
OUT.mkdir(parents=True,exist_ok=True)
out_data['input_sha256']=dict(sorted(PROVENANCE.items()))
(OUT/'data.json').write_text(json.dumps(out_data,indent=2,sort_keys=True)+'\n')
(OUT/'index.html').write_text(''.join(parts))
with (OUT/'stages.csv').open('w',newline='') as f:
    writer=csv.writer(f)
    writer.writerow(['stage','title','source','line','symbol','inputs','outputs','algorithm','phoenix_ms','elixir_ms','timing_scope','timing_mark','work','measurement_boundary'])
    for s in stages:
        writer.writerow([s['id'],s['title'],s['source']['path'],s['source']['line'],s['source']['symbol'],s['inputs'],s['outputs'],s['algorithm'],s['timing']['phoenix'],s['timing']['elixir'],s['scope'],s['mark'],s['work'],s['limit']])
print(f'Built {OUT.relative_to(ROOT)}/index.html: {len(stages)} stages, {sum(len(v) for v in records.values())} timing scopes, {len(PROVENANCE)} hashed inputs. Raw body, solver and Registry counters validated.')

if __name__=='__main__':
    from html.parser import HTMLParser
    class Check(HTMLParser):
        def __init__(self): super().__init__();self.links=[];self.ids=[]
        def handle_starttag(self,tag,attrs):
            a=dict(attrs)
            if 'id' in a:self.ids.append(a['id'])
            if tag=='a':self.links.append(a['href'])
    c=Check();c.feed((OUT/'index.html').read_text())
    assert len(c.ids)==len(set(c.ids))
    for href in c.links:
        if href.startswith('#'): assert href[1:] in c.ids,href
        elif not href.startswith('http'):assert (OUT/href).exists(),href
    print(f'Validated {len(c.links)} local links and {len(c.ids)} unique section IDs.')

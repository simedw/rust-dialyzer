#!/usr/bin/env python3
"""Run the Rust analyzer on a compiled Mix project.

Usage: check_project.py PROJECT [--env dev] [--app NAME ...] [--include-app NAME ...]

Run `mix compile` in PROJECT first. The analysis covers the project's apps and
everything they depend on, taken from `_build/ENV/lib`, Elixir and OTP, with no
PLT. Warnings are reported for the project's apps only. The inputs and the full
JSON result are written to `.cache/check/<project>/`.
"""
import argparse, json, re, subprocess, sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
WARNINGS = ['warn_behaviour', 'warn_bin_construction', 'warn_callgraph', 'warn_contract_range',
            'warn_contract_syntax', 'warn_contract_types', 'warn_failing_call', 'warn_fun_app',
            'warn_map_construction', 'warn_matching', 'warn_non_proper_list', 'warn_not_called',
            'warn_return_no_exit', 'warn_undefined_callbacks', 'warn_unknown']
NIL = {'root': 0, 'terms': [{'kind': 'nil'}]}
EMPTY_PRIOR = {
    'profile': {'otp': '28', 'elixir': '1.20.4', 'erts': '16.2', 'dialyzer': '5.4',
                'term_map_order': 'native_to_list_with_iterator',
                'core_options': ['no_copt', 'to_core', 'binary', 'return_errors', 'no_inline',
                                 'strict_record_tests', 'dialyzer', 'no_spawn_compiler_process']},
    'version': 5, 'modules': [], 'source': {'kind': 'classic_plt', 'path': '', 'sha256': '', 'files': []},
    'contracts': [], 'summaries': [], 'raw_callback_groups': NIL,
    'type_tables': {'root': 0, 'terms': [{'kind': 'map', 'entries': [], 'iterator_order': []}]},
    'exported_types': NIL, 'raw_summaries': NIL, 'raw_contracts': NIL,
}


def run(cmd):
    return subprocess.run(cmd, check=True, capture_output=True, text=True).stdout.strip()


def project_apps(project):
    files = [project / 'mix.exs'] + sorted(project.glob('apps/*/mix.exs'))
    apps = [m[1] for f in files if f.is_file() for m in [re.search(r'\bapp:\s*:(\w+)', f.read_text())] if m]
    if not apps:
        sys.exit(f'no app name found in {project}/mix.exs; pass --app')
    return apps


def app_index(bases):
    """App name -> (ebin, dependencies) from every .app file; later bases win."""
    apps = {}
    for base in bases:
        for appfile in sorted(base.glob('*/ebin/*.app')):
            text = appfile.read_text(errors='replace')
            def names(key):
                m = re.search(r'\{' + key + r',\s*\[([^\]]*)\]', text, re.S)
                return [x.strip().strip("'") for x in m[1].split(',') if x.strip()] if m else []
            optional = names('optional_applications')
            deps = [d for d in names('applications') + names('included_applications') if d not in optional]
            apps[appfile.stem] = (appfile.parent, deps)
    return apps


def closure(apps, roots):
    selected, pending = set(), list(roots)
    while pending:
        app = pending.pop()
        if app in selected or app not in apps:
            continue
        selected.add(app)
        pending.extend(apps[app][1])
    return selected


def term(arena, i):
    t = arena['terms'][i]
    k = t['kind']
    if k == 'nil':
        return '[]'
    if k == 'tuple':
        return '{' + ', '.join(term(arena, j) for j in t['items']) + '}'
    if k == 'list':
        items = [arena['terms'][j] for j in t['items']]
        if arena['terms'][t['tail']]['kind'] == 'nil' and all(x['kind'] == 'integer' and 32 <= int(x['value']) < 127 for x in items):
            return json.dumps(''.join(chr(int(x['value'])) for x in items))
        tail = '' if arena['terms'][t['tail']]['kind'] == 'nil' else ' | ' + term(arena, t['tail'])
        return '[' + ', '.join(term(arena, j) for j in t['items']) + tail + ']'
    if 'value' in t:
        return str(t['value'])
    return json.dumps(t)


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument('project', type=Path)
    p.add_argument('--env', default='dev')
    p.add_argument('--app', action='append', help='app to report warnings for (default: from mix.exs)')
    p.add_argument('--include-app', action='append', default=[], help='extra app to analyze, e.g. mix or ex_unit')
    p.add_argument('--bin', type=Path, default=REPO / 'target/release/core_cache_analyze')
    a = p.parse_args()

    project = a.project.resolve()
    build = project / '_build' / a.env
    if not (build / 'lib').is_dir():
        sys.exit(f'{build}/lib not found; run `MIX_ENV={a.env} mix compile` first')
    if not a.bin.is_file():
        sys.exit(f'{a.bin} not found; run `cargo build --release --bin core_cache_analyze`')
    otp = Path(run(['erl', '-noshell', '-eval', 'io:format("~s",[code:lib_dir()]),halt().']))
    dialyzer = Path(run(['erl', '-noshell', '-eval', 'io:format("~s",[code:lib_dir(dialyzer)]),halt().'])) / 'ebin'
    elixir = Path(run(['elixir', '-e', 'IO.write(Path.dirname(to_string(:code.lib_dir(:elixir))))']))

    apps = app_index([otp, elixir, build / 'lib'])
    own = a.app or project_apps(project)
    missing = [x for x in own if x not in apps]
    if missing:
        sys.exit(f'no compiled .app for {missing} under {build}/lib')
    selected = closure(apps, own + ['erts', 'kernel', 'stdlib', 'elixir'] + a.include_app)

    beams = {}
    for app in sorted(selected):
        for beam in sorted(apps[app][0].glob('*.beam')):
            beams[beam.name] = beam
    for beam in sorted((build / 'consolidated').glob('*.beam')):
        if beam.name in beams:
            beams[beam.name] = beam
    report = sorted(b.stem for app in own for b in apps[app][0].glob('*.beam'))
    paths = sorted(str(b) for b in beams.values())

    out = REPO / '.cache/check' / project.name
    out.mkdir(parents=True, exist_ok=True)
    (out / 'prior.json').write_text(json.dumps(EMPTY_PRIOR))
    (out / 'manifest.json').write_text(json.dumps({
        'beam_files': paths, 'runtime_context': 'native', 'core_emission': 'fused',
        'dialyzer_typesig_beam': str(dialyzer / 'dialyzer_typesig.beam'),
        'dialyzer_contracts_beam': str(dialyzer / 'dialyzer_contracts.beam')}, indent=1))
    (out / 'options.json').write_text(json.dumps({
        'id': project.name, 'location': 'column', 'legal_warnings': WARNINGS, 'purged_modules': [],
        'use_contracts': True, 'solvers': [], 'emit_core': False, 'report_modules': report}, indent=1))

    print(f'analyzing {len(paths)} modules from {len(selected)} apps, reporting on {", ".join(own)}', file=sys.stderr)
    with open(out / 'result.json', 'w') as result:
        code = subprocess.run([a.bin, out / 'prior.json', out / 'manifest.json', out / 'options.json'], stdout=result).returncode
    r = json.loads((out / 'result.json').read_text())
    if code or r.get('status') != 'ok':
        sys.exit(f"analysis failed: {r.get('stage')}: {r.get('detail')}")
    arena = r['warnings']
    root = arena['terms'][arena['root']]
    rows = [] if root['kind'] == 'nil' else root['items']
    for i in rows:
        tag, where, msg = arena['terms'][i]['items']
        file, pos = arena['terms'][where]['items']
        print(f"{term(arena, file).strip(chr(34))}:{term(arena, pos).strip('{}').replace(', ', ':')}: {term(arena, msg)}")
    print(f'{len(rows)} warnings ({r["load_seconds"]:.1f}s load, {r["analysis_seconds"]:.1f}s analysis); full result in {out}/result.json', file=sys.stderr)


if __name__ == '__main__':
    main()

#!/usr/bin/env python3
"""Inventory raw compiler nodes without consuming parser errors or oracle warnings."""
import argparse
import base64
import struct
import collections
import hashlib
import json
import re
from pathlib import Path

NODE_TAGS = set('attribute function clause var atom integer float char string nil tuple cons match op call remote bin bin_element map map_field_assoc map_field_exact record record_field record_index case if receive try catch block fun named_fun lc bc mc generate b_generate m_generate zip strict_generate strict_b_generate strict_m_generate type user_type remote_type ann_type constraint typed_record_field maybe maybe_match else eof'.split())
EXP_SUPPORTED = set('var atom integer float char string nil tuple cons match op call remote bin bin_element'.split())
TYPE_SUPPORTED = set('term any none no_return integer non_neg_integer pos_integer neg_integer float number atom boolean binary bitstring list nonempty_list nil tuple union'.split())
TYPE_STRUCTURE = {'fun', 'product'}
BIFS = {(n, 2) for n in ('+', '-', '*', '/', 'div', 'rem', '==', '=:=', '/=', '=/=', '<', '>', '=<', '>=')}
BIFS |= {(n, 1) for n in ('+', '-', 'length', 'byte_size', 'bit_size', 'hd', 'tl', 'is_integer', 'is_float', 'is_number', 'is_atom', 'is_binary', 'is_list', 'is_boolean')}
OMITTED_ATTRIBUTES = ['type', 'opaque', 'record', 'behaviour', 'behavior', 'callback', 'optional_callbacks', 'export', 'export_type', 'import', 'dialyzer', 'compile', 'on_load']
DEPENDENCIES = {
    'control_flow': ['clause_refinement', 'joins', 'pattern_coverage'],
    'higher_order': ['function_types', 'closure_environment', 'dynamic_call_resolution'],
    'maps': ['key_value_relations', 'required_optional_keys', 'pattern_refinement'],
    'records': ['record_declarations', 'tuple_shapes', 'field_resolution'],
    'binary_segments': ['segment_size_types', 'binary_pattern_refinement'],
    'comprehensions': ['generator_refinement', 'control_flow', 'list_binary_map_domains'],
    'exceptions': ['exception_effects', 'normal_return_flow'],
    'processes': ['pid_port_reference_domain', 'message_flow', 'external_signatures'],
    'named_types': ['type_declarations', 'type_variable_substitution', 'module_identity'],
    'contracts': ['success_typings', 'type_intersection'],
    'polymorphic_contracts': ['named_types', 'type_variables', 'constraint_solving'],
    'guards': ['clause_refinement', 'guard_failure_flow'],
    'guard_disjunction': ['guards', 'branch_environments', 'joins'],
    'external_calls': ['external_signatures', 'module_identity'],
    'recursion': ['call_graph', 'fixed_point'],
    'numeric_relations': ['literal_value_domain', 'range_domain'],
    'opaque_types': ['named_types', 'module_identity', 'nominal_type_boundaries'],
    'behaviours': ['callback_declarations', 'exports', 'external_signatures', 'contract_comparison'],
    'warning_policy': ['diagnostic_options', 'per_function_suppression'],
    'basic_terms': ['atom_integer_float_binary_list_tuple_domain'],
}


def node(value):
    if not isinstance(value, list) or not value or not isinstance(value[0], str) or value[0] not in NODE_TAGS:
        return False
    minimum = {'nil': 2, 'eof': 2, 'type': 4, 'attribute': 4, 'function': 5, 'clause': 5}.get(value[0], 3)
    if len(value) < minimum:
        return False
    if value[0] == 'typed_record_field':
        return True
    annotation = value[1]
    return isinstance(annotation, int) or (isinstance(annotation, list) and len(annotation) == 2 and all(isinstance(x, int) for x in annotation))


def walk(value, path='$'):
    stack = [(value, path)]
    while stack:
        value, path = stack.pop()
        if not isinstance(value, list):
            continue
        if node(value):
            yield value, path
        children = [(child, f'{path}[{i}]') for i, child in enumerate(value) if isinstance(child, list)]
        stack.extend(reversed(children))


def static_call(value, module):
    if value[0] == 'op':
        return 'erlang', value[2], len(value) - 3
    if value[0] != 'call' or len(value) < 4:
        return None
    callee = value[2]
    if node(callee) and callee[0] == 'atom':
        return module, callee[2], len(value[3])
    if node(callee) and callee[0] == 'remote' and all(node(x) and x[0] == 'atom' for x in callee[2:4]):
        return callee[2][2], callee[3][2], len(value[3])
    return None


def cyclic(graph):
    # Tarjan SCCs also distinguish recursion from ordinary cross-function calls.
    index, low, stack, active, found = {}, {}, [], set(), set()
    def visit(v):
        index[v] = low[v] = len(index)
        stack.append(v)
        active.add(v)
        for w in graph[v]:
            if w not in index:
                visit(w)
                low[v] = min(low[v], low[w])
            elif w in active:
                low[v] = min(low[v], index[w])
        if low[v] == index[v]:
            component = []
            while True:
                w = stack.pop()
                active.remove(w)
                component.append(w)
                if w == v:
                    break
            if len(component) > 1 or v in graph[v]:
                found.update(component)
    for v in graph:
        if v not in index:
            visit(v)
    return sorted('/'.join(map(str, v)) for v in found)


def decode_source_term(value):
    kind = value['kind']
    if kind == 'atom':
        return value['value']
    if kind == 'integer':
        return int(value['value'])
    if kind == 'float':
        return struct.unpack('>d', bytes.fromhex(value['value']))[0]
    if kind in {'binary', 'bitstring'}:
        payload = base64.b64decode(value.get('value', value.get('bytes')))
        return payload.decode('utf-8', errors='replace')
    if kind == 'nil':
        return []
    if kind in {'tuple', 'list'}:
        items = [decode_source_term(x) for x in value['items']]
        if kind == 'list' and value['tail']['kind'] != 'nil':
            return items + [decode_source_term(value['tail'])]
        return items
    if kind == 'map':
        return [[decode_source_term(k), decode_source_term(v)] for k, v in value['entries']]
    raise ValueError(f'unknown tagged source term kind {kind}')


class FlatSequence(list):
    """A shallow sequence of term references; children decode only when accessed."""
    def __init__(self, table, references):
        self.table = table
        self.references = references

    def __len__(self):
        return len(self.references)

    def __getitem__(self, index):
        if isinstance(index, slice):
            return FlatSequence(self.table, self.references[index])
        return self.table.value(self.references[index])

    def __iter__(self):
        return (self.table.value(reference) for reference in self.references)


class FlatTerms:
    def __init__(self, terms):
        self.terms = terms
        self.cache = {}
        for index, term in enumerate(terms):
            references = []
            if term['kind'] in {'tuple', 'list'}:
                references.extend(term['items'])
                if term['kind'] == 'list':
                    references.append(term['tail'])
            elif term['kind'] == 'map':
                references.extend(ref for pair in term['entries'] for ref in pair)
            if any(not isinstance(ref, int) or not 0 <= ref < index for ref in references):
                raise ValueError(f'term {index} contains an invalid or non-postorder reference')

    def value(self, reference):
        if not isinstance(reference, int) or not 0 <= reference < len(self.terms):
            raise ValueError(f'invalid term reference {reference}')
        if reference in self.cache:
            return self.cache[reference]
        term = self.terms[reference]
        kind = term['kind']
        if kind in {'tuple', 'list'}:
            references = term['items']
            if kind == 'list' and self.terms[term['tail']]['kind'] != 'nil':
                references = references + [term['tail']]
            value = FlatSequence(self, references)
        elif kind == 'map':
            value = [FlatSequence(self, pair) for pair in term['entries']]
        else:
            value = decode_source_term(term)
        self.cache[reference] = value
        return value


def core_nodes(value):
    stack = [value]
    while stack:
        value = stack.pop()
        if not isinstance(value, dict) or 'groups' not in value:
            raise ValueError('expected a Core node with child groups')
        yield value
        children = [child for group in value['groups'] for child in group]
        if value['kind'] == 'map':
            if 'map_arg' not in value:
                raise ValueError('Core map lacks map_arg')
            children.append(value['map_arg'])
        stack.extend(reversed(children))


def flat_core_nodes(module):
    nodes = module['nodes']
    terms = module['terms']
    for index, value in enumerate(nodes):
        references = [ref for group in value['groups'] for ref in group]
        if value['kind'] == 'map':
            if 'map_arg' not in value:
                raise ValueError(f'Core map {index} lacks map_arg')
            references.append(value['map_arg'])
        if any(not isinstance(ref, int) or not 0 <= ref < index for ref in references):
            raise ValueError(f'Core node {index} contains an invalid or non-postorder reference')
        for field in ('annotations', 'value'):
            if field in value and (not isinstance(value[field], int) or not 0 <= value[field] < len(terms)):
                raise ValueError(f'Core node {index} has invalid term reference in {field}')
    stack = [module['core']]
    while stack:
        reference = stack.pop()
        if not isinstance(reference, int) or not 0 <= reference < len(nodes):
            raise ValueError(f'invalid Core node reference {reference}')
        value = nodes[reference]
        yield reference, value
        children = [ref for group in value['groups'] for ref in group]
        if value['kind'] == 'map':
            children.append(value['map_arg'])
        stack.extend(reversed(children))


def lexical_attributes(case, upstream_root):
    evidence, missing = [], []
    for filename in case['files']:
        path = upstream_root / filename
        if not path.is_file():
            missing.append(str(path))
            continue
        text = path.read_text(errors='replace')
        pattern = r'^\s*-([a-z_]+)\b(?:\s|\()' if path.suffix == '.erl' else r'^\s*@([a-z_]+)\b'
        found = []
        for number, line in enumerate(text.splitlines(), 1):
            match = re.match(pattern, line)
            if match and match.group(1) in set(OMITTED_ATTRIBUTES) | {'typep', 'spec'}:
                found.append({'attribute': match.group(1), 'line': number})
        evidence.append({'file': filename, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest(), 'matches': found})
    return evidence, missing


def scan(case, path, upstream_root):
    original_path = path
    if path.with_name('core.json').is_file():
        path = path.with_name('core.json')
    result = {'id': case['id'], 'kind': case['kind'], 'program_file': str(original_path),
              'coverage': 'cached_runtime_forms_only', 'missing_source_metadata': OMITTED_ATTRIBUTES,
              'features': [], 'groups': [], 'missing_ir': [], 'counts': {}}
    if not path.is_file():
        result.update(coverage='missing_program', missing_ir=['missing_exported_program'])
        return result
    data = json.loads(path.read_text())
    if data.get('version') not in {1, 2, 3}:
        raise ValueError(f'unsupported export version in {path}')
    result['input_version'] = data['version']
    result['input_file'] = str(path)
    result['source_metadata_complete'] = all('source_forms' in m for m in data['modules'])
    core_counts = collections.Counter()
    core_map_args = 0
    unique_core_nodes = 0
    unreachable_core_nodes = 0
    source_term_table_size = 0
    for module in data['modules']:
        if data['version'] == 3:
            table = FlatTerms(module['terms'])
            source_term_table_size += len(module['terms'])
            module['source_forms'] = table.value(module['source_forms'])
            seen = set()
            for reference, value in flat_core_nodes(module):
                core_counts[value['kind']] += 1
                core_map_args += value['kind'] == 'map'
                seen.add(reference)
            unique_core_nodes += len(seen)
            unreachable_core_nodes += len(module['nodes']) - len(seen)
        else:
            if isinstance(module.get('source_forms'), dict):
                module['source_forms'] = decode_source_term(module['source_forms'])
            if 'core' in module:
                for value in core_nodes(module['core']):
                    core_counts[value['kind']] += 1
                    core_map_args += value['kind'] == 'map'
                    unique_core_nodes += 1
    result['core_reachable_unique_nodes'] = unique_core_nodes
    result['core_unreachable_nodes'] = unreachable_core_nodes
    result['source_term_table_size'] = source_term_table_size
    result['core_node_counts'] = dict(sorted(core_counts.items()))
    result['core_node_count'] = sum(core_counts.values())
    result['core_map_arg_nodes_visited'] = core_map_args
    result['core_traversal_complete'] = all('core' in m for m in data['modules']) and unreachable_core_nodes == 0
    result['program_sha256'] = hashlib.sha256(path.read_bytes()).hexdigest()
    result['module_count'] = len(data['modules'])
    if all('source_forms' in m for m in data['modules']):
        result['coverage'] = 'full_source_forms'
        result['missing_source_metadata'] = []
    counters = {key: collections.Counter() for key in ('nodes', 'types', 'attributes', 'operators', 'guard_nodes')}
    features, groups, gaps, samples = set(), set(), set(), {}
    functions = {(m['name'], f[2], f[3]) for m in data['modules'] for f in m.get('source_forms', m.get('forms', [])) if f[0] == 'function'}
    graph = {f: set() for f in functions}
    external = set()
    def mark(feature, group=None, gap=None, loc=None):
        features.add(feature)
        if group:
            groups.add(group)
        if gap:
            gaps.add(gap)
            if loc is not None:
                samples.setdefault(gap, loc)
    for module in data['modules']:
        name = module['name']
        for form_index, form in enumerate(module.get('source_forms', module.get('forms', []))):
            origin = f'{name}.forms[{form_index}]'
            for value, loc in walk(form, origin):
                tag = value[0]
                counters['nodes'][tag] += 1
                if tag == 'attribute':
                    counters['attributes'][value[2]] += 1
                    mark(f'attribute:{value[2]}')
                    declaration_groups = {'type': 'named_types', 'opaque': 'opaque_types', 'record': 'records',
                        'behaviour': 'behaviours', 'behavior': 'behaviours', 'callback': 'behaviours',
                        'optional_callbacks': 'behaviours', 'dialyzer': 'warning_policy'}
                    if value[2] in declaration_groups:
                        mark(f'attribute:{value[2]}', declaration_groups[value[2]], f'attribute_semantics:{value[2]}', loc)
                    if value[2] == 'spec':
                        mark('contracts', 'contracts')
                        if len(value[3][1]) > 1:
                            mark('overloaded_contracts', 'contracts', 'overloaded_specs', loc)
                elif tag == 'type':
                    typ = value[2]
                    counters['types'][typ] += 1
                    mark(f'type:{typ}')
                    if typ not in TYPE_SUPPORTED | TYPE_STRUCTURE:
                        mark(f'type:{typ}', gap=f'type:{typ}', loc=loc)
                    outer_contract = form[0] == 'attribute' and form[2] in {'spec', 'callback'} and bool(re.fullmatch(r'\[3\]\[1\]\[\d+\]', loc[len(origin):]))
                    if typ == 'fun' and not outer_contract:
                        mark('function_types', 'higher_order', 'nested_function_type', loc)
                    if typ == 'tuple' and value[3] == 'any':
                        mark('unbounded_tuple', gap='unbounded_tuple_type', loc=loc)
                    if typ in {'bounded_fun', 'constraint'}:
                        mark('polymorphic_contracts', 'polymorphic_contracts', 'type_variables_and_constraints', loc)
                    if typ in {'pid', 'port', 'reference'}:
                        mark('process_domain', 'processes', 'process_types', loc)
                    if typ in {'map', 'map_field_assoc', 'map_field_exact'}:
                        mark('maps', 'maps', 'map_types', loc)
                    if typ == 'record':
                        mark('records', 'records', 'record_types_and_declarations', loc)
                elif tag in {'ann_type', 'remote_type', 'user_type'}:
                    mark(f'type_node:{tag}')
                    if tag != 'ann_type':
                        mark('named_types', 'named_types', tag, loc)
                elif tag not in {'function', 'clause', 'eof'}:
                    mark(f'expression:{tag}')
                    if tag not in EXP_SUPPORTED:
                        mark(f'expression:{tag}', gap=f'expression:{tag}', loc=loc)
                if tag in {'case', 'if', 'receive', 'maybe', 'maybe_match'}:
                    mark('control_flow', 'control_flow')
                if tag in {'fun', 'named_fun'}:
                    mark('higher_order', 'higher_order')
                if tag in {'map', 'map_field_assoc', 'map_field_exact'}:
                    mark('maps', 'maps')
                if tag in {'record', 'record_field', 'record_index', 'typed_record_field'}:
                    mark('records', 'records')
                if tag in {'try', 'catch'}:
                    mark('exceptions', 'exceptions')
                if tag == 'receive':
                    mark('processes', 'processes')
                if tag in {'lc', 'bc', 'mc', 'generate', 'b_generate', 'm_generate', 'zip'}:
                    mark('comprehensions', 'comprehensions')
                if tag == 'bin' and any(not node(x[2]) or x[2][0] != 'string' for x in value[2]):
                    mark('binary_segments', 'binary_segments', 'non_literal_binary', loc)
                if tag == 'op':
                    counters['operators'][value[2]] += 1
                    if value[2] in {'<', '>', '=<', '>=', '==', '=:=', '/=', '=/='}:
                        mark('numeric_or_term_relations', 'numeric_relations', 'literal_and_range_precision', loc)
                    if (value[2], len(value) - 3) not in BIFS:
                        mark('operator_domain', gap=f'operator:{value[2]}/{len(value)-3}', loc=loc)
                if tag == 'call' and static_call(value, name) is None:
                    mark('dynamic_calls', 'higher_order', 'dynamic_call_resolution', loc)
                if tag == 'clause':
                    guard_groups = value[3]
                    if guard_groups:
                        mark('guards', 'guards')
                        for guard, _ in walk(guard_groups):
                            counters['guard_nodes'][guard[0]] += 1
                    if len(guard_groups) > 1:
                        mark('guard_disjunction', 'guard_disjunction', 'disjunctive_guards', loc)
            if form[0] == 'function':
                caller = name, form[2], form[3]
                for value, loc in walk(form):
                    target = static_call(value, name)
                    if target and target[0] == 'erlang' and target[1] in {'spawn', 'spawn_link', 'spawn_monitor', 'self', 'send', 'monitor', 'link', 'make_ref', 'open_port'}:
                        mark('process_operations', 'processes', 'process_effects_and_signatures', origin + loc)
                    if target and target[0] == 'erlang' and target[1] == 'apply':
                        mark('dynamic_apply', 'higher_order', 'dynamic_call_resolution', origin + loc)
                    if target and target[0] == 'erlang' and target[1] == '!':
                        mark('message_send', 'processes', 'message_flow', origin + loc)
                    if target in functions:
                        graph[caller].add(target)
                    elif target:
                        external.add('/'.join(map(str, target)))
                        if target[0] != 'erlang' or (target[1], target[2]) not in BIFS:
                            mark('external_calls', 'external_calls', 'external_signatures', origin + loc)
    recursions = cyclic(graph)
    if recursions:
        mark('recursion', 'recursion')
    lexical, missing_sources = lexical_attributes(case, upstream_root)
    result['lexical_evidence'] = lexical
    result['missing_source_files'] = missing_sources
    lexical_groups = set()
    for source in lexical:
        for match in source['matches']:
            attribute = match['attribute']
            features.add(f'lexical_attribute:{attribute}')
            assignments = {'type': 'named_types', 'typep': 'named_types', 'opaque': 'opaque_types',
                'record': 'records', 'behaviour': 'behaviours', 'behavior': 'behaviours',
                'callback': 'behaviours', 'optional_callbacks': 'behaviours', 'dialyzer': 'warning_policy',
                'spec': 'contracts'}
            if attribute in assignments:
                lexical_groups.add(assignments[attribute])
            if attribute in OMITTED_ATTRIBUTES and result['coverage'] == 'cached_runtime_forms_only':
                gaps.add(f'exported_metadata:{attribute}')
    result['lexical_only_groups'] = sorted(lexical_groups - groups)
    result['compiler_groups'] = sorted(groups)
    groups |= lexical_groups
    if not groups:
        groups.add('basic_terms')
    result.update(features=sorted(features), groups=sorted(groups), missing_ir=sorted(gaps),
                  gap_examples=dict(sorted(samples.items())), counts={k: dict(sorted(v.items())) for k, v in counters.items()},
                  external_calls=sorted(external), recursive_functions=recursions,
                  dependencies=sorted({d for g in groups for d in DEPENDENCIES[g]}))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--inventory', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--engine-root', type=Path)
    parser.add_argument('--upstream-root', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    inventory = json.loads(args.inventory.read_text())
    # The report is only an artifact locator. Never consume warning/status/error fields.
    report = json.loads(args.report.read_text())
    locators = {c['id']: Path(c['oracle_file']).parent / 'program.json' for c in report['cases']}
    registered = inventory['semantic_cases']
    if len(locators) != len(report['cases']):
        raise ValueError('duplicate report case IDs')
    if set(locators) != {c['id'] for c in registered}:
        raise ValueError('report and inventory case identities differ')
    upstream_root = args.upstream_root or args.inventory.resolve().parent.parent / '.cache' / 'upstream'
    cases = [scan(c, locators[c['id']], upstream_root) for c in registered]
    groups = {group: [c['id'] for c in cases if group in c['groups']] for group in DEPENDENCIES}
    overlap = {a: {b: len(set(groups[a]) & set(groups[b])) for b in groups if b != a and set(groups[a]) & set(groups[b])} for a in groups}
    engine_root = args.engine_root or args.inventory.resolve().parent.parent
    engine_hashes = {str(p.relative_to(engine_root)): hashlib.sha256(p.read_bytes()).hexdigest()
        for p in (engine_root / 'src' / 'ir.rs', engine_root / 'src' / 'engine.rs') if p.is_file()}
    coverage_counts = collections.Counter(c['coverage'] for c in cases)
    core_counts = collections.Counter()
    for case in cases:
        core_counts.update(case.get('core_node_counts', {}))
    output = {'coverage_counts': dict(coverage_counts), 'core_node_counts': dict(sorted(core_counts.items())),
              'core_cases': sum(c.get('core_traversal_complete', False) for c in cases), 'engine_source_sha256': engine_hashes, 'version': 1, 'case_count': len(cases), 'scanned_count': sum(c['coverage'] != 'missing_program' for c in cases),
              'input_policy': 'report contains artifact paths only; no diagnostic data influences features',
              'export_coverage': {'kind': 'per_case', 'missing_attribute_kinds': OMITTED_ATTRIBUTES,
                  'reason': 'beam_export.exs retains file,module,spec and runtime functions only'},
              'capability_baseline': {'expression_nodes': sorted(EXP_SUPPORTED), 'type_names': sorted(TYPE_SUPPORTED),
                  'limitations': ['syntactic acceptance is not semantic parity', 'type fun/product are only contract structure',
                      'literal-only binaries', 'single guard conjunction', 'static calls with finite builtin signatures',
                      'integer ranges and literal values are collapsed']},
              'group_dependencies': DEPENDENCIES, 'groups': groups, 'group_overlap_counts': overlap,
              'missing_ir_case_counts': dict(sorted(collections.Counter(g for c in cases for g in c['missing_ir']).items())),
              'cases': cases}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(output, indent=2, sort_keys=True) + '\n')
    print(json.dumps({'case_count': len(cases), 'scanned_count': output['scanned_count'],
        'group_counts': {k: len(v) for k, v in groups.items()}, 'output': str(args.output)}, sort_keys=True))


if __name__ == '__main__':
    main()

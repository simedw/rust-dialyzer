"""Compare independently extracted source forms and attributes to the OTP oracle."""
from functools import lru_cache
import json
from pathlib import Path
import sys

def decoder(module):
    terms = module['terms']
    @lru_cache(None)
    def at(index):
        term = terms[index]
        kind = term['kind']
        if kind in ('atom', 'integer', 'float', 'binary'):
            return (kind, term['value'])
        if kind == 'bitstring':
            return (kind, term['bytes'], term['bits'])
        if kind == 'nil':
            return ('list', (), None)
        if kind == 'tuple':
            return (kind, tuple(at(i) for i in term['items']))
        if kind == 'list':
            items = tuple(at(i) for i in term['items'])
            tail = at(term['tail'])
            if tail[0] == 'list':
                return ('list', items + tail[1], tail[2])
            return ('list', items, tail)
        if kind == 'map':
            return (kind, tuple(sorted((at(k), at(v)) for k, v in term['entries'])))
        raise ValueError(kind)
    return at

base = Path(sys.argv[1])
seed = json.loads((Path(sys.argv[2]) if len(sys.argv)>2 else base/'native-seed.json').read_text())
oracle = json.loads((base/'oracle-core.json').read_text())['modules'][0]
left, right = decoder(seed), decoder(oracle)
assert left(seed['source_forms']) == right(oracle['source_forms']), 'source forms differ'
expected = []
root = oracle['nodes'][oracle['core']]
for key, value in zip(root['groups'][2][::2], root['groups'][2][1::2]):
    k, v = oracle['nodes'][key], oracle['nodes'][value]
    expected.append((right(k['value'])[1], right(v['value']), right(v['annotations'])))
actual = [(a['name'], left(a['value']), left(a['annotations'])) for a in seed['attributes']]
assert actual == expected, 'source attribute payloads or locations differ'
report = dict(source_forms_equal=True, attribute_payloads_and_locations_equal=True,
              attributes=[a['name'] for a in seed['attributes']], count=len(actual),
              oracle_core_bodies_used_as_native_input=False)
(base/'metadata-verification.json').write_text(json.dumps(report, indent=2)+'\n')
print(json.dumps(report))

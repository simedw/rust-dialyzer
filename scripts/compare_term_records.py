#!/usr/bin/env python3
"""Compare exported term records independently of arena node numbering."""
import hashlib
import itertools
import json
import sys
from pathlib import Path

def fingerprint(arena):
    terms = arena['terms']
    memo = {}
    def visit(index):
        if index in memo:
            return memo[index]
        node = terms[index]
        kind = node['kind']
        if kind == 'tuple':
            value = [kind, [visit(i) for i in node['items']]]
        elif kind == 'list':
            items = []
            cursor = index
            while terms[cursor]['kind'] == 'list':
                segment = terms[cursor]
                items.extend(visit(i) for i in segment['items'])
                cursor = segment['tail']
            value = [kind, items, visit(cursor)]
        elif kind == 'map':
            value = [kind, [[visit(k),visit(v)] for k,v in node['entries']], node.get('iterator_order')]
        else:
            value = node
        memo[index] = hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',',':')).encode()).hexdigest()
        return memo[index]
    return visit(arena['root'])

count = 0
with Path(sys.argv[1]).open() as left, Path(sys.argv[2]).open() as right:
    for a,b in itertools.zip_longest(left,right):
        assert a is not None and b is not None, 'record counts differ'
        a,b = json.loads(a),json.loads(b)
        ra,rb = a.pop('record'),b.pop('record')
        assert a == b, ('record identity differs',a,b)
        assert fingerprint(ra) == fingerprint(rb), ('term differs',a)
        count += 1
print(json.dumps({'term_records_exact':True,'records':count}))

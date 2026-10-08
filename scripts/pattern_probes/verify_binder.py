#!/usr/bin/env python3
"""Compare every captured original OTP binder outcome, with no missing-row skip.
Native numeric map identities and Rust decimal map identities are normalized only
at this transport boundary. The native exception arena must be exactly error /
function_clause before matching the typed Rust constructor; other errors fail.
"""
import argparse, collections, hashlib, json, pathlib, sys

def load(path):
    return [json.loads(line) for line in pathlib.Path(path).read_text().splitlines()]

def warning_count(arena):
    term = arena['terms'][arena['root']]
    if term['kind'] == 'nil': return 0
    if term['kind'] == 'list' and arena['terms'][term['tail']]['kind'] == 'nil': return len(term['items'])
    raise ValueError('warning list is not a proper list')

def native(row):
    row = dict(row)
    if row['status'] == 'exception':
        if row['class'] != 'error' or row['reason'] != {'root':0,'terms':[{'kind':'atom','value':'function_clause'}]}:
            raise ValueError(f'Uncovered native exception: {row}')
        row.pop('class');row.pop('reason');row['error']='Native(FunctionClause)'
    if 'warnings' in row: row['warning_count'] = warning_count(row.pop('warnings'))
    if 'map' in row:
        row['map']=dict(row['map'])
        row['map']['modified']=list(map(str,row['map']['modified']))
        row['map']['substitutions']={k:str(v) for k,v in row['map']['substitutions'].items()}
    return row

def compare(args):
    ns,rs=load(args.native),load(args.rust)
    if len(ns)!=len(rs):raise ValueError(f'row counts differ: {len(ns)} / {len(rs)}')
    counter=collections.Counter(); failures=[];modules=0
    for n,r in zip(ns,rs):
        if n.get('id')!=r.get('id'):raise ValueError('row identities differ')
        if args.corpus:
            if len(n['modules'])!=len(r['modules']):raise ValueError('module counts differ')
            for nm,rm in zip(n['modules'],r['modules']):
                modules+=1
                if nm['module']!=rm['module'] or len(nm['results'])!=len(rm['results']):raise ValueError('module/pattern identities differ')
                for a,b in zip(nm['results'],rm['results']):
                    counter[a['status']]+=1
                    a=native(a)
                    if a!=b:failures.append({'case':n['id'],'module':nm['module'],'native':a,'rust':b})
        else:
            counter[n['status']]+=1
            a=native(n)
            if a!=r:failures.append({'native':a,'rust':r})
    report={'rows':len(ns),'modules':modules,'outcomes':sum(counter.values()),'status':dict(counter),'mismatches':len(failures),'sha256':{str(p):hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest() for p in [args.native,args.rust]}}
    print(json.dumps(report,sort_keys=True))
    if failures:
        pathlib.Path(args.report).write_text(json.dumps(failures,indent=2)+'\n')
        print(f'first mismatch: {str(failures[0])[:2000]}',file=sys.stderr)
        return 1
    return 0

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('native');p.add_argument('rust');p.add_argument('--corpus',action='store_true');p.add_argument('--report',default='data/mismatches.json')
    sys.exit(compare(p.parse_args()))

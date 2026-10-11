#!/usr/bin/env python3
"""Regenerate the pinned auto-import table from OTP's erl_internal.erl source."""
import argparse,pathlib,re
p=argparse.ArgumentParser();p.add_argument('source',type=pathlib.Path);p.add_argument('output',type=pathlib.Path);a=p.parse_args()
text=a.source.read_text()
pairs=re.findall(r'^bif\((\w+),\s*(\d+)\)\s*->\s*true;',text,re.M)
if len(pairs)<100:raise SystemExit('unexpected OTP BIF source schema')
lines=['//! Auto-imported BIF table from OTP 28.3 erl_internal.erl.','//! Apache-2.0; Copyright Ericsson AB and contributors.','pub fn is_auto_bif(name: &str, arity: usize) -> bool {','    matches!((name, arity),']
lines+=['        '+('  ' if n==0 else '| ')+f'("{name}", {arity})' for n,(name,arity) in enumerate(pairs)]
lines+=['    )','}'];a.output.write_text('\n'.join(lines)+'\n');print(f'{len(pairs)} auto-imported BIF signatures')

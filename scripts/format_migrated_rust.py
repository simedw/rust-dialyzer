#!/usr/bin/env python3
"""Format migration hunks only when the resulting Rust tokens stay identical."""
import argparse
import difflib
import json
from pathlib import Path
import re
import subprocess

def tokens(source):
    pattern = r'''//[^\n]*|/\*.*?\*/|r(\#+)".*?"\1|'(?:\\u\{[0-9a-fA-F_]+\}|\\.|[^'\\])'|"(?:\\.|[^"\\])*"|\w+|[^\s]'''
    return [match[0] for match in re.finditer(pattern, source, flags=re.S)
            if not match[0].startswith(('//', '/*'))]


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('inventory', type=Path)
    parser.add_argument('before', type=Path)
    args = parser.parse_args()
    for name in json.loads(args.inventory.read_text()):
        path = Path(name)
        original = (args.before / name).read_text().splitlines(keepends=True)
        source = path.read_text()
        current = source.splitlines(keepends=True)
        ranges = [(b0, b1) for tag, a0, a1, b0, b1 in
                  difflib.SequenceMatcher(None, original, current, autojunk=False).get_opcodes()
                  if tag != 'equal']
        result = subprocess.run(['rustfmt', '--edition', '2024', '--config', 'skip_children=true'],
                                input=source, capture_output=True, text=True, check=True)
        formatted = result.stdout.splitlines(keepends=True)
        changes = [(a0, a1, formatted[b0:b1]) for tag, a0, a1, b0, b1 in
                   difflib.SequenceMatcher(None, current, formatted, autojunk=False).get_opcodes()
                   if tag != 'equal' and any(a0 <= hi and a1 >= lo for lo, hi in ranges)]
        for start, end, replacement in reversed(changes):
            current[start:end] = replacement
        proposed = ''.join(current)
        if tokens(source) != tokens(proposed):
            print(name, 'partial formatting rejected; source retained')
        else:
            path.write_text(proposed)
            print(name, 'tokens preserved')

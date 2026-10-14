#!/usr/bin/env python3
"""Migrate Core external-function payloads, preserving surrounding source text.

Defaults to an inventory. --apply writes backups before any source edits.
This migration recognizes only Core's qualified names; unrelated hash keys
and runtime capture representations keep their own layouts.
"""
import argparse
import json
from pathlib import Path
import re
import shutil

ROOT = Path(__file__).resolve().parents[1]
ALIASES = {'src/native_runtime_hash.rs', 'src/beam_frontend/clean_view.rs'}
GLOB_VARIANTS = {'src/bin/whole_module_corpus_probe.rs'}


def mask(source):
    pattern = r'''//[^\n]*|/\*.*?\*/|r(\#+)".*?"\1|'(?:\\u\{[0-9a-fA-F_]+\}|\\.|[^'\\])'|"(?:\\.|[^"\\])*"'''
    return re.sub(pattern, lambda m: ''.join('\n' if c == '\n' else ' ' for c in m[0]), source, flags=re.S)


def closing(masked, start):
    depth = 0
    for i in range(start, len(masked)):
        if masked[i] == '{':
            depth += 1
        elif masked[i] == '}':
            depth -= 1
            if depth == 0:
                return i
    raise ValueError('unclosed payload')


def expression_end(masked, start):
    depth = 0
    for i in range(start, len(masked)):
        c = masked[i]
        if c in '([{':
            depth += 1
        elif c in ')]}':
            if depth == 0:
                return i
            depth -= 1
        elif c == ',' and depth == 0:
            return i
    raise ValueError('unclosed match arm')


def edits(path, source):
    masked = mask(source)
    changes = []
    for match in re.finditer(r'(?:(ErlangTerm|T)::)?ExternalFun\s*\{', masked):
        if match[1] is None and str(path.relative_to(ROOT)) not in GLOB_VARIANTS:
            continue
        if match[1] == 'T' and str(path.relative_to(ROOT)) not in ALIASES:
            continue
        start = match.end() - 1
        end = closing(masked, start)
        fields = source[start + 1:end]
        after = masked[end + 1:]
        prefix = (match[1] + '::' if match[1] else '') + 'ExternalFun'
        if after.lstrip().startswith('if '):
            arm_end = masked.index('=>', end + 1)
            old = source[end + 1:arm_end]
            new = re.sub(r'\b(module|function)\b', r'external_fun_payload.\1', old)
            changes.append((match.start(), arm_end, prefix + '(external_fun_payload)' + new))
        elif after.lstrip().startswith('=>'):
            if fields.strip() == '..':
                changes.append((match.start(), end + 1, prefix + '(_)'))
                continue
            body = end + 1 + after.index('=>') + 2
            while masked[body].isspace():
                body += 1
            declaration = 'let crate::core::ExternalFun {' + fields + '} = external_fun_payload.as_ref();'
            if str(path.relative_to(ROOT)) == 'src/beam_frontend/elixir.rs':
                declaration = declaration.replace('external_fun_payload.as_ref()', '*external_fun_payload')
            changes.append((match.start(), end + 1, prefix + '(external_fun_payload)'))
            if masked[body] == '{':
                changes.append((body + 1, body + 1, '\n' + declaration + '\n'))
            else:
                arm_end = expression_end(masked, body)
                changes.append((body, arm_end, '{ ' + declaration + ' ' + source[body:arm_end] + ' }'))
        else:
            changes.append((match.start(), end + 1,
                            prefix + '(Box::new(crate::core::ExternalFun {' + fields + '}))'))
    if str(path.relative_to(ROOT)).startswith('src/bin/'):
        changes = [(start, end, replacement.replace('crate::core::', 'rust_dialyzer_poc::core::'))
                   for start, end, replacement in changes]
    return changes


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apply', action='store_true')
    parser.add_argument('--backup', type=Path, required=True)
    args = parser.parse_args()
    inventory = {}
    for path in sorted((ROOT / 'src').rglob('*.rs')):
        source = path.read_text()
        changes = edits(path, source)
        if not changes:
            continue
        inventory[str(path.relative_to(ROOT))] = len(changes)
        if args.apply:
            backup = args.backup / path.relative_to(ROOT)
            backup.parent.mkdir(parents=True, exist_ok=True)
            if backup.exists():
                raise ValueError(f'backup already exists: {backup}')
            shutil.copyfile(path, backup)
            for start, end, replacement in sorted(changes, reverse=True):
                source = source[:start] + replacement + source[end:]
            path.write_text(source)
    print(json.dumps(inventory, indent=2))


if __name__ == '__main__':
    main()

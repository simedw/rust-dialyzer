#!/usr/bin/env python3
"""Inventory or migrate the two wide CoreExpr payloads. Back up edits first."""
import argparse
import json
from pathlib import Path
import re
import shutil

from outline_external_fun import ROOT, mask, closing, expression_end


def statement_end(masked, start):
    depth = 0
    for i in range(start, len(masked)):
        if masked[i] in '([{':
            depth += 1
        elif masked[i] in ')]}':
            depth -= 1
        elif masked[i] == ';' and depth == 0:
            return i + 1
    raise ValueError('unclosed let statement')


def if_body(masked, start):
    depth = 0
    for i in range(start, len(masked)):
        if masked[i] == '{' and depth == 0:
            return i + 1
        if masked[i] in '([':
            depth += 1
        elif masked[i] in ')]':
            depth -= 1
    raise ValueError('unclosed if-let body')


def edits(path, source):
    masked = mask(source)
    changes = []
    for match in re.finditer(r'(CoreExpr|Self)::(Try|Module)\s*\{', masked):
        relative = str(path.relative_to(ROOT))
        if match[1] == 'Self' and relative not in {'src/core.rs', 'src/core/wire.rs'}:
            continue
        end = closing(masked, match.end() - 1)
        fields = source[match.end():end]
        after = masked[end + 1:]
        stripped = after.lstrip()
        prefix = match[1] + '::' + match[2]
        payload = match[2].lower() + '_payload'
        namespace = 'rust_dialyzer_poc::core::' if relative.startswith('src/bin/') else 'crate::core::'
        typ = namespace + match[2] + 'Expr'
        line = source[source.rfind('\n', 0, match.start()) + 1:match.start()]
        indent = re.match(r'\s*', line)[0]
        declaration = 'let ' + typ + ' {' + fields + '} = ' + payload + '.as_ref();'
        if relative == 'src/constraints/generation.rs' and match[2] == 'Try':
            declaration = declaration.replace(payload + '.as_ref()', '*' + payload)
        if stripped.startswith('if '):
            arrow = masked.index('=>', end + 1)
            guard = source[end + 1:arrow]
            name = re.search(r'\bname\s*:\s*(\w+)', fields)
            bound = name[1] if name else 'name'
            guard = re.sub(r'\*\s*' + bound + r'\b', payload + '.name', guard)
            changes.append((match.start(), arrow, prefix + '(' + payload + ')' + guard))
        elif stripped.startswith('=>'):
            if fields.strip() == '..':
                changes.append((match.start(), end + 1, prefix + '(_)'))
                continue
            body = end + 1 + after.index('=>') + 2
            while masked[body].isspace():
                body += 1
            changes.append((match.start(), end + 1, prefix + '(' + payload + ')'))
            if masked[body] == '{':
                changes.append((body + 1, body + 1, '\n' + indent + '    ' + declaration + '\n'))
            else:
                stop = expression_end(masked, body)
                changes.append((body, stop, '{ ' + declaration + ' ' + source[body:stop] + ' }'))
        elif stripped.startswith('='):
            changes.append((match.start(), end + 1, prefix + '(' + payload + ')'))
            if re.search(r'\bif\s+let\s*$', line):
                stop = if_body(masked, end + 1)
                changes.append((stop, stop, '\n' + indent + '    ' + declaration + '\n'))
            else:
                stop = statement_end(masked, end + 1)
                changes.append((stop, stop, '\n' + indent + declaration))
        else:
            changes.append((match.start(), end + 1,
                            prefix + '(Box::new(' + typ + ' {' + fields + '}))'))
    return changes


if __name__ == '__main__':
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

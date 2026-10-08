from pathlib import Path

def directory_entries(path):
    entries = []
    for child in sorted(Path(path).iterdir()):
        try:
            kind = 'directory' if child.is_dir() else 'regular' if child.is_file() else 'other'
        except OSError:
            kind = 'error'
        entries.append([child.name, kind])
    return entries

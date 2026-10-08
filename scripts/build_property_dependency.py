#!/usr/bin/env python3
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    source = json.loads((ROOT / "corpus/sources.json").read_text())["proper"]
    directory = ROOT / ".cache/upstream/proper"
    actual = subprocess.check_output(
        ["git", "-C", directory, "rev-parse", "HEAD"], text=True
    ).strip()
    if actual != source["revision"]:
        raise RuntimeError(
            "PropEr checkout differs from pinned source; run corpus.py fetch"
        )
    changed = subprocess.check_output(
        ["git", "-C", directory, "diff", "--name-only", "HEAD"], text=True
    ).strip()
    if changed:
        raise RuntimeError(f"PropEr source modified: {changed}")
    (directory / "ebin").mkdir(exist_ok=True)
    shutil.copyfile(directory / "src/proper.app.src", directory / "ebin/proper.app")
    options = ["erlc", "+debug_info", "-pa", "ebin", "-I", "include", "-o", "ebin"]
    subprocess.run(options + ["src/vararg.erl"], cwd=directory, check=True)
    files = [
        str(path.relative_to(directory))
        for path in sorted((directory / "src").glob("*.erl"))
        if path.name != "vararg.erl"
    ]
    subprocess.run(options + files, cwd=directory, check=True)
    print(f"Built PropEr {source['tag']} at {actual}")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Archive large, untracked benchmark outputs after stopping benchmark jobs."""

import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess


ROOT = Path(__file__).resolve().parents[2]
ROOTS = ("reports", ".cache/phase0/full-runs", ".cache/preparation")
SUFFIXES = {".json", ".jsonl", ".stdout", ".stderr", ".log", ".txt"}
REFERENCE = ROOT / "reports/head-to-head-2026-10-09"


def digest(stream):
    result = hashlib.sha256()
    while block := stream.read(1024 * 1024):
        result.update(block)
    return result.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true", help="default: inventory only")
    parser.add_argument("--manifest", type=Path, required=True)
    args = parser.parse_args()
    tracked = set(subprocess.check_output(
        ["git", "ls-files", "-z"], cwd=ROOT
    ).decode().split("\0"))
    records = []
    for relative_root in ROOTS:
        for parent, dirs, files in os.walk(ROOT / relative_root, followlinks=False):
            dirs[:] = [name for name in dirs
                       if name not in {"target", ".git"}
                       and not Path(parent, name).is_symlink()]
            for name in sorted(files):
                source = Path(parent, name)
                if (source.is_symlink() or source.suffix not in SUFFIXES
                        or source.is_relative_to(REFERENCE)
                        or str(source.relative_to(ROOT)) in tracked):
                    continue
                size = source.stat().st_size
                if size < 10_000_000:
                    continue
                destination = source.with_name(source.name + ".gz")
                record = {"source": str(source.relative_to(ROOT)), "raw_bytes": size}
                if args.apply:
                    if destination.exists():
                        raise FileExistsError(destination)
                    temporary = destination.with_name(destination.name + ".partial")
                    with source.open("rb") as stream:
                        before = digest(stream)
                    with source.open("rb") as stream, temporary.open("wb") as raw:
                        with gzip.GzipFile(filename="", fileobj=raw, mode="wb",
                                           compresslevel=1, mtime=0) as archive:
                            shutil.copyfileobj(stream, archive, 1024 * 1024)
                        raw.flush()
                        os.fsync(raw.fileno())
                    with gzip.open(temporary, "rb") as stream:
                        if digest(stream) != before:
                            raise RuntimeError(f"Archive verification failed: {source}")
                    temporary.rename(destination)
                    record.update(sha256=before, gzip_bytes=destination.stat().st_size,
                                  archive=str(destination.relative_to(ROOT)))
                    records.append(record)
                    args.manifest.parent.mkdir(parents=True, exist_ok=True)
                    args.manifest.write_text(json.dumps(records, indent=2) + "\n")
                    source.unlink()
                    print(json.dumps(record), flush=True)
                else:
                    records.append(record)
    if not args.apply:
        args.manifest.write_text(json.dumps(records, indent=2) + "\n")
    print(json.dumps({"files": len(records),
                      "raw_bytes": sum(row["raw_bytes"] for row in records),
                      "recovered_bytes": sum(row["raw_bytes"] - row.get("gzip_bytes", row["raw_bytes"])
                                             for row in records)}), flush=True)


if __name__ == "__main__":
    main()
